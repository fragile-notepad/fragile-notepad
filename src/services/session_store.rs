//! Ordered recovery manifests with bounded, content-addressed text chunks.

use crate::core::{Session, SessionDocument};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::AsyncReadExt;

const MAX_SESSION_BYTES: u64 = 256 * 1024 * 1024; // Legacy inline JSON only.
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const CHUNK_BYTES: usize = 8 * 1024 * 1024;
const MAX_CHUNKS: usize = 100_000;

pub(crate) enum CapturedDocument {
    Deferred(Arc<SessionDocument>, bool),
    Live(Box<SessionDocument>, Option<ropey::Rope>),
}

/// Text shares the editor's immutable Rope nodes until a worker materializes it.
pub(crate) struct SessionSnapshot {
    pub layout: Session,
    pub documents: Vec<CapturedDocument>,
}

impl SessionSnapshot {
    pub(crate) fn materialize(&self) -> Session {
        let mut session = self.layout.clone();
        session.documents = self
            .documents
            .iter()
            .map(|document| match document {
                CapturedDocument::Deferred(entry, pinned) => {
                    let mut entry = (**entry).clone();
                    entry.is_pinned = *pinned;
                    entry
                }
                CapturedDocument::Live(entry, rope) => {
                    let mut entry = (**entry).clone();
                    entry.text = rope.as_ref().map(ToString::to_string);
                    entry
                }
            })
            .collect();
        session
    }
}

impl From<Session> for SessionSnapshot {
    fn from(mut session: Session) -> Self {
        let documents = std::mem::take(&mut session.documents)
            .into_iter()
            .map(|entry| {
                let pinned = entry.is_pinned;
                CapturedDocument::Deferred(Arc::new(entry), pinned)
            })
            .collect();
        Self {
            layout: session,
            documents,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    storage_version: u32,
    session: Session,
    recovery: Vec<Option<Vec<String>>>,
}

struct Prepared {
    manifest: Manifest,
    encoded: Vec<u8>,
    texts: Vec<Option<String>>,
}

#[derive(Default)]
struct SessionWriter {
    pending: Mutex<(u64, Option<Arc<SessionSnapshot>>)>,
    writing: tokio::sync::Mutex<()>,
}

impl SessionWriter {
    fn enqueue(&self, session: impl Into<SessionSnapshot>) -> Result<(), String> {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        pending.0 = pending.0.wrapping_add(1);
        pending.1 = Some(Arc::new(session.into()));
        Ok(())
    }

    async fn flush(&self, path: &Path) -> Result<(), String> {
        let _writing = self.writing.lock().await;
        loop {
            let snapshot = self
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            let (generation, Some(snapshot)) = snapshot else {
                return Ok(());
            };
            let prepared =
                tokio::task::spawn_blocking(move || prepare(snapshot.materialize(), CHUNK_BYTES))
                    .await
                    .map_err(|error| error.to_string())??;
            publish(path, &prepared, CHUNK_BYTES).await?;
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if pending.0 == generation {
                pending.1 = None;
            }
        }
    }
}

fn writer() -> &'static SessionWriter {
    static WRITER: OnceLock<SessionWriter> = OnceLock::new();
    WRITER.get_or_init(SessionWriter::default)
}

fn session_path() -> Result<PathBuf, String> {
    crate::platform::paths::config_dir()
        .map(|path| path.join("session.json"))
        .ok_or_else(|| "Session storage is unavailable".into())
}

#[derive(Debug, Clone)]
pub struct LoadedSession {
    pub session: Option<Session>,
    pub warning: Option<String>,
}

pub async fn load_session() -> Result<LoadedSession, String> {
    load_with_fallback(&session_path()?).await
}

async fn load_with_fallback(path: &Path) -> Result<LoadedSession, String> {
    match load_from(path).await {
        Ok(Some(session)) => Ok(LoadedSession {
            session: Some(session),
            warning: None,
        }),
        current => match load_from(&previous_path(path)).await {
            Ok(Some(session)) => Ok(LoadedSession {
                session: Some(session),
                warning: Some(format!(
                    "Recovered the previous session snapshot; latest recovery is unavailable ({}). Recent edits may be missing. Original recovery files were preserved.",
                    current
                        .err()
                        .unwrap_or_else(|| "latest snapshot missing".into())
                )),
            }),
            _ => current.map(|session| LoadedSession {
                session,
                warning: None,
            }),
        },
    }
}

pub(crate) fn save_session(
    session: impl Into<SessionSnapshot>,
) -> impl Future<Output = Result<(), String>> {
    let queued = writer().enqueue(session);
    async move {
        queued?;
        flush_session().await
    }
}

pub async fn flush_session() -> Result<(), String> {
    writer().flush(&session_path()?).await
}

fn previous_path(path: &Path) -> PathBuf {
    path.with_extension("previous.json")
}
fn chunks_path(path: &Path) -> PathBuf {
    path.with_extension("chunks")
}

async fn read_limited(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!(
            "Recovery file exceeds its {} MiB limit; previous recovery is preserved. Save large documents explicitly.",
            limit / (1024 * 1024)
        ));
    }
    Ok(Some(bytes))
}

async fn load_from(path: &Path) -> Result<Option<Session>, String> {
    let Some(bytes) = read_limited(path, MAX_SESSION_BYTES).await? else {
        return Ok(None);
    };
    let root = chunks_path(&if path.extension().is_some_and(|ext| ext == "json")
        && path
            .file_stem()
            .is_some_and(|stem| stem.to_string_lossy().ends_with(".previous"))
    {
        path.with_extension("").with_extension("json")
    } else {
        path.to_owned()
    });
    tokio::task::spawn_blocking(move || decode(&bytes, &root, CHUNK_BYTES))
        .await
        .map_err(|error| error.to_string())?
        .map(Some)
}

fn decode(bytes: &[u8], root: &Path, chunk_bytes: usize) -> Result<Session, String> {
    decode_snapshot(bytes, root, chunk_bytes, true)
}

/// Verification keeps only one bounded chunk and a UTF-8 boundary tail in RAM.
fn decode_snapshot(
    bytes: &[u8],
    root: &Path,
    chunk_bytes: usize,
    restore_text: bool,
) -> Result<Session, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| format!("Invalid session: {error}"))?;
    if value.get("storage_version").is_none() {
        let session: Session = serde_json::from_value(value).map_err(|error| error.to_string())?;
        session.validate()?;
        return Ok(session);
    }
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("Recovery manifest exceeds the 16 MiB limit".into());
    }
    let manifest: Manifest = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if manifest.storage_version != 1
        || manifest.recovery.len() != manifest.session.documents.len()
        || manifest
            .recovery
            .iter()
            .flatten()
            .map(Vec::len)
            .sum::<usize>()
            > MAX_CHUNKS
    {
        return Err("Invalid recovery manifest version or document/chunk count".into());
    }
    let mut session = manifest.session;
    // Validate layout/counts before touching text. An empty placeholder records
    // the existence of recovery data without allocating the document contents.
    for (entry, recovery) in session.documents.iter_mut().zip(&manifest.recovery) {
        if entry.text.is_some() {
            return Err("Recovery manifest contains inline text".into());
        }
        entry.text = recovery.as_ref().map(|_| String::new());
    }
    session.validate()?;
    for (entry, recovery) in session.documents.iter_mut().zip(manifest.recovery) {
        if let Some(hashes) = recovery {
            let mut text = Vec::new();
            let mut utf8_tail = Vec::new();
            for hash in hashes {
                if !valid_hash(&hash) {
                    return Err("Invalid recovery chunk name".into());
                }
                use std::io::Read;
                let file = std::fs::File::open(root.join(&hash))
                    .map_err(|error| format!("Recovery chunk {hash}: {error}"))?;
                let mut bytes = Vec::new();
                file.take(chunk_bytes as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| error.to_string())?;
                if bytes.len() > chunk_bytes || blake3::hash(&bytes).to_hex().as_str() != hash {
                    return Err(format!("Recovery chunk {hash} failed integrity validation"));
                }
                if restore_text {
                    text.len()
                        .checked_add(bytes.len())
                        .ok_or_else(|| "Recovery text size overflow".to_string())?;
                    text.try_reserve(bytes.len()).map_err(|error| format!("Not enough memory to restore recovery text: {error}. Recovery files were preserved."))?;
                    text.extend_from_slice(&bytes);
                } else {
                    utf8_tail.extend_from_slice(&bytes);
                    match std::str::from_utf8(&utf8_tail) {
                        Ok(_) => utf8_tail.clear(),
                        Err(error) if error.error_len().is_none() => {
                            utf8_tail = utf8_tail[error.valid_up_to()..].to_vec();
                        }
                        Err(error) => return Err(format!("Invalid recovery text: {error}")),
                    }
                }
            }
            if !utf8_tail.is_empty() {
                return Err("Incomplete UTF-8 recovery text".into());
            }
            entry.text = Some(String::from_utf8(text).map_err(|error| error.to_string())?);
        }
    }
    session.validate()?;
    Ok(session)
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn prepare(mut session: Session, chunk_bytes: usize) -> Result<Prepared, String> {
    session.validate()?;
    let texts: Vec<_> = session
        .documents
        .iter_mut()
        .map(|entry| entry.text.take())
        .collect();
    let recovery: Vec<_> = texts
        .iter()
        .map(|text| {
            text.as_ref().map(|text| {
                text.as_bytes()
                    .chunks(chunk_bytes)
                    .map(|bytes| blake3::hash(bytes).to_hex().to_string())
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    if recovery.iter().flatten().map(Vec::len).sum::<usize>() > MAX_CHUNKS {
        return Err("Session exceeds the 100,000 recovery chunk limit; previous recovery is preserved. Save large documents explicitly.".into());
    }
    let manifest = Manifest {
        storage_version: 1,
        session,
        recovery,
    };
    let encoded = encode_limited(&manifest, MAX_MANIFEST_BYTES)?;
    Ok(Prepared {
        manifest,
        encoded,
        texts,
    })
}

async fn publish(path: &Path, prepared: &Prepared, chunk_bytes: usize) -> Result<(), String> {
    let root = chunks_path(path);
    let mut directory = tokio::fs::DirBuilder::new();
    directory.recursive(true);
    #[cfg(unix)]
    directory.mode(0o700);
    directory
        .create(&root)
        .await
        .map_err(|error| error.to_string())?;
    let check_root = root.clone();
    let hashes: HashSet<_> = prepared
        .manifest
        .recovery
        .iter()
        .flatten()
        .flatten()
        .cloned()
        .collect();
    let mut written = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        hashes
            .into_iter()
            .filter(|hash| {
                let Ok(file) = std::fs::File::open(check_root.join(hash)) else {
                    return false;
                };
                let mut bytes = Vec::new();
                file.take(chunk_bytes as u64 + 1)
                    .read_to_end(&mut bytes)
                    .is_ok()
                    && bytes.len() <= chunk_bytes
                    && blake3::hash(&bytes).to_hex().as_str() == hash
            })
            .collect::<HashSet<_>>()
    })
    .await
    .map_err(|error| error.to_string())?;
    for (text, hashes) in prepared.texts.iter().zip(&prepared.manifest.recovery) {
        if let (Some(text), Some(hashes)) = (text, hashes) {
            for (bytes, hash) in text.as_bytes().chunks(chunk_bytes).zip(hashes) {
                if written.contains(hash) {
                    continue;
                }
                super::atomic_write::write_private(&root.join(hash), bytes)
                    .await
                    .map_err(|error| error.to_string())?;
                written.insert(hash.clone());
            }
        }
    }
    let previous = previous_path(path);
    // Only rotate a validated snapshot. Save a corrupt manifest separately
    // so it cannot replace the last valid recovery copy.
    if let Some(bytes) = read_limited(path, MAX_SESSION_BYTES).await? {
        let original = bytes.clone();
        let check_root = root.clone();
        let valid = tokio::task::spawn_blocking(move || {
            decode_snapshot(&bytes, &check_root, CHUNK_BYTES, false)
        })
        .await
        .map_err(|error| error.to_string())?
        .is_ok();
        let destination = if valid {
            previous.clone()
        } else {
            path.with_extension("unreadable.json")
        };
        super::atomic_write::write_private(&destination, &original)
            .await
            .map_err(|error| error.to_string())?;
    }
    super::atomic_write::write_private(path, &prepared.encoded)
        .await
        .map_err(|error| error.to_string())?;
    // Cleanup is best effort after commit. Keep chunks while a damaged
    // manifest remains, since some of them may still be recoverable.
    let unreadable_manifest = path.with_extension("unreadable.json");
    if !tokio::fs::try_exists(unreadable_manifest)
        .await
        .unwrap_or(true)
    {
        let mut keep: HashSet<String> = prepared
            .manifest
            .recovery
            .iter()
            .flatten()
            .flatten()
            .cloned()
            .collect();
        if let Ok(Some(bytes)) = read_limited(&previous, MAX_SESSION_BYTES).await
            && let Ok(manifest) = serde_json::from_slice::<Manifest>(&bytes)
        {
            keep.extend(manifest.recovery.into_iter().flatten().flatten());
        }
        if let Ok(mut entries) = tokio::fs::read_dir(&root).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                if let Some(name) = name.to_str()
                    && valid_hash(name)
                    && !keep.contains(name)
                {
                    let _ = tokio::fs::remove_file(entry.path()).await;
                }
            }
        }
    }
    Ok(())
}

fn encode_limited(value: &impl Serialize, limit: u64) -> Result<Vec<u8>, String> {
    struct LimitedOutput {
        bytes: Vec<u8>,
        limit: u64,
    }
    impl std::io::Write for LimitedOutput {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.bytes.len().saturating_add(bytes.len()) as u64 > self.limit {
                return Err(std::io::Error::other(
                    "Recovery metadata exceeds its capacity; previous recovery is preserved. Save documents explicitly.",
                ));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = LimitedOutput {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut output, value).map_err(|error| error.to_string())?;
    Ok(output.bytes)
}

#[cfg(test)]
fn encode(session: &Session) -> Result<Vec<u8>, String> {
    encode_limited(session, MAX_SESSION_BYTES)
}

#[cfg(test)]
#[path = "session_store_tests.rs"]
mod tests;
