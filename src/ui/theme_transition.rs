//! Draw both color modes from live widget state while their colors change.

use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, window};

use crate::message::Message;

const DURATION: Duration = Duration::from_millis(240);

/// Blend two live drawings of a complete window, retaining one input tree.
/// When `animate` is false, theme changes apply immediately. Enabling it also
/// applies the current theme immediately; subsequent changes animate.
pub fn view(content: Element<'_, Message>, theme: Theme, animate: bool) -> Element<'_, Message> {
    Element::new(ThemeTransition {
        content,
        theme,
        animate,
    })
}

struct ThemeTransition<'a> {
    content: Element<'a, Message>,
    theme: Theme,
    animate: bool,
}

#[derive(Clone)]
struct Blend {
    light: Theme,
    dark: Theme,
    amount: f32,
    target: f32,
    from: f32,
    started: Option<Instant>,
    animate: bool,
}

impl Blend {
    fn new(theme: Theme, animate: bool) -> Self {
        let dark = theme.palette().is_dark;
        Self {
            light: if dark { Theme::Light } else { theme.clone() },
            dark: if dark { theme } else { Theme::Dark },
            amount: f32::from(dark),
            target: f32::from(dark),
            from: f32::from(dark),
            started: None,
            animate,
        }
    }

    fn select(&mut self, theme: &Theme, animate: bool) {
        // Settle the final startup theme before animating later changes.
        let should_animate = self.animate && animate;
        self.animate = animate;
        let dark = theme.palette().is_dark;
        if dark {
            self.dark = theme.clone();
        } else {
            self.light = theme.clone();
        }
        let target = f32::from(dark);
        if !should_animate {
            self.amount = target;
            self.from = target;
            self.target = target;
            self.started = None;
        } else if target != self.target {
            self.from = self.amount;
            self.target = target;
            self.started = None;
        }
    }

    fn active(&self) -> bool {
        self.started.is_some() || self.amount != self.target
    }

    fn tick(&mut self, at: Instant) {
        if !self.active() {
            return;
        }
        let started = *self.started.get_or_insert(at);
        let elapsed = at.saturating_duration_since(started).as_secs_f32();
        let progress = (elapsed / DURATION.as_secs_f32()).min(1.0);
        let eased = progress * progress * (3.0 - 2.0 * progress);
        self.amount = self.from + (self.target - self.from) * eased;
        if progress >= 1.0 {
            self.amount = self.target;
            self.started = None;
        }
    }
}

impl Widget<Message, Theme, Renderer> for ThemeTransition<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Blend>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Blend::new(self.theme.clone(), self.animate))
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.state
            .downcast_mut::<Blend>()
            .select(&self.theme, self.animate);
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
        let blend = tree.state.downcast_mut::<Blend>();
        if let Event::Window(window::Event::RedrawRequested(at)) = event {
            blend.tick(*at);
        }
        if blend.active() {
            shell.request_redraw();
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn draw(
        &self,
        _tree: &Tree,
        _renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        // The root overlay draws the complete scene, including nested menus,
        // inside each variant's group. This keeps their overlap in the blend.
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
        _renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        Some(overlay::Element::new(Box::new(SceneOverlay {
            blend: tree.state.downcast_ref::<Blend>().clone(),
            scene: RefCell::new(Scene {
                content: &mut self.content,
                tree: &mut tree.children[0],
                layout,
                viewport: *viewport,
                translation,
            }),
        })))
    }
}

struct Scene<'a, 'b> {
    content: &'a mut Element<'b, Message>,
    tree: &'a mut Tree,
    layout: Layout<'a>,
    viewport: Rectangle,
    translation: Vector,
}

impl Scene<'_, '_> {
    fn overlay<'a>(
        &'a mut self,
        renderer: &Renderer,
    ) -> Option<overlay::Nested<'a, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(
                self.tree,
                self.layout,
                renderer,
                &self.viewport,
                self.translation,
            )
            .map(overlay::Nested::new)
    }

    fn draw(
        &mut self,
        renderer: &mut Renderer,
        theme: &Theme,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let interaction = self
            .overlay(renderer)
            .zip(layout.children().next())
            .map(|(mut overlay, layout)| overlay.mouse_interaction(layout, cursor, renderer))
            .unwrap_or_default();
        let style = renderer::Style {
            text_color: theme.palette().background.base.text,
        };
        renderer.with_layer(self.viewport, |renderer| {
            // Paint an opaque base so the finished variants blend as complete
            // windows even when their contents contain translucent controls.
            renderer.fill_quad(
                renderer::Quad {
                    bounds: self.viewport,
                    ..Default::default()
                },
                super::styles::app_shell_background(theme),
            );
            self.content.as_widget().draw(
                self.tree,
                renderer,
                theme,
                &style,
                self.layout,
                if interaction == mouse::Interaction::None {
                    cursor
                } else {
                    mouse::Cursor::Unavailable
                },
                &self.viewport,
            );
        });
        if let Some((mut overlay, layout)) = self.overlay(renderer).zip(layout.children().next()) {
            overlay.draw(renderer, theme, &style, layout, cursor);
        }
    }
}

struct SceneOverlay<'a, 'b> {
    blend: Blend,
    scene: RefCell<Scene<'a, 'b>>,
}

impl overlay::Overlay<Message, Theme, Renderer> for SceneOverlay<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        layout::Node::with_children(
            bounds,
            self.scene
                .get_mut()
                .overlay(renderer)
                .map(|mut overlay| overlay.layout(renderer, bounds))
                .into_iter()
                .collect(),
        )
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let mut scene = self.scene.borrow_mut();
        if self.blend.active() {
            scene.draw(renderer, &self.blend.light, layout, cursor);
            renderer.with_opacity_layer(layout.bounds(), self.blend.amount, |renderer| {
                scene.draw(renderer, &self.blend.dark, layout, cursor);
            });
        } else {
            let theme = if self.blend.target == 1.0 {
                &self.blend.dark
            } else {
                &self.blend.light
            };
            scene.draw(renderer, theme, layout, cursor);
        }
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        if let Some((mut overlay, layout)) = self
            .scene
            .get_mut()
            .overlay(renderer)
            .zip(layout.children().next())
        {
            overlay.operate(layout, renderer, operation);
        }
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        if let Some((mut overlay, layout)) = self
            .scene
            .get_mut()
            .overlay(renderer)
            .zip(layout.children().next())
        {
            overlay.update(event, layout, cursor, renderer, shell);
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.scene
            .borrow_mut()
            .overlay(renderer)
            .zip(layout.children().next())
            .map(|(mut overlay, layout)| overlay.mouse_interaction(layout, cursor, renderer))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::graphics::core::shell::Waker;
    use iced::advanced::renderer::Headless;
    use iced::{Color, Point};
    use iced_runtime::UserInterface;

    const SIZE: Size = Size::new(80.0, 48.0);

    fn renderer() -> Renderer {
        futures::executor::block_on(<Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .unwrap()
    }

    struct Scene {
        marker_x: f32,
    }

    impl Widget<Message, Theme, Renderer> for Scene {
        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Fill)
        }
        fn layout(&mut self, _: &mut Tree, _: &Renderer, limits: &layout::Limits) -> layout::Node {
            layout::Node::new(limits.max())
        }
        fn draw(
            &self,
            _: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            _: &renderer::Style,
            layout: Layout<'_>,
            _: mouse::Cursor,
            _: &Rectangle,
        ) {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: layout.bounds(),
                    ..Default::default()
                },
                if theme.palette().is_dark {
                    Color::BLACK
                } else {
                    Color::WHITE
                },
            );
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: self.marker_x,
                        y: 20.0,
                        width: 8.0,
                        height: 8.0,
                    },
                    ..Default::default()
                },
                if theme.palette().is_dark {
                    Color::from_rgb8(0, 0, 255)
                } else {
                    Color::from_rgb8(255, 0, 0)
                },
            );
        }
        fn overlay<'a>(
            &'a mut self,
            _: &'a mut Tree,
            _: Layout<'a>,
            _: &Renderer,
            _: &Rectangle,
            _: Vector,
        ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
            Some(overlay::Element::new(Box::new(Menu)))
        }
    }

    struct Menu;

    impl overlay::Overlay<Message, Theme, Renderer> for Menu {
        fn layout(&mut self, _: &Renderer, _: Size) -> layout::Node {
            layout::Node::new(Size::new(12.0, 12.0))
        }
        fn draw(
            &self,
            renderer: &mut Renderer,
            theme: &Theme,
            _: &renderer::Style,
            layout: Layout<'_>,
            _: mouse::Cursor,
        ) {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: layout.bounds(),
                    ..Default::default()
                },
                if theme.palette().is_dark {
                    Color::from_rgb8(0, 255, 0)
                } else {
                    Color::from_rgb8(255, 0, 0)
                },
            );
        }
    }

    fn draw(
        ui: &mut UserInterface<'_, Message, Theme, Renderer>,
        renderer: &mut Renderer,
    ) -> Vec<u8> {
        ui.draw(
            renderer,
            &Theme::Dark,
            &renderer::Style::default(),
            mouse::Cursor::Unavailable,
        );
        renderer.screenshot(
            Size::new(SIZE.width as u32, SIZE.height as u32),
            1.0,
            Color::BLACK,
        )
    }

    fn pixel(pixels: &[u8], x: usize, y: usize) -> &[u8] {
        let start = (y * SIZE.width as usize + x) * 4;
        &pixels[start..start + 4]
    }

    #[test]
    fn startup_color_modes_apply_immediately_in_either_load_order() {
        use crate::app::App;
        use crate::core::{AppearanceMode, EditorSettings};
        use crate::services::types::SettingsError;

        let saved = |appearance| {
            let mut settings = EditorSettings::default();
            settings.set_appearance(appearance);
            Ok(Some(settings))
        };
        for result in [
            saved(AppearanceMode::System),
            saved(AppearanceMode::Light),
            saved(AppearanceMode::Dark),
            Ok(None),
            Err(SettingsError::Unavailable),
        ] {
            let appearance = match &result {
                Ok(Some(settings)) => settings.appearance,
                _ => EditorSettings::default().appearance,
            };
            for settings_first in [false, true] {
                let (mut app, _) = App::new();
                let window_id = window::Id::unique();
                let mut tree = Tree::new(app.view(window_id).as_widget());
                let mut messages = [
                    Message::SystemColorModeChanged(iced::theme::Mode::Dark),
                    Message::SettingsLoaded(result.clone()),
                ];
                if settings_first {
                    messages.reverse();
                }
                for message in messages {
                    let _ = app.update(message);
                    tree.diff(app.view(window_id).as_widget_mut());
                    let blend = tree.state.downcast_ref::<Blend>();
                    assert!(
                        !blend.active(),
                        "{appearance:?}, settings first: {settings_first}"
                    );
                }
                let dark = appearance != AppearanceMode::Light;
                assert_eq!(tree.state.downcast_ref::<Blend>().amount, f32::from(dark));

                // Once startup finishes, an actual settings edit still animates.
                let _ = app.update(Message::DraftAppearanceSelected(if dark {
                    AppearanceMode::Light
                } else {
                    AppearanceMode::Dark
                }));
                let _ = app.update(Message::ApplySettings);
                tree.diff(app.view(window_id).as_widget_mut());
                let blend = tree.state.downcast_ref::<Blend>();
                assert!(blend.active());
                assert_eq!(blend.target, f32::from(!dark));

                if appearance == AppearanceMode::System {
                    let _ = app.update(Message::DraftAppearanceSelected(AppearanceMode::System));
                    let _ = app.update(Message::ApplySettings);
                    tree.diff(app.view(window_id).as_widget_mut());
                    let _ = app.update(Message::SystemColorModeChanged(iced::theme::Mode::Light));
                    tree.diff(app.view(window_id).as_widget_mut());
                    let blend = tree.state.downcast_ref::<Blend>();
                    assert!(blend.active());
                    assert_eq!(blend.target, 0.0);
                }
            }
        }
    }

    #[test]
    fn explicit_color_modes_animate_without_waiting_for_the_system_theme() {
        use crate::app::App;
        use crate::core::{AppearanceMode, EditorSettings};

        for appearance in [AppearanceMode::Light, AppearanceMode::Dark] {
            let (mut app, _) = App::new();
            let window_id = window::Id::unique();
            let mut tree = Tree::new(app.view(window_id).as_widget());
            let mut settings = EditorSettings::default();
            settings.set_appearance(appearance);
            let _ = app.update(Message::SettingsLoaded(Ok(Some(settings))));
            tree.diff(app.view(window_id).as_widget_mut());
            assert!(!tree.state.downcast_ref::<Blend>().active());

            let dark = appearance == AppearanceMode::Dark;
            let _ = app.update(Message::DraftAppearanceSelected(if dark {
                AppearanceMode::Light
            } else {
                AppearanceMode::Dark
            }));
            let _ = app.update(Message::ApplySettings);
            tree.diff(app.view(window_id).as_widget_mut());
            let blend = tree.state.downcast_ref::<Blend>();
            assert!(blend.active());
            assert_eq!(blend.target, f32::from(!dark));
        }
    }

    #[test]
    fn disabling_animation_settles_an_active_transition() {
        let mut blend = Blend::new(Theme::Light, true);
        blend.select(&Theme::Dark, true);
        let start = Instant::now();
        blend.tick(start);
        blend.tick(start + DURATION / 2);
        assert!(blend.active());

        blend.select(&Theme::Dark, false);
        assert!(!blend.active());
        assert_eq!(blend.amount, 1.0);

        blend.select(&Theme::Light, true);
        assert!(!blend.active());
        assert_eq!(blend.amount, 0.0);
        blend.select(&Theme::Dark, true);
        assert!(blend.active());
    }

    #[test]
    fn both_variants_draw_live_contents_and_menus_during_the_transition() {
        let mut renderer = renderer();
        let mut ui = UserInterface::build(
            view(Element::new(Scene { marker_x: 20.0 }), Theme::Light, true),
            SIZE,
            Default::default(),
            &mut renderer,
        );
        let light = draw(&mut ui, &mut renderer);
        assert_eq!(pixel(&light, 2, 2), [255, 0, 0, 255]);
        let cache = ui.into_cache();
        let mut ui = UserInterface::build(
            view(Element::new(Scene { marker_x: 40.0 }), Theme::Dark, true),
            SIZE,
            cache,
            &mut renderer,
        );
        let start = Instant::now();
        let mut messages = Vec::new();
        ui.update(
            &window::Headless,
            &Waker::noop(),
            &[
                Event::Window(window::Event::RedrawRequested(start)),
                Event::Window(window::Event::RedrawRequested(start + DURATION / 2)),
            ],
            mouse::Cursor::Unavailable,
            &mut renderer,
            &mut messages,
        );
        let halfway = draw(&mut ui, &mut renderer);
        let marker = pixel(&halfway, 42, 22);
        assert!(
            marker[0] > 100 && marker[2] > 100,
            "both live marker colors must blend: {marker:?}"
        );
        let old_position = pixel(&halfway, 22, 22);
        assert_eq!(old_position[0], old_position[1]);
        assert_eq!(old_position[1], old_position[2]);
        let menu = pixel(&halfway, 2, 2);
        assert!(
            menu[0] > 100 && menu[1] > 100,
            "menus must blend within each variant: {menu:?}"
        );
        ui.update(
            &window::Headless,
            &Waker::noop(),
            &[Event::Window(window::Event::RedrawRequested(
                start + DURATION,
            ))],
            mouse::Cursor::Unavailable,
            &mut renderer,
            &mut messages,
        );
        let dark = draw(&mut ui, &mut renderer);
        assert_eq!(pixel(&dark, 42, 22), [0, 0, 255, 255]);
    }

    #[test]
    fn reversing_a_transition_preserves_the_current_color_mix_and_finishes() {
        let mut blend = Blend::new(Theme::Light, true);
        blend.select(&Theme::Dark, true);
        let start = Instant::now();
        blend.tick(start);
        blend.tick(start + DURATION / 3);
        let midway = blend.amount;
        blend.select(&Theme::Light, true);
        assert_eq!(blend.amount, midway);
        blend.tick(start + DURATION / 3);
        assert_eq!(blend.amount, midway);
        blend.tick(start + DURATION * 2);
        assert_eq!(blend.amount, 0.0);
        assert!(!blend.active());
    }

    #[test]
    fn input_remains_single_and_keeps_focus_across_theme_changes() {
        use iced::widget::{button, column, text_input};
        use iced::{event, keyboard};
        let _guard = crate::font_system_test_guard();
        let mut renderer = renderer();
        let mut cache = Default::default();
        for (index, theme) in [Theme::Light, Theme::Dark, Theme::Light]
            .into_iter()
            .enumerate()
        {
            let content = column![
                button("press").on_press(Message::None).width(80).height(20),
                text_input("", "")
                    .id("theme-input")
                    .on_input(|_| Message::ZoomIn)
            ];
            let mut ui = UserInterface::build(
                view(content.into(), theme, true),
                SIZE,
                cache,
                &mut renderer,
            );
            if index == 0 {
                let mut focus =
                    iced::advanced::widget::operation::focusable::focus("theme-input".into());
                ui.operate(&renderer, &mut focus);
            }
            let mut messages = Vec::new();
            let (_, statuses) = ui.update(
                &window::Headless,
                &Waker::noop(),
                &[Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Character("x".into()),
                    modified_key: keyboard::Key::Character("x".into()),
                    physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyX),
                    location: keyboard::Location::Standard,
                    modifiers: keyboard::Modifiers::empty(),
                    text: Some("x".into()),
                    repeat: false,
                })],
                mouse::Cursor::Available(Point::new(70.0, 40.0)),
                &mut renderer,
                &mut messages,
            );
            assert_eq!(statuses, [event::Status::Captured]);
            assert_eq!(messages.len(), 1);
            assert!(matches!(messages[0], Message::ZoomIn));
            // Captured typing clears Iced's saved overlay layout. Painting the
            // same frame must still draw the live window and its transition.
            let pixels = draw(&mut ui, &mut renderer);
            assert!(pixels.chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]));
            cache = ui.into_cache();
        }
    }
}
