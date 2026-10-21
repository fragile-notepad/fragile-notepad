use super::*;
use crate::core::{SessionDocument, SessionWindow, TextEncoding};

#[test]
fn partitioned_text_exceeds_metadata_budget_without_inline_json_expansion() {
    let directory = test_dir("partitioned");
    let path = directory.join("session.json");
    let mut expected = session(&"\"\n\u{4e16}\u{754c}".repeat(700));
    expected
        .documents
        .extend(session("another document").documents);
    let prepared = prepare(expected.clone(), 1024).unwrap();
    assert!(
        prepared
            .manifest
            .session
            .documents
            .iter()
            .all(|entry| entry.text.is_none())
    );
    assert!(prepared.manifest.recovery[0].as_ref().unwrap().len() > 1);
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        publish(&path, &prepared, 1024).await.unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(decode(&bytes, &chunks_path(&path), 1024).unwrap(), expected);
        assert!(decode_snapshot(&bytes, &chunks_path(&path), 1024, false).is_ok());
        assert!(bytes.len() < 4096);
        assert!(encode_limited(&expected, 4096).is_err());
    });
    remove_empty_test_directory(directory);
}

#[test]
fn corrupt_session_falls_back_without_deleting_files() {
    let directory = test_dir("fallback");
    let path = directory.join("session.json");
    let first = session("first recoverable text");
    let second = session("second recoverable text");
    let writer = SessionWriter::default();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        writer.enqueue(first.clone()).unwrap();
        writer.flush(&path).await.unwrap();
        writer.enqueue(second).unwrap();
        writer.flush(&path).await.unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let current: Manifest = serde_json::from_slice(&bytes).unwrap();
        let damaged = chunks_path(&path).join(&current.recovery[0].as_ref().unwrap()[0]);
        std::fs::write(damaged, b"corrupt").unwrap();
        let loaded = load_with_fallback(&path).await.unwrap();
        assert_eq!(loaded.session, Some(first.clone()));
        assert!(
            loaded
                .warning
                .unwrap()
                .contains("Recent edits may be missing")
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);

        writer.enqueue(session("after recovery")).unwrap();
        writer.flush(&path).await.unwrap();
        assert_eq!(
            std::fs::read(path.with_extension("unreadable.json")).unwrap(),
            bytes
        );
        assert_eq!(load_from(&previous_path(&path)).await.unwrap(), Some(first));
    });
    remove_empty_test_directory(directory);
}

#[test]
fn failed_manifest_publication_preserves_last_complete_recovery() {
    let directory = test_dir("publication");
    let path = directory.join("session.json");
    let writer = SessionWriter::default();
    let expected = session("known good");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        writer.enqueue(expected.clone()).unwrap();
        writer.flush(&path).await.unwrap();
        // The previous-manifest destination is unavailable: blobs can be
        // written, but the last complete manifest must stay untouched.
        std::fs::create_dir(previous_path(&path)).unwrap();
        writer.enqueue(session("new unsaved edits")).unwrap();
        assert!(writer.flush(&path).await.is_err());
        assert_eq!(load_from(&path).await.unwrap(), Some(expected));
        std::fs::remove_dir(previous_path(&path)).unwrap();
        writer.flush(&path).await.unwrap();
        assert_eq!(
            load_from(&path).await.unwrap(),
            Some(session("new unsaved edits"))
        );
    });
    remove_empty_test_directory(directory);
}

#[test]
fn invalid_names_oversized_chunks_and_metadata_are_rejected() {
    let directory = test_dir("bounds");
    let prepared = prepare(session("enough text for several chunks"), 4).unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    let mut manifest = prepared.manifest;
    let hash = manifest.recovery[0].as_ref().unwrap()[0].clone();
    manifest.recovery[0].as_mut().unwrap()[0] = "../outside".into();
    let bytes = serde_json::to_vec(&manifest).unwrap();
    assert!(
        decode(&bytes, &directory, 4)
            .unwrap_err()
            .contains("chunk name")
    );
    manifest.recovery[0].as_mut().unwrap()[0] = hash.clone();
    std::fs::write(directory.join(hash), b"oversized").unwrap();
    let bytes = serde_json::to_vec(&manifest).unwrap();
    assert!(
        decode(&bytes, &directory, 4)
            .unwrap_err()
            .contains("integrity")
    );
    assert!(
        encode_limited(&manifest, 16)
            .unwrap_err()
            .contains("capacity")
    );
    remove_empty_test_directory(directory);
}

#[test]
fn captured_rope_is_immutable_after_later_editor_edits() {
    let mut buffer = crate::editor::EditorBuffer::from_text("before");
    let snapshot = SessionSnapshot {
        layout: Session::default(),
        documents: vec![CapturedDocument::Live(
            Box::default(),
            Some(buffer.snapshot_rope()),
        )],
    };
    buffer.append_text(" after");
    assert_eq!(buffer.text(), "before after");
    assert_eq!(
        snapshot.materialize().documents[0].text.as_deref(),
        Some("before")
    );
}

#[test]
fn garbage_collection_keeps_both_committed_generations() {
    let directory = test_dir("generations");
    let path = directory.join("session.json");
    let writer = SessionWriter::default();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for text in ["first", "second", "third"] {
            writer.enqueue(session(text)).unwrap();
            writer.flush(&path).await.unwrap();
        }
        assert_eq!(load_from(&path).await.unwrap(), Some(session("third")));
        assert_eq!(
            load_from(&previous_path(&path)).await.unwrap(),
            Some(session("second"))
        );
        let root = chunks_path(&path);
        assert!(
            !root
                .join(blake3::hash(b"first").to_hex().to_string())
                .exists()
        );
        assert!(
            root.join(blake3::hash(b"second").to_hex().to_string())
                .exists()
        );
        assert!(
            root.join(blake3::hash(b"third").to_hex().to_string())
                .exists()
        );
        std::fs::remove_file(&path).unwrap();
        let fallback = load_with_fallback(&path).await.unwrap();
        assert_eq!(fallback.session, Some(session("second")));
        assert!(fallback.warning.is_some());
    });
    remove_empty_test_directory(directory);
}

#[test]
fn unchanged_chunks_are_reused_and_damaged_chunks_are_repaired() {
    let directory = test_dir("reuse");
    let path = directory.join("session.json");
    let writer = SessionWriter::default();
    let expected = session("same recovery bytes");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        writer.enqueue(expected.clone()).unwrap();
        writer.flush(&path).await.unwrap();
        let chunk =
            chunks_path(&path).join(blake3::hash(b"same recovery bytes").to_hex().to_string());
        let old_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(946_684_800);
        std::fs::File::options()
            .write(true)
            .open(&chunk)
            .unwrap()
            .set_modified(old_time)
            .unwrap();
        let modified = std::fs::metadata(&chunk).unwrap().modified().unwrap();
        let mut changed_view = expected.clone();
        changed_view.documents[0].cursor_line = 1;
        writer.enqueue(changed_view.clone()).unwrap();
        writer.flush(&path).await.unwrap();
        assert_eq!(
            std::fs::metadata(&chunk).unwrap().modified().unwrap(),
            modified
        );

        std::fs::remove_file(&chunk).unwrap();
        let missing = load_with_fallback(&path).await;
        assert!(missing.is_err()); // Both snapshots reference the missing chunk.
        std::fs::write(&chunk, b"damaged shared chunk").unwrap();
        writer.enqueue(changed_view.clone()).unwrap();
        writer.flush(&path).await.unwrap();
        assert_eq!(std::fs::read(&chunk).unwrap(), b"same recovery bytes");
        assert_eq!(load_from(&path).await.unwrap(), Some(changed_view));
        assert_eq!(
            load_from(&previous_path(&path)).await.unwrap(),
            Some(session_with_cursor(&expected, 1))
        );
    });
    remove_empty_test_directory(directory);
}

fn session_with_cursor(session: &Session, line: usize) -> Session {
    let mut session = session.clone();
    session.documents[0].cursor_line = line;
    session
}

#[cfg(unix)]
#[test]
fn recovery_manifests_and_chunks_have_private_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let directory = test_dir("privacy");
    let path = directory.join("session.json");
    let writer = SessionWriter::default();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        writer.enqueue(session("private first")).unwrap();
        writer.flush(&path).await.unwrap();
        writer.enqueue(session("private second")).unwrap();
        writer.flush(&path).await.unwrap();
    });
    assert_eq!(
        std::fs::metadata(chunks_path(&path))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for file in [path.clone(), previous_path(&path)] {
        assert_eq!(
            std::fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    for entry in std::fs::read_dir(chunks_path(&path)).unwrap() {
        assert_eq!(
            entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    remove_empty_test_directory(directory);
}

fn remove_empty_test_directory(directory: PathBuf) {
    assert!(directory.starts_with(std::env::temp_dir()));
    assert!(
        directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("fragile-session-")
    );
    std::fs::remove_dir_all(directory).unwrap();
}

fn test_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "fragile-session-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
fn session(text: &str) -> Session {
    Session {
        documents: vec![SessionDocument {
            path: Some(PathBuf::from("\u{6587}\u{4ef6}.txt")),
            text: Some(text.into()),
            is_dirty: true,
            is_pinned: true,
            encoding: TextEncoding::Utf16LeBom,
            line_ending: Some("\r\n".into()),
            cursor_column: 2,
            horizontal_offset: 3.5,
            syntax_token: Some("rs".into()),
            collapsed_folds: vec![(1, 3)],
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn roundtrip_and_reverse_polling_keep_latest_session() {
    let directory = test_dir("ordered");
    let path = directory.join("session.json");
    let writer = SessionWriter::default();
    writer.enqueue(session("old")).unwrap();
    let old = writer.flush(&path);
    let latest = session("hello \u{4e16}\u{754c}\r\n");
    writer.enqueue(latest.clone()).unwrap();
    let new = writer.flush(&path);
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (a, b) = futures::join!(new, old);
        a.unwrap();
        b.unwrap();
        assert_eq!(load_from(&path).await.unwrap(), Some(latest));
    });
    std::fs::remove_file(path).unwrap();
    remove_empty_test_directory(directory);
}

#[test]
fn malformed_and_unknown_versions_are_rejected_without_changing_file() {
    let directory = test_dir("invalid");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("session.json");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for bytes in [
            b"not json".as_slice(),
            br#"{"version":3,"documents":[],"active_index":0}"#,
            br#"{"version":2,"documents":[],"active_index":0,"windows":[{}]}"#,
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(load_from(&path).await.is_err(), "{bytes:?}");
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    });
    std::fs::remove_file(path).unwrap();
    remove_empty_test_directory(directory);
}

#[test]
fn legacy_and_multiwindow_sessions_load_without_changing_file() {
    let directory = test_dir("versions");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("session.json");
    let mut multiwindow = session("first window");
    multiwindow.version = 2;
    multiwindow
        .documents
        .extend(session("second window").documents);
    multiwindow.windows = vec![
        SessionWindow {
            document_indices: vec![0],
            ..Default::default()
        },
        SessionWindow {
            document_indices: vec![1],
            ..Default::default()
        },
    ];
    multiwindow.focused_window = Some(1);
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for expected in [
            Session::default(),
            session("legacy recovery"),
            Session {
                version: 2,
                ..Default::default()
            },
            multiwindow,
        ] {
            let bytes = encode(&expected).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            assert_eq!(load_from(&path).await.unwrap(), Some(expected));
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    });
    std::fs::remove_file(path).unwrap();
    remove_empty_test_directory(directory);
}

#[test]
fn native_paths_and_invalid_metadata() {
    let mut value = session("recovery");
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        value.documents[0].path = Some(PathBuf::from(std::ffi::OsString::from_wide(&[
            0x61, 0xd800, 0x62,
        ])));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        value.documents[0].path = Some(PathBuf::from(std::ffi::OsString::from_vec(vec![
            b'a', 0xff, b'b',
        ])));
    }
    assert_eq!(
        serde_json::from_slice::<Session>(&encode(&value).unwrap()).unwrap(),
        value
    );
    value.documents[0].text = None;
    assert!(value.validate().is_err());
    value.documents[0].text = Some(String::new());
    value.documents[0].horizontal_offset = f32::NAN;
    assert!(value.validate().is_err());
}

#[test]
fn failed_write_retains_snapshot_for_retry() {
    let directory = test_dir("retry");
    std::fs::write(&directory, b"block directory creation").unwrap();
    let path = directory.join("session.json");
    let writer = SessionWriter::default();
    let expected = session("unsaved text");
    writer.enqueue(expected.clone()).unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        assert!(writer.flush(&path).await.is_err());
        std::fs::remove_file(&directory).unwrap();
        writer.flush(&path).await.unwrap();
        assert_eq!(load_from(&path).await.unwrap(), Some(expected));
    });
    std::fs::remove_file(path).unwrap();
    remove_empty_test_directory(directory);
}

#[test]
fn language_provenance_roundtrips_and_legacy_sessions_remain_readable() {
    for automatic in [None, Some(true), Some(false)] {
        let mut saved = session("fn main() {}");
        saved.documents[0].syntax_automatic = automatic;
        let bytes = encode(&saved).unwrap();
        let restored: Session = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored, saved);
    }
    let mut legacy = serde_json::to_value(session("fn main() {}")).unwrap();
    legacy["documents"][0]
        .as_object_mut()
        .unwrap()
        .remove("syntax_automatic");
    let restored: Session = serde_json::from_value(legacy).unwrap();
    assert_eq!(restored.documents[0].syntax_automatic, None);
}

#[test]
fn a_new_snapshot_during_a_write_is_flushed_before_completion() {
    let directory = test_dir("inflight");
    let path = directory.join("session.json");
    let writer = SessionWriter::default();
    writer.enqueue(session("old")).unwrap();
    let expected = session("new");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut in_flight = Box::pin(writer.flush(&path));
        assert!(futures::poll!(&mut in_flight).is_pending());
        assert!(writer.writing.try_lock().is_err());
        writer.enqueue(expected.clone()).unwrap();
        in_flight.await.unwrap();
        assert_eq!(load_from(&path).await.unwrap(), Some(expected));
    });
    std::fs::remove_file(path).unwrap();
    remove_empty_test_directory(directory);
}
