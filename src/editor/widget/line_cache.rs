use iced::advanced::text;
use iced::advanced::text::Paragraph as _;
use iced::{Font, Pixels, Point, Size, alignment};
use unicode_segmentation::UnicodeSegmentation;

use crate::editor::buffer::EditorBuffer;
use crate::editor::cjk::{CjkContext, CjkLanguage};
use crate::editor::decoration::DecorationModel;
use crate::editor::layout::{
    EditorLayout, EditorMetrics, HitTarget, byte_column_for_with_offset, hit_visible_row, row_y,
    scrolled_text_origin_x, visual_column_for_with_offset,
};
use crate::editor::position::EditorPosition;
use crate::editor::render::{RowRenderPlan, SelectionRenderPlan};
use crate::editor::viewport::ViewportModel;

#[cfg(test)]
use super::font::editor_font_runs;
use super::font::{
    EDITOR_FONT, EDITOR_TEXT_SHAPING, EditorFontRun, editor_font_runs_for_display_fragment,
    editor_font_runs_for_row, remap_font_runs,
};

const LINE_GEOMETRY_CACHE_MINIMUM: usize = 256;
const MAX_MEASURED_LINE_BYTES: usize = 4 * 1024;

#[derive(Debug)]
pub(super) struct LineGeometryCache<Paragraph> {
    entries: Vec<Option<LineGeometryEntry<Paragraph>>>,
    #[cfg(test)]
    build_count: usize,
}

impl<Paragraph> Default for LineGeometryCache<Paragraph> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            #[cfg(test)]
            build_count: 0,
        }
    }
}

#[derive(Debug)]
struct LineGeometryEntry<Paragraph> {
    visible_row: usize,
    start_visual_column: usize,
    text: String,
    metrics: EditorMetrics,
    scale_factor: Option<f32>,
    language: Option<CjkLanguage>,
    tab_width: usize,
    font_version: iced::advanced::graphics::text::Version,
    font_runs: Vec<EditorFontRun>,
    measure_ascii: bool,
    geometry: LineGeometry<Paragraph>,
}

impl<Paragraph> LineGeometryCache<Paragraph> {
    fn ensure_capacity(&mut self, visible_rows: usize) {
        let target = visible_rows
            .saturating_add(1)
            .next_power_of_two()
            .max(LINE_GEOMETRY_CACHE_MINIMUM);

        if self.entries.len() < target {
            let previous = std::mem::take(&mut self.entries);
            self.entries.resize_with(target, || None);
            for entry in previous.into_iter().flatten() {
                let slot = entry.visible_row % target;
                self.entries[slot] = Some(entry);
            }
        }
    }

    fn slot_for_row(&self, visible_row: usize) -> usize {
        debug_assert!(!self.entries.is_empty());
        visible_row % self.entries.len()
    }

    fn geometry(&self, slot: usize) -> Option<&LineGeometry<Paragraph>> {
        self.entries
            .get(slot)
            .and_then(Option::as_ref)
            .map(|entry| &entry.geometry)
    }

    #[cfg(test)]
    pub(super) fn build_count(&self) -> usize {
        self.build_count
    }
}

impl<Paragraph> LineGeometryCache<Paragraph>
where
    Paragraph: text::Paragraph<Font = Font>,
{
    fn ensure<Renderer>(
        &mut self,
        visible_row: usize,
        start_visual_column: usize,
        text: &str,
        metrics: EditorMetrics,
        renderer: &Renderer,
        language: Option<CjkLanguage>,
        tab_width: usize,
        font_runs: &[EditorFontRun],
        measure_ascii: bool,
    ) -> usize
    where
        Renderer: text::Renderer<Font = Font, Paragraph = Paragraph>,
    {
        let scale_factor = renderer.scale_factor();
        let font_version = iced::advanced::graphics::text::font_system()
            .read()
            .expect("Read font system")
            .version();
        let slot = self.slot_for_row(visible_row);
        let is_hit = self.entries[slot].as_ref().is_some_and(|entry| {
            entry.visible_row == visible_row
                && entry.start_visual_column == start_visual_column
                && entry.text == text
                && entry.metrics == metrics
                && entry.scale_factor == scale_factor
                && entry.language == language
                && entry.tab_width == tab_width
                && entry.font_version == font_version
                && entry.font_runs == font_runs
                && entry.measure_ascii == measure_ascii
        });

        if !is_hit {
            #[cfg(test)]
            {
                self.build_count += 1;
            }

            self.entries[slot] = Some(LineGeometryEntry {
                visible_row,
                start_visual_column,
                text: text.to_owned(),
                metrics,
                scale_factor,
                language,
                tab_width,
                font_version,
                font_runs: font_runs.to_vec(),
                measure_ascii,
                geometry: LineGeometry::new_with_font_runs(
                    text,
                    metrics,
                    renderer,
                    start_visual_column,
                    tab_width,
                    font_runs,
                    measure_ascii,
                ),
            });
        }

        slot
    }
}

#[cfg(test)]
pub(super) fn measured_text_hit_target<Renderer>(
    position: Point,
    layout: EditorLayout,
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
    renderer: &Renderer,
) -> HitTarget
where
    Renderer: text::Renderer<Font = Font>,
{
    measured_text_hit_target_with_context(
        position,
        layout,
        buffer,
        viewport,
        decorations,
        renderer,
        None,
    )
}

pub(super) fn measured_text_hit_target_with_context<Renderer>(
    position: Point,
    mut layout: EditorLayout,
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
    renderer: &Renderer,
    context: Option<&CjkContext>,
) -> HitTarget
where
    Renderer: text::Renderer<Font = Font>,
{
    let Some((visible_row, line)) = hit_visible_row(position.y, layout, viewport) else {
        return HitTarget::Outside;
    };
    let Some(segment) = viewport.row_segment(visible_row, buffer) else {
        return HitTarget::Outside;
    };
    if viewport.wrap_columns().is_some() {
        layout.scroll.horizontal_px = 0.0;
    }

    let line_text = viewport.display_text(line, buffer);
    let fragment = &line_text[segment.start_column..segment.end_column];
    let text_x = scrolled_text_origin_x(layout, decorations);
    let x = (position.x - text_x).max(0.0);
    let column = segment.start_column
        + LineGeometry::new_with_font_runs(
            fragment,
            layout.metrics,
            renderer,
            segment.start_visual_column,
            decorations.settings.indent_width,
            &editor_font_runs_for_display_fragment(
                fragment,
                context,
                line,
                segment.start_column,
                viewport,
            ),
            fold_needs_measured_geometry(line, viewport, decorations),
        )
        .byte_column_for_x(x, decorations.settings.indent_width);

    HitTarget::Text(
        buffer.clamp_position(viewport.source_position(EditorPosition::new(line, column))),
    )
}

/// Measures a document position within its visual row. The optional row keeps
/// an upstream caret at the end of a wrapped fragment on that fragment.
#[cfg(test)]
pub(crate) fn measured_position_point<Renderer>(
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
    layout: EditorLayout,
    position: EditorPosition,
    caret_row: Option<usize>,
    renderer: &Renderer,
) -> Point
where
    Renderer: text::Renderer<Font = Font>,
{
    measured_position_point_with_context(
        buffer,
        viewport,
        decorations,
        layout,
        position,
        caret_row,
        renderer,
        None,
    )
}

pub(crate) fn measured_position_point_with_context<Renderer>(
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
    layout: EditorLayout,
    position: EditorPosition,
    caret_row: Option<usize>,
    renderer: &Renderer,
    context: Option<&CjkContext>,
) -> Point
where
    Renderer: text::Renderer<Font = Font>,
{
    measured_position_point_impl(
        buffer,
        viewport,
        decorations,
        layout,
        position,
        caret_row,
        renderer,
        context,
        None,
    )
}

/// Retains caret shaping while recomputing its position in the current viewport.
pub(super) fn measured_position_point_with_cache<Renderer>(
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
    layout: EditorLayout,
    position: EditorPosition,
    caret_row: Option<usize>,
    renderer: &Renderer,
    context: Option<&CjkContext>,
    cache: &mut LineGeometryCache<Renderer::Paragraph>,
) -> Point
where
    Renderer: text::Renderer<Font = Font>,
{
    measured_position_point_impl(
        buffer,
        viewport,
        decorations,
        layout,
        position,
        caret_row,
        renderer,
        context,
        Some(cache),
    )
}

fn measured_position_point_impl<Renderer>(
    buffer: &EditorBuffer,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
    mut layout: EditorLayout,
    position: EditorPosition,
    caret_row: Option<usize>,
    renderer: &Renderer,
    context: Option<&CjkContext>,
    cache: Option<&mut LineGeometryCache<Renderer::Paragraph>>,
) -> Point
where
    Renderer: text::Renderer<Font = Font>,
{
    let position = buffer.clamp_position(position);
    let display_position = viewport.display_position(position).unwrap_or(position);
    if viewport.wrap_columns().is_some() {
        layout.scroll.horizontal_px = 0.0;
    }
    let visible_row = caret_row
        .filter(|row| {
            viewport.visible_row_to_document_line(*row) == Some(display_position.line)
                && viewport.row_segment(*row, buffer).is_some_and(|segment| {
                    display_position.column >= segment.start_column
                        && display_position.column <= segment.end_column
                })
        })
        .or_else(|| viewport.position_to_visible_row(position))
        .unwrap_or(layout.scroll.first_visible_row);
    let line = viewport.display_text(display_position.line, buffer);
    let (start, end, visual_offset) = viewport
        .row_segment(visible_row, buffer)
        .filter(|_| {
            viewport.visible_row_to_document_line(visible_row) == Some(display_position.line)
        })
        .map(|segment| {
            (
                segment.start_column,
                segment.end_column,
                segment.start_visual_column,
            )
        })
        .unwrap_or((0, line.len(), 0));
    let fragment = &line[start.min(line.len())..end.min(line.len())];
    let fonts = editor_font_runs_for_display_fragment(
        fragment,
        context,
        display_position.line,
        start,
        viewport,
    );
    let measure_ascii = fold_needs_measured_geometry(display_position.line, viewport, decorations);
    let uncached;
    let geometry = if let Some(cache) = cache {
        cache.ensure_capacity(layout.visible_row_capacity());
        let slot = cache.ensure(
            visible_row,
            visual_offset,
            fragment,
            layout.metrics,
            renderer,
            context.and_then(|context| context.language_for_line(display_position.line)),
            decorations.settings.indent_width,
            &fonts,
            measure_ascii,
        );
        cache.geometry(slot).expect("cached caret geometry")
    } else {
        uncached = LineGeometry::new_with_font_runs(
            fragment,
            layout.metrics,
            renderer,
            visual_offset,
            decorations.settings.indent_width,
            &fonts,
            measure_ascii,
        );
        &uncached
    };

    Point::new(
        measured_caret_x(
            geometry,
            display_position.column.saturating_sub(start),
            layout,
            decorations,
        ),
        row_y(visible_row, layout),
    )
}

pub(super) fn fold_needs_measured_geometry(
    line: usize,
    viewport: &ViewportModel,
    decorations: &DecorationModel,
) -> bool {
    viewport.projection(line).is_some()
        || decorations
            .line_decorations
            .get(line)
            .is_some_and(|decoration| {
                decoration.is_fold_collapsed && decoration.fold_delimiter.is_some()
            })
}

pub(super) fn measured_caret_x<Paragraph>(
    line_geometry: &LineGeometry<Paragraph>,
    column: usize,
    layout: EditorLayout,
    decorations: &DecorationModel,
) -> f32
where
    Paragraph: text::Paragraph<Font = Font>,
{
    scrolled_text_origin_x(layout, decorations)
        + line_geometry.x_for_byte_column(column, decorations.settings.indent_width)
}

pub(super) fn measured_virtual_caret_x<Paragraph>(
    line_geometry: &LineGeometry<Paragraph>,
    column: usize,
    virtual_column: Option<usize>,
    layout: EditorLayout,
    decorations: &DecorationModel,
) -> f32
where
    Paragraph: text::Paragraph<Font = Font>,
{
    virtual_column
        .map(|visual_column| {
            scrolled_text_origin_x(layout, decorations)
                + visual_column.saturating_sub(line_geometry.start_visual_column()) as f32
                    * layout.metrics.character_width
        })
        .unwrap_or_else(|| measured_caret_x(line_geometry, column, layout, decorations))
}

pub(super) fn measured_selection_x_and_width<Paragraph>(
    selection: &SelectionRenderPlan,
    line_geometry: &LineGeometry<Paragraph>,
    layout: EditorLayout,
    decorations: &DecorationModel,
) -> (f32, f32)
where
    Paragraph: text::Paragraph<Font = Font>,
{
    let start_x = measured_virtual_caret_x(
        line_geometry,
        selection.start_column,
        selection.start_virtual_column,
        layout,
        decorations,
    );
    let end_x = measured_virtual_caret_x(
        line_geometry,
        selection.end_column,
        selection.end_virtual_column,
        layout,
        decorations,
    );

    (start_x.min(end_x), (end_x - start_x).abs())
}

pub(super) struct RowGeometries<'a, Paragraph>
where
    Paragraph: text::Paragraph<Font = Font>,
{
    cache: &'a LineGeometryCache<Paragraph>,
    rows: Vec<(usize, usize)>,
}

impl<'a, Paragraph> RowGeometries<'a, Paragraph>
where
    Paragraph: text::Paragraph<Font = Font>,
{
    pub(super) fn new<Renderer>(
        rows: &[RowRenderPlan],
        metrics: EditorMetrics,
        cache: &'a mut LineGeometryCache<Paragraph>,
        renderer: &Renderer,
        context: Option<&CjkContext>,
        tab_width: usize,
    ) -> Self
    where
        Renderer: text::Renderer<Font = Font, Paragraph = Paragraph>,
    {
        cache.ensure_capacity(rows.len());
        let rows = rows
            .iter()
            .map(|row| {
                (
                    row.visible_row,
                    cache.ensure(
                        row.visible_row,
                        row.start_visual_column,
                        &row.text,
                        metrics,
                        renderer,
                        context.and_then(|context| context.language_for_line(row.line)),
                        tab_width,
                        &editor_font_runs_for_row(row, context),
                        !row.projection.is_empty() || row.collapsed_delimiter().is_some(),
                    ),
                )
            })
            .collect();

        Self {
            cache: &*cache,
            rows,
        }
    }

    pub(super) fn get_by_row_index(&self, row_index: usize) -> &LineGeometry<Paragraph> {
        let slot = self
            .rows
            .get(row_index)
            .map(|(_, slot)| *slot)
            .expect("visible row geometry index");

        self.cache.geometry(slot).expect("visible row geometry")
    }
}

#[derive(Debug)]
pub(super) enum LineGeometry<Paragraph> {
    Fast {
        text: String,
        character_width: f32,
        start_visual_column: usize,
    },
    Tabular {
        text: String,
        character_width: f32,
        start_visual_column: usize,
    },
    Measured {
        text: String,
        paragraph: Paragraph,
        byte_to_grapheme: Vec<(usize, usize)>,
        fallback_character_width: f32,
        start_visual_column: usize,
        expanded_byte_offsets: Option<Vec<(usize, usize)>>,
    },
}

impl<Paragraph> LineGeometry<Paragraph>
where
    Paragraph: text::Paragraph<Font = Font>,
{
    #[cfg(test)]
    pub(super) fn new_with_visual_offset<Renderer>(
        text: &str,
        metrics: EditorMetrics,
        renderer: &Renderer,
        start_visual_column: usize,
    ) -> Self
    where
        Renderer: text::Renderer<Font = Font, Paragraph = Paragraph>,
    {
        Self::new_with_language(text, metrics, renderer, start_visual_column, None)
    }

    #[cfg(test)]
    pub(super) fn new_with_language<Renderer>(
        text: &str,
        metrics: EditorMetrics,
        renderer: &Renderer,
        start_visual_column: usize,
        language: Option<CjkLanguage>,
    ) -> Self
    where
        Renderer: text::Renderer<Font = Font, Paragraph = Paragraph>,
    {
        Self::new_with_language_and_tabs(text, metrics, renderer, start_visual_column, language, 4)
    }

    #[cfg(test)]
    pub(super) fn new_with_language_and_tabs<Renderer>(
        text: &str,
        metrics: EditorMetrics,
        renderer: &Renderer,
        start_visual_column: usize,
        language: Option<CjkLanguage>,
        tab_width: usize,
    ) -> Self
    where
        Renderer: text::Renderer<Font = Font, Paragraph = Paragraph>,
    {
        Self::new_with_font_runs(
            text,
            metrics,
            renderer,
            start_visual_column,
            tab_width,
            &editor_font_runs(text, language),
            false,
        )
    }

    pub(super) fn new_with_font_runs<Renderer>(
        text: &str,
        metrics: EditorMetrics,
        renderer: &Renderer,
        start_visual_column: usize,
        tab_width: usize,
        font_runs: &[EditorFontRun],
        measure_ascii: bool,
    ) -> Self
    where
        Renderer: text::Renderer<Font = Font, Paragraph = Paragraph>,
    {
        // Projected folds need the font's actual advances for their plates and
        // hit targets; the platform monospace can differ from the column estimate.
        if !measure_ascii && can_use_fast_geometry(text) {
            return Self::Fast {
                text: text.to_owned(),
                character_width: metrics.character_width,
                start_visual_column,
            };
        }

        if requires_fallback_geometry(text) {
            return Self::Tabular {
                text: text.to_owned(),
                character_width: metrics.character_width,
                start_visual_column,
            };
        }

        let expanded = super::rich_text::expand_tabs_for_rendering_with_offset(
            text,
            tab_width,
            start_visual_column,
        );
        let shaped_text = expanded
            .as_ref()
            .map_or(text, |expanded| expanded.text.as_str());
        let expanded_fonts;
        let fonts = if let Some(expanded) = &expanded {
            expanded_fonts = remap_font_runs(font_runs, &expanded.byte_offsets);
            expanded_fonts.as_slice()
        } else {
            font_runs
        };
        let byte_to_grapheme = if let Some(expanded) = &expanded {
            let shaped_table = byte_to_grapheme_table(shaped_text);
            byte_to_grapheme_table(text)
                .into_iter()
                .map(|(byte, _)| {
                    (
                        byte,
                        grapheme_index_for_byte(&shaped_table, expanded.byte_offsets[byte]),
                    )
                })
                .collect()
        } else {
            byte_to_grapheme_table(text)
        };
        Self::Measured {
            text: text.to_owned(),
            paragraph: measured_paragraph(shaped_text, metrics, renderer, fonts),
            byte_to_grapheme,
            fallback_character_width: metrics.character_width,
            start_visual_column,
            expanded_byte_offsets: expanded.map(|expanded| {
                text.char_indices()
                    .map(|(byte, _)| (byte, expanded.byte_offsets[byte]))
                    .chain(std::iter::once((text.len(), expanded.text.len())))
                    .collect()
            }),
        }
    }

    pub(super) fn start_visual_column(&self) -> usize {
        match self {
            Self::Fast {
                start_visual_column,
                ..
            }
            | Self::Tabular {
                start_visual_column,
                ..
            }
            | Self::Measured {
                start_visual_column,
                ..
            } => *start_visual_column,
        }
    }

    pub(super) fn x_for_byte_column(&self, byte_column: usize, tab_width: usize) -> f32 {
        match self {
            Self::Fast {
                text,
                character_width,
                start_visual_column,
            }
            | Self::Tabular {
                text,
                character_width,
                start_visual_column,
            } => fallback_x_for_byte_column(
                text,
                byte_column,
                *character_width,
                tab_width,
                *start_visual_column,
            ),
            Self::Measured {
                text,
                paragraph,
                byte_to_grapheme,
                fallback_character_width,
                start_visual_column,
                ..
            } => {
                let byte_column = clamp_byte_boundary(text, byte_column);
                let grapheme_index = grapheme_index_for_byte(byte_to_grapheme, byte_column);

                paragraph
                    .grapheme_position(0, grapheme_index)
                    .map(|position| position.x)
                    .unwrap_or_else(|| {
                        fallback_x_for_byte_column(
                            text,
                            byte_column,
                            *fallback_character_width,
                            tab_width,
                            *start_visual_column,
                        )
                    })
            }
        }
    }

    pub(super) fn byte_column_for_x(&self, x: f32, tab_width: usize) -> usize {
        match self {
            Self::Fast {
                text,
                character_width,
                start_visual_column,
            }
            | Self::Tabular {
                text,
                character_width,
                start_visual_column,
            } => {
                let visual_column = (x / character_width).floor().max(0.0) as usize;
                byte_column_for_with_offset(
                    text,
                    start_visual_column.saturating_add(visual_column),
                    tab_width,
                    *start_visual_column,
                )
            }
            Self::Measured {
                text,
                paragraph,
                fallback_character_width,
                start_visual_column,
                expanded_byte_offsets,
                ..
            } => paragraph
                .hit_test(Point::new(x.max(0.0), 0.5))
                .map(text::Hit::cursor)
                .map(|offset| {
                    let offset = expanded_byte_offsets.as_ref().map_or(offset, |mapping| {
                        let index = mapping
                            .partition_point(|(_, expanded)| *expanded <= offset)
                            .saturating_sub(1);
                        mapping[index].0
                    });
                    clamp_byte_boundary(text, offset)
                })
                .unwrap_or_else(|| {
                    fallback_byte_column_for_x(
                        text,
                        x,
                        *fallback_character_width,
                        tab_width,
                        *start_visual_column,
                    )
                }),
        }
    }
}

fn measured_paragraph<Renderer>(
    text: &str,
    metrics: EditorMetrics,
    renderer: &Renderer,
    runs: &[EditorFontRun],
) -> Renderer::Paragraph
where
    Renderer: text::Renderer<Font = Font>,
{
    if runs.iter().all(|run| run.font == EDITOR_FONT) {
        return Renderer::Paragraph::with_text(measure_text(text, EDITOR_FONT, metrics, renderer));
    }
    let spans: Vec<text::Span<'_, (), Font>> = runs
        .iter()
        .map(|run| text::Span::new(&text[run.byte_range.clone()]).font(run.font))
        .collect();
    Renderer::Paragraph::with_spans(
        measure_text(text, EDITOR_FONT, metrics, renderer).with_content(spans.as_slice()),
    )
}

fn can_use_fast_geometry(text: &str) -> bool {
    text.bytes().all(|byte| byte.is_ascii())
}

fn requires_fallback_geometry(text: &str) -> bool {
    text.len() > MAX_MEASURED_LINE_BYTES || text.chars().any(|ch| ch != '\t' && ch.is_control())
}

fn fallback_x_for_byte_column(
    text: &str,
    byte_column: usize,
    character_width: f32,
    tab_width: usize,
    start_visual_column: usize,
) -> f32 {
    visual_column_for_with_offset(text, byte_column, tab_width, start_visual_column)
        .saturating_sub(start_visual_column) as f32
        * character_width
}

fn fallback_byte_column_for_x(
    text: &str,
    x: f32,
    character_width: f32,
    tab_width: usize,
    start_visual_column: usize,
) -> usize {
    let target = (x / character_width).floor().max(0.0) as usize;

    byte_column_for_with_offset(
        text,
        start_visual_column.saturating_add(target),
        tab_width,
        start_visual_column,
    )
}

fn measure_text<'a, Renderer>(
    content: &'a str,
    font: Font,
    metrics: EditorMetrics,
    renderer: &Renderer,
) -> text::Text<&'a str, Font>
where
    Renderer: text::Renderer<Font = Font>,
{
    text::Text {
        content,
        bounds: Size::new(f32::INFINITY, metrics.line_height),
        size: Pixels((metrics.line_height / 1.25).max(8.0)),
        line_height: text::LineHeight::Absolute(Pixels(metrics.line_height)),
        font,
        align_x: text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping: EDITOR_TEXT_SHAPING,
        wrapping: text::Wrapping::None,
        ellipsis: text::Ellipsis::None,
        hint_factor: renderer.scale_factor(),
    }
}

pub(super) fn byte_to_grapheme_table(text: &str) -> Vec<(usize, usize)> {
    let mut table = Vec::new();

    for (grapheme_index, (byte_offset, grapheme)) in text.grapheme_indices(true).enumerate() {
        table.push((byte_offset, grapheme_index));
        table.push((byte_offset + grapheme.len(), grapheme_index + 1));
    }

    table.sort_unstable_by_key(|(byte_offset, _)| *byte_offset);
    table.dedup_by_key(|(byte_offset, _)| *byte_offset);
    table
}

fn grapheme_index_for_byte(table: &[(usize, usize)], byte_column: usize) -> usize {
    table
        .binary_search_by_key(&byte_column, |(byte_offset, _)| *byte_offset)
        .map(|index| table[index].1)
        .unwrap_or_else(|index| {
            index
                .checked_sub(1)
                .and_then(|previous| table.get(previous))
                .map(|(_, grapheme_index)| *grapheme_index)
                .unwrap_or(0)
        })
}

fn clamp_byte_boundary(text: &str, byte_column: usize) -> usize {
    let mut column = byte_column.min(text.len());

    while column > 0 && !text.is_char_boundary(column) {
        column -= 1;
    }

    column
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::decoration::DecorationSettings;
    use crate::editor::fold::FoldModel;
    use crate::editor::layout::ScrollOffset;

    #[test]
    fn cached_caret_geometry_survives_scroll_reversal_and_repositions_ime() {
        let _font_guard = crate::font_system_test_guard();
        let buffer = EditorBuffer::from_text("caf\u{e9}\t漢字\n".repeat(40));
        let context = CjkContext::from_buffer(&buffer);
        let folds = FoldModel::default();
        let viewport = ViewportModel::new_with_buffer(&buffer, &folds, 4);
        let decorations = DecorationModel::from_folds(
            DecorationSettings::default(),
            buffer.line_count(),
            &folds,
            vec![],
        );
        let mut cache = LineGeometryCache::<()>::default();
        let position = EditorPosition::new(8, "caf\u{e9}\t漢".len());
        let mut positions = Vec::new();
        for first_visible_row in [0, 4, 8, 12, 8, 4, 0] {
            for horizontal_px in [0.0, 18.0, 0.0] {
                let layout = EditorLayout::new(
                    EditorMetrics::default(),
                    ScrollOffset {
                        first_visible_row,
                        horizontal_px,
                    },
                    320.0,
                    150.0,
                );
                let actual = measured_position_point_with_cache(
                    &buffer,
                    &viewport,
                    &decorations,
                    layout,
                    position,
                    None,
                    &(),
                    Some(&context),
                    &mut cache,
                );
                assert_eq!(
                    actual,
                    measured_position_point_with_context(
                        &buffer,
                        &viewport,
                        &decorations,
                        layout,
                        position,
                        None,
                        &(),
                        Some(&context)
                    )
                );
                positions.push(actual);
            }
        }
        assert_ne!(
            positions[0].x, positions[1].x,
            "horizontal scroll updates IME coordinates"
        );
        assert_ne!(
            positions[0].y, positions[3].y,
            "vertical scroll updates IME coordinates"
        );
        assert_eq!(
            positions.first(),
            positions.last(),
            "reversal restores the original position"
        );
        assert_eq!(
            cache.build_count(),
            1,
            "caret shaping is retained across scroll-only updates"
        );
        assert!(
            matches!(
                cache.geometry(cache.slot_for_row(8)),
                Some(LineGeometry::Measured { .. })
            ),
            "exercise measured Unicode/tab geometry, not the ASCII fast path"
        );
    }

    #[test]
    fn cached_caret_geometry_preserves_wrap_affinity_and_reflow() {
        let _font_guard = crate::font_system_test_guard();
        let buffer = EditorBuffer::from_text("abcdefghij\n漢字かな");
        let folds = FoldModel::default();
        let context = CjkContext::from_buffer(&buffer);
        let mut viewport = ViewportModel::new_wrapped(&buffer, &folds, 4, 4);
        let decorations = DecorationModel::from_folds(
            DecorationSettings::default(),
            buffer.line_count(),
            &folds,
            vec![],
        );
        let layout = EditorLayout::new(
            EditorMetrics::default(),
            ScrollOffset {
                first_visible_row: 0,
                horizontal_px: 80.0,
            },
            320.0,
            150.0,
        );
        let position = EditorPosition::new(0, 4);
        let mut cache = LineGeometryCache::<()>::default();
        let mut upstream = None;
        for affinity in [Some(0), None, Some(0), None] {
            let actual = measured_position_point_with_cache(
                &buffer,
                &viewport,
                &decorations,
                layout,
                position,
                affinity,
                &(),
                Some(&context),
                &mut cache,
            );
            assert_eq!(
                actual,
                measured_position_point_with_context(
                    &buffer,
                    &viewport,
                    &decorations,
                    layout,
                    position,
                    affinity,
                    &(),
                    Some(&context)
                )
            );
            if affinity.is_some() {
                upstream = Some(actual);
            } else {
                assert!(actual.y > upstream.unwrap().y);
                assert!(actual.x < upstream.unwrap().x);
            }
        }
        assert_eq!(
            cache.build_count(),
            2,
            "wrapped fragments retain separate geometry"
        );
        viewport = ViewportModel::new_wrapped(&buffer, &folds, 6, 4);
        let actual = measured_position_point_with_cache(
            &buffer,
            &viewport,
            &decorations,
            layout,
            position,
            None,
            &(),
            Some(&context),
            &mut cache,
        );
        assert_eq!(
            actual,
            measured_position_point_with_context(
                &buffer,
                &viewport,
                &decorations,
                layout,
                position,
                None,
                &(),
                Some(&context)
            )
        );
        assert_eq!(
            cache.build_count(),
            3,
            "reflow changes fragment text and geometry"
        );
    }

    #[test]
    fn cached_caret_geometry_invalidates_text_metrics_tabs_context_and_fonts() {
        let _font_guard = crate::font_system_test_guard();
        let mut buffer = EditorBuffer::from_text("漢字");
        let folds = FoldModel::default();
        let viewport = ViewportModel::new_with_buffer(&buffer, &folds, 4);
        let mut decorations = DecorationModel::from_folds(
            DecorationSettings::default(),
            buffer.line_count(),
            &folds,
            vec![],
        );
        let japanese = CjkContext::from_buffer(&EditorBuffer::from_text("かな漢字"));
        let korean = CjkContext::from_buffer(&EditorBuffer::from_text("한글漢字"));
        assert_ne!(japanese.language_for_line(0), korean.language_for_line(0));
        let mut layout =
            EditorLayout::new(EditorMetrics::default(), ScrollOffset::ZERO, 320.0, 150.0);
        let position = EditorPosition::new(0, "漢".len());
        let mut cache = LineGeometryCache::<()>::default();
        let check = |cache: &mut LineGeometryCache<()>,
                     buffer: &EditorBuffer,
                     decorations: &DecorationModel,
                     layout: EditorLayout,
                     context: &CjkContext| {
            let actual = measured_position_point_with_cache(
                buffer,
                &viewport,
                decorations,
                layout,
                position,
                None,
                &(),
                Some(context),
                cache,
            );
            assert_eq!(
                actual,
                measured_position_point_with_context(
                    buffer,
                    &viewport,
                    decorations,
                    layout,
                    position,
                    None,
                    &(),
                    Some(context)
                )
            );
        };
        check(&mut cache, &buffer, &decorations, layout, &japanese);
        check(&mut cache, &buffer, &decorations, layout, &japanese);
        assert_eq!(cache.build_count(), 1);
        check(&mut cache, &buffer, &decorations, layout, &korean);
        assert_eq!(
            cache.build_count(),
            2,
            "same Han text must respect changed logical language context"
        );
        buffer = EditorBuffer::from_text("漢語");
        check(&mut cache, &buffer, &decorations, layout, &korean);
        assert_eq!(cache.build_count(), 3);
        layout.metrics.character_width += 2.0;
        check(&mut cache, &buffer, &decorations, layout, &korean);
        assert_eq!(cache.build_count(), 4);
        decorations.settings.indent_width = 8;
        check(&mut cache, &buffer, &decorations, layout, &korean);
        assert_eq!(cache.build_count(), 5);
        // An owned font load always advances the generation, even when this
        // bundled face was previously loaded from a borrowed static address.
        iced::advanced::graphics::text::font_system()
            .write()
            .unwrap()
            .load_font(std::borrow::Cow::Owned(
                include_bytes!("../../../vendor/iced/graphics/fonts/Iced-Icons.ttf").to_vec(),
            ));
        check(&mut cache, &buffer, &decorations, layout, &korean);
        assert_eq!(
            cache.build_count(),
            6,
            "loaded-font generation invalidates cached geometry"
        );
    }

    #[test]
    fn geometry_cache_growth_and_smaller_pages_retain_recent_rows() {
        let _font_guard = crate::font_system_test_guard();
        let mut cache = LineGeometryCache::<()>::default();
        let metrics = EditorMetrics::default();
        cache.ensure_capacity(37);
        let rows = [0, 127, 255, 257];
        for row in rows {
            cache.ensure(
                row,
                0,
                &format!("row {row}"),
                metrics,
                &(),
                None,
                4,
                &[],
                false,
            );
        }
        assert_eq!(cache.build_count(), rows.len());
        // Crossing a capacity boundary must rehash, not discard, retained rows.
        // Short pages after scrolling to the end must not shrink the cache again.
        for page_rows in [256, 37, 2, 0, 256] {
            cache.ensure_capacity(page_rows);
            for row in rows.into_iter().rev() {
                let slot = cache.ensure(
                    row,
                    0,
                    &format!("row {row}"),
                    metrics,
                    &(),
                    None,
                    4,
                    &[],
                    false,
                );
                assert_eq!(cache.entries[slot].as_ref().unwrap().visible_row, row);
            }
        }
        assert_eq!(
            cache.build_count(),
            rows.len(),
            "resize and scroll reversal preserve cached paragraphs"
        );
    }

    #[test]
    fn wrapped_fragment_geometry_preserves_logical_tab_stops() {
        let metrics = EditorMetrics::new(18.0, 10.0);
        let geometry = LineGeometry::<()>::new_with_visual_offset("x\ty", metrics, &(), 5);

        assert_eq!(geometry.x_for_byte_column(1, 4), 10.0);
        assert_eq!(geometry.x_for_byte_column(2, 4), 30.0);
        assert_eq!(geometry.byte_column_for_x(29.0, 4), 1);
        assert_eq!(geometry.byte_column_for_x(30.0, 4), 2);
    }

    #[test]
    fn wrapped_cache_keeps_fragments_of_same_line_separate_and_reflows_offsets() {
        let _font_guard = crate::font_system_test_guard();
        let metrics = EditorMetrics::default();
        let mut cache = LineGeometryCache::<()>::default();
        cache.ensure_capacity(2);
        let first = cache.ensure(0, 0, "first", metrics, &(), None, 4, &[], false);
        let second = cache.ensure(1, 5, "\tend", metrics, &(), None, 4, &[], false);
        assert_ne!(first, second);
        assert_eq!(cache.geometry(first).unwrap().x_for_byte_column(5, 4), 40.0);
        assert_eq!(
            cache.geometry(second).unwrap().x_for_byte_column(1, 4),
            24.0
        );

        cache.ensure(1, 6, "\tend", metrics, &(), None, 4, &[], false);
        assert_eq!(
            cache.geometry(second).unwrap().x_for_byte_column(1, 4),
            16.0
        );
        assert_eq!(cache.build_count(), 3);
    }

    #[test]
    fn wrapped_hit_testing_and_ime_geometry_use_visual_rows_and_caret_affinity() {
        let buffer = EditorBuffer::from_text("abcdefghij\nnext");
        let folds = FoldModel::default();
        let viewport = ViewportModel::new_wrapped(&buffer, &folds, 4, 4);
        let decorations = DecorationModel::from_folds(
            DecorationSettings::default(),
            buffer.line_count(),
            &folds,
            vec![],
        );
        let metrics = EditorMetrics::default();
        let layout = EditorLayout::new(
            metrics,
            ScrollOffset {
                first_visible_row: 0,
                horizontal_px: 80.0,
            },
            300.0,
            100.0,
        );
        let origin = metrics.text_origin_x(&decorations);
        let point = Point::new(
            origin + metrics.character_width + 0.1,
            metrics.padding_top + metrics.line_height + 1.0,
        );

        assert_eq!(
            measured_text_hit_target(point, layout, &buffer, &viewport, &decorations, &()),
            HitTarget::Text(EditorPosition::new(0, 5))
        );
        assert_eq!(
            measured_position_point(
                &buffer,
                &viewport,
                &decorations,
                layout,
                EditorPosition::new(0, 5),
                None,
                &()
            ),
            Point::new(
                origin + metrics.character_width,
                metrics.padding_top + metrics.line_height
            )
        );
        assert_eq!(
            measured_position_point(
                &buffer,
                &viewport,
                &decorations,
                layout,
                EditorPosition::new(0, 4),
                Some(0),
                &()
            ),
            Point::new(origin + 4.0 * metrics.character_width, metrics.padding_top)
        );
        assert_eq!(
            measured_position_point(
                &buffer,
                &viewport,
                &decorations,
                layout,
                EditorPosition::new(0, 4),
                None,
                &()
            ),
            Point::new(origin, metrics.padding_top + metrics.line_height)
        );
    }

    #[test]
    fn line_geometry_cache_reuses_page_rows_across_wheel_scroll_frames() {
        let mut cache = LineGeometryCache::<()>::default();
        cache.ensure_capacity(37);

        for frame in 0..2 {
            for line in frame..frame + 37 {
                let slot = cache.slot_for_row(line);
                let is_hit = cache.entries[slot].as_ref().is_some_and(|entry| {
                    entry.visible_row == line
                        && entry.text == format!("line {line}")
                        && entry.metrics == EditorMetrics::default()
                        && entry.scale_factor.is_none()
                });

                if !is_hit {
                    cache.build_count += 1;
                    cache.entries[slot] = Some(LineGeometryEntry {
                        visible_row: line,
                        start_visual_column: 0,
                        text: format!("line {line}"),
                        metrics: EditorMetrics::default(),
                        scale_factor: None,
                        language: None,
                        tab_width: 4,
                        font_version: iced::advanced::graphics::text::Version::default(),
                        font_runs: Vec::new(),
                        measure_ascii: false,
                        geometry: LineGeometry::Fast {
                            text: format!("line {line}"),
                            character_width: EditorMetrics::default().character_width,
                            start_visual_column: 0,
                        },
                    });
                }
            }
        }

        assert_eq!(
            cache.build_count(),
            38,
            "second scroll frame should reuse 36 of 37 row geometries"
        );
    }

    #[test]
    fn long_or_control_heavy_unicode_lines_skip_full_paragraph_measurement() {
        assert!(!requires_fallback_geometry("caf\u{00e9}"));
        assert!(requires_fallback_geometry("caf\u{00e9}\0"));
        assert!(requires_fallback_geometry(
            &"\u{00e9}".repeat(MAX_MEASURED_LINE_BYTES)
        ));
    }
}
