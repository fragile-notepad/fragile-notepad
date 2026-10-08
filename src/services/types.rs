//! Service requests, events, and errors independent of application messages.

use crate::core::{
    DecodedText, DocumentId, DocumentLoadGeneration, EditorSettings, EncodingError, FileRevision,
    TextEncoding,
};
use std::{io, path::PathBuf, sync::Arc};

pub type FileOpenResult = Result<OpenedFile, FileError>;
pub type FileResult<T> = Result<T, FileError>;
pub type FileLoadResult = Result<FileLoadFinished, FileLoadFailure>;
pub type FileSaveResult = Result<PathBuf, FileError>;
pub type SettingsLoadResult = Result<Option<EditorSettings>, SettingsError>;
pub type SettingsSaveResult = Result<(), SettingsError>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveFileDialogOptions {
    pub file_name: Option<String>,
    pub filter: Option<SaveFileDialogFilter>,
    pub original_file: Option<(PathBuf, Option<FileRevision>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveFileDialogFilter {
    pub name: String,
    pub extension: String,
}

#[derive(Debug, Clone)]
pub struct OpenedFile {
    pub path: PathBuf,
    pub contents: Arc<DecodedText>,
    pub disk_revision: FileRevision,
}

#[derive(Debug, Clone)]
pub struct FileLoadRequest {
    pub document_id: DocumentId,
    pub generation: DocumentLoadGeneration,
    pub path: PathBuf,
    pub chunk_size: usize,
}

#[derive(Debug, Clone)]
pub enum FileLoadEvent {
    Progress(FileLoadProgress),
    Chunk(FileLoadChunk),
    Finished(FileLoadResult),
}

#[derive(Debug, Clone)]
pub struct FileLoadProgress {
    pub document_id: DocumentId,
    pub generation: DocumentLoadGeneration,
    pub path: PathBuf,
    pub bytes_read: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct FileLoadChunk {
    pub document_id: DocumentId,
    pub generation: DocumentLoadGeneration,
    pub path: PathBuf,
    pub text: Arc<String>,
    pub reset: bool,
    pub bytes_read: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct FileLoadFinished {
    pub document_id: DocumentId,
    pub generation: DocumentLoadGeneration,
    pub path: PathBuf,
    pub encoding: TextEncoding,
    pub had_errors: bool,
    pub fallback_contents: Option<Arc<DecodedText>>,
    pub bytes_read: u64,
    pub total_bytes: Option<u64>,
    pub disk_revision: FileRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLoadFailure {
    pub document_id: DocumentId,
    pub generation: DocumentLoadGeneration,
    pub path: PathBuf,
    pub error: FileError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    DialogClosed,
    Io(io::ErrorKind),
    Encoding(EncodingError),
    FileChanged,
    UnknownFileRevision,
}

impl FileError {
    pub fn summary(&self) -> &'static str {
        match self {
            Self::DialogClosed => "dialog closed",
            Self::Io(_) => "I/O error",
            Self::Encoding(_) => "encoding error",
            Self::FileChanged => {
                "file changed or was deleted on disk; use Save As to save to a different file"
            }
            Self::UnknownFileRevision => {
                "cannot verify the file on disk; use Save As to save to a different file"
            }
        }
    }
}

impl From<io::Error> for FileError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    Unavailable,
    Io(io::ErrorKind),
}

impl SettingsError {
    pub fn summary(&self) -> &'static str {
        match self {
            Self::Unavailable => "settings directory unavailable",
            Self::Io(_) => "I/O error",
        }
    }
}
