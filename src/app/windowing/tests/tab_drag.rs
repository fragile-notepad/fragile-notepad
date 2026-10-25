use super::*;

#[test]
fn horizontal_drag_outside_strip_or_window_stays_attached_with_vertical_jitter() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    let _remaining = add_tab(&mut app, source);
    let session = start_drag(&mut app, source, moving);
    let source_native = native(&app, source);
    for y in [20.0, 35.0, 60.0, 92.0, 112.0] {
        for x in [-600.0, -1.0, 350.0, 701.0, 1400.0] {
            for hovered in [None, Some((source_native, Point::new(x, y)))] {
                let _ = app.update(Message::TabDragEvent(DragEvent::Motion {
                    session,
                    window: source_native,
                    position: Point::new(x, y),
                    desktop: Some(Point::new(x + 1000.0, y + 200.0)),
                    hovered,
                }));
                assert_eq!(app.workspace.owner_of(moving), Some(source), "{x}, {y}");
                assert_eq!(app.editor_windows.len(), 1);
                assert!(app.tab_drag_position(source_native).is_some());
            }
        }
    }
    let _ = app.update(Message::TabDragEvent(DragEvent::Released { session }));
    assert_eq!(app.workspace.owner_of(moving), Some(source));
    assert!(app.tab_drag.is_none());
}

#[test]
fn detach_requires_crossing_the_larger_vertical_band_in_either_direction() {
    for (retained_y, detached_y) in [(14.0, 13.0), (113.0, 114.0)] {
        let (mut app, _) = App::new();
        let source = app.workspace.current_window_id();
        let moving = app.workspace.active_document_id();
        add_tab(&mut app, source);
        let session = start_drag(&mut app, source, moving);
        for (y, should_detach) in [(retained_y, false), (detached_y, true)] {
            let _ = app.update(Message::TabDragEvent(DragEvent::Motion {
                session,
                window: native(&app, source),
                position: Point::new(300.0, y),
                desktop: Some(Point::new(900.0, y + 300.0)),
                hovered: None,
            }));
            assert_eq!(
                app.workspace.owner_of(moving) != Some(source),
                should_detach
            );
        }
    }
}

#[test]
fn reattached_drag_uses_new_capture_coordinates_and_ignores_queued_old_samples() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    add_tab(&mut app, source);
    let target = another_window(&mut app);
    let session = start_drag(&mut app, source, moving);
    drag_into(&mut app, source, target, session);
    let count = app.editor_windows.len();
    for (window, point) in [
        (native(&app, source), Point::new(300.0, 180.0)),
        (native(&app, target), Point::new(-700.0, 60.0)),
        (native(&app, target), Point::new(1400.0, 92.0)),
    ] {
        let _ = app.update(Message::TabDragEvent(DragEvent::Motion {
            session,
            window,
            position: point,
            desktop: Some(Point::new(1200.0, 300.0)),
            hovered: None,
        }));
        assert_eq!(app.workspace.owner_of(moving), Some(target));
        assert_eq!(app.editor_windows.len(), count);
    }
    let _ = app.update(Message::TabDragEvent(DragEvent::Motion {
        session,
        window: native(&app, target),
        position: Point::new(300.0, 114.0),
        desktop: Some(Point::new(1200.0, 414.0)),
        hovered: None,
    }));
    assert_ne!(app.workspace.owner_of(moving), Some(target));
    let _ = app.update(Message::TabDragEvent(DragEvent::Cancelled {
        session,
        reason: CancelReason::Requested,
    }));
    assert_eq!(app.workspace.owner_of(moving), Some(source));
    assert_eq!(app.editor_windows.len(), count);
}

#[test]
fn new_detached_window_needs_no_show_or_restore_round_trip() {
    for reuse_source in [false, true] {
        let (mut app, _) = App::new();
        let source = app.workspace.current_window_id();
        let moving = app.workspace.active_document_id();
        if !reuse_source {
            add_tab(&mut app, source);
        }
        let session = start_drag(&mut app, source, moving);
        drag_out(&mut app, source, session);
        let detached = app.workspace.owner_of(moving).unwrap();
        assert_eq!(detached == source, reuse_source);
        let ready = app.detached_tab_ready(session, native(&app, detached));
        if reuse_source {
            // A reused source can still be maximized or fullscreen.
            assert!(ready.units() > 0);
        } else {
            // Creation settings register following before first presentation.
            assert_eq!(ready.units(), 0);
        }
    }
}

#[test]
fn stationary_pointer_keeps_reordered_tabs_stable_and_preserves_the_grab_offset() {
    let (mut app, _) = App::new();
    let source = app.workspace.current_window_id();
    let moving = app.workspace.active_document_id();
    let second = add_tab(&mut app, source);
    let third = add_tab(&mut app, source);
    let session = start_drag(&mut app, source, moving);
    let point = Point::new(190.0, 60.0);
    let _ = app.update(Message::TabDragEvent(DragEvent::Motion {
        session,
        window: native(&app, source),
        position: point,
        desktop: Some(point),
        hovered: Some((native(&app, source), point)),
    }));
    for _ in 0..8 {
        let shape = geometry(&app, source);
        send(&mut app, source, Message::TabStripGeometry(shape));
        assert_eq!(
            app.workspace.window_tab_ids(source),
            &[second, moving, third]
        );
        let preview = app.tab_drag_position(native(&app, source)).unwrap();
        assert_eq!(preview.pointer_x, 190.0);
        assert_eq!(preview.grab_offset, 20.0);
    }
    let _ = app.update(Message::TabDragEvent(DragEvent::Cancelled {
        session,
        reason: CancelReason::Requested,
    }));
    assert_eq!(
        app.workspace.window_tab_ids(source),
        &[moving, second, third]
    );
    assert!(app.tab_drag_position(native(&app, source)).is_none());
}
