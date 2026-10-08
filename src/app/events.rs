//! Application events describe facts; work requests describe coalescible reactions.

use super::bus::BusEvent;
use crate::core::DocumentId;
use crate::core::workspace::changes::WorkspaceEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Event {
    Workspace(WorkspaceEvent),
    SettingsChanged,
    Started,
    SessionReady,
    SearchRequested,
    AnalysisCompleted(DocumentId),
    AnalysisAvailable,
    SyntaxAvailable,
}

impl BusEvent for Event {
    type Key = Self;
    fn coalescing_key(&self) -> Option<Self> {
        match self {
            Self::Workspace(
                WorkspaceEvent::DocumentOpened(_)
                | WorkspaceEvent::DocumentClosed(_)
                | WorkspaceEvent::ActiveDocumentChanged(_),
            )
            | Self::AnalysisCompleted(_) => None,
            _ => Some(*self),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Work {
    Files,
    Search,
    SearchPreview,
    Find,
    FindSelection,
    LoadingFind,
    Outline,
    Analysis,
    Syntax,
    Session,
}

#[derive(Debug, Default)]
pub(super) struct PendingWork(u16);

impl PendingWork {
    pub(super) fn request(&mut self, work: Work) {
        self.0 |= 1 << work as u8;
    }
    pub(super) fn take(&mut self, work: Work) -> bool {
        let bit = 1 << work as u8;
        let pending = self.0 & bit != 0;
        self.0 &= !bit;
        pending
    }
}
