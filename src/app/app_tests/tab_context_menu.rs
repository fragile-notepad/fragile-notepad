use crate::app::App;
use crate::message::{Message, TabContextAction};
use iced::window;

#[test]
fn tab_context_popup_survives_full_app_view_rebuild_and_closes_its_inactive_target() {
    use iced::advanced::graphics::core::shell::Waker;
    use iced::advanced::mouse;
    use iced::advanced::renderer::{self, Headless};
    use iced::keyboard::{self, Key, key::Named};
    use iced::{Color, Event, Point, Renderer, Size};
    use iced_runtime::user_interface::{self, UserInterface};

    let _guard = crate::font_system_test_guard();
    let mut renderer = futures::executor::block_on(<Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("CPU renderer");
    let (mut app, _) = App::new();
    let clicked = app.workspace.active_document_id();
    app.workspace.create_untitled();
    let active = app.workspace.create_untitled();
    app.settings.appearance = crate::core::AppearanceMode::Dark;
    let native = app.main_window_id.unwrap();
    let theme = app.theme(native).unwrap();
    let size = Size::new(1000.0, 700.0);
    let frame = Event::Window(window::Event::RedrawRequested(iced::time::Instant::now()));
    let mut messages = Vec::new();
    let mut ui = UserInterface::build(
        app.view(native),
        size,
        user_interface::Cache::new(),
        &mut renderer,
    );
    ui.update(
        &window::Headless,
        &Waker::noop(),
        &[frame],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut messages,
    );
    let point = messages
        .iter()
        .find_map(|message| match message {
            Message::ForWindow(id, inner) if *id == native => match inner.as_ref() {
                Message::TabStripGeometry(geometry) => geometry
                    .tabs
                    .iter()
                    .find(|(id, _)| *id == clicked)
                    .map(|(_, bounds)| bounds.center()),
                _ => None,
            },
            _ => None,
        })
        .expect("full application exposes the clicked tab geometry");
    messages.clear();
    let (_, statuses) = ui.update(
        &window::Headless,
        &Waker::noop(),
        &[Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Right,
        ))],
        mouse::Cursor::Available(point),
        &mut renderer,
        &mut messages,
    );
    assert_eq!(statuses, vec![iced::event::Status::Captured]);
    assert!(messages.iter().any(|message| matches!(message,
        Message::ForWindow(id, inner)
            if *id == native && matches!(inner.as_ref(), Message::MenuClosed)
    )));
    let cache = ui.into_cache();
    for message in messages.drain(..) {
        let _ = app.update(message);
    }
    assert_eq!(app.workspace.active_document_id(), active);
    let mut ui = UserInterface::build(app.view(native), size, cache, &mut renderer);
    // The far-left interior of the second row contains only menu background.
    let sample_point = Point::new(point.x + 9.0, point.y + 45.0);
    let iced::Background::Color(background) = crate::ui::styles::menu_dropdown_band(&theme)
        .background
        .unwrap()
    else {
        panic!("menu background color");
    };
    let expected = background.into_rgba8();
    let sample = |renderer: &mut Renderer| -> [u8; 4] {
        let pixels = renderer.screenshot(Size::new(1000, 700), 1.0, Color::TRANSPARENT);
        let offset = (sample_point.y as usize * 1000 + sample_point.x as usize) * 4;
        pixels[offset..offset + 4].try_into().unwrap()
    };
    let entry_start = iced::time::Instant::now();
    for instant in [
        entry_start,
        entry_start + std::time::Duration::from_millis(25),
    ] {
        ui.update(
            &window::Headless,
            &Waker::noop(),
            &[Event::Window(window::Event::RedrawRequested(instant))],
            mouse::Cursor::Available(point),
            &mut renderer,
            &mut messages,
        );
    }
    ui.draw(
        &mut renderer,
        &theme,
        &renderer::Style::default(),
        mouse::Cursor::Available(point),
    );
    assert_ne!(
        sample(&mut renderer),
        expected,
        "entrance is initially translucent"
    );
    ui.update(
        &window::Headless,
        &Waker::noop(),
        &[Event::Window(window::Event::RedrawRequested(
            entry_start + std::time::Duration::from_millis(200),
        ))],
        mouse::Cursor::Available(point),
        &mut renderer,
        &mut messages,
    );
    // Preserve the settled animation through another complete view rebuild.
    let cache = ui.into_cache();
    let mut ui = UserInterface::build(app.view(native), size, cache, &mut renderer);
    let moved = Point::new(900.0, 650.0);
    for (event, cursor) in [
        (
            Event::Window(window::Event::RedrawRequested(
                entry_start + std::time::Duration::from_millis(250),
            )),
            mouse::Cursor::Available(point),
        ),
        (
            Event::Mouse(mouse::Event::CursorMoved { position: moved }),
            mouse::Cursor::Available(moved),
        ),
    ] {
        let (state, _) = ui.update(
            &window::Headless,
            &Waker::noop(),
            &[event],
            cursor,
            &mut renderer,
            &mut messages,
        );
        assert!(matches!(
            state,
            user_interface::State::Updated {
                input_method: iced::advanced::InputMethod::Disabled,
                ..
            }
        ));
        ui.draw(&mut renderer, &theme, &renderer::Style::default(), cursor);
        let actual = sample(&mut renderer);
        assert!(
            actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
            "popup must remain painted after redraw and pointer motion: {actual:?} != {expected:?}"
        );
    }
    for named in [Named::Home, Named::Enter] {
        ui.update(
            &window::Headless,
            &Waker::noop(),
            &[Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Named(named),
                modified_key: Key::Named(named),
                physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Enter),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::empty(),
                text: None,
                repeat: false,
            })],
            mouse::Cursor::Unavailable,
            &mut renderer,
            &mut messages,
        );
    }
    assert!(messages.iter().any(|message| matches!(message,
        Message::ForWindow(id, inner)
            if *id == native && matches!(inner.as_ref(),
                Message::TabContextAction(target, TabContextAction::Close) if *target == clicked)
    )));
    drop(ui);
    for message in messages {
        let _ = app.update(message);
    }
    assert!(app.workspace.document(clicked).is_none());
    assert_eq!(app.workspace.active_document_id(), active);
}
