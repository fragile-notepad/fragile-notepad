//! Wrapping, scrolling, and caret placement using source positions as scroll anchors.

use super::Document;
use crate::editor::wrap_measurement::WrapMeasurement;
use crate::editor::{EditorPosition, ViewportModel};

const DEFAULT_REVEAL_CONTEXT_ROWS: usize = 3;

impl Document {
    pub(super) fn clamp_scroll(&mut self) {
        self.scroll.first_visible_row = self
            .scroll
            .first_visible_row
            .min(self.viewport.visible_row_count().saturating_sub(1));
        if self.word_wrap {
            self.scroll.horizontal_px = 0.0;
        }
    }

    pub fn word_wrap(&self) -> bool {
        self.word_wrap
    }

    pub fn set_word_wrap(&mut self, enabled: bool) {
        if self.word_wrap == enabled {
            return;
        }
        let caret_was_visible = self.caret_is_in_view();
        self.word_wrap = enabled;
        self.preferred_vertical_column = None;
        self.rebuild_viewport();
        if caret_was_visible {
            self.ensure_caret_visible();
        }
    }

    pub fn set_wrap_column_limit(&mut self, columns: Option<usize>) {
        let columns = columns.map(|value| {
            value.clamp(
                crate::core::EditorSettings::MIN_WRAP_COLUMN,
                crate::core::EditorSettings::MAX_WRAP_COLUMN,
            )
        });
        if self.wrap_column_limit == columns {
            return;
        }
        let caret_was_visible = self.caret_is_in_view();
        self.wrap_column_limit = columns;
        if self.word_wrap {
            self.preferred_vertical_column = None;
            self.rebuild_viewport();
            if caret_was_visible {
                self.ensure_caret_visible();
            }
        }
    }

    fn caret_is_in_view(&self) -> bool {
        if !self.caret_visible_row().is_some_and(|row| {
            row >= self.scroll.first_visible_row
                && row
                    < self
                        .scroll
                        .first_visible_row
                        .saturating_add(self.viewport_visible_rows)
        }) {
            return false;
        }
        if self.word_wrap {
            return true;
        }
        let cursor = self.buffer.clamp_position(self.main_selection().cursor);
        let display = self.viewport.display_position(cursor).unwrap_or(cursor);
        let line = self.viewport.display_text(display.line, &self.buffer);
        let column = crate::editor::layout::visual_column_for(
            &line,
            display.column,
            self.decorations.settings.indent_width,
        );
        let x = column as f32 * self.viewport_character_width.max(1.0);
        x >= self.scroll.horizontal_px
            && x < self.scroll.horizontal_px + self.viewport_text_width.max(1.0)
    }

    pub fn update_viewport_geometry(
        &mut self,
        visible_rows: usize,
        text_width: f32,
        character_width: f32,
    ) {
        let caret_was_visible = self.caret_is_in_view();
        let previous_visible_rows = self.viewport_visible_rows;
        self.viewport_visible_rows = visible_rows.max(1);
        self.viewport_text_width = text_width.max(1.0);
        self.viewport_character_width = character_width.max(1.0);
        if self.word_wrap
            && (self.viewport.wrap_columns() != Some(self.wrap_columns())
                || self.viewport.fold_indicator_columns() != self.wrap_fold_indicator_columns()
                || self.viewport.wrap_measurement() != self.wrap_measurement)
        {
            self.preferred_vertical_column = None;
            self.rebuild_viewport();
        }
        if caret_was_visible
            && (self.word_wrap || self.viewport_visible_rows < previous_visible_rows)
        {
            self.ensure_caret_visible();
        }
    }

    /// Uses the editor's typography for the shared Unicode wrap map.
    pub fn update_viewport_geometry_with_typography(
        &mut self,
        visible_rows: usize,
        text_width: f32,
        character_width: f32,
        font_size: f32,
        hint_factor: Option<f32>,
    ) {
        self.wrap_measurement = Some(WrapMeasurement::new(
            character_width,
            font_size,
            hint_factor,
        ));
        self.update_viewport_geometry(visible_rows, text_width, character_width);
    }

    fn wrap_columns(&self) -> usize {
        // Keep the insertion caret inside the text area at the end of a full row.
        let character_width = self.viewport_character_width.max(1.0);
        let marker_width = if self.decorations.settings.show_end_of_line_markers {
            crate::editor::render::end_of_line_marker_reservation(character_width)
        } else {
            0.0
        };
        let window_columns = ((self.viewport_text_width - marker_width - 2.0).max(1.0)
            / character_width)
            .floor()
            .max(1.0) as usize;
        self.wrap_column_limit
            .map_or(window_columns, |limit| limit.min(window_columns))
    }

    fn wrap_fold_indicator_columns(&self) -> usize {
        let character_width = self.viewport_character_width.max(1.0);
        (crate::editor::render::collapsed_fold_indicator_reservation(character_width)
            / character_width)
            .ceil() as usize
    }

    /// Returns the exact recovery anchor until the first measured viewport is
    /// available, so an early snapshot cannot replace it with a rounded row.
    pub fn session_top_position(&self) -> Option<EditorPosition> {
        self.pending_session_top
            .or_else(|| self.viewport_top_position())
    }

    /// Restores saved scrolling without relying on a provisional wrap width.
    pub fn restore_session_scroll(
        &mut self,
        position: Option<EditorPosition>,
        fallback_row: usize,
        horizontal_offset: f32,
        wait_for_folds: bool,
    ) {
        self.pending_session_top = None;
        self.session_folds_pending = wait_for_folds;
        if let Some(position) = position {
            let position = self.buffer.clamp_position(position);
            self.restore_viewport_top(Some(position));
            if !self.viewport_geometry_initialized || self.session_folds_pending {
                self.pending_session_top = Some(position);
            }
        } else {
            self.scroll.first_visible_row =
                fallback_row.min(self.viewport.visible_row_count().saturating_sub(1));
        }
        self.scroll.horizontal_px = if self.word_wrap {
            0.0
        } else {
            horizontal_offset.max(0.0)
        };
    }

    /// Called after the widget reports its measured viewport geometry. This
    /// also records geometry received before a streamed document finishes.
    pub fn finish_session_scroll_restore(&mut self) {
        self.viewport_geometry_initialized = true;
        if let Some(position) = self.pending_session_top {
            self.restore_viewport_top(Some(position));
            if !self.session_folds_pending {
                self.pending_session_top = None;
            }
        }
    }

    fn viewport_top_position(&self) -> Option<EditorPosition> {
        self.viewport
            .visible_row_to_document_line(self.scroll.first_visible_row)
            .map(|line| {
                let column = if self.viewport.wrap_columns().is_some() {
                    self.viewport
                        .row_segment(self.scroll.first_visible_row, &self.buffer)
                        .map_or(0, |segment| segment.start_column)
                } else {
                    0
                };
                self.buffer.clamp_position(
                    self.viewport
                        .source_position(EditorPosition::new(line, column)),
                )
            })
    }

    pub(super) fn refresh_wrapped_lines(&mut self, first: usize, last: usize) {
        let top_position = self.viewport_top_position();
        let context = self.wrap_measurement.map(|_| self.cjk_context());
        if self.viewport.reflow_wrapped_lines_with_context(
            &self.buffer,
            &self.folds,
            first,
            last,
            context.as_deref(),
        ) {
            self.restore_viewport_top(top_position);
        } else {
            self.rebuild_viewport();
        }
    }

    pub(super) fn rebuild_viewport(&mut self) {
        let top_position = self.viewport_top_position();
        self.viewport = if self.word_wrap {
            let context = self.wrap_measurement.map(|_| self.cjk_context());
            ViewportModel::new_wrapped_with_measurement(
                &self.buffer,
                &self.folds,
                self.wrap_columns(),
                self.decorations.settings.indent_width,
                self.wrap_fold_indicator_columns(),
                self.wrap_measurement,
                context.as_deref(),
            )
        } else {
            ViewportModel::new_with_buffer(
                &self.buffer,
                &self.folds,
                self.decorations.settings.indent_width,
            )
        };
        self.restore_viewport_top(top_position);
    }

    // Hidden anchors move to the header of their enclosing collapsed fold.
    fn restore_viewport_top(&mut self, top_position: Option<EditorPosition>) {
        if let Some(mut position) = top_position {
            while self.viewport.display_position(position).is_none()
                && let Some(range) = self
                    .folds
                    .collapsed_covering_position(position)
                    .or_else(|| self.folds.collapsed_covering(position.line))
            {
                position = EditorPosition::new(range.start_line, 0);
            }
            if let Some(row) = self.viewport.position_to_visible_row(position) {
                self.scroll.first_visible_row = row;
            }
        }
        self.caret_row_affinities.clear();
        self.clamp_scroll();
    }

    /// Carets can sit at either side of a soft line break. Keyboard End,
    /// vertical navigation, and pointer placement retain the chosen display row.
    pub fn caret_visible_row(&self) -> Option<usize> {
        self.position_visible_row(self.main_selection().cursor)
    }

    pub fn position_visible_row(&self, position: EditorPosition) -> Option<usize> {
        let position = self.buffer.clamp_position(position);
        let display = self.viewport.display_position(position)?;
        if let Some(&(_, row)) = self
            .caret_row_affinities
            .iter()
            .find(|(caret, _)| *caret == position)
            && self.viewport.visible_row_to_document_line(row) == Some(display.line)
            && self
                .viewport
                .row_segment(row, &self.buffer)
                .is_some_and(|segment| {
                    display.column >= segment.start_column && display.column <= segment.end_column
                })
        {
            return Some(row);
        }
        self.viewport.position_to_visible_row(position)
    }

    /// Chooses the caret's row at a soft wrap; cleared by text changes or reflow.
    pub fn set_caret_row_affinity(&mut self, position: EditorPosition, row: usize) {
        let position = self.buffer.clamp_position(position);
        if let Some((_, previous_row)) = self
            .caret_row_affinities
            .iter_mut()
            .find(|(caret, _)| *caret == position)
        {
            *previous_row = row;
        } else {
            self.caret_row_affinities.push((position, row));
        }
    }

    pub fn caret_row_affinities(&self) -> &[(EditorPosition, usize)] {
        &self.caret_row_affinities
    }

    pub fn clear_caret_row_affinity(&mut self) {
        self.caret_row_affinities.clear();
    }

    pub fn ensure_caret_visible(&mut self) {
        let cursor = self.buffer.clamp_position(self.main_selection().cursor);
        let mut unfolded = false;
        while self.viewport.display_position(cursor).is_none()
            && let Some(range) = self
                .folds
                .collapsed_covering_position(cursor)
                .or_else(|| self.folds.collapsed_covering(cursor.line))
        {
            self.folds.set_collapsed(range, false);
            unfolded = true;
        }
        if unfolded {
            self.refresh_view_models();
        }
        if let Some(row) = self.caret_visible_row() {
            let capacity = self.viewport_visible_rows.max(1);
            if row < self.scroll.first_visible_row {
                self.scroll.first_visible_row = row;
            } else if row >= self.scroll.first_visible_row.saturating_add(capacity) {
                self.scroll.first_visible_row = row.saturating_sub(capacity - 1);
            }
        }
        if self.word_wrap {
            self.scroll.horizontal_px = 0.0;
            return;
        }
        let display = self.viewport.display_position(cursor).unwrap_or(cursor);
        let line = self.viewport.display_text(display.line, &self.buffer);
        let column = crate::editor::layout::visual_column_for(
            &line,
            display.column,
            self.decorations.settings.indent_width,
        );
        let char_width = self.viewport_character_width.max(1.0);
        let x = column as f32 * char_width;
        let width = self.viewport_text_width.max(char_width);
        if x < self.scroll.horizontal_px {
            self.scroll.horizontal_px = x;
        } else if x + char_width > self.scroll.horizontal_px + width {
            self.scroll.horizontal_px = (x + char_width - width).max(0.0);
        }
    }

    pub fn reveal_line(&mut self, line: usize) {
        self.reveal_line_with_context(line, DEFAULT_REVEAL_CONTEXT_ROWS);
    }

    pub fn reveal_position(&mut self, position: EditorPosition) {
        let position = self.buffer.clamp_position(position);
        let mut unfolded = false;
        while self.viewport.display_position(position).is_none()
            && let Some(range) = self
                .folds
                .collapsed_covering_position(position)
                .or_else(|| self.folds.collapsed_covering(position.line))
        {
            self.folds.set_collapsed(range, false);
            unfolded = true;
        }
        if unfolded {
            self.refresh_view_models();
        }
        let row = if position == self.main_selection().cursor {
            self.caret_visible_row()
        } else {
            self.viewport.position_to_visible_row(position)
        };
        if let Some(row) = row {
            let context =
                DEFAULT_REVEAL_CONTEXT_ROWS.min(self.viewport_visible_rows.saturating_sub(1));
            self.scroll.first_visible_row = row.saturating_sub(context);
        }
    }

    pub fn reveal_line_with_context(&mut self, line: usize, context_rows: usize) {
        let Some(visible_row) = self.viewport.document_line_to_visible_row(line) else {
            return;
        };

        let max = self.viewport.visible_row_count().saturating_sub(1);
        self.scroll.first_visible_row = visible_row.saturating_sub(context_rows).min(max);
    }
}
