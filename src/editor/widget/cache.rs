use iced::{Color, Pixels, Size};

use super::font::EditorFontRun;

const RICH_PARAGRAPH_CACHE_LIMIT: usize = 2048;

#[derive(Debug)]
pub(super) struct RichParagraphCache<Paragraph> {
    entries: Vec<Option<RichParagraphEntry<Paragraph>>>,
    #[cfg(test)]
    probe_count: usize,
}

impl<Paragraph> Default for RichParagraphCache<Paragraph> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            #[cfg(test)]
            probe_count: 0,
        }
    }
}

#[derive(Debug)]
struct RichParagraphEntry<Paragraph> {
    line: usize,
    text: String,
    syntax_spans: Vec<SyntaxSpanKey>,
    font_runs: Vec<EditorFontRun>,
    visible_start: usize,
    bounds: Size,
    size: Pixels,
    line_height: f32,
    scale_factor: Option<f32>,
    font_version: iced::advanced::graphics::text::Version,
    last_used_frame: u64,
    paragraph: Paragraph,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SyntaxSpanKey {
    pub start: usize,
    pub end: usize,
    pub color: Option<Color>,
}

impl<Paragraph> RichParagraphCache<Paragraph> {
    pub(super) fn get_or_insert_with(
        &mut self,
        line: usize,
        text: &str,
        syntax_spans: &[SyntaxSpanKey],
        visible_start: usize,
        bounds: Size,
        size: Pixels,
        line_height: f32,
        scale_factor: Option<f32>,
        frame_id: u64,
        build: impl FnOnce() -> Paragraph,
    ) -> &Paragraph {
        self.get_or_insert_with_fonts(
            line,
            text,
            syntax_spans,
            &[],
            visible_start,
            bounds,
            size,
            line_height,
            scale_factor,
            frame_id,
            build,
        )
    }

    pub(super) fn get_or_insert_with_fonts(
        &mut self,
        line: usize,
        text: &str,
        syntax_spans: &[SyntaxSpanKey],
        font_runs: &[EditorFontRun],
        visible_start: usize,
        bounds: Size,
        size: Pixels,
        line_height: f32,
        scale_factor: Option<f32>,
        frame_id: u64,
        build: impl FnOnce() -> Paragraph,
    ) -> &Paragraph {
        let font_version = iced::advanced::graphics::text::font_system()
            .read()
            .expect("Read font system")
            .version();
        if self.entries.is_empty() {
            self.entries
                .resize_with(RICH_PARAGRAPH_CACHE_LIMIT, || None);
        }

        #[cfg(test)]
        {
            self.probe_count += 1;
        }

        let slot = line % self.entries.len();
        let is_hit = self.entries[slot].as_ref().is_some_and(|entry| {
            entry.line == line
                && entry.text == text
                && entry.syntax_spans.as_slice() == syntax_spans
                && entry.font_runs.as_slice() == font_runs
                && entry.visible_start == visible_start
                && entry.bounds == bounds
                && entry.size == size
                && entry.line_height == line_height
                && entry.scale_factor == scale_factor
                && entry.font_version == font_version
        });

        if is_hit {
            let entry = self.entries[slot]
                .as_mut()
                .expect("rich paragraph cache hit entry");
            entry.last_used_frame = frame_id;
            return &entry.paragraph;
        }

        self.entries[slot] = Some(RichParagraphEntry {
            line,
            text: text.to_owned(),
            syntax_spans: syntax_spans.to_vec(),
            font_runs: font_runs.to_vec(),
            visible_start,
            bounds,
            size,
            line_height,
            scale_factor,
            font_version,
            last_used_frame: frame_id,
            paragraph: build(),
        });

        &self.entries[slot]
            .as_ref()
            .expect("inserted rich paragraph cache entry")
            .paragraph
    }

    pub(super) fn prune(&mut self, _frame_id: u64) {
        // The cache is direct-mapped and bounded by RICH_PARAGRAPH_CACHE_LIMIT.
        // Entries are replaced in their line slot, so no LRU scan is needed.
    }

    #[cfg(test)]
    pub(super) fn probe_count(&self) -> usize {
        self.probe_count
    }
}
