use crate::core::Document;
use crate::editor::layout::{byte_column_for, visual_column_for};
use crate::editor::{
    CaretMotion, DelimiterMatch, EditorPosition, EditorSelection, FunctionEntry,
    ProjectionFragment, containing_function, is_vertical_motion, matching_delimiter_near_caret,
    move_position, next_function_after, outline_for_syntax, position_for_byte_offset,
    previous_function_before, word_range_at_position,
};
use unicode_segmentation::UnicodeSegmentation;

pub(in crate::app) fn go_to_matching_delimiter(document: &mut Document) {
    let Some(delimiter_match) = delimiter_match_for_selection(document) else {
        return;
    };
    let text = document.buffer.text();
    let Some(position) = position_for_byte_offset(&text, delimiter_match.matching_delimiter) else {
        return;
    };

    document.set_main_selection(EditorSelection::new(position, position));
    document.preferred_vertical_column = None;
    document.reveal_position(position);
}

pub(in crate::app) fn select_matching_delimiter(document: &mut Document) {
    let Some(matching_position) = select_delimiter_range(document) else {
        return;
    };

    document.reveal_position(matching_position);
}

pub(in crate::app) fn select_delimiter_in_place(document: &mut Document) {
    let _ = select_delimiter_range(document);
}

pub(in crate::app) fn select_word_at(document: &mut Document, position: EditorPosition) {
    document.preferred_vertical_column = None;
    let position = document.buffer.clamp_position(position);

    let Some(range) = word_range_at_position(&document.buffer, position, &document.syntax_token)
    else {
        document.set_main_selection(EditorSelection::new(position, position));
        select_delimiter_in_place(document);
        return;
    };

    document.set_main_selection(EditorSelection::new(range.start, range.end));
}

pub(in crate::app) fn go_to_next_function(
    document: &mut Document,
    outline_entries: Option<&[FunctionEntry]>,
) {
    let fallback;
    let entries = match outline_entries {
        Some(entries) => entries,
        None => {
            fallback = outline_for_syntax(&document.buffer, &document.syntax_token);
            &fallback
        }
    };
    let Some(target) = next_function_after(entries, document.main_selection().cursor)
        .map(|entry| entry.range.start)
    else {
        return;
    };

    document.set_main_selection(EditorSelection::new(target, target));
    document.preferred_vertical_column = None;
    document.reveal_position(target);
}

pub(in crate::app) fn go_to_previous_function(
    document: &mut Document,
    outline_entries: Option<&[FunctionEntry]>,
) {
    let fallback;
    let entries = match outline_entries {
        Some(entries) => entries,
        None => {
            fallback = outline_for_syntax(&document.buffer, &document.syntax_token);
            &fallback
        }
    };
    let Some(target) = previous_function_before(entries, document.main_selection().cursor)
        .map(|entry| entry.range.start)
    else {
        return;
    };

    document.set_main_selection(EditorSelection::new(target, target));
    document.preferred_vertical_column = None;
    document.reveal_position(target);
}

pub(in crate::app) fn select_current_function(
    document: &mut Document,
    outline_entries: Option<&[FunctionEntry]>,
) {
    let fallback;
    let entries = match outline_entries {
        Some(entries) => entries,
        None => {
            fallback = outline_for_syntax(&document.buffer, &document.syntax_token);
            &fallback
        }
    };
    let Some(range) =
        containing_function(entries, document.main_selection().cursor).map(|entry| entry.range)
    else {
        return;
    };

    document.set_main_selection(EditorSelection::new(range.start, range.end));
    document.preferred_vertical_column = None;
    document.reveal_position(range.start);
}

pub(in crate::app) fn select_current_function_body(
    document: &mut Document,
    outline_entries: Option<&[FunctionEntry]>,
) {
    let fallback;
    let entries = match outline_entries {
        Some(entries) => entries,
        None => {
            fallback = outline_for_syntax(&document.buffer, &document.syntax_token);
            &fallback
        }
    };
    let Some(range) = containing_function(entries, document.main_selection().cursor)
        .and_then(|entry| entry.body_range)
    else {
        return;
    };

    document.set_main_selection(EditorSelection::new(range.start, range.end));
    document.preferred_vertical_column = None;
    document.reveal_position(range.start);
}

fn select_delimiter_range(document: &mut Document) -> Option<EditorPosition> {
    let delimiter_match = delimiter_match_for_selection(document)?;

    let start_offset = delimiter_match
        .delimiter
        .min(delimiter_match.matching_delimiter);
    let end_offset = delimiter_match
        .delimiter
        .max(delimiter_match.matching_delimiter)
        .saturating_add(1);
    let text = document.buffer.text();
    let start = position_for_byte_offset(&text, start_offset)?;
    let end = position_for_byte_offset(&text, end_offset)?;
    let matching_position = position_for_byte_offset(&text, delimiter_match.matching_delimiter)?;

    document.set_main_selection(EditorSelection::new(start, end));
    document.preferred_vertical_column = None;
    Some(matching_position)
}

fn delimiter_match_for_selection(document: &Document) -> Option<DelimiterMatch> {
    let caret_offset = document
        .buffer
        .byte_offset(document.main_selection().cursor);

    let text = document.buffer.text();
    matching_delimiter_near_caret(&text, caret_offset)
}

pub(in crate::app) fn move_document_position(
    document: &mut Document,
    position: EditorPosition,
    motion: CaretMotion,
) -> EditorPosition {
    let position = document.buffer.clamp_position(position);
    let row = document.position_visible_row(position).unwrap_or(0);
    let display = document
        .viewport
        .display_position(position)
        .unwrap_or(position);
    if !is_vertical_motion(motion) {
        document.preferred_vertical_column = None;
        document.clear_caret_row_affinity();
        if (document.word_wrap() || document.viewport.projection(display.line).is_some())
            && matches!(motion, CaretMotion::LineStart | CaretMotion::LineEnd)
            && let Some(segment) = document.viewport.row_segment(row, &document.buffer)
        {
            let target = document.viewport.source_position(EditorPosition::new(
                display.line,
                if motion == CaretMotion::LineStart {
                    segment.start_column
                } else {
                    segment.end_column
                },
            ));
            document.set_caret_row_affinity(target, row);
            return target;
        }
        if matches!(motion, CaretMotion::Left | CaretMotion::Right)
            && let Some(projection) = document.viewport.projection(display.line)
        {
            let forward = motion == CaretMotion::Right;
            let boundary = projection.fragments.iter().find_map(|fragment| {
                if let ProjectionFragment::Placeholder { display_range, .. } = fragment {
                    if forward && display.column == display_range.start {
                        return Some(display_range.end);
                    }
                    if !forward && display.column == display_range.end {
                        return Some(display_range.start);
                    }
                }
                None
            });
            let column = boundary.or_else(|| {
                if forward {
                    projection
                        .text
                        .get(display.column..)?
                        .graphemes(true)
                        .next()
                        .map(|grapheme| display.column + grapheme.len())
                } else {
                    projection
                        .text
                        .get(..display.column)?
                        .grapheme_indices(true)
                        .next_back()
                        .map(|(column, _)| column)
                }
            });
            if let Some(column) = column {
                return document
                    .viewport
                    .source_position(EditorPosition::new(display.line, column));
            }
        }
        if document.viewport.has_projections()
            && matches!(motion, CaretMotion::WordLeft | CaretMotion::WordRight)
        {
            let target = move_position(&document.buffer, position, motion);
            if let Some(range) = document.folds.collapsed_covering_position(target)
                && let Some(delimiter) = document.folds.delimiter(range)
            {
                return if motion == CaretMotion::WordLeft {
                    EditorPosition::new(range.start_line, delimiter.opening_column)
                } else {
                    EditorPosition::new(range.end_line, delimiter.closing_column)
                };
            }
            return target;
        }
        return move_position(&document.buffer, position, motion);
    }

    let tab_width = document.decorations.settings.indent_width;
    let current_text = document
        .viewport
        .display_text(display.line, &document.buffer);
    let row_start_column = document
        .viewport
        .row_segment(row, &document.buffer)
        .map_or(0, |segment| segment.start_visual_column);
    let current_column = visual_column_for(&current_text, display.column, tab_width)
        .saturating_sub(row_start_column);
    let preferred_column = *document
        .preferred_vertical_column
        .get_or_insert(current_column);
    let rows = match motion {
        CaretMotion::PageUp | CaretMotion::PageDown => document.viewport_visible_rows.max(1),
        _ => 1,
    };
    let target_row = match motion {
        CaretMotion::Up | CaretMotion::PageUp => row.saturating_sub(rows),
        _ => row
            .saturating_add(rows)
            .min(document.viewport.visible_row_count().saturating_sub(1)),
    };
    let target_line = document
        .viewport
        .visible_row_to_document_line(target_row)
        .unwrap_or(position.line);
    let target_text = document
        .viewport
        .display_text(target_line, &document.buffer);
    let column = if let Some(segment) = document.viewport.row_segment(target_row, &document.buffer)
    {
        byte_column_for(
            &target_text,
            segment.start_visual_column.saturating_add(preferred_column),
            tab_width,
        )
        .clamp(segment.start_column, segment.end_column)
    } else {
        byte_column_for(&target_text, preferred_column, tab_width)
    };
    let column = if column == target_text.len() {
        column
    } else {
        target_text
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .take_while(|offset| *offset <= column)
            .last()
            .unwrap_or(0)
    };
    let target = document
        .viewport
        .source_position(EditorPosition::new(target_line, column));
    document.set_caret_row_affinity(target, target_row);
    target
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::DocumentId;
    use crate::editor::FoldRange;

    const SOURCE: &str =
        "let mode = if check() {\n    abort();\n} else {\n    unwind();\n};\nafter();";

    fn folded(wrapped: bool) -> Document {
        let mut document = Document::from_path(DocumentId::new(1), "main.rs", SOURCE);
        document.update_viewport_geometry(8, 802.0, 8.0);
        document.set_word_wrap(wrapped);
        document.restore_collapsed_folds(&[(0, 2)]);
        document
    }

    #[test]
    fn visible_else_suffix_keeps_source_caret_and_independent_branch_folds() {
        for wrapped in [false, true] {
            let mut document = folded(wrapped);
            let suffix = EditorPosition::new(2, 4);
            document.set_main_selection(EditorSelection::new(suffix, suffix));
            document.ensure_caret_visible();
            assert!(document.folds.is_collapsed(FoldRange::new(0, 2)));
            assert_eq!(document.caret_visible_row(), Some(0));
            assert_eq!(document.main_selection().cursor, suffix);

            let down = move_document_position(&mut document, suffix, CaretMotion::Down);
            assert_eq!(down.line, 3);
            let up = move_document_position(&mut document, down, CaretMotion::Up);
            assert_eq!(up, suffix);
            document.ensure_caret_visible();
            assert!(document.folds.is_collapsed(FoldRange::new(0, 2)));

            let home = move_document_position(&mut document, suffix, CaretMotion::LineStart);
            assert_eq!(home, EditorPosition::new(0, 0));
            let end = move_document_position(&mut document, suffix, CaretMotion::LineEnd);
            assert_eq!(end, EditorPosition::new(2, "} else {".len()));
            document.folds.set_collapsed(FoldRange::new(2, 4), true);
            document.refresh_view_models();
            let end = move_document_position(&mut document, suffix, CaretMotion::LineEnd);
            assert_eq!(end, EditorPosition::new(4, "};".len()));
            assert_eq!(document.text(), SOURCE);
        }
    }

    #[test]
    fn arrows_step_across_the_fold_without_entering_hidden_source() {
        for wrapped in [false, true] {
            let mut document = folded(wrapped);
            let range = FoldRange::new(0, 2);
            let opening = document.folds.delimiter(range).unwrap().opening_column;
            let before = EditorPosition::new(0, opening);
            let after = move_document_position(&mut document, before, CaretMotion::Right);
            assert_eq!(after, EditorPosition::new(2, 1));
            document.set_main_selection(EditorSelection::new(after, after));
            document.ensure_caret_visible();
            assert!(document.folds.is_collapsed(range));
            assert_eq!(
                move_document_position(&mut document, after, CaretMotion::Left),
                before
            );
            assert_eq!(
                move_document_position(&mut document, after, CaretMotion::Right),
                EditorPosition::new(2, 2)
            );
        }
    }
}
