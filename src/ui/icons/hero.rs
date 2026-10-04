use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Layout, image, layout, mouse, renderer};
use iced::{Color, Element, Length, Rectangle, Size, Theme};

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use super::mask;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeroIcon {
    ArrowTurnDownLeft,
    ChevronDown,
    ChevronRight,
    Minus,
    Plus,
    QuestionMarkCircle,
    XMark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeroIconAsset {
    ArrowTurnDownLeft,
    ChevronDown,
    ChevronRight,
    Minus,
    Plus,
    QuestionMarkCircle,
    XMark,
}

impl HeroIconAsset {
    pub fn rgba_bytes(self) -> &'static [u8] {
        match self {
            HeroIconAsset::ArrowTurnDownLeft => {
                include_bytes!("../../../assets/icons/heroicons/rgba/arrow-turn-down-left.rgba")
            }
            HeroIconAsset::ChevronDown => {
                include_bytes!("../../../assets/icons/heroicons/rgba/chevron-down.rgba")
            }
            HeroIconAsset::ChevronRight => {
                include_bytes!("../../../assets/icons/heroicons/rgba/chevron-right.rgba")
            }
            HeroIconAsset::Minus => {
                include_bytes!("../../../assets/icons/heroicons/rgba/minus.rgba")
            }
            HeroIconAsset::Plus => include_bytes!("../../../assets/icons/heroicons/rgba/plus.rgba"),
            HeroIconAsset::QuestionMarkCircle => {
                include_bytes!("../../../assets/icons/heroicons/rgba/question-mark-circle.rgba")
            }
            HeroIconAsset::XMark => {
                include_bytes!("../../../assets/icons/heroicons/rgba/x-mark.rgba")
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum IconTone {
    Text,
    Muted,
}

pub fn icon<Message>(icon: HeroIcon, size: u32, tone: IconTone) -> Element<'static, Message>
where
    Message: 'static,
{
    Element::new(ThemedHeroIcon { icon, size, tone })
}

pub fn handle_with_color(icon: HeroIcon, color: Color) -> image::Handle {
    static CACHE: mask::IconCache<HeroIcon> = LazyLock::new(|| Mutex::new(HashMap::new()));

    mask::handle_with_color(icon, color, &CACHE, icon_bytes)
}

#[derive(Debug, Clone, Copy)]
struct ThemedHeroIcon {
    icon: HeroIcon,
    size: u32,
    tone: IconTone,
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for ThemedHeroIcon
where
    Renderer: image::Renderer<Handle = image::Handle>,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.size as f32),
            height: Length::Fixed(self.size as f32),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(self.size as f32, self.size as f32))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        renderer.draw_image(
            image::Image::new(handle_with_color(self.icon, icon_color(theme, self.tone)))
                .filter_method(image::FilterMethod::Linear),
            bounds,
            bounds,
        );
    }
}

fn icon_color(theme: &Theme, tone: IconTone) -> Color {
    let text = mask::theme_text_color(theme);

    match tone {
        IconTone::Text => text,
        IconTone::Muted => text.scale_alpha(0.72),
    }
}

fn icon_bytes(icon: HeroIcon) -> &'static [u8] {
    hero_asset(icon).rgba_bytes()
}

fn hero_asset(icon: HeroIcon) -> HeroIconAsset {
    match icon {
        HeroIcon::ArrowTurnDownLeft => HeroIconAsset::ArrowTurnDownLeft,
        HeroIcon::ChevronDown => HeroIconAsset::ChevronDown,
        HeroIcon::ChevronRight => HeroIconAsset::ChevronRight,
        HeroIcon::Minus => HeroIconAsset::Minus,
        HeroIcon::Plus => HeroIconAsset::Plus,
        HeroIcon::QuestionMarkCircle => HeroIconAsset::QuestionMarkCircle,
        HeroIcon::XMark => HeroIconAsset::XMark,
    }
}
