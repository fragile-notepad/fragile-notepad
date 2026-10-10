//! Retains static widget pixels while keeping their event and overlay paths live.
//!
//! Deadlines describe when a child needs another frame, not whether its pixels
//! change on earlier frames. Use `animated(true)` for smooth scrolling, fades,
//! or other children that advance on every redraw. These children draw live
//! while a frame is pending; the final frame refreshes the retained surface.
//! Keep independently animated widgets outside a static cache when possible.

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Theme, Vector, window};
use std::cell::{Cell, RefCell};

pub(crate) fn cached<'a>(
    content: impl Into<Element<'a, crate::message::Message>>,
) -> Element<'a, crate::message::Message> {
    Element::new(RasterCache::new(content))
}

pub(crate) fn cached_animated<'a>(
    content: impl Into<Element<'a, crate::message::Message>>,
) -> Element<'a, crate::message::Message> {
    Element::new(RasterCache::new(content).animated(true))
}

pub(crate) fn cached_editor<'a>(
    content: impl Into<Element<'a, crate::message::Message>>,
) -> Element<'a, crate::message::Message> {
    let mut cache = RasterCache::new(content);
    cache.paint_state = Some(editor_paint_state::<iced::Renderer>);
    Element::new(cache)
}

fn editor_paint_state<Renderer: iced::advanced::text::Renderer>(tree: &Tree) -> (u64, bool) {
    use crate::editor::AdvancedEditorState;
    fn find<Paragraph: 'static>(tree: &Tree) -> Option<&AdvancedEditorState<Paragraph>> {
        if tree.tag == tree::Tag::of::<AdvancedEditorState<Paragraph>>() {
            Some(tree.state.downcast_ref())
        } else {
            tree.children.iter().find_map(find::<Paragraph>)
        }
    }
    find::<Renderer::Paragraph>(tree)
        .map(|state| state.raster_cache_state(iced::time::Instant::now()))
        .unwrap_or((0, true))
}

struct RasterCache<'a, Message, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    animated: bool,
    paint_state: Option<fn(&Tree) -> (u64, bool)>,
}

impl<'a, Message, Renderer> RasterCache<'a, Message, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            animated: false,
            paint_state: None,
        }
    }

    fn animated(mut self, animated: bool) -> Self {
        self.animated = animated;
        self
    }
}

#[derive(Debug, Default)]
struct State {
    cache: renderer::Cache,
    revision: Cell<u64>,
    pending_frame: Option<window::RedrawRequest>,
    draw_context: RefCell<Option<DrawContext>>,
    local_paint: Cell<Option<(u64, bool)>>,
}

impl State {
    fn invalidate(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
    }

    fn pending_frame(&self) -> window::RedrawRequest {
        self.pending_frame.unwrap_or(window::RedrawRequest::Wait)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct DrawContext {
    bounds: Rectangle,
    viewport: Rectangle,
    cursor: mouse::Cursor,
    theme: Theme,
    text_color: iced::Color,
    scale_factor: Option<f32>,
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for RasterCache<'_, Message, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        // A rebuilt view can carry new data or style closures without changing
        // the child's type. Avoid depending on document or application stamps.
        tree.state.downcast_ref::<State>().invalidate();
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // Responsive children can rebuild during layout even at the same size.
        tree.state.downcast_ref::<State>().invalidate();
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        // Focus and scroll operations may mutate local state without an event.
        tree.state.downcast_ref::<State>().invalidate();
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let previous_frame = state.pending_frame();
        let mut messages = Vec::new();
        let mut local = shell.local(&mut messages);
        if shell.is_event_captured() {
            local.capture_event();
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            &mut local,
            viewport,
        );

        let next_frame = local.redraw_request();
        let previous_due = match event {
            Event::Window(window::Event::RedrawRequested(now)) => match previous_frame {
                window::RedrawRequest::NextFrame => true,
                window::RedrawRequest::At(deadline) => *now >= deadline,
                window::RedrawRequest::Wait => false,
            },
            _ => false,
        };
        let continuous = self.animated
            && (previous_frame != window::RedrawRequest::Wait
                || next_frame != window::RedrawRequest::Wait);
        if !matches!(event, Event::Window(window::Event::RedrawRequested(_)))
            || previous_due
            || next_frame == window::RedrawRequest::NextFrame
            || continuous
            || !local.is_empty()
            || local.is_layout_invalid().is_some()
            || local.are_widgets_invalid()
        {
            state.invalidate();
        }
        // Unrelated input and early redraws must not discard a still-pending
        // caret/tooltip deadline. A due frame consumes it before rescheduling.
        state.pending_frame = Some(if previous_due {
            next_frame
        } else {
            previous_frame.min(next_frame)
        });
        shell.merge(local, std::convert::identity);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let context = DrawContext {
            bounds,
            viewport: *viewport,
            cursor,
            theme: theme.clone(),
            text_color: style.text_color,
            scale_factor: renderer.scale_factor(),
        };
        if state.draw_context.borrow().as_ref() != Some(&context) {
            state.invalidate();
            *state.draw_context.borrow_mut() = Some(context);
        }
        // Read local paint clocks even on cache hits. Fold fading and fast-text
        // settling can complete on an unrelated frame before a child deadline.
        let local_paint = self.paint_state.map(|snapshot| snapshot(&tree.children[0]));
        if state.local_paint.replace(local_paint) != local_paint {
            state.invalidate();
        }
        let draw = |renderer: &mut Renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        };
        let live = local_paint.map_or(
            self.animated && state.pending_frame() != window::RedrawRequest::Wait,
            |(_, live)| live,
        );
        if live {
            // Keep changing pictures out of the retained path instead of
            // paying for a new offscreen texture pass on every animation frame.
            renderer.with_layer(bounds, draw);
        } else {
            renderer.with_cached_layer(&state.cache, state.revision.get(), bounds, draw);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        // Overlays are composed after the base widget and must never become
        // part of its retained pixels or disappear on a cache hit.
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::graphics::core::shell::Waker;
    use iced::advanced::image;
    use iced::advanced::renderer::{Headless, Renderer as _};
    use iced::advanced::widget::{Id, operation::focusable};
    use iced::time::{Duration, Instant};
    use iced::{Background, Color, Point, Transformation};
    use std::collections::HashMap;
    use std::rc::Rc;

    const SIZE: Size = Size::new(320.0, 120.0);

    #[derive(Default)]
    struct RecordingRenderer {
        retained: HashMap<u64, (u64, Rectangle)>,
        hits: usize,
        misses: usize,
        scale: Option<f32>,
    }

    impl renderer::Renderer for RecordingRenderer {
        fn start_layer(&mut self, _bounds: Rectangle) {}
        fn end_layer(&mut self) {}
        fn start_transformation(&mut self, _transformation: Transformation) {}
        fn end_transformation(&mut self) {}
        fn fill_quad(&mut self, _quad: renderer::Quad, _background: impl Into<Background>) {}
        fn allocate_image(
            &mut self,
            _handle: &image::Handle,
            callback: impl FnOnce(Result<image::Allocation, image::Error>) + Send + 'static,
        ) {
            callback(Err(image::Error::Unsupported));
        }
        fn hint(&mut self, scale_factor: f32) {
            self.scale = Some(scale_factor);
        }
        fn scale_factor(&self) -> Option<f32> {
            self.scale
        }
        fn reset(&mut self, _new_bounds: Rectangle) {}
        fn with_cached_layer(
            &mut self,
            cache: &renderer::Cache,
            key: u64,
            bounds: Rectangle,
            draw: impl FnOnce(&mut Self),
        ) {
            if self.retained.get(&cache.id()) == Some(&(key, bounds)) {
                self.hits += 1;
            } else {
                self.misses += 1;
                draw(self);
                self.retained.insert(cache.id(), (key, bounds));
            }
        }
    }

    #[derive(Default)]
    struct ProbeState(bool);

    impl focusable::Focusable for ProbeState {
        fn is_focused(&self) -> bool {
            self.0
        }
        fn focus(&mut self) {
            self.0 = true;
        }
        fn unfocus(&mut self) {
            self.0 = false;
        }
    }

    #[derive(Clone)]
    struct Probe {
        next_frame: Rc<Cell<window::RedrawRequest>>,
        updates: Rc<Cell<usize>>,
        draws: Rc<Cell<usize>>,
        overlays: Rc<Cell<usize>>,
    }

    impl Default for Probe {
        fn default() -> Self {
            Self {
                next_frame: Rc::new(Cell::new(window::RedrawRequest::Wait)),
                updates: Rc::default(),
                draws: Rc::default(),
                overlays: Rc::default(),
            }
        }
    }

    impl Widget<(), Theme, RecordingRenderer> for Probe {
        fn tag(&self) -> tree::Tag {
            tree::Tag::of::<ProbeState>()
        }
        fn state(&self) -> tree::State {
            tree::State::new(ProbeState::default())
        }
        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Fill)
        }
        fn layout(
            &mut self,
            _tree: &mut Tree,
            _renderer: &RecordingRenderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            layout::Node::new(limits.max())
        }
        fn draw(
            &self,
            _tree: &Tree,
            _renderer: &mut RecordingRenderer,
            _theme: &Theme,
            _style: &renderer::Style,
            _layout: Layout<'_>,
            _cursor: mouse::Cursor,
            _viewport: &Rectangle,
        ) {
            self.draws.set(self.draws.get() + 1);
        }
        fn update(
            &mut self,
            _tree: &mut Tree,
            event: &Event,
            _layout: Layout<'_>,
            _cursor: mouse::Cursor,
            _renderer: &RecordingRenderer,
            shell: &mut Shell<'_, ()>,
            _viewport: &Rectangle,
        ) {
            self.updates.set(self.updates.get() + 1);
            shell.request_redraw_at(self.next_frame.get());
            if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_))) {
                shell.publish(());
                shell.capture_event();
            }
        }
        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout<'_>,
            _renderer: &RecordingRenderer,
            operation: &mut dyn Operation,
        ) {
            operation.focusable(
                Some(&Id::new("cache-probe")),
                layout.bounds(),
                tree.state.downcast_mut::<ProbeState>(),
            );
        }
        fn mouse_interaction(
            &self,
            _tree: &Tree,
            _layout: Layout<'_>,
            _cursor: mouse::Cursor,
            _viewport: &Rectangle,
            _renderer: &RecordingRenderer,
        ) -> mouse::Interaction {
            mouse::Interaction::Pointer
        }
        fn overlay<'a>(
            &'a mut self,
            _tree: &'a mut Tree,
            _layout: Layout<'a>,
            _renderer: &RecordingRenderer,
            _viewport: &Rectangle,
            _translation: Vector,
        ) -> Option<overlay::Element<'a, (), Theme, RecordingRenderer>> {
            self.overlays.set(self.overlays.get() + 1);
            Some(overlay::Element::new(Box::new(ProbeOverlay)))
        }
    }

    struct ProbeOverlay;

    impl overlay::Overlay<(), Theme, RecordingRenderer> for ProbeOverlay {
        fn layout(&mut self, _renderer: &RecordingRenderer, _bounds: Size) -> layout::Node {
            layout::Node::new(Size::new(16.0, 16.0))
        }
        fn draw(
            &self,
            _renderer: &mut RecordingRenderer,
            _theme: &Theme,
            _style: &renderer::Style,
            _layout: Layout<'_>,
            _cursor: mouse::Cursor,
        ) {
        }
    }

    fn mount<Message, Renderer: renderer::Renderer>(
        content: &mut Element<'_, Message, Theme, Renderer>,
        renderer: &Renderer,
        size: Size,
    ) -> (Tree, layout::Node) {
        let mut tree = Tree::empty();
        tree.diff(content.as_widget_mut());
        let node = content.as_widget_mut().layout(
            &mut tree,
            renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        (tree, node)
    }

    fn update_probe(
        content: &mut Element<'_, (), Theme, RecordingRenderer>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &RecordingRenderer,
        event: Event,
        sibling_redraw: bool,
    ) -> (window::RedrawRequest, usize, bool) {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
        if sibling_redraw {
            shell.request_redraw();
        }
        content.as_widget_mut().update(
            tree,
            &event,
            Layout::new(node),
            mouse::Cursor::Unavailable,
            renderer,
            &mut shell,
            &Rectangle::with_size(SIZE),
        );
        let result = (
            shell.redraw_request(),
            !shell.is_empty(),
            shell.is_event_captured(),
        );
        drop(shell);
        assert_eq!(result.1, !messages.is_empty());
        (result.0, messages.len(), result.2)
    }

    fn draw_probe(
        content: &Element<'_, (), Theme, RecordingRenderer>,
        tree: &Tree,
        node: &layout::Node,
        renderer: &mut RecordingRenderer,
        theme: &Theme,
        cursor: mouse::Cursor,
        viewport: Rectangle,
    ) {
        content.as_widget().draw(
            tree,
            renderer,
            theme,
            &renderer::Style::default(),
            Layout::new(node),
            cursor,
            &viewport,
        );
    }

    #[test]
    fn unrelated_redraws_reuse_pixels_and_input_preserves_the_caret_deadline() {
        let probe = Probe::default();
        let now = Instant::now();
        let deadline = now + Duration::from_millis(500);
        probe.next_frame.set(window::RedrawRequest::At(deadline));
        let mut content = Element::new(RasterCache::new(Element::new(probe.clone())));
        let mut renderer = RecordingRenderer::default();
        let (mut tree, node) = mount(&mut content, &renderer, SIZE);
        let viewport = Rectangle::with_size(SIZE);
        update_probe(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(now)),
            true,
        );
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Light,
            mouse::Cursor::Unavailable,
            viewport,
        );
        update_probe(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(
                now + Duration::from_millis(16),
            )),
            true,
        );
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Light,
            mouse::Cursor::Unavailable,
            viewport,
        );
        assert_eq!((renderer.hits, renderer.misses), (1, 1));

        // A child can ignore a pointer event without re-requesting its timer.
        probe.next_frame.set(window::RedrawRequest::Wait);
        update_probe(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::CursorMoved {
                position: Point::ORIGIN,
            }),
            false,
        );
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Light,
            mouse::Cursor::Unavailable,
            viewport,
        );
        probe.next_frame.set(window::RedrawRequest::At(
            deadline + Duration::from_millis(500),
        ));
        update_probe(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(deadline)),
            false,
        );
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Light,
            mouse::Cursor::Unavailable,
            viewport,
        );
        assert_eq!((renderer.hits, renderer.misses), (1, 3));
        assert_eq!(probe.updates.get(), 4);
    }

    #[test]
    fn focus_hover_theme_scale_and_relayout_refresh_the_surface() {
        let probe = Probe::default();
        let mut content = Element::new(RasterCache::new(Element::new(probe)));
        let mut renderer = RecordingRenderer::default();
        let (mut tree, mut node) = mount(&mut content, &renderer, SIZE);
        let viewport = Rectangle::with_size(SIZE);
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Light,
            mouse::Cursor::Unavailable,
            viewport,
        );
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Light,
            mouse::Cursor::Unavailable,
            viewport,
        );
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Dark,
            mouse::Cursor::Unavailable,
            viewport,
        );
        let cursor = mouse::Cursor::Available(Point::new(12.0, 8.0));
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Dark,
            cursor,
            viewport,
        );
        renderer.hint(1.5);
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Dark,
            cursor,
            viewport,
        );
        content.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut focusable::focus::<()>(Id::new("cache-probe")),
        );
        assert!(tree.children[0].state.downcast_ref::<ProbeState>().0);
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Dark,
            cursor,
            viewport,
        );
        tree.diff(content.as_widget_mut());
        node = content.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(400.0, 150.0)),
        );
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Dark,
            cursor,
            Rectangle::with_size(node.size()),
        );
        assert_eq!((renderer.hits, renderer.misses), (1, 6));
    }

    #[test]
    fn continuous_animation_draws_early_frames_live_and_caches_the_final_frame() {
        let probe = Probe::default();
        let now = Instant::now();
        let deadline = now + Duration::from_millis(100);
        probe.next_frame.set(window::RedrawRequest::At(deadline));
        let mut content =
            Element::new(RasterCache::new(Element::new(probe.clone())).animated(true));
        let mut renderer = RecordingRenderer::default();
        let (mut tree, node) = mount(&mut content, &renderer, SIZE);
        let viewport = Rectangle::with_size(SIZE);
        for at in [now, now + Duration::from_millis(16)] {
            update_probe(
                &mut content,
                &mut tree,
                &node,
                &renderer,
                Event::Window(window::Event::RedrawRequested(at)),
                false,
            );
            draw_probe(
                &content,
                &tree,
                &node,
                &mut renderer,
                &Theme::Light,
                mouse::Cursor::Unavailable,
                viewport,
            );
        }
        assert_eq!(probe.draws.get(), 2);
        assert_eq!((renderer.hits, renderer.misses), (0, 0));
        probe.next_frame.set(window::RedrawRequest::Wait);
        update_probe(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Window(window::Event::RedrawRequested(deadline)),
            false,
        );
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Light,
            mouse::Cursor::Unavailable,
            viewport,
        );
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Light,
            mouse::Cursor::Unavailable,
            viewport,
        );
        assert_eq!((renderer.hits, renderer.misses), (1, 1));
    }

    #[test]
    fn local_paint_changes_refresh_without_an_event_and_live_paint_rejoins_the_cache() {
        fn snapshot(tree: &Tree) -> (u64, bool) {
            let value = tree.state.downcast_ref::<ProbeState>().0;
            (u64::from(value), value)
        }
        let probe = Probe::default();
        let mut cache = RasterCache::new(Element::new(probe));
        cache.paint_state = Some(snapshot);
        let mut content = Element::new(cache);
        let mut renderer = RecordingRenderer::default();
        let (mut tree, node) = mount(&mut content, &renderer, SIZE);
        let viewport = Rectangle::with_size(SIZE);
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Light,
            mouse::Cursor::Unavailable,
            viewport,
        );
        tree.children[0].state.downcast_mut::<ProbeState>().0 = true;
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Light,
            mouse::Cursor::Unavailable,
            viewport,
        );
        assert_eq!((renderer.hits, renderer.misses), (0, 1));
        tree.children[0].state.downcast_mut::<ProbeState>().0 = false;
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Light,
            mouse::Cursor::Unavailable,
            viewport,
        );
        draw_probe(
            &content,
            &tree,
            &node,
            &mut renderer,
            &Theme::Light,
            mouse::Cursor::Unavailable,
            viewport,
        );
        assert_eq!((renderer.hits, renderer.misses), (1, 2));
    }

    #[test]
    fn cached_draws_keep_messages_capture_mouse_and_overlays_live() {
        let probe = Probe::default();
        let mut content = Element::new(RasterCache::new(Element::new(probe.clone())));
        let mut renderer = RecordingRenderer::default();
        let (mut tree, node) = mount(&mut content, &renderer, SIZE);
        let viewport = Rectangle::with_size(SIZE);
        for _ in 0..2 {
            draw_probe(
                &content,
                &tree,
                &node,
                &mut renderer,
                &Theme::Light,
                mouse::Cursor::Unavailable,
                viewport,
            );
            assert!(
                content
                    .as_widget_mut()
                    .overlay(
                        &mut tree,
                        Layout::new(&node),
                        &renderer,
                        &viewport,
                        Vector::ZERO
                    )
                    .is_some()
            );
        }
        assert_eq!(probe.draws.get(), 1);
        assert_eq!(probe.overlays.get(), 2);
        assert_eq!(
            content.as_widget().mouse_interaction(
                &tree,
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &viewport,
                &renderer
            ),
            mouse::Interaction::Pointer
        );
        let (_, messages, captured) = update_probe(
            &mut content,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            false,
        );
        assert_eq!(messages, 1);
        assert!(captured);
    }

    #[test]
    fn software_pixels_match_through_caret_hover_theme_and_resize() {
        use iced::widget::{button, column, text, text_input};

        let mut renderer = futures::executor::block_on(<iced::Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .expect("software renderer");
        let scene = || -> Element<'static, ()> {
            column![
                text_input("Note", "Cached text")
                    .id("cache-input")
                    .on_input(|_| ()),
                button(text("Action")).on_press(()),
            ]
            .spacing(6)
            .into()
        };
        let mut plain = scene();
        let mut cached = Element::new(RasterCache::new(scene()));
        let (mut plain_tree, _) = mount(&mut plain, &renderer, SIZE);
        let (mut cached_tree, _) = mount(&mut cached, &renderer, SIZE);
        let now = Instant::now();
        for (size, theme, cursor, at) in [
            (SIZE, Theme::Light, mouse::Cursor::Unavailable, now),
            (
                SIZE,
                Theme::Light,
                mouse::Cursor::Available(Point::new(12.0, 50.0)),
                now + Duration::from_millis(750),
            ),
            (
                Size::new(400.0, 150.0),
                Theme::Dark,
                mouse::Cursor::Unavailable,
                now + Duration::from_millis(1250),
            ),
        ] {
            let mut pixels = Vec::new();
            for (content, tree) in [
                (&mut plain, &mut plain_tree),
                (&mut cached, &mut cached_tree),
            ] {
                let viewport = Rectangle::with_size(size);
                let node = content.as_widget_mut().layout(
                    tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                if at == now {
                    content.as_widget_mut().operate(
                        tree,
                        Layout::new(&node),
                        &renderer,
                        &mut focusable::focus::<()>(Id::new("cache-input")),
                    );
                }
                let mut messages = Vec::new();
                let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
                content.as_widget_mut().update(
                    tree,
                    &Event::Window(window::Event::RedrawRequested(at)),
                    Layout::new(&node),
                    cursor,
                    &renderer,
                    &mut shell,
                    &viewport,
                );
                renderer.reset(viewport);
                content.as_widget().draw(
                    tree,
                    &mut renderer,
                    &theme,
                    &renderer::Style::default(),
                    Layout::new(&node),
                    cursor,
                    &viewport,
                );
                pixels.push(renderer.screenshot(
                    Size::new(size.width as u32, size.height as u32),
                    1.0,
                    Color::WHITE,
                ));
            }
            assert_eq!(
                pixels[0], pixels[1],
                "cached software pixels at {at:?}, {size:?}"
            );
        }
    }

    #[test]
    fn cached_editor_preserves_caret_ime_and_context_menu_state() {
        use crate::core::{Document, DocumentId, EditorSettings};
        use crate::editor::EditorBuffer;
        use crate::ui::editor;

        let mut renderer = futures::executor::block_on(<iced::Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .expect("software renderer");
        let settings = EditorSettings::default();
        let mut document = Document::untitled(DocumentId::new(1));
        document.buffer = EditorBuffer::from_text("fn main() {\n    let note = 1;\n}\n");
        document.refresh_after_text_change();
        let mut plain = editor::view(&document, &settings);
        let mut cached = cached_editor(editor::view(&document, &settings));
        let (mut plain_tree, plain_node) = mount(&mut plain, &renderer, SIZE);
        let (mut cached_tree, cached_node) = mount(&mut cached, &renderer, SIZE);
        for (content, tree, node) in [
            (&mut plain, &mut plain_tree, &plain_node),
            (&mut cached, &mut cached_tree, &cached_node),
        ] {
            content.as_widget_mut().operate(
                tree,
                Layout::new(node),
                &renderer,
                &mut focusable::focus::<()>(Id::new(editor::EDITOR_ID)),
            );
        }
        let now = Instant::now();
        let viewport = Rectangle::with_size(SIZE);
        for event in [
            Event::Window(window::Event::RedrawRequested(now)),
            Event::Window(window::Event::RedrawRequested(
                now + Duration::from_millis(750),
            )),
            Event::InputMethod(iced::advanced::input_method::Event::Preedit(
                "あ".to_owned(),
                Some(0..3),
            )),
            Event::Window(window::Event::Unfocused),
        ] {
            let mut pixels = Vec::new();
            for (content, tree, node) in [
                (&mut plain, &mut plain_tree, &plain_node),
                (&mut cached, &mut cached_tree, &cached_node),
            ] {
                let mut messages = Vec::new();
                let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
                content.as_widget_mut().update(
                    tree,
                    &event,
                    Layout::new(node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut shell,
                    &viewport,
                );
                renderer.reset(viewport);
                content.as_widget().draw(
                    tree,
                    &mut renderer,
                    &Theme::Light,
                    &renderer::Style::default(),
                    Layout::new(node),
                    mouse::Cursor::Unavailable,
                    &viewport,
                );
                pixels.push(renderer.screenshot(
                    Size::new(SIZE.width as u32, SIZE.height as u32),
                    1.0,
                    Color::WHITE,
                ));
            }
            assert_eq!(pixels[0], pixels[1], "editor pixels after {event:?}");
        }
        // The outer cache leaves ContextMenu's direct AdvancedEditor downcast intact.
        let mut messages = Vec::new();
        let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
        cached.as_widget_mut().update(
            &mut cached_tree,
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
            Layout::new(&cached_node),
            mouse::Cursor::Available(Point::new(200.0, 40.0)),
            &renderer,
            &mut shell,
            &viewport,
        );
        assert!(
            cached
                .as_widget_mut()
                .overlay(
                    &mut cached_tree,
                    Layout::new(&cached_node),
                    &renderer,
                    &viewport,
                    Vector::ZERO
                )
                .is_some()
        );

        use iced::keyboard::{self, Key, key::Named};
        let key = |named| {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Named(named),
                modified_key: Key::Named(named),
                physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Enter),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::empty(),
                text: None,
                repeat: false,
            })
        };
        // Close the pointer-opened overlay, then exercise the keyboard path
        // that directly downcasts ContextMenu's immediate editor child.
        for _ in 0..2 {
            {
                let mut overlay = cached
                    .as_widget_mut()
                    .overlay(
                        &mut cached_tree,
                        Layout::new(&cached_node),
                        &renderer,
                        &viewport,
                        Vector::ZERO,
                    )
                    .expect("context menu");
                let menu_node = overlay.as_overlay_mut().layout(&renderer, SIZE);
                let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
                overlay.as_overlay_mut().update(
                    &key(Named::Escape),
                    Layout::new(&menu_node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut shell,
                );
            }
            assert!(
                cached
                    .as_widget_mut()
                    .overlay(
                        &mut cached_tree,
                        Layout::new(&cached_node),
                        &renderer,
                        &viewport,
                        Vector::ZERO
                    )
                    .is_none()
            );
            let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
            cached.as_widget_mut().update(
                &mut cached_tree,
                &Event::Window(window::Event::RedrawRequested(Instant::now())),
                Layout::new(&cached_node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut shell,
                &viewport,
            );
            assert!(matches!(
                shell.input_method(),
                iced::advanced::InputMethod::Enabled { .. }
            ));
            let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
            cached.as_widget_mut().update(
                &mut cached_tree,
                &key(Named::ContextMenu),
                Layout::new(&cached_node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut shell,
                &viewport,
            );
            assert!(shell.is_event_captured());
            assert!(matches!(
                shell.input_method(),
                iced::advanced::InputMethod::Disabled
            ));
        }
    }

    #[test]
    fn cached_status_bar_has_no_animation_deadline() {
        use crate::core::{Document, DocumentId, EditorSettings};

        let renderer = futures::executor::block_on(<iced::Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .expect("software renderer");
        let document = Document::untitled(DocumentId::new(2));
        let settings = EditorSettings::default();
        let mut content = cached(crate::ui::status_bar::view(
            Some(&document),
            &settings,
            Some("indexing"),
        ));
        let (mut tree, node) = mount(&mut content, &renderer, SIZE);
        let now = Instant::now();
        for at in [
            now,
            now + Duration::from_millis(16),
            now + Duration::from_secs(1),
        ] {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&window::Headless, Waker::noop(), &mut messages);
            content.as_widget_mut().update(
                &mut tree,
                &Event::Window(window::Event::RedrawRequested(at)),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut shell,
                &Rectangle::with_size(SIZE),
            );
            assert_eq!(shell.redraw_request(), window::RedrawRequest::Wait);
            assert!(shell.is_empty());
            assert_eq!(
                tree.state.downcast_ref::<State>().pending_frame(),
                window::RedrawRequest::Wait
            );
        }
    }
}
