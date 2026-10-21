use iced::Element;

use super::vector::{self, VectorIcon, compiled};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColoredIcon {
    AllCharacters,
    Close,
    CloseAll,
    Copy,
    Cut,
    Delete,
    DocumentSaved,
    DocumentUnsaved,
    Find,
    FunctionList,
    IndentGuide,
    New,
    Open,
    Paste,
    Print,
    Redo,
    Replace,
    Save,
    SaveAll,
    TabClose,
    Undo,
    WordWrap,
    ZoomIn,
    ZoomOut,
}

pub fn icon<Message: 'static>(icon: ColoredIcon, size: u32) -> Element<'static, Message> {
    vector::icon(
        asset(icon),
        size,
        crate::ui::styles::shortcut_text_color,
        1.0,
    )
}

pub fn disabled_icon<Message: 'static>(icon: ColoredIcon, size: u32) -> Element<'static, Message> {
    vector::disabled_icon(
        asset(icon),
        size,
        crate::ui::styles::shortcut_text_color,
        crate::ui::styles::tool_bar_background,
        0.38,
    )
}

fn asset(icon: ColoredIcon) -> &'static VectorIcon {
    match icon {
        ColoredIcon::AllCharacters => &compiled::COLORED_ACCESSORIES_CHARACTER_MAP,
        ColoredIcon::Close => &compiled::COLORED_DOCUMENT_CLOSE,
        ColoredIcon::CloseAll => &compiled::COLORED_DOCUMENT_CLOSE_ALL,
        ColoredIcon::Delete => &compiled::COLORED_EDIT_DELETE,
        ColoredIcon::Copy => &compiled::COLORED_EDIT_COPY,
        ColoredIcon::Cut => &compiled::COLORED_EDIT_CUT,
        ColoredIcon::DocumentSaved => &compiled::COLORED_TAB_DOCUMENT_SAVED,
        ColoredIcon::DocumentUnsaved => &compiled::COLORED_TAB_DOCUMENT_UNSAVED,
        ColoredIcon::Find => &compiled::COLORED_EDIT_FIND,
        ColoredIcon::FunctionList => &compiled::COLORED_TEXT_X_SCRIPT,
        ColoredIcon::IndentGuide => &compiled::COLORED_FORMAT_INDENT_MORE,
        ColoredIcon::New => &compiled::COLORED_DOCUMENT_NEW,
        ColoredIcon::Open => &compiled::COLORED_DOCUMENT_OPEN,
        ColoredIcon::Paste => &compiled::COLORED_EDIT_PASTE,
        ColoredIcon::Print => &compiled::COLORED_DOCUMENT_PRINT,
        ColoredIcon::Redo => &compiled::COLORED_EDIT_REDO,
        ColoredIcon::Replace => &compiled::COLORED_EDIT_FIND_REPLACE,
        ColoredIcon::Save => &compiled::COLORED_DOCUMENT_SAVE,
        ColoredIcon::SaveAll => &compiled::COLORED_DOCUMENT_SAVE_ALL,
        ColoredIcon::TabClose => &compiled::COLORED_TAB_CLOSE,
        ColoredIcon::Undo => &compiled::COLORED_EDIT_UNDO,
        ColoredIcon::WordWrap => &compiled::COLORED_FORMAT_JUSTIFY_FILL,
        ColoredIcon::ZoomIn => &compiled::COLORED_ZOOM_IN,
        ColoredIcon::ZoomOut => &compiled::COLORED_ZOOM_OUT,
    }
}
