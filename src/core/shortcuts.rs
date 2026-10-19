use iced::keyboard::{self, key};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ShortcutCommand {
    NewFile,
    NewWindow,
    CloseFile,
    CloseWindow,
    Quit,
    OpenFile,
    SaveFile,
    SaveFileAs,
    ToggleFind,
    AdvancedFind,
    AdvancedReplace,
    GoToLine,
    Cut,
    Copy,
    Paste,
    Undo,
    Redo,
    SelectAll,
    AddCaretAbove,
    AddCaretBelow,
    SplitSelectionIntoLines,
    ConvertSelectionToRectangle,
    DuplicateLine,
    DeleteLine,
    CopyLine,
    CutLine,
    FoldCurrent,
    UnfoldCurrent,
    ToggleCurrentFold,
    FoldAll,
    UnfoldAll,
    GoToMatchingDelimiter,
    SelectMatchingDelimiter,
    NextFunction,
    PreviousFunction,
    SelectCurrentFunction,
    SelectCurrentFunctionBody,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Indent,
    Unindent,
}

impl ShortcutCommand {
    pub const ALL: [Self; if cfg!(target_os = "macos") { 42 } else { 40 }] = [
        Self::NewFile,
        Self::NewWindow,
        #[cfg(target_os = "macos")]
        Self::CloseFile,
        #[cfg(target_os = "macos")]
        Self::CloseWindow,
        Self::Quit,
        Self::OpenFile,
        Self::SaveFile,
        Self::SaveFileAs,
        Self::ToggleFind,
        Self::AdvancedFind,
        Self::AdvancedReplace,
        Self::GoToLine,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::Undo,
        Self::Redo,
        Self::SelectAll,
        Self::AddCaretAbove,
        Self::AddCaretBelow,
        Self::SplitSelectionIntoLines,
        Self::ConvertSelectionToRectangle,
        Self::DuplicateLine,
        Self::DeleteLine,
        Self::CopyLine,
        Self::CutLine,
        Self::FoldCurrent,
        Self::UnfoldCurrent,
        Self::ToggleCurrentFold,
        Self::FoldAll,
        Self::UnfoldAll,
        Self::GoToMatchingDelimiter,
        Self::SelectMatchingDelimiter,
        Self::NextFunction,
        Self::PreviousFunction,
        Self::SelectCurrentFunction,
        Self::SelectCurrentFunctionBody,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::ZoomReset,
        Self::Indent,
        Self::Unindent,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::NewFile => "new_file",
            Self::NewWindow => "new_window",
            Self::CloseFile => "close_file",
            Self::CloseWindow => "close_window",
            Self::Quit => "quit",
            Self::OpenFile => "open_file",
            Self::SaveFile => "save_file",
            Self::SaveFileAs => "save_file_as",
            Self::ToggleFind => "toggle_find",
            Self::AdvancedFind => "advanced_find",
            Self::AdvancedReplace => "advanced_replace",
            Self::GoToLine => "go_to_line",
            Self::Cut => "cut",
            Self::Copy => "copy",
            Self::Paste => "paste",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::SelectAll => "select_all",
            Self::AddCaretAbove => "add_caret_above",
            Self::AddCaretBelow => "add_caret_below",
            Self::SplitSelectionIntoLines => "split_selection_into_lines",
            Self::ConvertSelectionToRectangle => "convert_selection_to_rectangle",
            Self::DuplicateLine => "duplicate_line",
            Self::DeleteLine => "delete_line",
            Self::CopyLine => "copy_line",
            Self::CutLine => "cut_line",
            Self::FoldCurrent => "fold_current",
            Self::UnfoldCurrent => "unfold_current",
            Self::ToggleCurrentFold => "toggle_current_fold",
            Self::FoldAll => "fold_all",
            Self::UnfoldAll => "unfold_all",
            Self::GoToMatchingDelimiter => "go_to_matching_delimiter",
            Self::SelectMatchingDelimiter => "select_matching_delimiter",
            Self::NextFunction => "next_function",
            Self::PreviousFunction => "previous_function",
            Self::SelectCurrentFunction => "select_current_function",
            Self::SelectCurrentFunctionBody => "select_current_function_body",
            Self::ZoomIn => "zoom_in",
            Self::ZoomOut => "zoom_out",
            Self::ZoomReset => "zoom_reset",
            Self::Indent => "indent",
            Self::Unindent => "unindent",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::NewFile => "New file",
            Self::NewWindow => "New window",
            Self::CloseFile => "Close file",
            Self::CloseWindow => "Close window",
            Self::Quit => "Quit",
            Self::OpenFile => "Open file",
            Self::SaveFile => "Save file",
            Self::SaveFileAs => "Save file as",
            Self::ToggleFind => "Find",
            Self::AdvancedFind => "Advanced find",
            Self::AdvancedReplace => "Advanced replace",
            Self::GoToLine => "Go to line",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::SelectAll => "Select all",
            Self::AddCaretAbove => "Add caret above",
            Self::AddCaretBelow => "Add caret below",
            Self::SplitSelectionIntoLines => "Split selection into lines",
            Self::ConvertSelectionToRectangle => "Convert selection to rectangle",
            Self::DuplicateLine => "Duplicate line",
            Self::DeleteLine => "Delete line",
            Self::CopyLine => "Copy line",
            Self::CutLine => "Cut line",
            Self::FoldCurrent => "Fold current",
            Self::UnfoldCurrent => "Unfold current",
            Self::ToggleCurrentFold => "Toggle current fold",
            Self::FoldAll => "Fold all",
            Self::UnfoldAll => "Unfold all",
            Self::GoToMatchingDelimiter => "Go to matching delimiter",
            Self::SelectMatchingDelimiter => "Select matching delimiter",
            Self::NextFunction => "Next function",
            Self::PreviousFunction => "Previous function",
            Self::SelectCurrentFunction => "Select current function",
            Self::SelectCurrentFunctionBody => "Select current function body",
            Self::ZoomIn => "Zoom in",
            Self::ZoomOut => "Zoom out",
            Self::ZoomReset => "Reset zoom",
            Self::Indent => "Indent",
            Self::Unindent => "Unindent",
        }
    }

    pub const fn group(self) -> ShortcutGroup {
        match self {
            Self::NewFile
            | Self::NewWindow
            | Self::CloseFile
            | Self::CloseWindow
            | Self::Quit
            | Self::OpenFile
            | Self::SaveFile
            | Self::SaveFileAs => ShortcutGroup::File,
            Self::ToggleFind
            | Self::AdvancedFind
            | Self::AdvancedReplace
            | Self::GoToLine
            | Self::GoToMatchingDelimiter
            | Self::SelectMatchingDelimiter
            | Self::NextFunction
            | Self::PreviousFunction
            | Self::SelectCurrentFunction
            | Self::SelectCurrentFunctionBody => ShortcutGroup::Search,
            Self::FoldCurrent
            | Self::UnfoldCurrent
            | Self::ToggleCurrentFold
            | Self::FoldAll
            | Self::UnfoldAll
            | Self::ZoomIn
            | Self::ZoomOut
            | Self::ZoomReset => ShortcutGroup::View,
            _ => ShortcutGroup::Edit,
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|command| command.key() == key)
    }

    fn default_binding(self) -> Option<KeyBinding> {
        match self {
            Self::NewFile => Some(KeyBinding::primary(ShortcutKey::character('n'))),
            Self::NewWindow => Some(KeyBinding::primary_shift(ShortcutKey::character('n'))),
            Self::CloseFile => {
                cfg!(target_os = "macos").then(|| KeyBinding::primary(ShortcutKey::character('w')))
            }
            Self::CloseWindow => cfg!(target_os = "macos")
                .then(|| KeyBinding::primary_shift(ShortcutKey::character('w'))),
            Self::Quit => Some(KeyBinding::primary(ShortcutKey::character('q'))),
            Self::OpenFile => Some(KeyBinding::primary(ShortcutKey::character('o'))),
            Self::SaveFile => Some(KeyBinding::primary(ShortcutKey::character('s'))),
            Self::SaveFileAs => Some(KeyBinding::primary_shift(ShortcutKey::character('s'))),
            Self::ToggleFind => Some(KeyBinding::primary(ShortcutKey::character('f'))),
            Self::AdvancedFind => Some(KeyBinding::new(
                ShortcutModifiers::primary_alt_shift(),
                ShortcutKey::character('f'),
            )),
            Self::AdvancedReplace => Some(KeyBinding::primary(ShortcutKey::character('h'))),
            Self::GoToLine => Some(KeyBinding::primary(ShortcutKey::character('g'))),
            Self::Cut => Some(KeyBinding::primary(ShortcutKey::character('x'))),
            Self::Copy => Some(KeyBinding::primary(ShortcutKey::character('c'))),
            Self::Paste => Some(KeyBinding::primary(ShortcutKey::character('v'))),
            Self::Undo => Some(KeyBinding::primary(ShortcutKey::character('z'))),
            Self::Redo => Some(KeyBinding::primary(ShortcutKey::character('y'))),
            Self::SelectAll => Some(KeyBinding::primary(ShortcutKey::character('a'))),
            Self::AddCaretAbove => Some(KeyBinding::new(
                ShortcutModifiers::primary_alt(),
                ShortcutKey::Named(NamedShortcutKey::ArrowUp),
            )),
            Self::AddCaretBelow => Some(KeyBinding::new(
                ShortcutModifiers::primary_alt(),
                ShortcutKey::Named(NamedShortcutKey::ArrowDown),
            )),
            Self::SplitSelectionIntoLines => {
                Some(KeyBinding::primary_shift(ShortcutKey::character('l')))
            }
            Self::ConvertSelectionToRectangle => Some(KeyBinding::new(
                ShortcutModifiers::primary_alt_shift(),
                ShortcutKey::character('r'),
            )),
            Self::DuplicateLine => Some(KeyBinding::primary(ShortcutKey::character('d'))),
            Self::DeleteLine => None,
            Self::CopyLine => None,
            Self::CutLine => None,
            Self::FoldCurrent => Some(KeyBinding::new(
                ShortcutModifiers::primary_alt(),
                ShortcutKey::character('f'),
            )),
            Self::UnfoldCurrent => Some(KeyBinding::new(
                ShortcutModifiers::primary_alt_shift(),
                ShortcutKey::character('u'),
            )),
            Self::ToggleCurrentFold => None,
            Self::FoldAll => Some(KeyBinding::new(
                ShortcutModifiers::alt(),
                ShortcutKey::character('0'),
            )),
            Self::UnfoldAll => Some(KeyBinding::new(
                ShortcutModifiers::alt_shift(),
                ShortcutKey::character('0'),
            )),
            Self::GoToMatchingDelimiter => Some(KeyBinding::primary(ShortcutKey::character('b'))),
            Self::SelectMatchingDelimiter => {
                Some(KeyBinding::primary_shift(ShortcutKey::character('b')))
            }
            Self::NextFunction
            | Self::PreviousFunction
            | Self::SelectCurrentFunction
            | Self::SelectCurrentFunctionBody => None,
            Self::ZoomIn => Some(KeyBinding::primary(ShortcutKey::Named(
                NamedShortcutKey::Plus,
            ))),
            Self::ZoomOut => Some(KeyBinding::primary(ShortcutKey::Named(
                NamedShortcutKey::Minus,
            ))),
            Self::ZoomReset => Some(KeyBinding::primary(ShortcutKey::character('0'))),
            Self::Indent => Some(KeyBinding::new(
                ShortcutModifiers::none(),
                ShortcutKey::Named(NamedShortcutKey::Tab),
            )),
            Self::Unindent => Some(KeyBinding::new(
                ShortcutModifiers::shift(),
                ShortcutKey::Named(NamedShortcutKey::Tab),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutGroup {
    File,
    Edit,
    Search,
    View,
}

impl ShortcutGroup {
    pub const ALL: [Self; 4] = [Self::File, Self::Edit, Self::Search, Self::View];

    pub const fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::Search => "Search",
            Self::View => "View",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutMap {
    entries: Vec<ShortcutEntry>,
}

impl ShortcutMap {
    /// Restore explicit choices before applying defaults introduced by upgrades.
    /// An explicit `None` keeps a shortcut unassigned; duplicate entries retain
    /// the first persisted choice.
    pub(super) fn from_persisted(
        assignments: impl IntoIterator<Item = (ShortcutCommand, Option<KeyBinding>)>,
    ) -> Self {
        let assignments: Vec<_> = assignments.into_iter().collect();
        let mut shortcuts = Self::with_defaults();
        for entry in &mut shortcuts.entries {
            if let Some((_, binding)) = assignments
                .iter()
                .find(|(command, _)| *command == entry.command)
            {
                entry.binding = *binding;
            }
        }
        shortcuts.preserve_existing_bindings(&assignments);
        shortcuts
    }

    pub fn with_defaults() -> Self {
        Self {
            entries: ShortcutCommand::ALL
                .iter()
                .copied()
                .map(|command| ShortcutEntry {
                    command,
                    binding: command.default_binding(),
                })
                .collect(),
        }
    }

    pub fn binding_display(&self, command: ShortcutCommand) -> String {
        self.binding(command)
            .map(KeyBinding::display)
            .unwrap_or_else(|| String::from("Unassigned"))
    }

    pub fn binding(&self, command: ShortcutCommand) -> Option<KeyBinding> {
        self.entries
            .iter()
            .find(|entry| entry.command == command)
            .and_then(|entry| entry.binding)
    }

    pub fn set_binding(
        &mut self,
        command: ShortcutCommand,
        binding: KeyBinding,
    ) -> Result<(), ShortcutConflict> {
        if let Some(conflict) = self.command_for_binding(binding)
            && conflict != command
        {
            return Err(ShortcutConflict {
                binding,
                command: conflict,
            });
        }

        if let Some(entry) = self.entry_mut(command) {
            entry.binding = Some(binding);
        }

        Ok(())
    }

    pub fn set_unchecked(&mut self, command: ShortcutCommand, binding: Option<KeyBinding>) {
        if let Some(entry) = self.entry_mut(command) {
            entry.binding = binding;
        }
    }

    pub fn clear(&mut self, command: ShortcutCommand) {
        if let Some(entry) = self.entry_mut(command) {
            entry.binding = None;
        }
    }

    pub fn reset_to_defaults(&mut self) {
        *self = Self::with_defaults();
    }

    pub fn command_for_binding(&self, binding: KeyBinding) -> Option<ShortcutCommand> {
        self.entries.iter().find_map(|entry| {
            entry
                .binding
                .is_some_and(|candidate| candidate.matches(binding))
                .then_some(entry.command)
        })
    }

    pub fn resolve(
        &self,
        key: &keyboard::Key,
        modified_key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
    ) -> Option<ShortcutCommand> {
        KeyBinding::from_event(key, modifiers)
            .and_then(|binding| self.command_for_binding(binding))
            .or_else(|| {
                KeyBinding::from_event(modified_key, modifiers)
                    .and_then(|binding| self.command_for_binding(binding))
            })
    }

    pub fn entries(&self) -> &[ShortcutEntry] {
        &self.entries
    }

    fn preserve_existing_bindings(
        &mut self,
        assignments: &[(ShortcutCommand, Option<KeyBinding>)],
    ) {
        // These defaults were added after settings with custom bindings had
        // shipped. Their defaults must not take another command's binding.
        for command in [
            ShortcutCommand::NewWindow,
            ShortcutCommand::Quit,
            ShortcutCommand::CloseFile,
            ShortcutCommand::CloseWindow,
        ] {
            if assignments.iter().any(|(assigned, _)| *assigned == command) {
                continue;
            }
            if let Some(binding) = self.binding(command)
                && self.entries.iter().any(|entry| {
                    entry.command != command
                        && entry
                            .binding
                            .is_some_and(|candidate| candidate.matches(binding))
                })
            {
                self.clear(command);
            }
        }
    }

    fn entry_mut(&mut self, command: ShortcutCommand) -> Option<&mut ShortcutEntry> {
        self.entries
            .iter_mut()
            .find(|entry| entry.command == command)
    }
}

impl Default for ShortcutMap {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutEntry {
    pub command: ShortcutCommand,
    pub binding: Option<KeyBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortcutConflict {
    pub binding: KeyBinding,
    pub command: ShortcutCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub modifiers: ShortcutModifiers,
    pub key: ShortcutKey,
}

impl KeyBinding {
    pub const fn new(modifiers: ShortcutModifiers, key: ShortcutKey) -> Self {
        Self { modifiers, key }
    }

    pub const fn primary(key: ShortcutKey) -> Self {
        Self::new(ShortcutModifiers::primary(), key)
    }

    pub const fn primary_shift(key: ShortcutKey) -> Self {
        Self::new(ShortcutModifiers::primary_shift(), key)
    }

    pub fn from_event(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Option<Self> {
        let shortcut_key = ShortcutKey::from_iced_key(key)?;

        Some(Self {
            modifiers: ShortcutModifiers::from_iced(modifiers),
            key: shortcut_key,
        })
    }

    pub fn parse(value: &str) -> Option<Self> {
        let mut modifiers = ShortcutModifiers::none();
        let mut key = None;

        for raw_part in value.split('+') {
            let part = raw_part.trim().to_ascii_lowercase();
            if part.is_empty() {
                return None;
            }

            match part.as_str() {
                "primary" | "cmd" | "command" => modifiers.primary = true,
                "ctrl" | "control" => modifiers.ctrl = true,
                "shift" => modifiers.shift = true,
                "alt" | "option" => modifiers.alt = true,
                "logo" | "super" | "win" => modifiers.logo = true,
                _ => {
                    if key.is_some() {
                        return None;
                    }
                    key = ShortcutKey::parse(&part);
                }
            }
        }

        Some(Self {
            modifiers,
            key: key?,
        })
    }

    pub fn persisted(self) -> String {
        let mut parts = self.modifiers.persisted_parts();
        parts.push(self.key.persisted());

        parts.join("+")
    }

    pub fn display(self) -> String {
        let mut parts = self.modifiers.text_parts();
        parts.push(self.key.display());

        parts.join("+")
    }

    pub fn display_parts(self) -> ShortcutDisplay {
        ShortcutDisplay {
            modifiers: self.modifiers.modifier_display_parts(),
            key: self.key.display(),
        }
    }

    pub(crate) fn matches(self, other: Self) -> bool {
        // A lookup can receive either a captured physical key or a persisted
        // binding. Resolve the symbolic primary modifier before comparing;
        // persisted primary+N has no Ctrl/Logo bit of its own.
        let mut other = other;
        if other.modifiers.primary {
            other.modifiers.primary = false;
            if cfg!(target_os = "macos") {
                other.modifiers.logo = true;
            } else {
                other.modifiers.ctrl = true;
            }
        }
        let key_matches = self.key == other.key
            || matches!(
                (self.key, other.key),
                (
                    ShortcutKey::Named(NamedShortcutKey::Plus),
                    ShortcutKey::Named(NamedShortcutKey::Equals)
                )
            );
        let modifiers_match = if self.key == ShortcutKey::Named(NamedShortcutKey::Plus)
            && self.modifiers.primary
            && !self.modifiers.shift
            && other.modifiers.shift
        {
            let mut without_shift = other.modifiers;
            without_shift.shift = false;
            self.modifiers.matches(without_shift)
        } else {
            self.modifiers.matches(other.modifiers)
        };

        key_matches && modifiers_match
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ShortcutModifiers {
    pub primary: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub logo: bool,
}

impl ShortcutModifiers {
    pub const fn none() -> Self {
        Self {
            primary: false,
            ctrl: false,
            shift: false,
            alt: false,
            logo: false,
        }
    }

    pub const fn primary() -> Self {
        Self {
            primary: true,
            ..Self::none()
        }
    }

    pub const fn primary_shift() -> Self {
        Self {
            primary: true,
            shift: true,
            ..Self::none()
        }
    }

    pub const fn primary_alt() -> Self {
        Self {
            primary: true,
            alt: true,
            ..Self::none()
        }
    }

    pub const fn primary_alt_shift() -> Self {
        Self {
            primary: true,
            shift: true,
            alt: true,
            ..Self::none()
        }
    }

    pub const fn shift() -> Self {
        Self {
            shift: true,
            ..Self::none()
        }
    }

    pub const fn alt() -> Self {
        Self {
            alt: true,
            ..Self::none()
        }
    }

    pub const fn alt_shift() -> Self {
        Self {
            shift: true,
            alt: true,
            ..Self::none()
        }
    }

    pub fn from_iced(modifiers: keyboard::Modifiers) -> Self {
        Self {
            primary: false,
            ctrl: modifiers.control(),
            shift: modifiers.shift(),
            alt: modifiers.alt(),
            logo: modifiers.logo(),
        }
    }

    fn matches(self, other: Self) -> bool {
        self.shift == other.shift
            && self.alt == other.alt
            && if self.primary {
                other.is_platform_primary()
            } else {
                self.ctrl == other.ctrl && self.logo == other.logo
            }
    }

    fn is_platform_primary(self) -> bool {
        if cfg!(target_os = "macos") {
            self.logo && !self.ctrl
        } else {
            self.ctrl && !self.logo
        }
    }

    fn persisted_parts(self) -> Vec<String> {
        let mut parts = Vec::new();

        if self.primary {
            parts.push(String::from("primary"));
        }
        if self.ctrl {
            parts.push(String::from("ctrl"));
        }
        if self.shift {
            parts.push(String::from("shift"));
        }
        if self.alt {
            parts.push(String::from("alt"));
        }
        if self.logo {
            parts.push(String::from("logo"));
        }

        parts
    }

    fn text_parts(self) -> Vec<String> {
        let mut parts = Vec::new();

        if self.primary {
            parts.push(if cfg!(target_os = "macos") {
                String::from("Cmd")
            } else {
                String::from("Ctrl")
            });
        }
        if self.ctrl {
            parts.push(String::from("Ctrl"));
        }
        if self.shift {
            parts.push(String::from("Shift"));
        }
        if self.alt {
            parts.push(if cfg!(target_os = "macos") {
                String::from("Option")
            } else {
                String::from("Alt")
            });
        }
        if self.logo {
            parts.push(if cfg!(target_os = "macos") {
                String::from("Cmd")
            } else {
                String::from("Win")
            });
        }

        parts
    }

    fn modifier_display_parts(self) -> Vec<ShortcutDisplayPart> {
        let mut parts = Vec::new();

        if self.primary {
            parts.push(if cfg!(target_os = "macos") {
                ShortcutDisplayPart::Icon(ShortcutModifierIcon::Command)
            } else {
                ShortcutDisplayPart::Text("Ctrl")
            });
        }
        if self.ctrl {
            parts.push(ShortcutDisplayPart::Text("Ctrl"));
        }
        if self.shift {
            parts.push(ShortcutDisplayPart::Icon(ShortcutModifierIcon::Shift));
        }
        if self.alt {
            parts.push(if cfg!(target_os = "macos") {
                ShortcutDisplayPart::Icon(ShortcutModifierIcon::Option)
            } else {
                ShortcutDisplayPart::Text("Alt")
            });
        }
        if self.logo {
            parts.push(if cfg!(target_os = "macos") {
                ShortcutDisplayPart::Icon(ShortcutModifierIcon::Command)
            } else {
                ShortcutDisplayPart::Icon(ShortcutModifierIcon::Windows)
            });
        }

        parts
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutDisplay {
    pub modifiers: Vec<ShortcutDisplayPart>,
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutDisplayPart {
    Text(&'static str),
    Icon(ShortcutModifierIcon),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutModifierIcon {
    Command,
    Option,
    Shift,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutKey {
    Character(char),
    Named(NamedShortcutKey),
}

impl ShortcutKey {
    pub const fn character(ch: char) -> Self {
        Self::Character(ch)
    }

    fn from_iced_key(key: &keyboard::Key) -> Option<Self> {
        match key.as_ref() {
            keyboard::Key::Character(value) => {
                let mut chars = value.chars();
                let ch = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                Some(match ch {
                    '+' => Self::Named(NamedShortcutKey::Plus),
                    '=' => Self::Named(NamedShortcutKey::Equals),
                    '-' => Self::Named(NamedShortcutKey::Minus),
                    _ => Self::Character(ch.to_ascii_lowercase()),
                })
            }
            keyboard::Key::Named(named) => NamedShortcutKey::from_iced(named).map(Self::Named),
            keyboard::Key::Unidentified => None,
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "plus" => Some(Self::Named(NamedShortcutKey::Plus)),
            "equals" => Some(Self::Named(NamedShortcutKey::Equals)),
            "minus" => Some(Self::Named(NamedShortcutKey::Minus)),
            "tab" => Some(Self::Named(NamedShortcutKey::Tab)),
            "enter" => Some(Self::Named(NamedShortcutKey::Enter)),
            "space" => Some(Self::Named(NamedShortcutKey::Space)),
            "backspace" => Some(Self::Named(NamedShortcutKey::Backspace)),
            "delete" => Some(Self::Named(NamedShortcutKey::Delete)),
            "home" => Some(Self::Named(NamedShortcutKey::Home)),
            "end" => Some(Self::Named(NamedShortcutKey::End)),
            "pageup" => Some(Self::Named(NamedShortcutKey::PageUp)),
            "pagedown" => Some(Self::Named(NamedShortcutKey::PageDown)),
            "left" => Some(Self::Named(NamedShortcutKey::ArrowLeft)),
            "right" => Some(Self::Named(NamedShortcutKey::ArrowRight)),
            "up" => Some(Self::Named(NamedShortcutKey::ArrowUp)),
            "down" => Some(Self::Named(NamedShortcutKey::ArrowDown)),
            "escape" | "esc" => Some(Self::Named(NamedShortcutKey::Escape)),
            "f1" => Some(Self::Named(NamedShortcutKey::F1)),
            "f2" => Some(Self::Named(NamedShortcutKey::F2)),
            "f3" => Some(Self::Named(NamedShortcutKey::F3)),
            "f4" => Some(Self::Named(NamedShortcutKey::F4)),
            "f5" => Some(Self::Named(NamedShortcutKey::F5)),
            "f6" => Some(Self::Named(NamedShortcutKey::F6)),
            "f7" => Some(Self::Named(NamedShortcutKey::F7)),
            "f8" => Some(Self::Named(NamedShortcutKey::F8)),
            "f9" => Some(Self::Named(NamedShortcutKey::F9)),
            "f10" => Some(Self::Named(NamedShortcutKey::F10)),
            "f11" => Some(Self::Named(NamedShortcutKey::F11)),
            "f12" => Some(Self::Named(NamedShortcutKey::F12)),
            _ => {
                let mut chars = value.chars();
                let ch = chars.next()?;
                chars.next().is_none().then_some(Self::Character(ch))
            }
        }
    }

    fn persisted(self) -> String {
        match self {
            Self::Character(ch) => ch.to_string(),
            Self::Named(named) => named.persisted().to_owned(),
        }
    }

    fn display(self) -> String {
        match self {
            Self::Character(ch) => ch.to_ascii_uppercase().to_string(),
            Self::Named(named) => named.display().to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedShortcutKey {
    Plus,
    Equals,
    Minus,
    Tab,
    Enter,
    Space,
    Backspace,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Escape,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

impl NamedShortcutKey {
    fn from_iced(named: key::Named) -> Option<Self> {
        match named {
            key::Named::Space => Some(Self::Space),
            key::Named::Tab => Some(Self::Tab),
            key::Named::Enter => Some(Self::Enter),
            key::Named::Backspace => Some(Self::Backspace),
            key::Named::Delete => Some(Self::Delete),
            key::Named::Home => Some(Self::Home),
            key::Named::End => Some(Self::End),
            key::Named::PageUp => Some(Self::PageUp),
            key::Named::PageDown => Some(Self::PageDown),
            key::Named::ArrowLeft => Some(Self::ArrowLeft),
            key::Named::ArrowRight => Some(Self::ArrowRight),
            key::Named::ArrowUp => Some(Self::ArrowUp),
            key::Named::ArrowDown => Some(Self::ArrowDown),
            key::Named::Escape => Some(Self::Escape),
            key::Named::F1 => Some(Self::F1),
            key::Named::F2 => Some(Self::F2),
            key::Named::F3 => Some(Self::F3),
            key::Named::F4 => Some(Self::F4),
            key::Named::F5 => Some(Self::F5),
            key::Named::F6 => Some(Self::F6),
            key::Named::F7 => Some(Self::F7),
            key::Named::F8 => Some(Self::F8),
            key::Named::F9 => Some(Self::F9),
            key::Named::F10 => Some(Self::F10),
            key::Named::F11 => Some(Self::F11),
            key::Named::F12 => Some(Self::F12),
            _ => None,
        }
    }

    fn persisted(self) -> &'static str {
        match self {
            Self::Plus => "plus",
            Self::Equals => "equals",
            Self::Minus => "minus",
            Self::Tab => "tab",
            Self::Enter => "enter",
            Self::Space => "space",
            Self::Backspace => "backspace",
            Self::Delete => "delete",
            Self::Home => "home",
            Self::End => "end",
            Self::PageUp => "pageup",
            Self::PageDown => "pagedown",
            Self::ArrowLeft => "left",
            Self::ArrowRight => "right",
            Self::ArrowUp => "up",
            Self::ArrowDown => "down",
            Self::Escape => "escape",
            Self::F1 => "f1",
            Self::F2 => "f2",
            Self::F3 => "f3",
            Self::F4 => "f4",
            Self::F5 => "f5",
            Self::F6 => "f6",
            Self::F7 => "f7",
            Self::F8 => "f8",
            Self::F9 => "f9",
            Self::F10 => "f10",
            Self::F11 => "f11",
            Self::F12 => "f12",
        }
    }

    fn display(self) -> &'static str {
        match self {
            Self::Plus => "+",
            Self::Equals => "=",
            Self::Minus => "-",
            Self::Tab => "Tab",
            Self::Enter => "Enter",
            Self::Space => "Space",
            Self::Backspace => "Backspace",
            Self::Delete => "Delete",
            Self::Home => "Home",
            Self::End => "End",
            Self::PageUp => "Page Up",
            Self::PageDown => "Page Down",
            Self::ArrowLeft => "Left",
            Self::ArrowRight => "Right",
            Self::ArrowUp => "Up",
            Self::ArrowDown => "Down",
            Self::Escape => "Esc",
            Self::F1 => "F1",
            Self::F2 => "F2",
            Self::F3 => "F3",
            Self::F4 => "F4",
            Self::F5 => "F5",
            Self::F6 => "F6",
            Self::F7 => "F7",
            Self::F8 => "F8",
            Self::F9 => "F9",
            Self::F10 => "F10",
            Self::F11 => "F11",
            Self::F12 => "F12",
        }
    }
}
