use iced::advanced::graphics::geometry;
use iced::{Color, Element, Radians, Rectangle};

use super::vector::{self, VectorIcon, compiled};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeroIcon {
    ArrowTurnDownLeft,
    ArrowTurnDownRight,
    ChevronDown,
    ChevronRight,
    ExclamationTriangle,
    Minus,
    Plus,
    QuestionMarkCircle,
    XMark,
}

#[derive(Debug, Clone, Copy)]
pub enum IconTone {
    Text,
    Muted,
    Warning,
}

pub fn icon<Message: 'static>(
    icon: HeroIcon,
    size: u32,
    tone: IconTone,
) -> Element<'static, Message> {
    icon_with_opacity(icon, size, tone, 1.0)
}

pub fn icon_with_opacity<Message: 'static>(
    icon: HeroIcon,
    size: u32,
    tone: IconTone,
    opacity: f32,
) -> Element<'static, Message> {
    let color = match tone {
        IconTone::Text => crate::ui::styles::shortcut_text_color,
        IconTone::Muted => {
            |theme: &iced::Theme| crate::ui::styles::shortcut_text_color(theme).scale_alpha(0.72)
        }
        IconTone::Warning => |theme: &iced::Theme| theme.palette().warning.base.color,
    };
    vector::icon(asset(icon), size, color, opacity.clamp(0.0, 1.0))
}

pub fn draw(
    renderer: &mut impl geometry::Renderer,
    icon: HeroIcon,
    bounds: Rectangle,
    clip: Rectangle,
    color: Color,
    rotation: Radians,
) {
    asset(icon).draw(renderer, bounds, clip, color, 1.0, rotation);
}

fn asset(icon: HeroIcon) -> &'static VectorIcon {
    match icon {
        HeroIcon::ArrowTurnDownLeft => &compiled::HEROICONS_ARROW_TURN_DOWN_LEFT,
        HeroIcon::ArrowTurnDownRight => &compiled::HEROICONS_ARROW_TURN_DOWN_RIGHT,
        HeroIcon::ChevronDown => &compiled::HEROICONS_CHEVRON_DOWN,
        HeroIcon::ChevronRight => &compiled::HEROICONS_CHEVRON_RIGHT,
        HeroIcon::ExclamationTriangle => &compiled::HEROICONS_EXCLAMATION_TRIANGLE,
        HeroIcon::Minus => &compiled::HEROICONS_MINUS,
        HeroIcon::Plus => &compiled::HEROICONS_PLUS,
        HeroIcon::QuestionMarkCircle => &compiled::HEROICONS_QUESTION_MARK_CIRCLE,
        HeroIcon::XMark => &compiled::HEROICONS_X_MARK,
    }
}
