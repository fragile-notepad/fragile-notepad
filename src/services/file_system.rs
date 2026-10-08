//! Disk I/O and encoding helpers.

use super::types::{FileError, FileOpenResult, FileSaveResult, OpenedFile};
use crate::core::{FileRevision, TextEncoding, decode_bytes, encode_text};
use tokio::io::AsyncReadExt;

use std::path::PathBuf;
use std::sync::Arc;

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

pub async fn save_file(path: PathBuf, contents: Vec<u8>) -> FileSaveResult {
    super::atomic_write::write(&path, &contents)
        .await
        .map_err(|error| FileError::Io(error.kind()))?;

    Ok(path)
}

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
                    source.clone()
                )
                .await,
                Err(FileError::FileChanged)
            );
            let copy = file.0.with_file_name("copy.txt");
            super::super::file_dialogs::save_picked_file(
                copy.clone(),
                b"editor contents".to_vec(),
                source,
            )
            .await
            .unwrap();
            assert_eq!(fs::read(&file.0).unwrap(), b"external");
            assert_eq!(fs::read(copy).unwrap(), b"editor contents");
        });
    }
}
