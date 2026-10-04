use iced::advanced::layout;
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Layout, Shell, Widget};
use iced::widget::{button, column, container, mouse_area, row, scrollable, space, text};
use iced::{Center, Element, Event, Fill, Length, Point, Rectangle, Size};

use crate::core::{KeyBinding, ShortcutDisplay, ShortcutDisplayPart, ShortcutModifierIcon};
use crate::message::{Menu, MenuPath, Message};
use crate::ui::icons::hero::{self, HeroIcon, IconTone};
use crate::ui::icons::shortcut::{self, ShortcutIcon};
use crate::ui::{motion, styles};

const ROW_HEIGHT: f32 = 27.0;
const PANEL_PADDING: u16 = 3;
const PANEL_OUTER_PADDING: u16 = 4;
const LABEL_TEXT_SIZE: f32 = 13.0;
const SHORTCUT_TEXT_SIZE: f32 = 12.0;
const SUBMENU_ICON_WIDTH: f32 = 14.0;
const ROW_HORIZONTAL_PADDING: f32 = 18.0;
const PANEL_HORIZONTAL_PADDING: f32 = 8.0;
const LABEL_SHORTCUT_GAP: f32 = 24.0;
const LABEL_SUBMENU_GAP: f32 = 16.0;
const TEXT_WIDTH_FACTOR: f32 = 0.62;
const SHORTCUT_ICON_SIZE: u32 = 13;
const SHORTCUT_PART_GAP: f32 = 3.0;

#[derive(Debug, Clone)]
pub enum MenuNode {
    Item {
        label: String,
        shortcut: Option<MenuShortcutHint>,
        message: Message,
    },
    Disabled {
        label: String,
        shortcut: Option<MenuShortcutHint>,
    },
    Submenu {
        id: String,
        label: String,
        children: Vec<MenuNode>,
    },
    Separator,
}

#[derive(Debug, Clone)]
pub enum MenuShortcutHint {
    Text(String),
    Binding(ShortcutDisplay),
}

#[derive(Debug, Clone)]
pub struct MenuTree {
    pub entries: Vec<MenuNode>,
    pub width: f32,
    pub max_height: Option<f32>,
}

impl MenuTree {
    pub fn new(entries: Vec<MenuNode>, width: f32) -> Self {
        Self {
            entries,
            width,
            max_height: None,
        }
    }

    pub fn max_height(mut self, height: f32) -> Self {
        self.max_height = Some(height);
        self
    }
}

impl MenuShortcutHint {
    fn intrinsic_width(&self) -> f32 {
        match self {
            Self::Text(shortcut) => text_width(shortcut, SHORTCUT_TEXT_SIZE),
            Self::Binding(binding) => binding_width(binding),
        }
    }
}

impl MenuNode {
    fn intrinsic_width(&self) -> f32 {
        match self {
            Self::Item {
                label, shortcut, ..
            } => {
                ROW_HORIZONTAL_PADDING
                    + text_width(label, LABEL_TEXT_SIZE)
                    + shortcut.as_ref().map_or(0.0, |shortcut| {
                        LABEL_SHORTCUT_GAP + shortcut.intrinsic_width()
                    })
            }
            Self::Disabled { label, shortcut } => {
                ROW_HORIZONTAL_PADDING
                    + text_width(label, LABEL_TEXT_SIZE)
                    + shortcut.as_ref().map_or(0.0, |shortcut| {
                        LABEL_SHORTCUT_GAP + shortcut.intrinsic_width()
                    })
            }
            Self::Submenu { label, .. } => {
                ROW_HORIZONTAL_PADDING
                    + text_width(label, LABEL_TEXT_SIZE)
                    + LABEL_SUBMENU_GAP
                    + SUBMENU_ICON_WIDTH
            }
            Self::Separator => 0.0,
        }
    }
}

pub fn item(label: impl Into<String>, message: Message) -> MenuNode {
    MenuNode::Item {
        label: label.into(),
        shortcut: None,
        message,
    }
}

pub fn item_with_shortcut(
    label: impl Into<String>,
    shortcut: impl Into<String>,
    message: Message,
) -> MenuNode {
    MenuNode::Item {
        label: label.into(),
        shortcut: Some(MenuShortcutHint::Text(shortcut.into())),
        message,
    }
}

pub fn item_with_shortcut_binding(
    label: impl Into<String>,
    binding: KeyBinding,
    message: Message,
) -> MenuNode {
    MenuNode::Item {
        label: label.into(),
        shortcut: Some(MenuShortcutHint::Binding(binding.display_parts())),
        message,
    }
}

pub fn disabled(label: impl Into<String>) -> MenuNode {
    MenuNode::Disabled {
        label: label.into(),
        shortcut: None,
    }
}

pub fn submenu(
    id: impl Into<String>,
    label: impl Into<String>,
    children: Vec<MenuNode>,
) -> MenuNode {
    MenuNode::Submenu {
        id: id.into(),
        label: label.into(),
        children,
    }
}

pub fn separator() -> MenuNode {
    MenuNode::Separator
}

pub fn view(menu: Menu, tree: MenuTree, active_path: &[String]) -> Element<'_, Message> {
    let base_width = panel_width(&tree.entries, tree.width);
    let flyouts = active_flyouts(&tree.entries, active_path, tree.width);
    let base = motion::dropdown_with_key(
        format!("{menu:?}"),
        menu_panel(tree.entries, base_width, tree.max_height, active_path, 0),
    );
    let layers = flyouts
        .into_iter()
        .map(|flyout| {
            PositionedLayer::new(
                motion::dropdown_with_key(
                    format!("{menu:?}/{:?}", &active_path[..flyout.depth]),
                    menu_panel(
                        flyout.entries,
                        flyout.width,
                        None,
                        active_path,
                        flyout.depth,
                    ),
                ),
                flyout.x,
                flyout.y,
                flyout.row_offset,
            )
        })
        .collect();

    MenuCascade::new(base, layers).into()
}

fn menu_panel(
    entries: Vec<MenuNode>,
    width: f32,
    max_height: Option<f32>,
    active_path: &[String],
    depth: usize,
) -> Element<'_, Message> {
    let content = entries
        .into_iter()
        .fold(column![].spacing(0).padding(3), |column, entry| {
            column.push(menu_entry_view(entry, active_path, depth, width))
        });

    let content: Element<_> = if let Some(height) = max_height {
        scrollable(content)
            .smooth_scroll(true)
            .height(Length::Fixed(height))
            .into()
    } else {
        content.into()
    };

    let panel = container(content)
        .padding([0, PANEL_OUTER_PADDING])
        .width(Length::Fixed(width))
        .style(styles::menu_dropdown_band);

    panel.into()
}

#[derive(Debug, Clone)]
struct Flyout {
    entries: Vec<MenuNode>,
    depth: usize,
    width: f32,
    x: f32,
    y: f32,
    row_offset: f32,
}

struct PositionedLayer<'a> {
    element: Element<'a, Message>,
    x: f32,
    y: f32,
    row_offset: f32,
}

impl<'a> PositionedLayer<'a> {
    fn new(element: Element<'a, Message>, x: f32, y: f32, row_offset: f32) -> Self {
        Self {
            element,
            x,
            y,
            row_offset,
        }
    }
}

struct MenuCascade<'a> {
    base: Element<'a, Message>,
    layers: Vec<PositionedLayer<'a>>,
}

impl<'a> MenuCascade<'a> {
    fn new(base: Element<'a, Message>, layers: Vec<PositionedLayer<'a>>) -> Self {
        Self { base, layers }
    }
}

impl Widget<Message, iced::Theme, iced::Renderer> for MenuCascade<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn state(&self) -> tree::State {
        tree::State::None
    }

    fn diff(&mut self, tree: &mut Tree) {
        let mut children = std::iter::once(&mut self.base)
            .chain(self.layers.iter_mut().map(|layer| &mut layer.element))
            .collect::<Vec<_>>();

        tree.diff_children(&mut children);
    }

    fn size(&self) -> Size<Length> {
        self.base.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let base = self
            .base
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        let base_size = base.size();
        let available = limits.max();

        let mut parent_x = 0.0;
        let mut parent_y = 0.0;
        let mut parent_width = base_size.width;
        let layer_nodes =
            self.layers
                .iter_mut()
                .zip(&mut tree.children[1..])
                .map(|(layer, tree)| {
                    // Measure against the full available area first. Positioning is
                    // resolved below, so an edge-adjacent flyout is still measured at
                    // its configured width before it is flipped to the other side.
                    let max_size = Size::new(available.width, available.height);
                    let node = layer.element.as_widget_mut().layout(
                        tree,
                        renderer,
                        &layout::Limits::new(Size::ZERO, max_size),
                    );
                    let size = node.size();

                    let right_x = parent_x + parent_width;
                    let left_x = parent_x - size.width;
                    let right_fits = right_x + size.width <= available.width;
                    let left_fits = left_x >= 0.0;
                    let x = if right_fits || !left_fits {
                        right_x
                    } else {
                        left_x
                    }
                    .clamp(0.0, (available.width - size.width).max(0.0));

                    let y = (parent_y + layer.row_offset)
                        .clamp(0.0, (available.height - size.height).max(0.0));

                    layer.x = x;
                    layer.y = y;
                    parent_x = x;
                    parent_y = y;
                    parent_width = size.width;

                    node.move_to(Point::new(x, y))
                });

        layout::Node::with_children(
            base_size,
            std::iter::once(base).chain(layer_nodes).collect(),
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            std::iter::once(&mut self.base)
                .chain(self.layers.iter_mut().map(|layer| &mut layer.element))
                .zip(&mut tree.children)
                .zip(layout.children())
                .for_each(|((element, tree), layout)| {
                    element
                        .as_widget_mut()
                        .operate(tree, layout, renderer, operation);
                });
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        for index in (0..tree.children.len()).rev() {
            if shell.is_event_captured() {
                break;
            }

            if index == 0 {
                self.base.as_widget_mut().update(
                    &mut tree.children[index],
                    event,
                    layout.child(index),
                    cursor,
                    renderer,
                    shell,
                    viewport,
                );
            } else if let Some(layer) = self.layers.get_mut(index - 1) {
                layer.element.as_widget_mut().update(
                    &mut tree.children[index],
                    event,
                    layout.child(index),
                    cursor,
                    renderer,
                    shell,
                    viewport,
                );
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        for index in (0..tree.children.len()).rev() {
            let interaction = if index == 0 {
                self.base.as_widget().mouse_interaction(
                    &tree.children[index],
                    layout.child(index),
                    cursor,
                    viewport,
                    renderer,
                )
            } else if let Some(layer) = self.layers.get(index - 1) {
                layer.element.as_widget().mouse_interaction(
                    &tree.children[index],
                    layout.child(index),
                    cursor,
                    viewport,
                    renderer,
                )
            } else {
                mouse::Interaction::None
            };

            if interaction != mouse::Interaction::None {
                return interaction;
            }
        }

        mouse::Interaction::None
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        std::iter::once(&self.base)
            .chain(self.layers.iter().map(|layer| &layer.element))
            .zip(&tree.children)
            .zip(layout.children())
            .for_each(|((element, tree), layout)| {
                element
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, viewport);
            });
    }
}

impl<'a> From<MenuCascade<'a>> for Element<'a, Message> {
    fn from(menu: MenuCascade<'a>) -> Self {
        Self::new(menu)
    }
}

fn active_flyouts(entries: &[MenuNode], active_path: &[String], min_width: f32) -> Vec<Flyout> {
    let mut flyouts = Vec::new();
    let mut entries = entries;
    let mut parent_x = 0.0;
    let mut parent_y = 0.0;
    let mut parent_width = panel_width(entries, min_width);

    for (depth, id) in active_path.iter().enumerate() {
        let Some((row_index, children)) = active_submenu(entries, id) else {
            break;
        };
        let x = parent_x + parent_width;
        let row_offset = submenu_y(entries, row_index);
        let y = parent_y + row_offset;
        let width = panel_width(children, min_width);

        flyouts.push(Flyout {
            entries: children.clone(),
            depth: depth + 1,
            width,
            x,
            y,
            row_offset,
        });

        parent_x = x;
        parent_y = y;
        parent_width = width;
        entries = children;
    }

    flyouts
}

fn active_submenu<'a>(entries: &'a [MenuNode], id: &str) -> Option<(usize, &'a Vec<MenuNode>)> {
    entries.iter().enumerate().find_map(|(index, entry)| {
        if let MenuNode::Submenu {
            id: entry_id,
            children,
            ..
        } = entry
            && entry_id == id
        {
            return Some((index, children));
        }

        None
    })
}

fn submenu_y(entries: &[MenuNode], row_index: usize) -> f32 {
    f32::from(PANEL_PADDING)
        + entries
            .iter()
            .take(row_index)
            .map(|entry| {
                if matches!(entry, MenuNode::Separator) {
                    7.0
                } else {
                    ROW_HEIGHT
                }
            })
            .sum::<f32>()
}

pub(super) fn panel_width(entries: &[MenuNode], min_width: f32) -> f32 {
    let content_width = entries
        .iter()
        .map(MenuNode::intrinsic_width)
        .fold(0.0, f32::max)
        + PANEL_HORIZONTAL_PADDING;

    content_width.max(min_width).ceil()
}

fn text_width(text: &str, size: f32) -> f32 {
    text.chars()
        .map(|ch| {
            if ch.is_ascii() {
                if ch.is_ascii_uppercase() {
                    size * 0.66
                } else if ch.is_ascii_whitespace() {
                    size * 0.35
                } else {
                    size * TEXT_WIDTH_FACTOR
                }
            } else {
                size
            }
        })
        .sum()
}

fn binding_width(binding: &ShortcutDisplay) -> f32 {
    binding
        .modifiers
        .iter()
        .map(shortcut_part_width)
        .chain(std::iter::once(text_width(
            &binding.key,
            SHORTCUT_TEXT_SIZE,
        )))
        .enumerate()
        .map(|(index, width)| {
            if index == 0 {
                width
            } else {
                SHORTCUT_PART_GAP + width
            }
        })
        .sum()
}

fn shortcut_part_width(part: &ShortcutDisplayPart) -> f32 {
    match part {
        ShortcutDisplayPart::Text(label) => text_width(label, SHORTCUT_TEXT_SIZE),
        ShortcutDisplayPart::Icon(_) => SHORTCUT_ICON_SIZE as f32,
    }
}

fn menu_entry_view(
    entry: MenuNode,
    active_path: &[String],
    depth: usize,
    width: f32,
) -> Element<'_, Message> {
    match entry {
        MenuNode::Item {
            label,
            shortcut,
            message,
        } => mouse_area(
            button(
                row![
                    text(label).size(13),
                    space::horizontal(),
                    shortcut_hint_view(shortcut),
                ]
                .align_y(Center)
                .width(Fill),
            )
            .height(Length::Fixed(ROW_HEIGHT))
            .width(Fill)
            .padding([5, 9])
            .style(styles::menu_dropdown_item)
            .on_press(message),
        )
        .on_enter(Message::MenuPathHovered(MenuPath {
            depth,
            segments: active_path_to(active_path, depth),
        }))
        .into(),
        MenuNode::Disabled { label, shortcut } => mouse_area(
            container(
                row![
                    text(label).size(13),
                    space::horizontal(),
                    shortcut_hint_view(shortcut)
                ]
                .align_y(Center),
            )
            .height(Length::Fixed(ROW_HEIGHT))
            .width(Fill)
            .padding([5, 9])
            .style(styles::menu_dropdown_disabled),
        )
        .on_enter(Message::MenuPathHovered(MenuPath {
            depth,
            segments: active_path_to(active_path, depth),
        }))
        .into(),
        MenuNode::Submenu { id, label, .. } => {
            let is_active = active_path.get(depth) == Some(&id);
            let path = MenuPath {
                depth,
                segments: active_path_with(active_path, depth, &id),
            };

            mouse_area(
                container(
                    row![
                        text(label).size(13),
                        space::horizontal(),
                        hero::icon(HeroIcon::ChevronRight, 14, IconTone::Muted)
                    ]
                    .align_y(Center)
                    .width(Fill),
                )
                .height(Length::Fixed(ROW_HEIGHT))
                .width(Fill)
                .padding([5, 9])
                .style(styles::menu_submenu_item(is_active)),
            )
            .on_enter(Message::MenuPathHovered(path))
            .into()
        }
        MenuNode::Separator => mouse_area(
            container(
                container(space::horizontal())
                    .height(1)
                    .style(styles::separator),
            )
            .height(7)
            .width(Length::Fixed(width - 8.0))
            .padding([3, 0])
            .style(styles::transparent),
        )
        .on_enter(Message::MenuPathHovered(MenuPath {
            depth,
            segments: active_path_to(active_path, depth),
        }))
        .into(),
    }
}

fn shortcut_hint_view<'a>(shortcut: Option<MenuShortcutHint>) -> Element<'a, Message> {
    match shortcut {
        Some(MenuShortcutHint::Text(shortcut)) => text(shortcut)
            .size(12)
            .style(styles::menu_shortcut_hint)
            .into(),
        Some(MenuShortcutHint::Binding(binding)) => shortcut_binding_view(binding),
        None => space::horizontal().into(),
    }
}

fn shortcut_binding_view<'a>(display: ShortcutDisplay) -> Element<'a, Message> {
    let mut binding = row![].spacing(SHORTCUT_PART_GAP).align_y(Center);

    for modifier in display.modifiers {
        binding = binding.push(shortcut_display_part(modifier));
    }

    binding = binding.push(text(display.key).size(12).style(styles::menu_shortcut_hint));
    binding.into()
}

fn shortcut_display_part<'a>(part: ShortcutDisplayPart) -> Element<'a, Message> {
    match part {
        ShortcutDisplayPart::Text(label) => text(label)
            .size(12)
            .style(styles::menu_shortcut_hint)
            .into(),
        ShortcutDisplayPart::Icon(icon) => shortcut_modifier_icon(icon),
    }
}

fn shortcut_modifier_icon<'a>(icon: ShortcutModifierIcon) -> Element<'a, Message> {
    let icon = match icon {
        ShortcutModifierIcon::Command => ShortcutIcon::Command,
        ShortcutModifierIcon::Option => ShortcutIcon::Option,
        ShortcutModifierIcon::Shift => ShortcutIcon::Shift,
        ShortcutModifierIcon::Windows => ShortcutIcon::Windows,
    };

    shortcut::icon_with_color(icon, SHORTCUT_ICON_SIZE, styles::menu_shortcut_hint_color)
}

fn active_path_with(active_path: &[String], depth: usize, id: &str) -> Vec<String> {
    let mut path = active_path.iter().take(depth).cloned().collect::<Vec<_>>();
    path.push(id.to_owned());
    path
}

fn active_path_to(active_path: &[String], depth: usize) -> Vec<String> {
    active_path.iter().take(depth).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::{
        Message, ROW_HEIGHT, active_flyouts, item, item_with_shortcut, item_with_shortcut_binding,
        panel_width, submenu,
    };
    use crate::core::{KeyBinding, ShortcutKey, ShortcutModifiers};

    #[test]
    fn nested_flyouts_follow_parent_width_and_row_offsets() {
        for root_offset in [false, true] {
            for label in [
                "Character sets",
                "A very long parent submenu label that expands the menu",
            ] {
                let children = vec![
                    item("Arabic", Message::None),
                    item("Baltic", Message::None),
                    submenu(
                        "western",
                        "Western European",
                        vec![item("OEM-US", Message::None)],
                    ),
                ];
                let child_width = panel_width(&children, 260.0);
                let mut entries = Vec::new();
                if root_offset {
                    entries.push(item("Root item", Message::None));
                }
                entries.push(submenu("sets", label, children));
                let root_width = panel_width(&entries, 260.0);
                let flyouts =
                    active_flyouts(&entries, &["sets".to_owned(), "western".to_owned()], 260.0);
                let parent_y = 3.0 + if root_offset { ROW_HEIGHT } else { 0.0 };

                assert_eq!(flyouts.len(), 2);
                assert_eq!(
                    (flyouts[0].depth, flyouts[0].x, flyouts[0].y),
                    (1, root_width, parent_y)
                );
                assert_eq!(
                    (flyouts[1].depth, flyouts[1].x, flyouts[1].y),
                    (
                        2,
                        root_width + child_width,
                        parent_y + 3.0 + ROW_HEIGHT * 2.0
                    )
                );
            }
        }
    }

    #[test]
    fn menu_panel_width_uses_configured_width_as_minimum() {
        let entries = vec![item("Open", Message::None)];

        assert_eq!(panel_width(&entries, 202.0), 202.0);
    }

    #[test]
    fn menu_panel_width_expands_for_long_label_and_shortcut() {
        let entries = vec![item_with_shortcut(
            "Select All Between {} [] or ()",
            "Ctrl+Shift+Alt+Comma",
            Message::None,
        )];

        assert!(panel_width(&entries, 202.0) > 202.0);
    }

    #[test]
    fn menu_panel_width_accounts_for_icon_shortcut_parts() {
        let binding = KeyBinding::new(
            ShortcutModifiers::primary_shift(),
            ShortcutKey::character('s'),
        );
        let entries = vec![item_with_shortcut_binding(
            "Save As...",
            binding,
            Message::None,
        )];

        assert_eq!(panel_width(&entries, 202.0), 202.0);
    }
}
