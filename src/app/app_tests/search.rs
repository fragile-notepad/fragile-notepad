use super::test_support::*;
use crate::core::DocumentId;

#[test]
fn wrapped_find_reveals_the_screen_row_containing_a_deep_match() {
    let (mut app, _) = App::new();
    let text = format!("{}needle{}", "x".repeat(600), "z".repeat(100));
    set_active_document_text(
        &mut app,
        &text,
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
    );
    let document = app.workspace.active_document_mut().unwrap();
    document.update_viewport_geometry(6, 82.0, 8.0);
    document.set_word_wrap(true);
    let _ = app.update(Message::AdvancedSearchQueryChanged("needle".into()));
    let _ = app.update(Message::AdvancedFindNextRun);
    let document = app.workspace.active_document().unwrap();
    let start = document.main_selection().range().start;
    assert_eq!(start, EditorPosition::new(0, 600));
    let row = document.viewport.position_to_visible_row(start).unwrap();
    assert!(row >= 60);
    assert!(document.scroll.first_visible_row <= row);
    assert!(row < document.scroll.first_visible_row + document.viewport_visible_rows);
}

fn deferred_search_document(
    app: &mut App,
    path: &str,
    recovered: Option<&str>,
) -> (DocumentId, crate::core::DocumentLoadGeneration) {
    let id = app.workspace.generate_document_id();
    let generation = crate::core::DocumentLoadGeneration::next();
    let mut document = crate::core::Document::loading(id, path, generation);
    document.load_state = crate::core::DocumentLoadState::Deferred { generation };
    document.defer_analysis = true;
    app.workspace.push_document(document);
    app.session.defer_document(
        id,
        crate::core::session::SessionDocument {
            path: Some(path.into()),
            text: recovered.map(str::to_owned),
            is_dirty: recovered.is_some(),
            ..Default::default()
        },
    );
    (id, generation)
}

fn finish_search_document(
    app: &mut App,
    id: DocumentId,
    generation: crate::core::DocumentLoadGeneration,
    text: &str,
) {
    app.workspace.document_mut(id).unwrap().complete_loading(
        generation,
        crate::core::DecodedText {
            text: text.to_owned(),
            encoding: crate::core::TextEncoding::Utf8,
            had_errors: false,
        },
    );
    let _ = app.update(Message::None);
}

#[test]
fn find_all_hydrates_deferred_disk_and_recovered_tabs_without_switching_tabs() {
    let (mut app, _) = App::new();
    let active = app.workspace.active_document_id();
    set_active_document_text(
        &mut app,
        "needle",
        EditorSelection::new(EditorPosition::new(0, 2), EditorPosition::new(0, 2)),
    );
    let (disk, generation) = deferred_search_document(&mut app, "disk.txt", None);
    let (recovered, _) =
        deferred_search_document(&mut app, "recovered.txt", Some("needle recovered"));
    let order = app
        .workspace
        .documents()
        .iter()
        .map(|document| document.id)
        .collect::<Vec<_>>();
    app.search_dialog.query = "needle".into();
    let _ = app.update(Message::AdvancedFindAllOpenRun);
    assert!(app.pending_search.is_some());
    assert!(
        app.workspace
            .document(recovered)
            .unwrap()
            .has_complete_text_index()
    );
    assert!(app.workspace.document(disk).unwrap().is_loading());
    finish_search_document(&mut app, disk, generation, "needle disk");
    assert!(app.pending_search.is_none());
    assert_eq!(app.search_dialog.results.len(), 3);
    assert_eq!(app.workspace.active_document_id(), active);
    assert_eq!(
        app.workspace
            .active_document()
            .unwrap()
            .main_selection()
            .cursor
            .column,
        2
    );
    assert_eq!(
        app.workspace
            .documents()
            .iter()
            .map(|document| document.id)
            .collect::<Vec<_>>(),
        order
    );
}

#[test]
fn count_hydrates_matching_scope_and_clears_previously_displayed_results() {
    let (mut app, _) = App::new();
    let active = app.workspace.active_document_id();
    set_active_document_text(
        &mut app,
        "needle needle",
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
    );
    app.search_dialog.set_query("needle");
    let _ = app.update(Message::AdvancedFindAllCurrentRun);
    assert_eq!(app.search_dialog.results.len(), 2);
    let (disk, generation) = deferred_search_document(&mut app, "disk.txt", None);
    let (excluded, _) = deferred_search_document(&mut app, "excluded.rs", None);
    app.search_dialog.active_tab = crate::message::AdvancedSearchTab::FindInFiles;
    app.search_dialog.include_pattern = "*.txt".into();
    let _ = app.update(Message::AdvancedCountRun);
    assert!(app.pending_search.is_some());
    assert!(app.search_dialog.results.is_empty());
    assert_eq!(app.search_dialog.match_count, 0);
    finish_search_document(&mut app, disk, generation, "needle needle needle");

    assert!(app.pending_search.is_none());
    assert_eq!(app.search_dialog.match_count, 3);
    assert_eq!(app.search_dialog.status, "3 matches");
    assert!(app.search_dialog.results.is_empty());
    assert_eq!(app.workspace.active_document_id(), active);
    assert!(matches!(
        app.workspace.document(excluded).unwrap().load_state,
        crate::core::DocumentLoadState::Deferred { .. }
    ));
}

#[test]
fn search_result_options_apply_to_next_search_and_preserve_last_valid_preferences() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::SettingsLoaded(Ok(None)));
    set_active_document_text(
        &mut app,
        &"a".repeat(1024),
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
    );
    app.search_dialog.set_query("a");
    let _ = app.update(Message::AdvancedFindAllCurrentRun);
    assert_eq!(app.search_dialog.results.len(), 500);
    let _ = app.update(Message::AdvancedPreviewContextChanged("4".into()));
    let _ = app.update(Message::AdvancedPreviewCharsChanged("16".into()));
    let _ = app.update(Message::AdvancedResultLimitChanged("3".into()));
    assert_eq!(app.settings.search_results.result_limit, 3);
    assert_eq!(app.settings.search_results.preview_chars, 16);
    assert!(app.settings_persistence.flush_scheduled());
    assert_eq!(
        app.settings_dialog.draft.search_results,
        app.settings.search_results
    );
    assert_eq!(
        app.search_dialog.results.len(),
        500,
        "existing rows stay until the next search"
    );
    let _ = app.update(Message::AdvancedFindAllCurrentRun);
    assert_eq!(app.search_dialog.results.len(), 3);
    assert!(
        app.search_dialog
            .results
            .iter()
            .all(|result| result.preview.chars().count() <= 18)
    );
    let _ = app.update(Message::AdvancedResultLimitChanged("0".into()));
    let _ = app.update(Message::AdvancedFindAllCurrentRun);
    assert!(app.search_dialog.result_options_visible);
    assert!(app.search_dialog.status.contains("between 1 and 10,000"));
    assert_eq!(app.settings.search_results.result_limit, 3);
    let _ = app.update(Message::AdvancedCountRun);
    assert_eq!(app.search_dialog.match_count, 1024);
    assert!(app.search_dialog.results.is_empty());
    let _ = app.update(Message::AdvancedResultOptionsReset);
    assert_eq!(
        app.settings.search_results,
        crate::core::SearchResultSettings::default()
    );
    assert!(app.search_dialog.parsed_result_settings().is_ok());
}

#[test]
fn early_search_options_reset_survives_saved_settings_load() {
    let (mut app, _) = App::new();
    let _ = app.update(Message::AdvancedResultOptionsReset);
    let mut saved = crate::core::EditorSettings::default();
    saved.search_results.result_limit = 1000;
    let _ = app.update(Message::SettingsLoaded(Ok(Some(saved))));
    assert_eq!(app.settings.search_results.result_limit, 500);
    assert_eq!(app.search_dialog.result_limit_input, "500");
}

#[test]
fn deferred_replace_all_uses_captured_options_scope_and_replacement() {
    let (mut app, _) = App::new();
    set_active_document_text(
        &mut app,
        "old OLD",
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
    );
    let original_active = app.workspace.active_document_id();
    let (disk, generation) = deferred_search_document(&mut app, "disk.txt", None);
    app.search_dialog.query = "old".into();
    app.search_dialog.replacement = "new".into();
    app.search_dialog.case_sensitive = true;
    let _ = app.update(Message::AdvancedReplaceAllOpenRun);
    assert_eq!(
        app.workspace.document(original_active).unwrap().text(),
        "old OLD",
        "wait before any mutation"
    );
    // Direct changes stand in for unrelated UI refreshes; captured request owns its inputs.
    app.search_dialog.query = "OLD".into();
    app.search_dialog.replacement = "wrong".into();
    app.search_dialog.case_sensitive = false;
    let extra = app.workspace.insert_loaded_file("later.txt", "old");
    finish_search_document(&mut app, disk, generation, "old OLD");
    assert_eq!(
        app.workspace.document(original_active).unwrap().text(),
        "new OLD"
    );
    assert_eq!(app.workspace.document(disk).unwrap().text(), "new OLD");
    assert_eq!(app.workspace.document(extra).unwrap().text(), "old");
}

#[test]
fn deferred_replace_all_aborts_before_mutation_on_failed_or_closed_target() {
    for closed in [false, true] {
        let (mut app, _) = App::new();
        set_active_document_text(
            &mut app,
            "old",
            EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
        );
        let active = app.workspace.active_document_id();
        let (disk, generation) = deferred_search_document(&mut app, "missing.txt", None);
        app.search_dialog.query = "old".into();
        app.search_dialog.replacement = "new".into();
        let _ = app.update(Message::AdvancedReplaceAllOpenRun);
        if closed {
            app.workspace.close(disk);
        } else {
            app.workspace
                .document_mut(disk)
                .unwrap()
                .fail_loading(generation);
        }
        let _ = app.update(Message::None);
        assert!(app.pending_search.is_none());
        assert!(app.search_dialog.status.contains("canceled"));
        assert_eq!(app.workspace.document(active).unwrap().text(), "old");
    }
}

#[test]
fn changing_query_cancels_deferred_replacement() {
    let (mut app, _) = App::new();
    let (disk, generation) = deferred_search_document(&mut app, "disk.txt", None);
    app.search_dialog.query = "old".into();
    app.search_dialog.replacement = "new".into();
    let _ = app.update(Message::AdvancedReplaceAllOpenRun);
    let _ = app.update(Message::AdvancedSearchQueryChanged("different".into()));
    finish_search_document(&mut app, disk, generation, "old");
    assert_eq!(app.workspace.document(disk).unwrap().text(), "old");
}

#[test]
fn new_find_operation_supersedes_deferred_replace_and_keeps_included_scope() {
    let (mut app, _) = App::new();
    let (disk, generation) = deferred_search_document(&mut app, "disk.txt", None);
    let (excluded, _) = deferred_search_document(&mut app, "excluded.rs", None);
    app.search_dialog.query = "old".into();
    app.search_dialog.replacement = "new".into();
    app.search_dialog.include_pattern = "*.txt".into();
    let _ = app.update(Message::AdvancedReplaceAllOpenRun);
    let _ = app.update(Message::AdvancedFindAllOpenRun);
    finish_search_document(&mut app, disk, generation, "old");
    assert_eq!(app.workspace.document(disk).unwrap().text(), "old");
    assert_eq!(app.search_dialog.results.len(), 1);
    assert!(matches!(
        app.workspace.document(excluded).unwrap().load_state,
        crate::core::DocumentLoadState::Deferred { .. }
    ));
}

#[test]
fn recovered_unsaved_document_is_included_in_replace_all() {
    let (mut app, _) = App::new();
    let active = app.workspace.active_document_id();
    let (recovered, _) = deferred_search_document(&mut app, "recovered.txt", Some("old old"));
    app.search_dialog.query = "old".into();
    app.search_dialog.replacement = "new".into();
    let _ = app.update(Message::AdvancedReplaceAllOpenRun);
    assert!(app.pending_search.is_none());
    assert_eq!(app.workspace.document(recovered).unwrap().text(), "new new");
    assert!(app.workspace.document_mut(recovered).unwrap().undo());
    assert_eq!(app.workspace.document(recovered).unwrap().text(), "old old");
    assert_eq!(app.workspace.active_document_id(), active);
}

#[test]
fn replace_all_is_one_undoable_multiline_transaction() {
    let (mut app, _) = App::new();
    let original = "one\ntwo\none\ntwo";
    set_active_document_text(
        &mut app,
        original,
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
    );
    let _ = app.update(Message::FindQueryChanged("one\ntwo".into()));
    let _ = app.update(Message::FindReplacementChanged("X".into()));
    let _ = app.update(Message::ReplaceAll);
    assert_eq!(app.workspace.active_document().unwrap().text(), "X\nX");
    let _ = app.update(Message::Undo);
    assert_eq!(app.workspace.active_document().unwrap().text(), original);
    let _ = app.update(Message::Redo);
    assert_eq!(app.workspace.active_document().unwrap().text(), "X\nX");
}

#[test]
fn regex_replace_all_uses_original_context_and_one_history_entry() {
    let (mut app, _) = App::new();
    set_active_document_text(
        &mut app,
        "xfoo yfoo",
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
    );
    app.search_dialog.query = r"\B(foo)".into();
    app.search_dialog.replacement = "<$1>".into();
    app.search_dialog.mode = crate::core::SearchMode::Regex;
    let _ = app.update(Message::AdvancedReplaceAllCurrentRun);
    assert_eq!(
        app.workspace.active_document().unwrap().text(),
        "x<foo> y<foo>"
    );
    let _ = app.update(Message::Undo);
    assert_eq!(app.workspace.active_document().unwrap().text(), "xfoo yfoo");
}

#[test]
fn replace_all_large_match_count_updates_document_once() {
    let (mut app, _) = App::new();
    set_active_document_text(
        &mut app,
        &"a ".repeat(2000),
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 0)),
    );
    let before = app.workspace.active_document().unwrap().revision();
    let _ = app.update(Message::FindQueryChanged("a".into()));
    let _ = app.update(Message::FindReplacementChanged("longer".into()));
    let started = std::time::Instant::now();
    let _ = app.update(Message::ReplaceAll);
    eprintln!(
        "replace_all 2000 matches: {:.2} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    let document = app.workspace.active_document().unwrap();
    assert_eq!(document.text(), "longer ".repeat(2000));
    assert_eq!(document.revision(), before + 1);
    assert!(document.selection_set().is_single());
}

#[test]
fn advanced_search_result_selection_scrolls_target_line_into_view() {
    let (mut app, _) = App::new();
    let document_id = app.workspace.active_document_id();
    let contents = (0..120)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");

    {
        let document = app
            .workspace
            .active_document_mut()
            .expect("active document");
        document.buffer = crate::editor::EditorBuffer::from_text(contents);
        document.refresh_after_text_change();
        document.mark_clean();
    }

    let selection = EditorSelection::new(EditorPosition::new(90, 5), EditorPosition::new(90, 7));
    let _ = app.update(Message::AdvancedSearchResultSelected(
        document_id,
        selection,
    ));

    let document = app.workspace.active_document().expect("active document");
    assert_eq!(document.selection, selection);
    assert!(
        document.scroll.first_visible_row <= 90,
        "scroll should move before or to the target row"
    );
    assert!(
        document.scroll.first_visible_row >= 80,
        "scroll should move near the target row, got {}",
        document.scroll.first_visible_row
    );
}

#[test]
fn advanced_find_next_respects_disabled_wrap_around() {
    let (mut app, _) = App::new();

    {
        let document = app
            .workspace
            .active_document_mut()
            .expect("active document");
        document.buffer = EditorBuffer::from_text("alpha beta");
        document.selection =
            EditorSelection::new(EditorPosition::new(0, 10), EditorPosition::new(0, 10));
        document.refresh_after_text_change();
        document.mark_clean();
    }

    app.search_dialog.set_query("alpha");
    let _ = app.update(Message::AdvancedSearchWrapAroundToggled(false));
    let _ = app.update(Message::AdvancedFindNextRun);

    let document = app.workspace.active_document().expect("active document");
    assert_eq!(
        document.selection,
        EditorSelection::new(EditorPosition::new(0, 10), EditorPosition::new(0, 10))
    );
}

#[test]
fn select_and_find_next_sets_query_from_selection_and_selects_next_match() {
    let (mut app, _) = App::new();

    set_active_document_text(
        &mut app,
        "alpha beta alpha",
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 5)),
    );

    let _ = app.update(Message::SelectAndFindNext);

    let document = app.workspace.active_document().expect("active document");
    assert_eq!(app.find.query, "alpha");
    assert_eq!(
        document.selection,
        EditorSelection::new(EditorPosition::new(0, 11), EditorPosition::new(0, 16))
    );
}

#[test]
fn volatile_find_next_uses_selection_without_replacing_existing_query() {
    let (mut app, _) = App::new();
    app.find.set_query("beta");

    set_active_document_text(
        &mut app,
        "alpha beta alpha",
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 5)),
    );

    let _ = app.update(Message::VolatileFindNext);

    let document = app.workspace.active_document().expect("active document");
    assert_eq!(app.find.query, "beta");
    assert_eq!(
        document.selection,
        EditorSelection::new(EditorPosition::new(0, 11), EditorPosition::new(0, 16))
    );
}

#[test]
fn advanced_find_next_wraps_when_enabled() {
    let (mut app, _) = App::new();

    {
        let document = app
            .workspace
            .active_document_mut()
            .expect("active document");
        document.buffer = EditorBuffer::from_text("alpha beta");
        document.selection =
            EditorSelection::new(EditorPosition::new(0, 10), EditorPosition::new(0, 10));
        document.refresh_after_text_change();
        document.mark_clean();
    }

    app.search_dialog.set_query("alpha");
    let _ = app.update(Message::AdvancedFindNextRun);

    let document = app.workspace.active_document().expect("active document");
    assert_eq!(
        document.selection,
        EditorSelection::new(EditorPosition::new(0, 0), EditorPosition::new(0, 5))
    );
}
