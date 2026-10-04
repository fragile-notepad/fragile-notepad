use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Layout, image, layout, mouse, renderer};
use iced::{Color, Element, Length, Rectangle, Size, Theme};

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use super::mask;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutIcon {
    Command,
    Option,
    PinAngle,
    PinAngleFill,
    Shift,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutIconAsset {
    Command,
    Option,
    PinAngle,
    PinAngleFill,
    ShiftFill,
    Shift,
    Windows,
}

impl ShortcutIconAsset {
    pub fn rgba_bytes(self) -> &'static [u8] {
        match self {
            ShortcutIconAsset::Command => {
                include_bytes!("../../../assets/icons/bootstrap/rgba/command.rgba")
            }
            ShortcutIconAsset::Option => {
                include_bytes!("../../../assets/icons/bootstrap/rgba/option.rgba")
            }
            ShortcutIconAsset::PinAngle => {
                include_bytes!("../../../assets/icons/bootstrap/rgba/pin-angle.rgba")
            }
            ShortcutIconAsset::PinAngleFill => {
                include_bytes!("../../../assets/icons/bootstrap/rgba/pin-angle-fill.rgba")
            }
            ShortcutIconAsset::Shift => {
                include_bytes!("../../../assets/icons/bootstrap/rgba/shift.rgba")
            }
            ShortcutIconAsset::ShiftFill => {
                include_bytes!("../../../assets/icons/bootstrap/rgba/shift-fill.rgba")
            }
            ShortcutIconAsset::Windows => {
                include_bytes!("../../../assets/icons/bootstrap/rgba/windows.rgba")
            }
        }
    }
}

pub fn pin_handle(is_pinned: bool) -> image::Handle {
    let asset = if is_pinned {
        ShortcutIconAsset::PinAngleFill
    } else {
        ShortcutIconAsset::PinAngle
    };

    static CACHE: LazyLock<Mutex<HashMap<ShortcutIconAsset, image::Handle>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    let mut cache = CACHE.lock().expect("shortcut icon cache");
    cache
        .entry(asset)
        .or_insert_with(|| {
            image::Handle::from_rgba(
                super::ICON_SIZE,
                super::ICON_SIZE,
                asset.rgba_bytes().to_vec(),
            )
        })
        .clone()
}

pub fn icon_with_color<Message>(
    icon: ShortcutIcon,
    size: u32,
    color: fn(&Theme) -> Color,
) -> Element<'static, Message>
where
    Message: 'static,
{
    Element::new(ThemedShortcutIcon { icon, size, color })
}

#[derive(Debug, Clone, Copy)]
struct ThemedShortcutIcon {
    icon: ShortcutIcon,
    size: u32,
    color: fn(&Theme) -> Color,
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for ThemedShortcutIcon
where
    Renderer: image::Renderer<Handle = image::Handle>,
{
    fn size(&self) -> Size<Length> {
        Size::new(
            Length::Fixed(self.size as f32),
            Length::Fixed(self.size as f32),
        )
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
            image::Image::new(handle_with_color(self.icon, (self.color)(theme)))
                .filter_method(image::FilterMethod::Linear),
            bounds,
            bounds,
        );
    }
}

fn handle_with_color(icon: ShortcutIcon, color: iced::Color) -> image::Handle {
    static CACHE: mask::IconCache<ShortcutIcon> = LazyLock::new(|| Mutex::new(HashMap::new()));

    mask::handle_with_color(icon, color, &CACHE, icon_bytes)
}

fn icon_bytes(icon: ShortcutIcon) -> &'static [u8] {
    shortcut_asset(icon).rgba_bytes()
}

fn shortcut_asset(icon: ShortcutIcon) -> ShortcutIconAsset {
    match icon {
        ShortcutIcon::Command => ShortcutIconAsset::Command,
        ShortcutIcon::Option => ShortcutIconAsset::Option,
        ShortcutIcon::PinAngle => ShortcutIconAsset::PinAngle,
        ShortcutIcon::PinAngleFill => ShortcutIconAsset::PinAngleFill,
        ShortcutIcon::Shift => ShortcutIconAsset::Shift,
        ShortcutIcon::Windows => ShortcutIconAsset::Windows,
    }
}
