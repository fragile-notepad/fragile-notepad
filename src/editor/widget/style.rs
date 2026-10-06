use iced::{Color, Theme};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorStyle {
    pub surface: Color,
    pub gutter: Color,
    pub text: Color,
    pub line_numbers: Color,
    pub fold_controls: Color,
    pub fold_control_background: Color,
    pub active_line: Color,
    pub selection: Color,
    pub indent_guides: [Color; 4],
    pub whitespace_markers: Color,
    pub hidden_line_indicators: Color,
    pub caret: Color,
    pub syntax_fallback_text: Color,
}

impl EditorStyle {
    pub fn from_theme(theme: &Theme) -> Self {
        let palette = theme.palette();
        let is_dark = palette.is_dark;
        let (surface, gutter, text, muted, faint, active_line, selection, guide, fold_background) =
            if is_dark {
                (
                    Color::from_rgb8(26, 27, 29),
                    Color::from_rgb8(33, 34, 37),
                    Color::from_rgb8(232, 233, 235),
                    Color::from_rgb8(172, 175, 181),
                    Color::from_rgb8(131, 135, 142),
                    Color::from_rgb8(33, 34, 37),
                    Color::from_rgba(64.0 / 255.0, 156.0 / 255.0, 1.0, 0.28),
                    [
                        Color::from_rgba(64.0 / 255.0, 156.0 / 255.0, 1.0, 0.18),
                        Color::from_rgba(61.0 / 255.0, 190.0 / 255.0, 167.0 / 255.0, 0.18),
                        Color::from_rgba(225.0 / 255.0, 174.0 / 255.0, 75.0 / 255.0, 0.18),
                        Color::from_rgba(177.0 / 255.0, 131.0 / 255.0, 232.0 / 255.0, 0.18),
                    ],
                    Color::from_rgb8(37, 38, 41),
                )
            } else {
                (
                    Color::from_rgb8(255, 255, 255),
                    Color::from_rgb8(245, 245, 245),
                    Color::from_rgb8(32, 33, 35),
                    Color::from_rgb8(96, 98, 102),
                    Color::from_rgb8(128, 131, 136),
                    Color::from_rgb8(247, 247, 248),
                    Color::from_rgba(0.0, 112.0 / 255.0, 204.0 / 255.0, 0.24),
                    [
                        Color::from_rgba(0.0, 112.0 / 255.0, 204.0 / 255.0, 0.16),
                        Color::from_rgba(0.0, 135.0 / 255.0, 116.0 / 255.0, 0.16),
                        Color::from_rgba(176.0 / 255.0, 117.0 / 255.0, 20.0 / 255.0, 0.16),
                        Color::from_rgba(136.0 / 255.0, 77.0 / 255.0, 190.0 / 255.0, 0.16),
                    ],
                    Color::from_rgb8(250, 250, 250),
                )
            };

        Self {
            surface,
            gutter,
            text,
            line_numbers: faint,
            fold_controls: muted,
            fold_control_background: fold_background,
            active_line,
            selection,
            indent_guides: guide,
            whitespace_markers: faint,
            hidden_line_indicators: muted,
            caret: text,
            syntax_fallback_text: text,
        }
    }
}
