//! Pure document and editing state modules.

pub mod close;
pub mod document;
pub mod encoding;
pub mod file_revision;
pub mod search;
pub mod session;
pub mod settings;
pub mod shortcuts;
pub mod workspace;

pub use close::DirtyCloseDecision;
pub use document::{
    Document, DocumentId, DocumentIndexState, DocumentLoadGeneration, DocumentLoadState,
    MAX_FULL_DOCUMENT_ANALYSIS_BYTES,
};
pub use encoding::{DecodedText, EncodingError, TextEncoding, decode_bytes, encode_text};
pub use file_revision::FileRevision;
pub use search::{FindState, PreparedSearch, SearchError, SearchMode, SearchOptions, TextMatch};
pub use session::{Session, SessionDocument};
pub use settings::{
    AppearanceMode, EditorSettings, HardwareAccelerationMode, IndentationMode, SearchResultSettings,
};
pub use shortcuts::{
    KeyBinding, ShortcutCommand, ShortcutConflict, ShortcutDisplay, ShortcutDisplayPart,
    ShortcutEntry, ShortcutGroup, ShortcutKey, ShortcutMap, ShortcutModifierIcon,
    ShortcutModifiers,
};
pub use workspace::Workspace;
