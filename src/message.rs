use crate::core::DirtyCloseDecision;
use crate::services::types::{
    FileLoadChunk, FileLoadEvent, FileLoadProgress, FileLoadResult, FileOpenResult, FileResult,
    FileSaveResult, SettingsLoadResult, SettingsSaveResult,
};

use iced::event;
use iced::highlighter;
use iced::window;

use crate::core::{
    AppearanceMode, DocumentId, HardwareAccelerationMode, IndentationMode, KeyBinding, SearchMode,
    ShortcutCommand, ShortcutConflict, TextEncoding,
};
use crate::editor::{
    EditorAction, EditorPosition, EditorSelection, OutlineParseResult, SelectionSet,
};
use crate::ipc::ActivationRequest;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    File,
    Edit,
    Search,
    View,
    Encoding,
    Language,
    Settings,
    Window,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuPath {
    pub depth: usize,
    pub segments: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    General,
    Appearance,
    Editor,
    Shortcuts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvancedSearchTab {
    Find,
    Replace,
    FindInFiles,
    ReplaceInFiles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AboutTab {
    About,
    Debug,
    Licenses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowTarget {
    Main,
    Settings,
    AdvancedSearch,
}

// One catalog defines public messages, typed feature inputs, and routing.
// Handlers cannot receive a message from another feature. Delivery policy is
// declared with each variant so new asynchronous outputs must choose a policy.
macro_rules! message_catalog {
    ($( $route:ident($domain:ident) { $( $(#[$attr:meta])* $variant:ident $(($($arg:ident: $ty:ty),* $(,)?))? => $delivery:ident, )* } )*) => {
        #[derive(Debug, Clone)]
        pub enum Message {
            $( $( $(#[$attr])* $variant $(($($ty),*))?, )* )*
        }
        $(
            #[derive(Debug, Clone)]
            pub(crate) enum $domain {
                $( $(#[$attr])* $variant $(($($ty),*))?, )*
            }
            impl From<$domain> for Message {
                fn from(message: $domain) -> Self {
                    match message {
                        $( $(#[$attr])* $domain::$variant $(($($arg),*))? => Self::$variant $(($($arg),*))?, )*
                    }
                }
            }
        )*
        pub(crate) enum RoutedMessage { $( $route($domain), )* }
        impl Message {
            pub(crate) fn route(self) -> RoutedMessage {
                match self {
                    $( $( $(#[$attr])* Self::$variant $(($($arg),*))? => RoutedMessage::$route($domain::$variant $(($($arg),*))?), )* )*
                }
            }
            pub(crate) fn shutdown_delivery(&self) -> ShutdownDelivery {
                match self {
                    $( $( $(#[$attr])* Self::$variant $(($($arg),*))? => {
                        $( $(let _ = $arg;)* )?
                        ShutdownDelivery::$delivery
                    }, )* )*
                }
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShutdownDelivery {
    Reject,
    Defer,
    Resolve,
}

message_catalog! {
    Settings(SettingsMessage) {
        DraftThemeSelected(arg0: highlighter::Theme) => Reject,
        SystemColorModeChanged(arg0: iced::theme::Mode) => Reject,
        DraftWordWrapToggled(arg0: bool) => Reject,
        DraftFixedWrapSelected(arg0: bool) => Reject,
        DraftWrapColumnChanged(arg0: String) => Reject,
        DraftWrapColumnPreset(arg0: usize) => Reject,
        DraftAutoSaveToggled(arg0: bool) => Reject,
        DraftAppearanceSelected(arg0: AppearanceMode) => Reject,
        DraftHardwareAccelerationSelected(arg0: HardwareAccelerationMode) => Reject,
        DraftIndentationSelected(arg0: IndentationMode) => Reject,
        DraftLineNumbersToggled(arg0: bool) => Reject,
        DraftVisibleSpacesToggled(arg0: bool) => Reject,
        DraftVisibleTabsToggled(arg0: bool) => Reject,
        DraftEolMarkersToggled(arg0: bool) => Reject,
        DraftIndentationGuidesToggled(arg0: bool) => Reject,
        DraftWrapIndicatorToggled(arg0: bool) => Reject,
        DraftWrapGuideToggled(arg0: bool) => Reject,
        DraftFoldingControlsToggled(arg0: bool) => Reject,
        SettingsCategorySelected(arg0: SettingsCategory) => Reject,
        ShortcutGroupSelected(arg0: crate::core::ShortcutGroup) => Reject,
        SettingsZoomIn => Reject,
        SettingsZoomOut => Reject,
        SettingsZoomReset => Reject,
        SettingsScrollSpeedIncrease => Reject,
        SettingsScrollSpeedDecrease => Reject,
        SettingsScrollSpeedReset => Reject,
        ApplySettings => Reject,
        SaveSettings => Reject,
        SettingsLoaded(arg0: SettingsLoadResult) => Defer,
        SettingsPersisted(arg0: SettingsSaveResult) => Defer,
        CancelSettings => Reject,
        ToggleSettingsPanel => Reject,
        ShortcutCaptureStarted(arg0: ShortcutCommand) => Reject,
        ShortcutCaptured(arg0: ShortcutCommand, arg1: KeyBinding) => Reject,
        ShortcutCleared(arg0: ShortcutCommand) => Reject,
        ShortcutsResetToDefaults => Reject,
        ShortcutConflictDismissed => Reject,
        ShortcutCaptureConflict(arg0: ShortcutConflict) => Reject,
        ZoomIn => Reject,
        ZoomOut => Reject,
        ZoomReset => Reject,
        ToggleWordWrap => Reject,
        ToggleLineNumbers => Reject,
        ToggleSpaceAndTab => Reject,
        ToggleVisibleSpaces => Reject,
        ToggleVisibleTabs => Reject,
        ToggleEolMarkers => Reject,
        ToggleAllCharacters => Reject,
        ToggleIndentationGuides => Reject,
        ToggleFoldingControls => Reject,
    }
    Files(FileMessage) {
        TabSelected(arg0: DocumentId) => Reject,
        TabClosed(arg0: DocumentId) => Reject,
        TabPinToggled(arg0: DocumentId) => Reject,
        TabDragStarted(arg0: DocumentId) => Reject,
        TabDragHovered(arg0: DocumentId) => Reject,
        TabDragLeft(arg0: DocumentId) => Reject,
        TabDragReleased(arg0: DocumentId) => Reject,
        NewFile => Reject,
        OpenFile => Reject,
        FileDropped(arg0: window::Id, arg1: PathBuf) => Reject,
        FilePicked(arg0: FileResult<PathBuf>) => Defer,
        FileOpened(arg0: FileOpenResult) => Defer,
        FileLoadProgress(arg0: FileLoadProgress) => Defer,
        FileLoadChunk(arg0: FileLoadChunk) => Defer,
        FileLoadFinished(arg0: FileLoadResult) => Defer,
        SaveFile => Reject,
        SaveAllFiles => Reject,
        SaveFileAs => Reject,
        SaveCopyAs => Reject,
        FileSaved(arg0: SaveRequest, arg1: FileSaveResult) => Defer,
        FileCopySaved(arg0: SaveRequest, arg1: FileSaveResult) => Defer,
        ReloadFromDisk => Reject,
        EncodingSelected(arg0: TextEncoding) => Reject,
        EncodingConverted(arg0: TextEncoding) => Reject,
        CloseFile => Reject,
        CloseAllFiles => Reject,
        CloseAllButActiveFile => Reject,
        CloseAllButPinnedFiles => Reject,
        CloseAllToLeft => Reject,
        CloseAllToRight => Reject,
        CloseAllUnchanged => Reject,
        DirtyCloseResolved(arg0: DocumentId, arg1: DirtyCloseDecision) => Reject,
        DirtyCloseFadeFinished(arg0: DocumentId) => Defer,
    }
    Search(SearchMessage) {
        AdvancedResultOptionsToggled => Reject,
        AdvancedResultLimitChanged(arg0: String) => Reject,
        AdvancedPreviewCharsChanged(arg0: String) => Reject,
        AdvancedPreviewContextChanged(arg0: String) => Reject,
        AdvancedResultOptionsReset => Reject,
        FindQueryChanged(arg0: String) => Reject,
        FindReplacementChanged(arg0: String) => Reject,
        FindCaseSensitiveToggled(arg0: bool) => Reject,
        FindWholeWordToggled(arg0: bool) => Reject,
        FindModeSelected(arg0: SearchMode) => Reject,
        FindWrapAroundToggled(arg0: bool) => Reject,
        ToggleInlineReplace => Reject,
        ShowInlineReplace => Reject,
        ToggleFind => Reject,
        HideFind => Reject,
        FindNext => Reject,
        FindPrevious => Reject,
        SelectAndFindNext => Reject,
        SelectAndFindPrevious => Reject,
        VolatileFindNext => Reject,
        VolatileFindPrevious => Reject,
        ReplaceCurrent => Reject,
        ReplaceAll => Reject,
        ToggleAdvancedSearch(arg0: AdvancedSearchTab) => Reject,
        AdvancedSearchTabSelected(arg0: AdvancedSearchTab) => Reject,
        AdvancedSearchQueryChanged(arg0: String) => Reject,
        AdvancedSearchReplacementChanged(arg0: String) => Reject,
        AdvancedSearchCaseSensitiveToggled(arg0: bool) => Reject,
        AdvancedSearchWholeWordToggled(arg0: bool) => Reject,
        AdvancedSearchWrapAroundToggled(arg0: bool) => Reject,
        AdvancedSearchModeSelected(arg0: SearchMode) => Reject,
        AdvancedSearchIncludeChanged(arg0: String) => Reject,
        AdvancedSearchRun => Reject,
        AdvancedCountRun => Reject,
        AdvancedFindNextRun => Reject,
        AdvancedFindPreviousRun => Reject,
        AdvancedSearchPreviewDue(arg0: u64) => Defer,
        AdvancedFindAllCurrentRun => Reject,
        AdvancedFindAllOpenRun => Reject,
        AdvancedReplaceRun => Reject,
        AdvancedReplaceAllRun => Reject,
        AdvancedReplaceAllCurrentRun => Reject,
        AdvancedReplaceAllOpenRun => Reject,
        AdvancedSearchResultSelected(arg0: DocumentId, arg1: EditorSelection) => Reject,
        AdvancedSearchClosed => Reject,
    }
    Window(WindowMessage) {
        WindowOpened(arg0: window::Id) => Defer,
        WindowMaximized(arg0: window::Id, arg1: bool) => Defer,
        WindowChrome(arg0: window::Id, arg1: crate::ui::title_bar::Action) => Reject,
        WindowCloseRequested(arg0: window::Id) => Reject,
        WindowClosed(arg0: window::Id) => Defer,
    }
    Menu(MenuMessage) {
        MenuToggled(arg0: Menu) => Reject,
        MenuHovered(arg0: Menu) => Reject,
        MenuPathHovered(arg0: MenuPath) => Reject,
        MenuClosed => Reject,
    }
    GoToLine(GoToLineMessage) {
        GoToLineOpened => Reject,
        GoToLineChanged(arg0: String) => Reject,
        GoToLineSubmitted => Reject,
        GoToLineClosed => Reject,
    }
    History(HistoryMessage) {
        Undo => Reject,
        Redo => Reject,
    }
    Application(ApplicationMessage) {
        None => Reject,
        OpenPaths(arg0: Vec<PathBuf>) => Reject,
        ForwardedFiles(arg0: Vec<PathBuf>, arg1: ActivationRequest, arg2: crate::ipc::AdmissionReceipt) => Reject,
        StartupReady => Defer,
        StartupFrameReady => Defer,
        SessionLoaded(arg0: Result<Option<crate::core::session::Session>, String>) => Defer,
        SessionFlush => Defer,
        SessionPersisted(arg0: Result<(), String>) => Defer,
        ShutdownPersisted(arg0: Result<(), String>) => Resolve,
        SettingsFlush => Defer,
        RefreshLoadingFind => Defer,
        DocumentAnalyzed(arg0: crate::core::document::DocumentAnalysis) => Defer,
        SyntaxParsed(arg0: u64, arg1: Result<crate::editor::render::SyntaxParseResult, String>) => Defer,
        Shortcut(arg0: ShortcutCommand) => Reject,
        EditorAction(arg0: DocumentId, arg1: EditorAction) => Reject,
        OutlineParseCompleted(arg0: OutlineParseResult) => Defer,
        ClipboardRead(arg0: PasteRequest, arg1: ClipboardReadResult) => Defer,
        ClipboardWritten(arg0: ClipboardWriteResult) => Defer,
        BackendBoostRequested => Reject,
        BackendBoostConfigured(arg0: iced::backend::StrictHandoffOutcome) => Defer,
        ChromeAnimationFrame(arg0: Instant) => Defer,
        RuntimeEvent(arg0: event::Event, arg1: event::Status, arg2: window::Id) => Reject,
        ToggleFunctionList => Reject,
        FunctionListQueryChanged(arg0: String) => Reject,
        FunctionListEntrySelected(arg0: EditorPosition) => Reject,
        FoldCurrent => Reject,
        UnfoldCurrent => Reject,
        ToggleCurrentFold => Reject,
        FoldAll => Reject,
        UnfoldAll => Reject,
        GoToMatchingDelimiter => Reject,
        SelectMatchingDelimiter => Reject,
        NextFunction => Reject,
        PreviousFunction => Reject,
        SelectCurrentFunction => Reject,
        SelectCurrentFunctionBody => Reject,
        Uppercase => Reject,
        Lowercase => Reject,
        TrimTrailingSpaces => Reject,
        JoinLines => Reject,
        Cut => Reject,
        Copy => Reject,
        Paste => Reject,
        Delete => Reject,
        AboutOpened => Reject,
        AboutTabSelected(arg0: AboutTab) => Reject,
        AboutClosed => Reject,
        WindowListOpened => Reject,
        WindowListClosed => Reject,
        WindowFocusRequested(arg0: WindowTarget) => Reject,
        WindowFocusNext => Reject,
        WindowFocusPrevious => Reject,
        #[cfg(debug_assertions)]
        ToggleTitleBarStyle => Reject,
        SingleInstanceShowRequested(arg0: ActivationRequest) => Reject,
        LanguageSelected(arg0: String) => Reject,
    }
}

pub type ClipboardReadResult = Result<Arc<String>, iced::clipboard::Error>;
pub type ClipboardWriteResult = Result<(), iced::clipboard::Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardMode {
    Linear,
    Rectangular { line_count: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteRequest {
    pub document_id: DocumentId,
    pub revision: u64,
    pub selection: EditorSelection,
    pub selection_set: SelectionSet,
    pub clipboard_mode: ClipboardMode,
}

#[derive(Debug, Clone)]
pub struct SaveRequest {
    pub document_id: DocumentId,
    pub revision: u64,
    pub snapshot: Arc<Vec<u8>>,
}

impl From<FileLoadEvent> for Message {
    fn from(event: FileLoadEvent) -> Self {
        match event {
            FileLoadEvent::Progress(progress) => Self::FileLoadProgress(progress),
            FileLoadEvent::Chunk(chunk) => Self::FileLoadChunk(chunk),
            FileLoadEvent::Finished(result) => Self::FileLoadFinished(result),
        }
    }
}
