use iced::advanced::layout;
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Layout, Renderer as _, Shell, Widget};
use iced::widget::{button, container, image, mouse_area, row, text, tooltip};
use iced::{Background, Border, Center, Color, Element, Event, Fill, Length, Rectangle, Size};

use crate::core::{Document, DocumentId, Workspace};
use crate::message::Message;
use crate::ui::controls::centered_button_content;
use crate::ui::icons::colored::{self, ColoredIcon};
use crate::ui::icons::hero::{self, HeroIcon, IconTone};
use crate::ui::icons::shortcut;
use crate::ui::styles;

const TAB_HEIGHT: f32 = 27.0;
const TAB_TOP_BAR_HEIGHT: f32 = 3.0;
const TAB_CLOSE_WIDTH: f32 = 26.0;
const NEW_FILE_WIDTH: f32 = 28.0;
const NEW_FILE_MARGIN: f32 = 4.0;
const TAB_LABEL_MAX_CHARS: usize = 28;
const TAB_LABEL_MIN_WIDTH: f32 = 62.0;
const TAB_LABEL_MAX_WIDTH: f32 = 172.0;
const TAB_LABEL_CHAR_WIDTH: f32 = 7.0;
const TAB_TEXT_SIZE: u32 = 13;
const TOOLTIP_TEXT_SIZE: u32 = 13;

mod scroll_strip;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragVisual {
    Idle,
    Dragged,
    ValidTarget,
    InvalidTarget,
}

impl DragVisual {
    const fn is_dragged(self) -> bool {
        matches!(self, Self::Dragged)
    }
}

pub fn view(
    workspace: &Workspace,
    dragged_tab: Option<DocumentId>,
    hovered_drop_tab: Option<DocumentId>,
) -> Element<'_, Message> {
    let tabs = workspace
        .documents()
        .iter()
        .fold(row![].spacing(0).align_y(Center), |tabs, document| {
            tabs.push(tab(
                document,
                document.id == workspace.active_document_id(),
                drag_visual(workspace, document, dragged_tab, hovered_drop_tab),
            ))
        })
        .push(new_file_button());

    container(scroll_strip::view(tabs))
        .height(TAB_HEIGHT + 1.0)
        .width(Fill)
        .style(styles::tab_strip)
        .into()
}

fn new_file_button() -> Element<'static, Message> {
    let button = button(centered_button_content(hero::icon(
        HeroIcon::Plus,
        16,
        IconTone::Text,
    )))
    .width(NEW_FILE_WIDTH)
    .height(23)
    .padding(0)
    .style(styles::icon_button)
    .on_press(Message::NewFile);

    tooltip(
        container(button).padding([0.0, NEW_FILE_MARGIN]),
        container(text("New File").size(TOOLTIP_TEXT_SIZE))
            .padding([4, 7])
            .style(styles::tooltip),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

fn tab(document: &Document, is_active: bool, drag_visual: DragVisual) -> Element<'_, Message> {
    let title = tab_title(document);
    let compact_title = compact_tab_title(&title);
    let label_width = tab_label_width(&compact_title);

    let title_area = mouse_area(
        container(
            row![
                image::Image::new(tab_state_icon(TabFileState::from_document(document)))
                    .width(14)
                    .height(14)
                    .filter_method(image::FilterMethod::Linear),
                text(compact_title)
                    .size(TAB_TEXT_SIZE)
                    .width(Length::Fixed(label_width)),
            ]
            .spacing(5)
            .align_y(Center),
        )
        .padding([3, 7])
        .height(23)
        .style(styles::tab_title_area(is_active, drag_visual.is_dragged())),
    )
    .on_press(Message::TabDragStarted(document.id))
    .on_enter(Message::TabDragHovered(document.id))
    .on_exit(Message::TabDragLeft(document.id))
    .on_release(Message::TabDragReleased(document.id));

    let pin_button = button(centered_button_content(shortcut::icon_with_color(
        if document.is_pinned {
            shortcut::ShortcutIcon::PinAngleFill
        } else {
            shortcut::ShortcutIcon::PinAngle
        },
        14,
        styles::shortcut_text_color,
    )))
    .width(19)
    .height(23)
    .padding(0)
    .style(styles::tab_pin_button(is_active, document.is_pinned))
    .on_press(Message::TabPinToggled(document.id));

    let close_button = button(centered_button_content(
        image::Image::new(close_icon())
            .width(22)
            .height(22)
            .filter_method(image::FilterMethod::Linear),
    ))
    .width(TAB_CLOSE_WIDTH)
    .height(23)
    .padding(0)
    .style(styles::tab_close_button(is_active))
    .on_press(Message::TabClosed(document.id));

    let tab = MaskedTab::new(
        row![title_area, pin_button, close_button]
            .spacing(0)
            .align_y(Center),
        is_active,
        drag_visual_style(drag_visual),
        drag_visual.is_dragged(),
    )
    .height(TAB_HEIGHT);

    tooltip(
        tab,
        container(text(title).size(TOOLTIP_TEXT_SIZE))
            .padding([4, 7])
            .style(styles::tooltip),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

struct MaskedTab<'a> {
    content: Element<'a, Message>,
    height: Length,
    is_active: bool,
    drag_visual: styles::TabDragVisual,
    is_dragged: bool,
}

impl<'a> MaskedTab<'a> {
    fn new(
        content: impl Into<Element<'a, Message>>,
        is_active: bool,
        drag_visual: styles::TabDragVisual,
        is_dragged: bool,
    ) -> Self {
        Self {
            content: content.into(),
            height: Length::Fit,
            is_active,
            drag_visual,
            is_dragged,
        }
    }

    fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}

impl Widget<Message, iced::Theme, iced::Renderer> for MaskedTab<'_> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn diff(&mut self, tree: &mut Tree) {
        self.content.as_widget_mut().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        let content_size = self.content.as_widget().size();

        Size {
            width: content_size.width,
            height: self.height.enclose(content_size.height),
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.height(self.height);
        let content = self
            .content
            .as_widget_mut()
            .layout(tree, renderer, &limits.loose());
        let size = limits.resolve(content.size().width, self.height, content.size());

        layout::Node::with_children(
            size,
            vec![content.move_to(iced::Point::new(0.0, TAB_TOP_BAR_HEIGHT))],
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
            self.content.as_widget_mut().operate(
                tree,
                layout.children().next().unwrap(),
                renderer,
                operation,
            );
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
        self.content.as_widget_mut().update(
            tree,
            event,
            layout.children().next().unwrap(),
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            tree,
            layout.children().next().unwrap(),
            cursor,
            viewport,
            renderer,
        )
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
        let bounds = layout.bounds();
        let tab_style = styles::tab_container_style(theme, self.is_active, self.drag_visual);
        let top_bar_style = styles::tab_top_bar_style(theme, self.is_active, self.is_dragged);

        if bounds.intersection(viewport).is_some() {
            draw_tab_quad(renderer, bounds, tab_style);
            draw_masked_tab_top_bar(renderer, bounds, tab_style.border, top_bar_style);

            self.content.as_widget().draw(
                tree,
                renderer,
                theme,
                style,
                layout.children().next().unwrap(),
                cursor,
                viewport,
            );
        }
    }
}

impl<'a> From<MaskedTab<'a>> for Element<'a, Message> {
    fn from(tab: MaskedTab<'a>) -> Self {
        Self::new(tab)
    }
}

fn draw_tab_quad(renderer: &mut iced::Renderer, bounds: Rectangle, style: container::Style) {
    if let Some(background) = style.background {
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: style.border,
                shadow: style.shadow,
                snap: style.snap,
            },
            background,
        );
    }
}

fn draw_masked_tab_top_bar(
    renderer: &mut iced::Renderer,
    tab_bounds: Rectangle,
    tab_border: Border,
    style: container::Style,
) {
    let (top_bar_bounds, mask_border) = masked_top_bar_geometry(tab_bounds, tab_border);

    renderer.with_layer(top_bar_bounds, |renderer| {
        renderer.fill_quad(
            renderer::Quad {
                bounds: tab_bounds,
                border: mask_border,
                snap: style.snap,
                ..renderer::Quad::default()
            },
            style
                .background
                .unwrap_or(Background::Color(Color::TRANSPARENT)),
        );
    });
}

fn masked_top_bar_geometry(tab_bounds: Rectangle, tab_border: Border) -> (Rectangle, Border) {
    (
        Rectangle {
            height: TAB_TOP_BAR_HEIGHT,
            ..tab_bounds
        },
        Border {
            width: 0.0,
            color: Color::TRANSPARENT,
            ..tab_border
        },
    )
}

fn drag_visual(
    workspace: &Workspace,
    document: &Document,
    dragged_tab: Option<DocumentId>,
    hovered_drop_tab: Option<DocumentId>,
) -> DragVisual {
    let Some(dragged_id) = dragged_tab else {
        return DragVisual::Idle;
    };

    if dragged_id == document.id {
        return DragVisual::Dragged;
    }

    if hovered_drop_tab != Some(document.id) {
        return DragVisual::Idle;
    }

    let Some(dragged_document) = workspace.document(dragged_id) else {
        return DragVisual::Idle;
    };

    if dragged_document.is_pinned == document.is_pinned {
        DragVisual::ValidTarget
    } else {
        DragVisual::InvalidTarget
    }
}

fn drag_visual_style(drag_visual: DragVisual) -> styles::TabDragVisual {
    match drag_visual {
        DragVisual::Idle => styles::TabDragVisual::Idle,
        DragVisual::Dragged => styles::TabDragVisual::Dragged,
        DragVisual::ValidTarget => styles::TabDragVisual::ValidTarget,
        DragVisual::InvalidTarget => styles::TabDragVisual::InvalidTarget,
    }
}

fn tab_title(document: &Document) -> String {
    document.title()
}

fn compact_tab_title(title: &str) -> String {
    if title.chars().count() <= TAB_LABEL_MAX_CHARS {
        return title.to_owned();
    }

    let extension = title
        .rsplit_once('.')
        .filter(|(stem, extension)| !stem.is_empty() && !extension.is_empty())
        .map(|(_, extension)| format!(".{extension}"))
        .unwrap_or_default();
    let extension_chars = extension.chars().count();
    let reserved = extension_chars.min(8);
    let prefix_chars = TAB_LABEL_MAX_CHARS.saturating_sub(reserved + 3).max(8);

    let mut compact = title.chars().take(prefix_chars).collect::<String>();
    compact.push_str("...");
    compact.push_str(&extension);
    compact
}

fn tab_label_width(label: &str) -> f32 {
    (label.chars().count() as f32 * TAB_LABEL_CHAR_WIDTH)
        .clamp(TAB_LABEL_MIN_WIDTH, TAB_LABEL_MAX_WIDTH)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabFileState {
    Saved,
    Unsaved,
}

impl TabFileState {
    fn from_document(document: &Document) -> Self {
        if document.is_dirty {
            Self::Unsaved
        } else {
            Self::Saved
        }
    }
}

fn tab_state_icon(state: TabFileState) -> image::Handle {
    match state {
        TabFileState::Saved => colored::handle(ColoredIcon::DocumentSaved),
        TabFileState::Unsaved => colored::handle(ColoredIcon::DocumentUnsaved),
    }
}

fn close_icon() -> image::Handle {
    colored::handle(ColoredIcon::TabClose)
}

#[cfg(test)]
mod tests {
    use super::{TAB_TOP_BAR_HEIGHT, TabFileState, masked_top_bar_geometry, tab_title};
    use crate::core::{Document, DocumentId};
    use iced::{Border, Color, Rectangle};

    #[test]
    fn tab_state_and_title_follow_edits_and_saves() {
        let mut document = Document::untitled(DocumentId::new(7));
        for dirty in [false, true, false] {
            if dirty {
                document.mark_dirty();
            } else {
                document.mark_clean();
            }
            assert_eq!(
                TabFileState::from_document(&document),
                if dirty {
                    TabFileState::Unsaved
                } else {
                    TabFileState::Saved
                }
            );
            assert_eq!(
                tab_title(&document),
                if dirty { "*Untitled 7" } else { "Untitled 7" }
            );
        }
    }

    #[test]
    fn masked_top_bar_uses_tab_shape_as_mask() {
        let tab_bounds = Rectangle {
            x: 7.0,
            y: 11.0,
            width: 120.0,
            height: 27.0,
        };
        let tab_border = Border {
            width: 2.0,
            color: Color::BLACK,
            radius: 4.0.into(),
        };

        let (clip_bounds, mask_border) = masked_top_bar_geometry(tab_bounds, tab_border);

        assert_eq!(clip_bounds.x, tab_bounds.x);
        assert_eq!(clip_bounds.y, tab_bounds.y);
        assert_eq!(clip_bounds.width, tab_bounds.width);
        assert_eq!(clip_bounds.height, TAB_TOP_BAR_HEIGHT);
        assert_eq!(mask_border.radius, tab_border.radius);
        assert_eq!(mask_border.width, 0.0);
        assert_eq!(mask_border.color, Color::TRANSPARENT);
    }
}
