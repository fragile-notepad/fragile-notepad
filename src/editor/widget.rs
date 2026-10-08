use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Layout, Shell, image as advanced_image, mouse, text};
use iced::time::Duration;
use iced::time::Instant;
use iced::{Background, Element, Event, Font, Length, Rectangle, Size, Theme, highlighter};
#[cfg(test)]
use iced::{Color, Pixels, Point, alignment};
use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant as StdInstant;

use crate::core::ShortcutMap;
use crate::editor::action::EditorAction;

use super::buffer::EditorBuffer;
use super::cjk::CjkContext;
use super::decoration::DecorationModel;
use super::fold::FoldRange;
use super::fold_projection::ProjectionFragment;
#[cfg(test)]
use super::layout::scrolled_text_origin_x;
use super::layout::{
    EditorLayout, EditorMetrics, HitTarget, ScrollOffset, hit_test, hit_visible_row, row_y,
};
use super::position::{EditorPosition, SelectionSet};
#[cfg(test)]
use super::render::{
    RowRenderPlan, SelectionRenderPlan,
    build_render_plan_for_selection_set_with_cache_and_caret_row,
};
use super::render::{
    SyntaxLineCache, build_render_plan_for_selection_set_with_cache_and_caret_rows,
    collapsed_condition_indicator_bounds, collapsed_delimiter_indicator_bounds,
    collapsed_fold_indicator_bounds, condition_fold_placeholder, fold_delimiter_for_fragment,
};
use super::viewport::ViewportModel;

mod actions;
mod cache;
mod draw;
mod font;
mod interaction;
pub(crate) mod line_cache;
mod markers;
mod rich_text;
mod scrollbar;
mod state;
mod style;

pub use actions::key_action;
#[cfg(test)]
use cache::{RichParagraphCache, SyntaxSpanKey};
use draw::{draw_plan, draw_vertical_scrollbar};
pub use font::{
    EDITOR_FONT, EDITOR_FONT_ROUTE, EDITOR_TEXT_SHAPING, EditorFontRoute, EditorFontRun,
    editor_font_runs, editor_font_runs_from_cjk_runs, regional_cjk_font,
};
#[cfg(test)]
use interaction::scroll_delta_lines;
use interaction::{InteractionContext, UpdateOutcome, handle_event};
use line_cache::{LineGeometry, LineGeometryCache, measured_caret_x};
#[cfg(test)]
use line_cache::{byte_to_grapheme_table, measured_selection_x_and_width};
#[cfg(test)]
use rich_text::{first_visible_syntax_span, visible_rich_text_range, visible_styled_text_range};
pub use scrollbar::{
    VerticalScrollbarGeometry, scrollbar_row_for_position, vertical_scrollbar_geometry,
};
pub use state::AdvancedEditorState;
use state::is_scroll_fast_frame;
#[cfg(test)]
use state::{CARET_BLINK_INTERVAL_MS, caret_visible_at};
pub use style::EditorStyle;

pub struct AdvancedEditor<'a, Message> {
    id: Option<widget::Id>,
    buffer: &'a EditorBuffer,
    viewport: &'a ViewportModel,
    decorations: &'a DecorationModel,
    syntax_cache: &'a RefCell<SyntaxLineCache>,
    syntax_settings: highlighter::Settings,
    selections: SelectionSet,
    metrics: EditorMetrics,
    scroll: ScrollOffset,
    caret_row: Option<usize>,
    caret_rows: &'a [(EditorPosition, usize)],
    scroll_speed: f32,
    wrap_guide_column: usize,
    viewport_key: u64,
    cjk_context: Option<Arc<CjkContext>>,
    shortcuts: &'a ShortcutMap,
    width: Length,
    height: Length,
    on_action: Box<dyn Fn(EditorAction) -> Message + 'a>,
}

impl<'a, Message> AdvancedEditor<'a, Message> {
    pub fn new(
        buffer: &'a EditorBuffer,
        viewport: &'a ViewportModel,
        decorations: &'a DecorationModel,
        syntax_cache: &'a RefCell<SyntaxLineCache>,
        syntax_settings: highlighter::Settings,
        selections: impl Into<SelectionSet>,
        on_action: impl Fn(EditorAction) -> Message + 'a,
    ) -> Self {
        Self {
            id: None,
            buffer,
            viewport,
            decorations,
            syntax_cache,
            syntax_settings,
            selections: selections.into(),
            metrics: EditorMetrics::default(),
            scroll: ScrollOffset::ZERO,
            caret_row: None,
            caret_rows: &[],
            scroll_speed: 1.5,
            wrap_guide_column: crate::core::EditorSettings::DEFAULT_WRAP_COLUMN,
            viewport_key: 0,
            cjk_context: None,
            shortcuts: &DEFAULT_SHORTCUTS,
            width: Length::Fill,
            height: Length::Fill,
            on_action: Box::new(on_action),
        }
    }

    pub fn id(mut self, id: impl Into<widget::Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn viewport_key(mut self, key: u64) -> Self {
        self.viewport_key = key;
        self
    }

    pub fn cjk_context(mut self, context: Arc<CjkContext>) -> Self {
        self.cjk_context = Some(context);
        self
    }

    pub fn metrics(mut self, metrics: EditorMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    pub fn scroll(mut self, scroll: ScrollOffset) -> Self {
        self.scroll = scroll;
        self
    }

    pub fn caret_row(mut self, caret_row: Option<usize>) -> Self {
        self.caret_row = caret_row;
        self
    }

    pub fn caret_rows(mut self, caret_rows: &'a [(EditorPosition, usize)]) -> Self {
        self.caret_rows = caret_rows;
        self
    }

    pub fn scroll_speed(mut self, scroll_speed: f32) -> Self {
        self.scroll_speed = scroll_speed.max(0.0);
        self
    }

    /// Sets the column guide when wrapping is off. Wrapped views use their
    /// actual wrap boundary, including when it follows the window width.
    pub fn wrap_guide_column(mut self, column: usize) -> Self {
        self.wrap_guide_column = column.max(1);
        self
    }

    pub fn shortcuts(mut self, shortcuts: &'a ShortcutMap) -> Self {
        self.shortcuts = shortcuts;
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for AdvancedEditor<'_, Message>
where
    Message: Clone,
    Renderer: iced::advanced::Renderer
        + text::Renderer<Font = Font>
        + advanced_image::Renderer<Handle = advanced_image::Handle>,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.resolve(self.width, self.height, Size::new(320.0, 180.0)))
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let trace_enabled = crate::perf_trace::enabled();
        let draw_started = trace_enabled.then(StdInstant::now);
        let bounds = layout.bounds();
        let editor_layout = self.editor_layout(bounds);
        let editor_style = EditorStyle::from_theme(theme);
        let state = tree
            .state
            .downcast_ref::<AdvancedEditorState<Renderer::Paragraph>>();
        let fold_controls_hovered = self.fold_controls_hovered(bounds, cursor);
        // A first draw can arrive before a pointer event (e.g. a restored view).
        if state.fold_controls_hovered.replace(fold_controls_hovered) != fold_controls_hovered {
            let target = f32::from(fold_controls_hovered);
            state.fold_fade.set(state::FoldFade {
                from: target,
                target,
                started: Instant::now(),
            });
        }
        let fold_controls_opacity = state.fold_fade.get().opacity(Instant::now());
        let fast_text = is_scroll_fast_frame(state);
        let caret_visible = state.is_caret_visible() && state.text_drag.is_none();
        // Drawing only consumes completed spans. Parser work is scheduled by
        // the app on a blocking worker, including during GPU warm-up.
        let syntax_settings = highlighter::Settings {
            theme: self.syntax_settings.theme.variant(theme.palette().is_dark),
            token: self.syntax_settings.token.clone(),
        };
        self.syntax_cache.borrow_mut().configure(&syntax_settings);
        let syntax_cache = self.syntax_cache.borrow();
        let plan_started = trace_enabled.then(StdInstant::now);
        let main_caret = self
            .caret_row
            .map(|row| (self.selections.main().cursor, row));
        let caret_rows = if self.caret_rows.is_empty() {
            main_caret.as_slice()
        } else {
            self.caret_rows
        };
        let plan = build_render_plan_for_selection_set_with_cache_and_caret_rows(
            self.buffer,
            self.viewport,
            self.decorations,
            self.selections.clone(),
            editor_layout,
            &syntax_cache,
            caret_rows,
        );
        let plan_us = plan_started.map_or(0, |started| started.elapsed().as_micros());
        let plan_rows = if trace_enabled { plan.rows.len() } else { 0 };
        let plan_spans = if trace_enabled {
            plan.rows
                .iter()
                .map(|row| row.syntax_spans.len())
                .sum::<usize>()
        } else {
            0
        };
        let selection_range_lines = if trace_enabled {
            self.selections
                .projected_lines(self.buffer, self.decorations.settings.indent_width)
                .into_iter()
                .filter(|line| !line.range().is_empty())
                .count()
        } else {
            0
        };
        let visible_selection_lines = if trace_enabled {
            plan.selections.len()
        } else {
            0
        };
        let visible_selection_area = if trace_enabled {
            let height = (editor_layout.metrics.line_height - 2.0).max(1.0);
            plan.selections
                .iter()
                .map(|selection| selection.width.max(1.0) * height)
                .sum::<f32>()
        } else {
            0.0
        };
        let visible_selection_max_width = if trace_enabled {
            plan.selections
                .iter()
                .map(|selection| selection.width.max(1.0))
                .fold(0.0, f32::max)
        } else {
            0.0
        };

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..renderer::Quad::default()
            },
            Background::Color(editor_style.surface),
        );

        let frame_id = state.render_frame.get().wrapping_add(1);
        state.render_frame.set(frame_id);
        let mut rich_paragraphs = state.rich_paragraphs.borrow_mut();
        let mut line_geometries = state.line_geometries.borrow_mut();
        let record_started = trace_enabled.then(StdInstant::now);
        renderer.with_layer(bounds, |renderer| {
            draw_plan(
                renderer,
                bounds,
                editor_layout,
                self.decorations,
                &plan,
                editor_style,
                self.viewport.visible_row_count(),
                Some(
                    self.viewport
                        .wrap_columns()
                        .unwrap_or(self.wrap_guide_column),
                ),
                fast_text,
                caret_visible,
                fold_controls_opacity,
                frame_id,
                &mut rich_paragraphs,
                &mut line_geometries,
                self.cjk_context.as_deref(),
            );
            draw_vertical_scrollbar(
                renderer,
                editor_layout,
                self.viewport.visible_row_count(),
                bounds,
                editor_style,
            );
            if let Some(drag) = &state.text_drag
                && let Some((target, row)) = drag.target
            {
                let point = line_cache::measured_position_point_with_context(
                    self.buffer,
                    self.viewport,
                    self.decorations,
                    editor_layout,
                    target,
                    row,
                    renderer,
                    self.cjk_context.as_deref(),
                );
                renderer.with_layer(
                    crate::editor::layout::text_area_bounds(
                        bounds,
                        editor_layout,
                        self.decorations,
                    ),
                    |renderer| {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: bounds.x + point.x,
                                    y: bounds.y + point.y,
                                    width: 2.0,
                                    height: self.metrics.line_height,
                                },
                                ..renderer::Quad::default()
                            },
                            Background::Color(editor_style.caret),
                        )
                    },
                );
            }
        });
        let record_us = record_started.map_or(0, |started| started.elapsed().as_micros());
        rich_paragraphs.prune(frame_id);

        if let Some(draw_started) = draw_started {
            crate::perf_trace::event(
                "editor_draw",
                format_args!(
                    "total_us={} syntax_us=0 plan_us={plan_us} record_us={record_us} bounds={:.0}x{:.0} first_row={} rows={plan_rows} spans={plan_spans} selection_range_lines={selection_range_lines} visible_selection_lines={visible_selection_lines} visible_selection_area={visible_selection_area:.1} visible_selection_max_width={visible_selection_max_width:.1} fast_text={fast_text} token={}",
                    draw_started.elapsed().as_micros(),
                    bounds.width,
                    bounds.height,
                    editor_layout.scroll.first_visible_row,
                    self.syntax_settings.token,
                ),
            );
        }
    }

    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<AdvancedEditorState<Renderer::Paragraph>>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(AdvancedEditorState::<Renderer::Paragraph>::default())
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let state = tree
            .state
            .downcast_mut::<AdvancedEditorState<Renderer::Paragraph>>();

        operation.focusable(self.id.as_ref(), layout.bounds(), state);
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let trace_enabled = crate::perf_trace::enabled();
        let update_started = trace_enabled.then(StdInstant::now);
        let state = tree
            .state
            .downcast_mut::<AdvancedEditorState<Renderer::Paragraph>>();
        let fold_controls_hovered = self.fold_controls_hovered(layout.bounds(), cursor);
        if state.fold_controls_hovered.replace(fold_controls_hovered) != fold_controls_hovered {
            let now = Instant::now();
            state.fold_fade.set(state::FoldFade {
                from: state.fold_fade.get().opacity(now),
                target: f32::from(fold_controls_hovered),
                started: now,
            });
            shell.request_redraw();
        }
        if state.fold_fade.get().animating(Instant::now()) {
            shell.request_redraw_at(Instant::now() + Duration::from_millis(16));
        }
        let editor_layout = self.editor_layout(layout.bounds());
        let outcome = if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) && let Some(range) =
            self.hit_collapsed_indicator(layout, cursor, renderer)
        {
            state.is_focused = true;
            state.reset_caret_blink();
            state.cancel_pointer_drag();
            state.clear_text_click();
            state.preedit = None;
            shell.publish((self.on_action)(EditorAction::Focus));
            shell.publish((self.on_action)(EditorAction::ToggleFold(range)));
            shell.capture_event();
            shell.request_redraw();

            UpdateOutcome {
                perf_event: "editor_fold_toggle",
                should_capture: true,
            }
        } else {
            handle_event(
                InteractionContext {
                    buffer: self.buffer,
                    viewport: self.viewport,
                    decorations: self.decorations,
                    selections: &self.selections,
                    metrics: self.metrics,
                    caret_row: self.caret_row,
                    scroll_speed: self.scroll_speed,
                    viewport_key: self.viewport_key,
                    cjk_context: self.cjk_context.as_deref(),
                    shortcuts: self.shortcuts,
                    on_action: &*self.on_action,
                },
                state,
                event,
                layout.bounds(),
                cursor,
                editor_layout,
                renderer,
                shell,
            )
        };

        if let Some(update_started) = update_started {
            crate::perf_trace::event(
                outcome.perf_event,
                format_args!(
                    "elapsed_us={} capture={} first_row={} bounds={:.0}x{:.0}",
                    update_started.elapsed().as_micros(),
                    outcome.should_capture,
                    editor_layout.scroll.first_visible_row,
                    layout.bounds().width,
                    layout.bounds().height,
                ),
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        if tree
            .state
            .downcast_ref::<AdvancedEditorState<Renderer::Paragraph>>()
            .text_drag
            .as_ref()
            .is_some_and(|drag| drag.started)
        {
            return mouse::Interaction::Grabbing;
        }
        let editor_layout = self.editor_layout(bounds);
        let Some(position) = cursor.position_in(bounds) else {
            return mouse::Interaction::None;
        };
        let over_scrollbar =
            vertical_scrollbar_geometry(editor_layout, self.viewport.visible_row_count())
                .is_some_and(|scrollbar| scrollbar.track.contains(position));
        let over_fold_control = position.x < self.metrics.text_origin_x(self.decorations)
            && matches!(
                hit_test(
                    position.x,
                    position.y,
                    editor_layout,
                    self.buffer,
                    self.viewport,
                    self.decorations,
                ),
                HitTarget::FoldControl { .. }
            );

        if over_scrollbar
            || over_fold_control
            || self
                .hit_collapsed_indicator(layout, cursor, renderer)
                .is_some()
        {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::Text
        }
    }
}

static DEFAULT_SHORTCUTS: std::sync::LazyLock<ShortcutMap> =
    std::sync::LazyLock::new(ShortcutMap::default);

impl<Message> AdvancedEditor<'_, Message> {
    fn fold_controls_hovered(&self, bounds: Rectangle, cursor: mouse::Cursor) -> bool {
        self.decorations.settings.show_folding_controls
            && cursor
                .position_in(bounds)
                .is_some_and(|position| position.x < self.metrics.text_origin_x(self.decorations))
    }

    fn editor_layout(&self, bounds: Rectangle) -> EditorLayout {
        let mut scroll = self.scroll;
        if self.viewport.wrap_columns().is_some() {
            scroll.horizontal_px = 0.0;
        }
        EditorLayout::new(self.metrics, scroll, bounds.width, bounds.height)
    }

    fn hit_collapsed_indicator<Renderer>(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> Option<FoldRange>
    where
        Renderer: text::Renderer<Font = Font>,
    {
        let bounds = layout.bounds();
        let position = cursor.position_in(bounds)?;
        let editor_layout = self.editor_layout(bounds);
        let text_right =
            vertical_scrollbar_geometry(editor_layout, self.viewport.visible_row_count())
                .map_or(bounds.width, |scrollbar| scrollbar.track.x);

        if position.x < self.metrics.text_origin_x(self.decorations) || position.x >= text_right {
            return None;
        }

        let (visible_row, line) = hit_visible_row(position.y, editor_layout, self.viewport)?;
        let segment = self.viewport.row_segment(visible_row, self.buffer)?;
        if let Some(projection) = self.viewport.projection(line) {
            let fragment = &projection.text[segment.start_column..segment.end_column];
            let geometry = LineGeometry::new_with_font_runs(
                fragment,
                self.metrics,
                renderer,
                segment.start_visual_column,
                self.decorations.settings.indent_width,
                &font::editor_font_runs_for_display_fragment(
                    fragment,
                    self.cjk_context.as_deref(),
                    line,
                    segment.start_column,
                    self.viewport,
                ),
            );
            for projected in &projection.fragments {
                if let ProjectionFragment::Placeholder {
                    display_range,
                    range,
                    ..
                } = projected
                {
                    let start = display_range.start.max(segment.start_column);
                    let end = display_range.end.min(segment.end_column);
                    if start >= end {
                        continue;
                    }
                    let left = measured_caret_x(
                        &geometry,
                        start - segment.start_column,
                        editor_layout,
                        self.decorations,
                    );
                    let right = measured_caret_x(
                        &geometry,
                        end - segment.start_column,
                        editor_layout,
                        self.decorations,
                    );
                    let indicator = Rectangle {
                        x: left,
                        y: row_y(visible_row, editor_layout) + self.metrics.line_height * 0.05,
                        width: (right - left).max(0.0),
                        height: self.metrics.line_height * 0.9,
                    };
                    if indicator.contains(position) {
                        return Some(*range);
                    }
                }
            }
            return None;
        }
        if !segment.is_last {
            return None;
        }
        let decoration = self.decorations.line_decorations.get(line)?;
        decoration.fold_range?;
        // Brace and indentation folds can share a header. Match the longest
        // collapsed span, which is the range the viewport currently hides.
        let hidden = self
            .decorations
            .hidden_line_spans
            .iter()
            .filter(|span| span.header_line == line)
            .max_by_key(|span| span.last_hidden_line)?;
        let line_text = self.buffer.line(line)?;
        let fragment = &line_text[segment.start_column..segment.end_column];
        let delimiter = decoration
            .fold_delimiter
            .filter(|_| {
                decoration.fold_range == Some(FoldRange::new(line, hidden.last_hidden_line))
            })
            .and_then(|delimiter| {
                fold_delimiter_for_fragment(delimiter, segment.start_column, fragment)
            });
        let line_geometry = LineGeometry::new_with_font_runs(
            fragment,
            self.metrics,
            renderer,
            segment.start_visual_column,
            self.decorations.settings.indent_width,
            &font::editor_font_runs_for_fragment(
                fragment,
                self.cjk_context.as_deref(),
                line,
                segment.start_column,
            ),
        );
        let anchor_x = measured_caret_x(
            &line_geometry,
            delimiter.map_or(fragment.len(), |delimiter| delimiter.opening_column),
            editor_layout,
            self.decorations,
        );
        let indicator = if delimiter.is_some() {
            collapsed_delimiter_indicator_bounds(
                self.metrics,
                row_y(visible_row, editor_layout),
                anchor_x,
            )
        } else if condition_fold_placeholder(
            self.buffer,
            hidden.first_hidden_line,
            hidden.last_hidden_line,
        )
        .is_some()
        {
            collapsed_condition_indicator_bounds(
                self.metrics,
                row_y(visible_row, editor_layout),
                anchor_x,
                self.decorations.settings.show_end_of_line_markers,
            )
        } else {
            collapsed_fold_indicator_bounds(
                self.metrics,
                row_y(visible_row, editor_layout),
                anchor_x,
                self.decorations.settings.show_end_of_line_markers,
            )
        };

        indicator
            .contains(position)
            .then_some(FoldRange::new(line, hidden.last_hidden_line))
    }
}

impl<'a, Message, Renderer> From<AdvancedEditor<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Renderer: iced::advanced::Renderer
        + text::Renderer<Font = Font>
        + advanced_image::Renderer<Handle = advanced_image::Handle>
        + 'a,
{
    fn from(editor: AdvancedEditor<'a, Message>) -> Self {
        Element::new(editor)
    }
}

#[cfg(test)]
mod tests;
