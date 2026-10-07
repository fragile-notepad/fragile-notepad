use crate::core::Document;
use crate::editor::{EditorPosition, EditorSelection, FoldModel, FoldRange};

pub(in crate::app) fn toggle_fold(document: &mut Document, range: FoldRange) {
    let was_collapsed = document.folds.is_collapsed(range);

    if document.folds.toggle(range) {
        let before_selection = document.main_selection();
        if !was_collapsed && document.folds.is_collapsed(range) {
            document.set_main_selection(clamp_to_fold_header(
                before_selection,
                range,
                &document.folds,
            ));
        }
        if document.main_selection() != before_selection {
            document.preferred_vertical_column = None;
        }
        document.refresh_view_models();
    }
}

pub(in crate::app) fn set_current_fold_collapsed(document: &mut Document, collapsed: bool) {
    let Some(range) = document
        .folds
        .range_at_or_parent(document.main_selection().cursor.line)
    else {
        return;
    };

    set_fold_collapsed(document, range, collapsed);
}

pub(in crate::app) fn toggle_current_fold(document: &mut Document) {
    let Some(range) = document
        .folds
        .range_at_or_parent(document.main_selection().cursor.line)
    else {
        return;
    };

    toggle_fold(document, range);
}

fn set_fold_collapsed(document: &mut Document, range: FoldRange, collapsed: bool) {
    let was_collapsed = document.folds.is_collapsed(range);

    if document.folds.set_collapsed(range, collapsed) {
        let before_selection = document.main_selection();
        if !was_collapsed && document.folds.is_collapsed(range) {
            document.set_main_selection(clamp_to_fold_header(
                before_selection,
                range,
                &document.folds,
            ));
        }
        if document.main_selection() != before_selection {
            document.preferred_vertical_column = None;
        }
        document.refresh_view_models();
    }
}

pub(in crate::app) fn set_all_folds_collapsed(document: &mut Document, collapsed: bool) {
    if document.folds.set_all_collapsed(collapsed) {
        let before_selection = document.main_selection();
        if collapsed {
            document
                .set_main_selection(clamp_to_collapsed_header(before_selection, &document.folds));
        }
        if document.main_selection() != before_selection {
            document.preferred_vertical_column = None;
        }
        document.refresh_view_models();
    }
}

fn hidden_position(folds: &FoldModel, range: FoldRange, position: EditorPosition) -> bool {
    range.contains_hidden_line(position.line)
        && folds.delimiter(range).is_none_or(|delimiter| {
            position.line != range.end_line || position.column < delimiter.closing_column
        })
}

fn clamp_to_fold_header(
    selection: EditorSelection,
    range: FoldRange,
    folds: &FoldModel,
) -> EditorSelection {
    if !hidden_position(folds, range, selection.anchor)
        && !hidden_position(folds, range, selection.cursor)
    {
        return selection;
    }

    let header = EditorPosition::new(range.start_line, 0);
    EditorSelection::new(header, header)
}

fn clamp_to_collapsed_header(selection: EditorSelection, folds: &FoldModel) -> EditorSelection {
    let Some(range) = folds
        .collapsed_ranges()
        .copied()
        .filter(|range| {
            hidden_position(folds, *range, selection.cursor)
                || hidden_position(folds, *range, selection.anchor)
        })
        .min_by_key(|range| (range.start_line, std::cmp::Reverse(range.end_line)))
    else {
        return selection;
    };

    let header = EditorPosition::new(range.start_line, 0);
    EditorSelection::new(header, header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::DocumentId;

    #[test]
    fn collapsing_branches_keeps_selections_in_visible_closing_suffixes() {
        let source = "if check() {\n    abort();\n} else {\n    unwind();\n};";
        let mut document = Document::from_path(DocumentId::new(1), "main.rs", source);
        let suffix = EditorPosition::new(2, 4);
        let selection = EditorSelection::new(suffix, suffix);
        document.set_main_selection(selection);
        toggle_fold(&mut document, FoldRange::new(0, 2));
        assert_eq!(document.main_selection(), selection);
        document.ensure_caret_visible();
        assert!(document.folds.is_collapsed(FoldRange::new(0, 2)));

        let semicolon = EditorPosition::new(4, 2);
        let selection = EditorSelection::new(semicolon, semicolon);
        document.set_main_selection(selection);
        set_all_folds_collapsed(&mut document, true);
        assert_eq!(document.main_selection(), selection);
        document.ensure_caret_visible();
        assert!(document.folds.is_collapsed(FoldRange::new(0, 2)));
        assert!(document.folds.is_collapsed(FoldRange::new(2, 4)));
        assert_eq!(document.text(), source);
    }
}
