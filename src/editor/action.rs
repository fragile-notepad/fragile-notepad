//! Editor commands and navigation intent, independent of the editor widget.

use super::fold::FoldRange;
use super::position::{EditorPosition, EditorSelection, SelectionSet};
use crate::core::ShortcutCommand;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorAction {
    InsertText(String),
    InsertNewline,
    Backspace,
    Delete,
    MoveCaret(CaretMotion),
    Select(CaretMotion),
    SelectAll,
    ReplaceSelection(String),
    MoveSelection {
        source: SelectionSet,
        target: EditorPosition,
    },
    Indent,
    Unindent,
    DuplicateLine,
    DeleteLine,
    CopyLine,
    CutLine,
    Uppercase,
    Lowercase,
    TrimTrailingSpaces,
    JoinLines,
    ScrollLines(i32),
    ScrollToRow(usize),
    ViewportChanged {
        visible_rows: usize,
        text_width: u32,
        character_width_milli: u32,
        font_size_milli: u32,
        hint_factor_milli: Option<u32>,
    },
    ToggleFold(FoldRange),
    FoldCurrent,
    UnfoldCurrent,
    ToggleCurrentFold,
    FoldAll,
    UnfoldAll,
    GoToMatchingDelimiter,
    SelectMatchingDelimiter,
    SelectMatchingDelimiterInPlace,
    NextFunction,
    PreviousFunction,
    SelectCurrentFunction,
    SelectCurrentFunctionBody,
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    Focus,
    PlaceCaret(EditorPosition),
    PlaceCaretOnRow {
        position: EditorPosition,
        row: usize,
    },
    SelectWordAt(EditorPosition),
    SelectRegion(EditorSelection),
    SelectRegionOnRow {
        selection: EditorSelection,
        row: usize,
    },
    AddCaretAbove,
    AddCaretBelow,
    SplitSelectionIntoLines,
    ConvertSelectionToRectangle,
}

impl EditorAction {
    pub fn from_shortcut(command: ShortcutCommand) -> Option<Self> {
        shortcut_action(command)
    }

    pub fn mutates_document(&self) -> bool {
        matches!(
            self,
            Self::InsertText(_)
                | Self::InsertNewline
                | Self::Backspace
                | Self::Delete
                | Self::ReplaceSelection(_)
                | Self::MoveSelection { .. }
                | Self::Indent
                | Self::Unindent
                | Self::DuplicateLine
                | Self::DeleteLine
                | Self::CutLine
                | Self::Uppercase
                | Self::Lowercase
                | Self::TrimTrailingSpaces
                | Self::JoinLines
                | Self::Cut
                | Self::Paste
                | Self::Undo
                | Self::Redo
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaretMotion {
    Left,
    Right,
    WordLeft,
    WordRight,
    Up,
    Down,
    ParagraphUp,
    ParagraphDown,
    LineStart,
    LineEnd,
    PageUp,
    PageDown,
    DocumentStart,
    DocumentEnd,
}

pub fn shortcut_action(command: ShortcutCommand) -> Option<EditorAction> {
    match command {
        ShortcutCommand::Cut => Some(EditorAction::Cut),
        ShortcutCommand::Copy => Some(EditorAction::Copy),
        ShortcutCommand::Paste => Some(EditorAction::Paste),
        ShortcutCommand::Undo => Some(EditorAction::Undo),
        ShortcutCommand::Redo => Some(EditorAction::Redo),
        ShortcutCommand::SelectAll => Some(EditorAction::SelectAll),
        ShortcutCommand::DuplicateLine => Some(EditorAction::DuplicateLine),
        ShortcutCommand::DeleteLine => Some(EditorAction::DeleteLine),
        ShortcutCommand::CopyLine => Some(EditorAction::CopyLine),
        ShortcutCommand::CutLine => Some(EditorAction::CutLine),
        ShortcutCommand::FoldCurrent => Some(EditorAction::FoldCurrent),
        ShortcutCommand::UnfoldCurrent => Some(EditorAction::UnfoldCurrent),
        ShortcutCommand::ToggleCurrentFold => Some(EditorAction::ToggleCurrentFold),
        ShortcutCommand::FoldAll => Some(EditorAction::FoldAll),
        ShortcutCommand::UnfoldAll => Some(EditorAction::UnfoldAll),
        ShortcutCommand::GoToMatchingDelimiter => Some(EditorAction::GoToMatchingDelimiter),
        ShortcutCommand::SelectMatchingDelimiter => Some(EditorAction::SelectMatchingDelimiter),
        ShortcutCommand::NextFunction => Some(EditorAction::NextFunction),
        ShortcutCommand::PreviousFunction => Some(EditorAction::PreviousFunction),
        ShortcutCommand::SelectCurrentFunction => Some(EditorAction::SelectCurrentFunction),
        ShortcutCommand::SelectCurrentFunctionBody => Some(EditorAction::SelectCurrentFunctionBody),
        ShortcutCommand::AddCaretAbove => Some(EditorAction::AddCaretAbove),
        ShortcutCommand::AddCaretBelow => Some(EditorAction::AddCaretBelow),
        ShortcutCommand::SplitSelectionIntoLines => Some(EditorAction::SplitSelectionIntoLines),
        ShortcutCommand::ConvertSelectionToRectangle => {
            Some(EditorAction::ConvertSelectionToRectangle)
        }
        ShortcutCommand::Indent => Some(EditorAction::Indent),
        ShortcutCommand::Unindent => Some(EditorAction::Unindent),
        _ => None,
    }
}
