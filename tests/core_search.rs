use fragile_notepad::core::search::{
    FindState, PreparedSearch, SearchMode, SearchOptions, TextMatch, compute_matches,
    compute_matches_in_chunks, replace_all, replace_current,
};

#[test]
fn visiting_matches_preserves_offsets_and_filters_without_collecting() {
    for (mode, query) in [(SearchMode::Normal, "café"), (SearchMode::Regex, "café|a+")] {
        let search = PreparedSearch::new(
            query,
            SearchOptions {
                mode,
                case_sensitive: false,
                whole_word: true,
            },
        )
        .unwrap()
        .unwrap();
        let text = "CAFÉ caféx café aa aab";
        let mut visited = Vec::new();
        search.for_each_match_in_chunks(["CAFÉ ca", "féx café a", "a aab"], |found| {
            visited.push(found)
        });
        assert_eq!(visited, search.matches(text));
        let mut count = 0;
        search.for_each_match_in_chunks([text], |_| count += 1);
        assert_eq!(count, visited.len());
    }
}

#[test]
fn literal_search_is_independent_of_every_chunk_split() {
    for text in [
        "aaaaaa",
        "aab aa a",
        "abcab abc",
        "café CAFÉ caféx",
        "ΣΟΣ σος",
        "a\r\nb",
    ] {
        let boundaries: Vec<_> = text
            .char_indices()
            .map(|(i, _)| i)
            .chain([text.len()])
            .collect();
        for query in ["a", "aa", "abc", "café", "ΣΟΣ", "\r\n"] {
            for case_sensitive in [false, true] {
                for whole_word in [false, true] {
                    let search = PreparedSearch::new(
                        query,
                        SearchOptions::normal(case_sensitive, whole_word),
                    )
                    .unwrap()
                    .unwrap();
                    let expected = search.matches(text);
                    for &a in &boundaries {
                        for &b in boundaries.iter().filter(|&&b| b >= a) {
                            assert_eq!(
                                search.matches_in_chunks([&text[..a], "", &text[a..b], &text[b..]]),
                                expected,
                                "text={text:?} query={query:?} splits={a},{b} sensitive={case_sensitive} whole={whole_word}"
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(
        compute_matches_in_chunks(["a", "b"], "a", SearchOptions::normal(true, true)).is_empty()
    );
    assert_eq!(
        compute_matches_in_chunks(["aa", "aa"], "aa", SearchOptions::normal(true, false)),
        vec![TextMatch::new(0, 2), TextMatch::new(2, 4)]
    );
}

#[test]
fn regex_replacement_preserves_match_context() {
    for (query, text, replacement, expected) in [
        (r"\B(foo)", "xfoo", "$1", "foo"),
        (r"(foo)\B", "foox", "<$1>", "<foo>"),
        (r"(?m)^(foo)$", "before\nfoo\nafter", "$1!", "foo!"),
    ] {
        let search = PreparedSearch::new(
            query,
            SearchOptions {
                mode: SearchMode::Regex,
                case_sensitive: true,
                whole_word: false,
            },
        )
        .unwrap()
        .unwrap();
        let found = search.matches(text)[0];
        assert_eq!(
            search.replacement_for_match(text, found, replacement),
            expected
        );
    }
}

#[test]
fn compute_matches_respects_case_sensitivity() {
    assert_eq!(
        compute_matches("Note note NOTE", "note", true),
        vec![TextMatch::new(5, 9)]
    );

    assert_eq!(
        compute_matches("Note note NOTE", "note", false),
        vec![
            TextMatch::new(0, 4),
            TextMatch::new(5, 9),
            TextMatch::new(10, 14),
        ]
    );
}

#[test]
fn compute_matches_returns_no_matches_for_empty_input_or_query() {
    assert!(compute_matches("", "note", false).is_empty());
    assert!(compute_matches("note", "", false).is_empty());
}

#[test]
fn replace_current_rejects_invalid_ranges() {
    assert_eq!(replace_current("abc", TextMatch::new(2, 1), "x"), None);
    assert_eq!(replace_current("abc", TextMatch::new(0, 4), "x"), None);
    assert_eq!(replace_current("éx", TextMatch::new(1, 2), "x"), None);
}

#[test]
fn find_state_refreshes_and_navigates_matches_with_wrapping() {
    let mut find = FindState::with_query("one");

    find.refresh_matches("one two one");

    assert_eq!(find.current(), Some(TextMatch::new(0, 3)));
    assert_eq!(find.next(), Some(TextMatch::new(8, 11)));
    assert_eq!(find.next(), Some(TextMatch::new(0, 3)));
    assert_eq!(find.previous(), Some(TextMatch::new(8, 11)));
}

#[test]
fn find_state_clears_navigation_when_query_has_no_matches() {
    let mut find = FindState::with_query("missing");

    find.refresh_matches("one two one");

    assert!(find.matches.is_empty());
    assert_eq!(find.current(), None);
    assert_eq!(find.next(), None);
    assert_eq!(find.previous(), None);
}

#[test]
fn find_state_replace_current_replaces_selected_match_and_refreshes() {
    let mut find = FindState::with_query("cat");
    find.set_replacement("dog");
    find.refresh_matches("cat cat");

    let replaced = find.replace_current("cat cat");

    assert_eq!(replaced.as_deref(), Some("dog cat"));
    assert_eq!(find.matches, vec![TextMatch::new(4, 7)]);
    assert_eq!(find.current(), Some(TextMatch::new(4, 7)));
}

#[test]
fn find_state_replace_all_replaces_all_matches_and_reports_count() {
    let mut find = FindState::with_query("cat");
    find.set_replacement("dog");
    find.refresh_matches("cat Cat scatter cat");

    let (replaced, count) = find.replace_all("cat Cat scatter cat");

    assert_eq!(replaced, "dog dog sdogter dog");
    assert_eq!(count, 4);
    assert!(find.matches.is_empty());
    assert_eq!(find.current(), None);
}

#[test]
fn replace_all_helper_uses_case_sensitive_matching_when_requested() {
    let (replaced, count) = replace_all("cat Cat cat", "cat", "dog", true);

    assert_eq!(replaced, "dog Cat dog");
    assert_eq!(count, 2);
}

#[test]
fn compute_matches_reports_utf8_byte_offsets_for_multiline_text() {
    let text = "one\n茅cho\none";

    assert_eq!(
        compute_matches(text, "one", true),
        vec![TextMatch::new(0, 3), TextMatch::new(11, 14)]
    );
    assert_eq!(
        compute_matches(text, "茅c", true),
        vec![TextMatch::new(4, 8)]
    );
}

#[test]
fn compute_matches_in_chunks_finds_literal_matches_across_boundaries() {
    let chunks = ["al", "pha beta al", "pha"];

    assert_eq!(
        compute_matches_in_chunks(chunks, "alpha", SearchOptions::normal(true, false),),
        vec![TextMatch::new(0, 5), TextMatch::new(11, 16)]
    );
}

#[test]
fn compute_matches_in_chunks_handles_unicode_and_crlf_boundaries() {
    let chunks = ["caf", "\u{00e9}\r", "\nnext caf", "\u{00e9}"];

    assert_eq!(
        compute_matches_in_chunks(chunks, "caf\u{00e9}", SearchOptions::normal(true, false),),
        vec![
            TextMatch::new(0, "caf\u{00e9}".len()),
            TextMatch::new(12, 17)
        ]
    );
    assert_eq!(
        compute_matches_in_chunks(chunks, "\r\nnext", SearchOptions::normal(true, false),),
        vec![TextMatch::new(
            "caf\u{00e9}".len(),
            "caf\u{00e9}\r\nnext".len()
        )]
    );
}

#[test]
fn compute_matches_in_chunks_reports_only_currently_available_partial_results() {
    let partial_chunks = ["alpha beta alp"];
    let complete_chunks = ["alpha beta alp", "ha"];

    assert_eq!(
        compute_matches_in_chunks(partial_chunks, "alpha", SearchOptions::normal(true, false),),
        vec![TextMatch::new(0, 5)]
    );
    assert_eq!(
        compute_matches_in_chunks(complete_chunks, "alpha", SearchOptions::normal(true, false),),
        vec![TextMatch::new(0, 5), TextMatch::new(11, 16)]
    );
}

#[test]
fn prepared_regex_search_still_materializes_chunked_text_for_compatibility() {
    let search = PreparedSearch::new(
        r"caf.",
        SearchOptions {
            case_sensitive: true,
            whole_word: false,
            mode: SearchMode::Regex,
        },
    )
    .unwrap()
    .unwrap();

    assert_eq!(
        search.matches_in_chunks(["ca", "f\u{00e9} cafe"]),
        vec![
            TextMatch::new(0, "caf\u{00e9}".len()),
            TextMatch::new(6, 10)
        ]
    );
}

#[test]
fn find_state_refresh_matches_in_chunks_preserves_navigation() {
    let mut find = FindState::with_query("two");

    find.refresh_matches_in_chunks(["one t", "wo two"]);

    assert_eq!(find.current(), Some(TextMatch::new(4, 7)));
    assert_eq!(find.next(), Some(TextMatch::new(8, 11)));
}

#[test]
fn replace_current_handles_multibyte_boundaries_and_preserves_surrounding_text() {
    let text = "alpha 茅cho omega";
    let start = "alpha ".len();
    let end = start + "茅cho".len();

    assert_eq!(
        replace_current(text, TextMatch::new(start, end), "echo").as_deref(),
        Some("alpha echo omega")
    );
}

#[test]
fn find_state_replace_current_uses_current_match_after_navigation() {
    let mut find = FindState::with_query("one");
    find.set_replacement("two");
    find.refresh_matches("one one one");

    assert_eq!(find.next(), Some(TextMatch::new(4, 7)));

    let replaced = find.replace_current("one one one");

    assert_eq!(replaced.as_deref(), Some("one two one"));
    assert_eq!(
        find.matches,
        vec![TextMatch::new(0, 3), TextMatch::new(8, 11)]
    );
    assert_eq!(find.current(), Some(TextMatch::new(8, 11)));
}

#[test]
fn prepared_regex_search_expands_capture_replacements() {
    let search = PreparedSearch::new(
        r"(\w+)-(\d+)",
        SearchOptions {
            case_sensitive: true,
            whole_word: false,
            mode: SearchMode::Regex,
        },
    )
    .unwrap()
    .unwrap();
    let text_match = search.matches("task-42").remove(0);

    assert_eq!(
        search.replacement_for_match("task-42", text_match, "$2:$1"),
        "42:task"
    );
}

#[test]
fn prepared_extended_search_expands_query_and_replacement_escapes() {
    let search = PreparedSearch::new(
        r"one\ntwo",
        SearchOptions {
            case_sensitive: true,
            whole_word: false,
            mode: SearchMode::Extended,
        },
    )
    .unwrap()
    .unwrap();
    let text = "zero\none\ntwo\nthree";
    let text_match = search.matches(text).remove(0);

    assert_eq!(
        text_match,
        TextMatch::new("zero\n".len(), "zero\none\ntwo".len())
    );
    assert_eq!(
        search.replacement_for_match(text, text_match, r"alpha\tbeta"),
        "alpha\tbeta"
    );
}

#[test]
fn zero_width_regex_matches_include_unicode_boundaries_and_empty_documents() {
    let options = SearchOptions {
        mode: SearchMode::Regex,
        case_sensitive: true,
        whole_word: false,
    };
    let search = PreparedSearch::new(r"(?m)^|$", options).unwrap().unwrap();
    assert_eq!(
        search.matches("é\nβ"),
        vec![
            TextMatch::new(0, 0),
            TextMatch::new(2, 2),
            TextMatch::new(3, 3),
            TextMatch::new(5, 5),
        ]
    );
    assert_eq!(
        search.matches_in_chunks(["é", "\n", "β"]),
        search.matches("é\nβ")
    );
    assert_eq!(search.matches(""), vec![TextMatch::new(0, 0)]);
}

#[test]
fn find_state_regex_replacements_expand_captures_and_insert_at_anchors() {
    let mut find = FindState::with_query(r"(?m)^(\w+)");
    find.set_mode(SearchMode::Regex);
    find.set_replacement("<$1>");
    let (replaced, count) = find.replace_all("café\nβeta");
    assert_eq!((replaced.as_str(), count), ("<café>\n<βeta>", 2));

    find.set_query(r"(?m)^");
    find.set_replacement("> ");
    let (replaced, count) = find.replace_all("café\nβeta");
    assert_eq!((replaced.as_str(), count), ("> café\n> βeta", 2));
}

#[test]
fn find_state_reports_regex_errors_and_recovers_after_query_edits() {
    let mut find = FindState::with_query("[");
    find.set_mode(SearchMode::Regex);
    find.refresh_matches("alpha");
    assert!(find.error.is_some());
    assert!(find.matches.is_empty());
    find.set_query("a");
    find.refresh_matches("alpha");
    assert!(find.error.is_none());
    assert_eq!(find.matches.len(), 2);
}

#[test]
fn find_state_navigation_can_stop_at_document_boundaries() {
    let mut find = FindState::with_query("a");
    find.wrap_around = false;
    find.refresh_matches("a a");
    assert_eq!(find.previous(), None);
    assert_eq!(find.next(), Some(TextMatch::new(2, 3)));
    assert_eq!(find.next(), None);
    assert_eq!(find.current(), Some(TextMatch::new(2, 3)));
}

#[test]
fn multiline_regex_treats_crlf_as_one_editor_line_boundary() {
    let options = SearchOptions {
        mode: SearchMode::Regex,
        case_sensitive: true,
        whole_word: false,
    };
    let line = PreparedSearch::new(r"(?m)^café$", options)
        .unwrap()
        .unwrap();
    assert_eq!(line.matches("café\r\nnext"), vec![TextMatch::new(0, 5)]);
    let ends = PreparedSearch::new(r"(?m)$", options).unwrap().unwrap();
    assert_eq!(
        ends.matches("café\r\nnext"),
        vec![TextMatch::new(5, 5), TextMatch::new(11, 11)]
    );
}

#[test]
fn match_visitors_can_stop_after_unicode_matches_across_chunks() {
    for mode in [SearchMode::Normal, SearchMode::Regex] {
        let search = PreparedSearch::new(
            "é",
            SearchOptions {
                mode,
                case_sensitive: true,
                whole_word: false,
            },
        )
        .unwrap()
        .unwrap();
        let mut visited = Vec::new();
        let stopped = search.try_for_each_match_in_chunks(["x", "éé ", "é tail"], |found| {
            visited.push(found);
            std::ops::ControlFlow::Break("enough")
        });
        assert_eq!(stopped, std::ops::ControlFlow::Break("enough"));
        assert_eq!(visited, vec![TextMatch::new(1, 3)]);
    }

    let consumed = std::cell::Cell::new(0);
    let chunks = ["xé ", "unvisited tail"]
        .into_iter()
        .inspect(|_| consumed.set(consumed.get() + 1));
    let search = PreparedSearch::new("é", SearchOptions::normal(true, false))
        .unwrap()
        .unwrap();
    let _ = search.try_for_each_match_in_chunks(chunks, |_| std::ops::ControlFlow::Break(()));
    assert_eq!(consumed.get(), 1);
}

#[test]
fn limited_find_state_reports_only_confirmed_overflow_and_replaces_all_matches() {
    let mut find = FindState::with_query("a");
    find.match_limit = Some(500);
    find.refresh_matches(&"a".repeat(500));
    assert_eq!(find.matches.len(), 500);
    assert!(!find.matches_limited);
    find.refresh_matches(&"a".repeat(501));
    assert_eq!(find.matches.len(), 500);
    assert!(find.matches_limited);
    find.set_replacement("b");
    let (replaced, count) = find.replace_all(&"a".repeat(501));
    assert_eq!(replaced, "b".repeat(501));
    assert_eq!(count, 501);
}
