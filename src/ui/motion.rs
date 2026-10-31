//! Widget-local entrances, disclosures, and search activity indicators.

mod status_light;
mod surfaces;

pub use status_light::{StatusLightState, status_light};
pub use surfaces::{dropdown, dropdown_with_key, fade, popup, popup_with_progress, reveal};

#[cfg(test)]
mod test_support;

// Fade paint colors instead of covering the editor behind a translucent modal.
pub(super) fn fade_container(
    mut style: iced::widget::container::Style,
    opacity: f32,
) -> iced::widget::container::Style {
    style.background = style.background.map(|color| color.scale_alpha(opacity));
    style.text_color = style.text_color.map(|color| color.scale_alpha(opacity));
    style.border.color = style.border.color.scale_alpha(opacity);
    style.shadow.color = style.shadow.color.scale_alpha(opacity);
    style
}

pub(super) fn fade_button(
    mut style: iced::widget::button::Style,
    opacity: f32,
) -> iced::widget::button::Style {
    style.background = style.background.map(|color| color.scale_alpha(opacity));
    style.text_color = style.text_color.scale_alpha(opacity);
    style.border.color = style.border.color.scale_alpha(opacity);
    style.shadow.color = style.shadow.color.scale_alpha(opacity);
    style
}
