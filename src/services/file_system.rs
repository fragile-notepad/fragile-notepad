//! Whole-file reads, saves, and checks for changes made outside the editor.

use super::types::{FileError, FileOpenResult, FileSaveResult, OpenedFile};
use crate::core::{DiskState, DocumentId, FileRevision, TextEncoding, decode_bytes, encode_text};
use tokio::io::AsyncReadExt;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

/// Size and modification time cached by polling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    length: u64,
    modified: Option<SystemTime>,
}

#[derive(Debug, Clone)]
pub struct FileCheck {
    pub check_id: u64,
    pub document_id: DocumentId,
    pub path: PathBuf,
    pub expected: FileRevision,
    pub stamp: Option<FileStamp>,
    /// None means cached size and modification time matched; no bytes were read.
    pub state: Option<DiskState>,
}

/// Compares file bytes with `expected`; a matching `stamp` skips the read.
/// Set `stamp` to `None` to force a byte comparison.
pub async fn check_file(mut check: FileCheck) -> FileCheck {
    let previous = check.stamp.take();
    check.state = Some(match tokio::fs::metadata(&check.path).await {
        Ok(metadata) if metadata.is_file() => {
            let stamp = FileStamp {
                length: metadata.len(),
                modified: metadata.modified().ok(),
            };
            check.stamp = Some(stamp);
            if stamp.modified.is_some() && previous == Some(stamp) {
                check.state = None;
                return check;
            }
            match disk_revision(&check.path).await {
                Ok(current) if current == check.expected => DiskState::Current,
                Ok(_) => DiskState::Changed,
                Err(error) => disk_error_state(error.kind()),
            }
        }
        Ok(_) => DiskState::Unreadable(std::io::ErrorKind::IsADirectory),
        Err(error) => disk_error_state(error.kind()),
    });
    // Retry unreadable files even if their metadata stays the same.
    if matches!(
        check.state,
        Some(DiskState::Unreadable(_) | DiskState::Missing)
    ) {
        check.stamp = None;
    }
    check
}

fn disk_error_state(kind: std::io::ErrorKind) -> DiskState {
    if kind == std::io::ErrorKind::NotFound {
        DiskState::Missing
    } else {
        DiskState::Unreadable(kind)
    }
}

/// Decoding errors are returned in `contents.had_errors`, rather than failing the read.
pub async fn load_file(path: PathBuf) -> FileOpenResult {
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|error| FileError::Io(error.kind()))?;
    let contents = Arc::new(decode_bytes(&bytes));

    Ok(OpenedFile {
        path,
        contents,
        disk_revision: FileRevision::from_bytes(&bytes),
    })
}

/// Replaces a file only after comparing its current bytes with `expected`.
/// Missing `expected` returns `UnknownFileRevision`; changed or deleted files
/// return `FileChanged`. The comparison does not exclude concurrent writers.
pub async fn save_file_if_unchanged(
    path: PathBuf,
    contents: Vec<u8>,
    expected: Option<FileRevision>,
) -> FileSaveResult {
    let expected = expected.ok_or(FileError::UnknownFileRevision)?;
    super::atomic_write::write_with_check(&path, &contents, move |target| async move {
        match disk_revision(&target).await {
            Ok(current) if current == expected => Ok(()),
            Ok(_) => Err(FileError::FileChanged),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(FileError::FileChanged)
            }
            Err(error) => Err(error.into()),
        }
    })
    .await?;
    Ok(path)
}

async fn disk_revision(path: &std::path::Path) -> std::io::Result<FileRevision> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut buffer = vec![0; super::chunked_file::DEFAULT_CHUNK_SIZE];
    let mut hasher = blake3::Hasher::new();
    loop {
        let length = file.read(&mut buffer).await?;
        if length == 0 {
            return Ok(FileRevision(*hasher.finalize().as_bytes()));
        }
        hasher.update(&buffer[..length]);
    }
}

/// Replaces the destination bytes without comparing them with a loaded revision.
pub async fn save_file(path: PathBuf, contents: Vec<u8>) -> FileSaveResult {
    super::atomic_write::write(&path, &contents)
        .await
        .map_err(|error| FileError::Io(error.kind()))?;

    Ok(path)
}

/// This does not add a final line ending; the document prepares that text first.
pub fn encode_for_save(text: &str, encoding: TextEncoding) -> Result<Vec<u8>, FileError> {
    encode_text(text, encoding).map_err(FileError::Encoding)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestFile(PathBuf);

    impl TestFile {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let directory = std::env::temp_dir().join(format!(
                "fragile-save-conflict-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&directory).unwrap();
            let path = directory.join("document.txt");
            fs::write(&path, b"original").unwrap();
            Self(path)
        }

        fn only_original_remains(&self) {
            assert_eq!(fs::read_dir(self.0.parent().unwrap()).unwrap().count(), 1);
        }
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
            let _ = fs::remove_file(self.0.with_file_name("copy.txt"));
            let _ = fs::remove_dir(self.0.parent().unwrap());
        }
    }

    fn run(test: impl Future<Output = ()>) {
        tokio::runtime::Runtime::new().unwrap().block_on(test);
    }

    #[test]
    fn unchanged_files_and_subsequent_saves_succeed() {
        let file = TestFile::new();
        run(async {
            let opened = load_file(file.0.clone()).await.unwrap();
            save_file_if_unchanged(
                file.0.clone(),
                b"updated".to_vec(),
                Some(opened.disk_revision),
            )
            .await
            .unwrap();
            let saved_revision = FileRevision::from_bytes(b"updated");
            save_file_if_unchanged(
                file.0.clone(),
                b"updated again".to_vec(),
                Some(saved_revision),
            )
            .await
            .unwrap();
            assert_eq!(fs::read(&file.0).unwrap(), b"updated again");
            file.only_original_remains();
        });
    }

    #[test]
    fn forced_checks_compare_bytes_even_when_size_and_time_are_unchanged() {
        let file = TestFile::new();
        run(async {
            let original = FileCheck {
                check_id: 1,
                document_id: DocumentId::new(1),
                path: file.0.clone(),
                expected: FileRevision::from_bytes(b"original"),
                stamp: None,
                state: None,
            };
            let checked = check_file(original).await;
            assert_eq!(checked.state, Some(DiskState::Current));
            let modified = fs::metadata(&file.0).unwrap().modified().unwrap();
            fs::write(&file.0, b"external").unwrap();
            fs::OpenOptions::new()
                .write(true)
                .open(&file.0)
                .unwrap()
                .set_modified(modified)
                .unwrap();
            let cached = check_file(checked).await;
            assert_eq!(cached.state, None);
            let forced = check_file(FileCheck {
                stamp: None,
                ..cached
            })
            .await;
            assert_eq!(forced.state, Some(DiskState::Changed));
            assert_eq!(fs::read(&file.0).unwrap(), b"external");
        });
    }

    #[test]
    fn a_directory_replacing_a_file_is_reported_and_checked_again_afterwards() {
        let file = TestFile::new();
        fs::remove_file(&file.0).unwrap();
        fs::create_dir(&file.0).unwrap();
        run(async {
            let check = FileCheck {
                check_id: 1,
                document_id: DocumentId::new(1),
                path: file.0.clone(),
                expected: FileRevision::from_bytes(b"original"),
                stamp: None,
                state: None,
            };
            let unreadable = check_file(check).await;
            assert_eq!(
                unreadable.state,
                Some(DiskState::Unreadable(std::io::ErrorKind::IsADirectory))
            );
            assert!(unreadable.stamp.is_none());
            fs::remove_dir(&file.0).unwrap();
            fs::write(&file.0, b"original").unwrap();
            assert_eq!(check_file(unreadable).await.state, Some(DiskState::Current));
        });
    }

    #[test]
    fn external_edits_are_rejected_even_with_the_same_size_and_timestamp() {
        let file = TestFile::new();
        run(async {
            let opened = load_file(file.0.clone()).await.unwrap();
            let modified = fs::metadata(&file.0).unwrap().modified().unwrap();
            fs::write(&file.0, b"external").unwrap();
            fs::OpenOptions::new()
                .write(true)
                .open(&file.0)
                .unwrap()
                .set_modified(modified)
                .unwrap();
            let result = save_file_if_unchanged(
                file.0.clone(),
                b"editor contents".to_vec(),
                Some(opened.disk_revision),
            )
            .await;
            assert_eq!(result, Err(FileError::FileChanged));
            assert_eq!(fs::read(&file.0).unwrap(), b"external");
            file.only_original_remains();
        });
    }

    #[test]
    fn deleted_and_unknown_originals_cannot_be_overwritten() {
        let file = TestFile::new();
        run(async {
            let opened = load_file(file.0.clone()).await.unwrap();
            assert_eq!(
                save_file_if_unchanged(file.0.clone(), b"editor contents".to_vec(), None).await,
                Err(FileError::UnknownFileRevision)
            );
            assert_eq!(fs::read(&file.0).unwrap(), b"original");
            fs::remove_file(&file.0).unwrap();
            assert_eq!(
                save_file_if_unchanged(
                    file.0.clone(),
                    b"editor contents".to_vec(),
                    Some(opened.disk_revision)
                )
                .await,
                Err(FileError::FileChanged)
            );
            assert!(!file.0.exists());
            assert_eq!(fs::read_dir(file.0.parent().unwrap()).unwrap().count(), 0);
        });
    }

    #[test]
    fn save_as_checks_the_original_but_allows_a_different_destination() {
        let file = TestFile::new();
        run(async {
            let opened = load_file(file.0.clone()).await.unwrap();
            let source = Some((file.0.clone(), Some(opened.disk_revision)));
            fs::write(&file.0, b"external").unwrap();
            let alias = file.0.parent().unwrap().join(".").join("document.txt");
            assert_eq!(
                super::super::file_dialogs::save_picked_file(
                    alias,
                    b"editor contents".to_vec(),
                    source.clone(),
                    false,
                )
                .await,
                Err(FileError::FileChanged)
            );
            let copy = file.0.with_file_name("copy.txt");
            super::super::file_dialogs::save_picked_file(
                copy.clone(),
                b"editor contents".to_vec(),
                source,
                false,
            )
            .await
            .unwrap();
            assert_eq!(fs::read(&file.0).unwrap(), b"external");
            assert_eq!(fs::read(copy).unwrap(), b"editor contents");
        });
    }

    #[test]
    fn decoding_errors_protect_original_in_save_as_and_copy() {
        let file = TestFile::new();
        let malformed = b"\xef\xbb\xbfhello\xff";
        fs::write(&file.0, malformed).unwrap();
        run(async {
            let opened = load_file(file.0.clone()).await.unwrap();
            assert!(opened.contents.had_errors);
            let document = crate::core::Document::from_decoded(
                crate::core::DocumentId::new(1),
                file.0.clone(),
                opened.contents.as_ref().clone(),
            );
            let contents = document.bytes_for_save().unwrap();
            let source = Some((file.0.clone(), Some(opened.disk_revision)));
            for target in [
                file.0.clone(),
                file.0.parent().unwrap().join(".").join("document.txt"),
            ] {
                assert_eq!(
                    super::super::file_dialogs::save_picked_file(
                        target,
                        contents.clone(),
                        source.clone(),
                        true,
                    )
                    .await,
                    Err(FileError::DecodingErrors)
                );
                assert_eq!(fs::read(&file.0).unwrap(), malformed);
            }
            let copy = file.0.with_file_name("copy.txt");
            super::super::file_dialogs::save_picked_file(
                copy.clone(),
                contents.clone(),
                source,
                true,
            )
            .await
            .unwrap();
            assert_eq!(fs::read(copy).unwrap(), contents);
            assert_eq!(fs::read(&file.0).unwrap(), malformed);
        });
    }
}
