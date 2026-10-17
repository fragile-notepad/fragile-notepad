use fragile_notepad::core::Workspace;

#[test]
fn pinning_moves_document_to_pinned_group_without_changing_active_document() {
    let mut workspace = Workspace::new();
    let first = workspace.active_document_id();
    let second = workspace.create_untitled();
    let third = workspace.create_untitled();

    assert!(workspace.toggle_pin(third));

    assert_eq!(
        workspace
            .documents()
            .iter()
            .map(|document| document.id)
            .collect::<Vec<_>>(),
        vec![third, first, second]
    );
    assert_eq!(workspace.pinned_count(), 1);
    assert_eq!(workspace.active_document_id(), third);
}

#[test]
fn unpinning_moves_document_to_end_of_unpinned_group() {
    let mut workspace = Workspace::new();
    let first = workspace.active_document_id();
    let second = workspace.create_untitled();
    let third = workspace.create_untitled();

    workspace.toggle_pin(second);
    workspace.toggle_pin(third);
    assert_eq!(workspace.pinned_count(), 2);

    assert!(workspace.toggle_pin(second));

    assert_eq!(
        workspace
            .documents()
            .iter()
            .map(|document| document.id)
            .collect::<Vec<_>>(),
        vec![third, first, second]
    );
    assert_eq!(workspace.pinned_count(), 1);
}

#[test]
fn reorder_forward_places_document_after_target() {
    let mut workspace = Workspace::new();
    let first = workspace.active_document_id();
    let second = workspace.create_untitled();
    let third = workspace.create_untitled();

    assert!(workspace.reorder(first, second));

    assert_eq!(
        workspace
            .documents()
            .iter()
            .map(|document| document.id)
            .collect::<Vec<_>>(),
        vec![second, first, third]
    );
}

#[test]
fn reorder_backward_places_document_before_target() {
    let mut workspace = Workspace::new();
    let first = workspace.active_document_id();
    let second = workspace.create_untitled();
    let third = workspace.create_untitled();

    assert!(workspace.reorder(third, first));

    assert_eq!(
        workspace
            .documents()
            .iter()
            .map(|document| document.id)
            .collect::<Vec<_>>(),
        vec![third, first, second]
    );
}

#[test]
fn reorder_rejects_crossing_pinned_boundary() {
    let mut workspace = Workspace::new();
    let first = workspace.active_document_id();
    let second = workspace.create_untitled();

    workspace.toggle_pin(second);

    assert!(!workspace.reorder(first, second));
    assert_eq!(
        workspace
            .documents()
            .iter()
            .map(|document| document.id)
            .collect::<Vec<_>>(),
        vec![second, first]
    );
}

#[test]
fn windows_keep_independent_active_tabs_and_unique_document_owners() {
    let mut workspace = Workspace::new();
    let first_window = workspace.current_window_id();
    let first = workspace.active_document_id();
    let second = workspace.create_untitled();
    let second_window = workspace.create_window();
    let third = workspace.active_document_id();

    assert_eq!(workspace.owner_of(first), Some(first_window));
    assert_eq!(workspace.owner_of(third), Some(second_window));
    assert_eq!(
        workspace.active_document_in(first_window).unwrap().id,
        second
    );
    assert_eq!(
        workspace.active_document_in(second_window).unwrap().id,
        third
    );
    assert!(workspace.set_current_window(first_window));
    assert_eq!(workspace.document_ids(), vec![first, second]);
    assert_eq!(workspace.documents().len(), 3);
    assert!(!workspace.select_in_window(first_window, third));
}

#[test]
fn transferring_tabs_preserves_live_documents_and_only_emits_layout_events() {
    use fragile_notepad::core::workspace::changes::WorkspaceEvent;
    let mut workspace = Workspace::new();
    let source = workspace.current_window_id();
    let first = workspace.active_document_id();
    let moved = workspace.insert_loaded_file("moving.txt", "keep this text");
    workspace.document_mut(moved).unwrap().mark_dirty();
    let target = workspace.create_window();
    let target_original = workspace.active_document_id();
    workspace.publish_changes(|_| {});

    assert!(workspace.transfer_tabs(source, &[moved], target, 0));
    assert_eq!(workspace.window_tab_ids(source), &[first]);
    assert_eq!(workspace.window_tab_ids(target), &[moved, target_original]);
    assert_eq!(workspace.owner_of(moved), Some(target));
    assert!(workspace.document(moved).unwrap().is_dirty);
    assert_eq!(
        workspace.document(moved).unwrap().buffer.text(),
        "keep this text"
    );
    let mut events = Vec::new();
    workspace.publish_changes(|event| events.push(event));
    assert!(events.contains(&WorkspaceEvent::WindowTabsChanged(source)));
    assert!(events.contains(&WorkspaceEvent::WindowTabsChanged(target)));
    assert!(!events.iter().any(|event| matches!(
        event,
        WorkspaceEvent::DocumentOpened(_) | WorkspaceEvent::DocumentClosed(_)
    )));
}

#[test]
fn transferring_all_tabs_leaves_a_removable_empty_source() {
    let mut workspace = Workspace::new();
    let source = workspace.current_window_id();
    let id = workspace.active_document_id();
    let target = workspace.create_empty_window();
    assert!(workspace.transfer_tabs(source, &[id], target, 0));
    assert!(workspace.active_document_in(source).is_none());
    assert!(workspace.remove_empty_window(source));
    assert_eq!(workspace.current_window_id(), target);
    assert_eq!(workspace.active_document_id(), id);
    assert_eq!(workspace.documents().len(), 1);
    assert!(!workspace.remove_empty_window(target));
}

#[test]
fn group_transfer_keeps_visual_order_and_pinned_boundary() {
    let mut workspace = Workspace::new();
    let source = workspace.current_window_id();
    let first = workspace.active_document_id();
    let pinned = workspace.create_untitled();
    workspace.toggle_pin(pinned);
    let third = workspace.create_untitled();
    let target = workspace.create_window();
    let original = workspace.active_document_id();
    assert!(workspace.transfer_tabs(source, &[third, pinned], target, 1));
    assert_eq!(workspace.window_tab_ids(source), &[first]);
    assert_eq!(workspace.window_tab_ids(target), &[pinned, original, third]);
    assert_eq!(
        workspace.window(target).unwrap().selected_tabs,
        vec![pinned, third]
    );
    assert_eq!(workspace.active_document_in(target).unwrap().id, third);
}

#[test]
fn invalid_group_transfer_is_atomic_and_reorder_adjusts_original_insertion_slot() {
    let mut workspace = Workspace::new();
    let window = workspace.current_window_id();
    let first = workspace.active_document_id();
    let second = workspace.create_untitled();
    let third = workspace.create_untitled();
    let fourth = workspace.create_untitled();
    let original = workspace.window_tab_ids(window).to_vec();
    assert!(!workspace.transfer_tabs(window, &[first, first], window, 4));
    assert_eq!(workspace.window_tab_ids(window), original);
    assert!(workspace.transfer_tabs(window, &[first, second], window, 4));
    assert_eq!(
        workspace.window_tab_ids(window),
        &[third, fourth, first, second]
    );
}

#[test]
fn range_and_toggle_selection_follow_each_windows_visual_tab_order() {
    let mut workspace = Workspace::new();
    let window = workspace.current_window_id();
    let first = workspace.active_document_id();
    let second = workspace.create_untitled();
    let third = workspace.create_untitled();
    workspace.select(first);
    workspace.select_tab_range(window, third);
    assert_eq!(
        workspace.window(window).unwrap().selected_tabs,
        vec![first, second, third]
    );
    workspace.toggle_tab_selection(window, second);
    assert_eq!(
        workspace.window(window).unwrap().selected_tabs,
        vec![first, third]
    );
    workspace.toggle_tab_selection(window, first);
    workspace.toggle_tab_selection(window, third);
    assert_eq!(workspace.window(window).unwrap().selected_tabs, vec![third]);
}

#[test]
fn canceling_drag_restores_membership_without_replacing_edited_buffers() {
    let mut workspace = Workspace::new();
    let source = workspace.current_window_id();
    let id = workspace.active_document_id();
    let original = workspace.window(source).unwrap().clone();
    let target = workspace.create_empty_window();
    workspace.transfer_tabs(source, &[id], target, 0);
    workspace.document_mut(id).unwrap().buffer =
        fragile_notepad::editor::EditorBuffer::from_text("edited during drag");
    assert!(workspace.restore_window_layout(&[(source, original)]));
    assert_eq!(workspace.owner_of(id), Some(source));
    assert!(workspace.window_tab_ids(target).is_empty());
    assert_eq!(
        workspace.document(id).unwrap().buffer.text(),
        "edited during drag"
    );
}
