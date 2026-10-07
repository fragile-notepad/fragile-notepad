use super::fold::{FoldModel, FoldRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecorationSettings {
    pub show_line_numbers: bool,
    pub show_spaces: bool,
    pub show_tabs: bool,
    pub show_end_of_line_markers: bool,
    pub show_indentation_guides: bool,
    pub show_wrap_indicator: bool,
    pub show_wrap_guide: bool,
    pub show_folding_controls: bool,
    pub indent_width: usize,
}

impl Default for DecorationSettings {
    fn default() -> Self {
        Self {
            show_line_numbers: true,
            show_spaces: false,
            show_tabs: false,
            show_end_of_line_markers: false,
            show_indentation_guides: true,
            show_wrap_indicator: true,
            show_wrap_guide: true,
            show_folding_controls: true,
            indent_width: 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HiddenLineSpan {
    pub header_line: usize,
    pub first_hidden_line: usize,
    pub last_hidden_line: usize,
}

impl HiddenLineSpan {
    pub fn from_fold(range: FoldRange) -> Option<Self> {
        range.is_foldable().then_some(Self {
            header_line: range.start_line,
            first_hidden_line: range.start_line + 1,
            last_hidden_line: range.end_line,
        })
    }

    pub fn hidden_line_count(self) -> usize {
        self.last_hidden_line - self.first_hidden_line + 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndentGuide {
    pub line: usize,
    pub depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineDecoration {
    pub line: usize,
    pub line_number: Option<usize>,
    pub fold_range: Option<FoldRange>,
    pub has_fold_control: bool,
    pub is_fold_collapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecorationModel {
    pub settings: DecorationSettings,
    pub hidden_line_spans: Vec<HiddenLineSpan>,
    pub indent_guides: Vec<IndentGuide>,
    pub line_decorations: Vec<LineDecoration>,
}

impl DecorationModel {
    pub fn new(settings: DecorationSettings) -> Self {
        Self {
            settings,
            hidden_line_spans: Vec::new(),
            indent_guides: Vec::new(),
            line_decorations: Vec::new(),
        }
    }

    pub fn from_folds(
        settings: DecorationSettings,
        line_count: usize,
        folds: &FoldModel,
        indent_guides: Vec<IndentGuide>,
    ) -> Self {
        let hidden_line_spans = folds
            .collapsed_ranges()
            .copied()
            .filter_map(HiddenLineSpan::from_fold)
            .collect();
        let mut fold_ranges_by_start = vec![None; line_count];
        let priority = |range: FoldRange| (folds.is_collapsed(range), range.end_line);
        for range in folds.ranges().iter().copied() {
            if let Some(entry) = fold_ranges_by_start.get_mut(range.start_line)
                && entry.is_none_or(|current| priority(range) > priority(current))
            {
                *entry = Some(range);
            }
        }

        let line_decorations = (0..line_count)
            .map(|line| {
                let fold_range = fold_ranges_by_start[line];

                LineDecoration {
                    line,
                    line_number: settings.show_line_numbers.then_some(line + 1),
                    fold_range,
                    has_fold_control: settings.show_folding_controls && fold_range.is_some(),
                    is_fold_collapsed: fold_range.is_some_and(|range| folds.is_collapsed(range)),
                }
            })
            .collect();

        Self {
            settings,
            hidden_line_spans,
            indent_guides,
            line_decorations,
        }
    }

    /// Synchronizes the inexpensive decorations used by a loading document.
    /// Full fold and indentation metadata is computed after loading finishes.
    pub fn sync_loading_line_count(&mut self, line_count: usize) {
        self.hidden_line_spans.clear();
        self.indent_guides.clear();

        if line_count < self.line_decorations.len() {
            self.line_decorations.truncate(line_count);
        }

        for line in self.line_decorations.len()..line_count {
            self.line_decorations.push(LineDecoration {
                line,
                line_number: self.settings.show_line_numbers.then_some(line + 1),
                fold_range: None,
                has_fold_control: false,
                is_fold_collapsed: false,
            });
        }
    }
}
