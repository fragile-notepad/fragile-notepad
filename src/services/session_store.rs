//! Ordered, atomic session recovery persistence.

use crate::core::Session;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::AsyncReadExt;

const MAX_SESSION_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Default)]
struct SessionWriter {
    pending: Mutex<(u64, Option<Arc<Session>>)>,
    writing: tokio::sync::Mutex<()>,
}

impl SessionWriter {
    fn enqueue(&self, session: Session) -> Result<(), String> {
        session.validate()?;
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        pending.0 = pending.0.wrapping_add(1);
        pending.1 = Some(Arc::new(session));
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
            let (generation, Some(session)) = snapshot else {
                return Ok(());
            };
            let bytes = encode(&session)?;
            super::atomic_write::write_private(path, &bytes)
                .await
                .map_err(|error| error.to_string())?;
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

pub async fn load_session() -> Result<Option<Session>, String> {
    load_from(&session_path()?).await
}

pub fn save_session(session: Session) -> impl Future<Output = Result<(), String>> {
    let queued = writer().enqueue(session);
    async move {
        queued?;
        flush_session().await
    }
}

pub async fn flush_session() -> Result<(), String> {
    writer().flush(&session_path()?).await
}

async fn load_from(path: &Path) -> Result<Option<Session>, String> {
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_SESSION_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_SESSION_BYTES {
        return Err("Session exceeds the 256 MiB recovery limit".into());
    }
    let session: Session =
        serde_json::from_slice(&bytes).map_err(|error| format!("Invalid session: {error}"))?;
    session.validate()?;
    Ok(Some(session))
}

fn encode(session: &Session) -> Result<Vec<u8>, String> {
    struct LimitedOutput(Vec<u8>);
    impl std::io::Write for LimitedOutput {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0.len().saturating_add(bytes.len()) as u64 > MAX_SESSION_BYTES {
                return Err(std::io::Error::other(
                    "Session exceeds the 256 MiB recovery limit",
                ));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = LimitedOutput(Vec::new());
    serde_json::to_writer(&mut output, session).map_err(|error| error.to_string())?;
    Ok(output.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{SessionDocument, SessionWindow, TextEncoding};

    fn remove_empty_test_directory(directory: PathBuf) {
        for attempt in 0..20 {
            match std::fs::remove_dir(&directory) {
                Ok(()) => return,
                Err(error)
                    if error.kind() == std::io::ErrorKind::DirectoryNotEmpty && attempt < 19 =>
                {
                    // Windows may keep a recently removed file delete-pending.
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("remove empty test directory: {error}"),
            }
        }
    }

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "fragile-session-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    fn session(text: &str) -> Session {
        Session {
            documents: vec![SessionDocument {
                path: Some(PathBuf::from("\u{6587}\u{4ef6}.txt")),
                text: Some(text.into()),
                is_dirty: true,
                is_pinned: true,
                encoding: TextEncoding::Utf16LeBom,
                line_ending: Some("\r\n".into()),
                cursor_column: 2,
                horizontal_offset: 3.5,
                syntax_token: Some("rs".into()),
                collapsed_folds: vec![(1, 3)],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn roundtrip_and_reverse_polling_keep_latest_session() {
        let directory = test_dir("ordered");
        let path = directory.join("session.json");
        let writer = SessionWriter::default();
        writer.enqueue(session("old")).unwrap();
        let old = writer.flush(&path);
        let latest = session("hello \u{4e16}\u{754c}\r\n");
        writer.enqueue(latest.clone()).unwrap();
        let new = writer.flush(&path);
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (a, b) = futures::join!(new, old);
            a.unwrap();
            b.unwrap();
            assert_eq!(load_from(&path).await.unwrap(), Some(latest));
        });
        std::fs::remove_file(path).unwrap();
        remove_empty_test_directory(directory);
    }

    #[test]
    fn malformed_and_unknown_versions_are_rejected_without_changing_file() {
        let directory = test_dir("invalid");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("session.json");
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            for bytes in [
                b"not json".as_slice(),
                br#"{"version":3,"documents":[],"active_index":0}"#,
                br#"{"version":2,"documents":[],"active_index":0,"windows":[{}]}"#,
            ] {
                std::fs::write(&path, bytes).unwrap();
                assert!(load_from(&path).await.is_err(), "{bytes:?}");
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
            }
        });
        std::fs::remove_file(path).unwrap();
        remove_empty_test_directory(directory);
    }

    #[test]
    fn legacy_and_multiwindow_sessions_load_without_changing_file() {
        let directory = test_dir("versions");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("session.json");
        let mut multiwindow = session("first window");
        multiwindow.version = 2;
        multiwindow
            .documents
            .extend(session("second window").documents);
        multiwindow.windows = vec![
            SessionWindow {
                document_indices: vec![0],
                ..Default::default()
            },
            SessionWindow {
                document_indices: vec![1],
                ..Default::default()
            },
        ];
        multiwindow.focused_window = Some(1);
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            for expected in [
                Session::default(),
                session("legacy recovery"),
                Session {
                    version: 2,
                    ..Default::default()
                },
                multiwindow,
            ] {
                let bytes = encode(&expected).unwrap();
                std::fs::write(&path, &bytes).unwrap();
                assert_eq!(load_from(&path).await.unwrap(), Some(expected));
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
            }
        });
        std::fs::remove_file(path).unwrap();
        remove_empty_test_directory(directory);
    }

    #[test]
    fn native_paths_and_invalid_metadata() {
        let mut value = session("recovery");
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            value.documents[0].path = Some(PathBuf::from(std::ffi::OsString::from_wide(&[
                0x61, 0xd800, 0x62,
            ])));
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            value.documents[0].path = Some(PathBuf::from(std::ffi::OsString::from_vec(vec![
                b'a', 0xff, b'b',
            ])));
        }
        assert_eq!(
            serde_json::from_slice::<Session>(&encode(&value).unwrap()).unwrap(),
            value
        );
        value.documents[0].text = None;
        assert!(value.validate().is_err());
        value.documents[0].text = Some(String::new());
        value.documents[0].horizontal_offset = f32::NAN;
        assert!(value.validate().is_err());
    }

    #[test]
    fn failed_write_retains_snapshot_for_retry() {
        let directory = test_dir("retry");
        std::fs::write(&directory, b"block directory creation").unwrap();
        let path = directory.join("session.json");
        let writer = SessionWriter::default();
        let expected = session("unsaved text");
        writer.enqueue(expected.clone()).unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            assert!(writer.flush(&path).await.is_err());
            std::fs::remove_file(&directory).unwrap();
            writer.flush(&path).await.unwrap();
            assert_eq!(load_from(&path).await.unwrap(), Some(expected));
        });
        std::fs::remove_file(path).unwrap();
        remove_empty_test_directory(directory);
    }

    #[test]
    fn language_provenance_roundtrips_and_legacy_sessions_remain_readable() {
        for automatic in [None, Some(true), Some(false)] {
            let mut saved = session("fn main() {}");
            saved.documents[0].syntax_automatic = automatic;
            let bytes = encode(&saved).unwrap();
            let restored: Session = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(restored, saved);
        }
        let mut legacy = serde_json::to_value(session("fn main() {}")).unwrap();
        legacy["documents"][0]
            .as_object_mut()
            .unwrap()
            .remove("syntax_automatic");
        let restored: Session = serde_json::from_value(legacy).unwrap();
        assert_eq!(restored.documents[0].syntax_automatic, None);
    }

    #[test]
    fn a_new_snapshot_during_a_write_is_flushed_before_completion() {
        let directory = test_dir("inflight");
        let path = directory.join("session.json");
        let writer = SessionWriter::default();
        writer.enqueue(session("old")).unwrap();
        let expected = session("new");
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut in_flight = Box::pin(writer.flush(&path));
            assert!(futures::poll!(&mut in_flight).is_pending());
            assert!(writer.writing.try_lock().is_err());
            writer.enqueue(expected.clone()).unwrap();
            in_flight.await.unwrap();
            assert_eq!(load_from(&path).await.unwrap(), Some(expected));
        });
        std::fs::remove_file(path).unwrap();
        remove_empty_test_directory(directory);
    }
}
