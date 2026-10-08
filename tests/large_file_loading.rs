use fragile_notepad::core::{Document, DocumentId, DocumentLoadGeneration};
use fragile_notepad::services::chunked_file::{DEFAULT_CHUNK_SIZE, load_file_chunks};
use fragile_notepad::services::types::{FileLoadEvent, FileLoadRequest};
use fragile_notepad::ui::status_bar::document_status_label;
use futures::{StreamExt, pin_mut};
use iced::widget::text_editor::LineEnding;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use std::time::Instant;

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(1);

fn temp_file_path(name: &str) -> PathBuf {
    let id = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "fragile-notepad-large-file-loading-{name}-{id}.txt"
    ))
}

fn collect_load_events(request: FileLoadRequest) -> Vec<FileLoadEvent> {
    futures::executor::block_on(async {
        let stream = load_file_chunks(request);
        pin_mut!(stream);
        stream.collect::<Vec<_>>().await
    })
}

fn preview_from_chunks(events: &[FileLoadEvent]) -> String {
    let mut preview = String::new();
    for event in events {
        let FileLoadEvent::Chunk(chunk) = event else {
            continue;
        };
        if chunk.reset {
            preview.clear();
        }
        preview.push_str(&chunk.text);
    }
    preview
}

#[test]
fn streamed_revisions_cover_original_bytes_across_encodings_and_chunk_boundaries() {
    for bytes in [
        b"".as_slice(),
        b"plain UTF-8 text\r\n",
        b"\xef\xbb\xbftext",
        b"valid prefix\xffinvalid utf8",
        b"\xff\xfet\0e\0x\0t\0",
        b"\xfe\xff\0t\0e\0x\0t",
    ] {
        let path = temp_file_path("disk-revision");
        fs::write(&path, bytes).unwrap();
        for chunk_size in [1, 3, DEFAULT_CHUNK_SIZE] {
            let events = collect_load_events(FileLoadRequest {
                document_id: DocumentId::new(51),
                generation: DocumentLoadGeneration::next(),
                path: path.clone(),
                chunk_size,
            });
            let finished = events
                .iter()
                .find_map(|event| match event {
                    FileLoadEvent::Finished(Ok(finished)) => Some(finished),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                finished.disk_revision,
                fragile_notepad::core::FileRevision::from_bytes(bytes)
            );
        }
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn malformed_utf8_bom_preserves_text_and_reports_decoding_errors() {
    for (name, bytes, expected) in [
        (
            "invalid-middle",
            b"\xef\xbb\xbfa\xffb".as_slice(),
            "a\u{fffd}b",
        ),
        (
            "truncated-end",
            b"\xef\xbb\xbfa\xc3".as_slice(),
            "a\u{fffd}",
        ),
    ] {
        let path = temp_file_path(name);
        fs::write(&path, bytes).expect("write temp input");
        for chunk_size in [1, 3, 64] {
            let events = collect_load_events(FileLoadRequest {
                document_id: DocumentId::new(41),
                generation: DocumentLoadGeneration::next(),
                path: path.clone(),
                chunk_size,
            });
            assert_eq!(preview_from_chunks(&events), expected);
            let finished = events
                .iter()
                .find_map(|event| match event {
                    FileLoadEvent::Finished(Ok(finished)) => Some(finished),
                    _ => None,
                })
                .expect("successful load with decoding warning");
            assert_eq!(
                finished.encoding,
                fragile_notepad::core::TextEncoding::Utf8Bom
            );
            assert!(finished.had_errors);
        }
        fs::remove_file(path).expect("remove temp input");
    }
}

#[test]
fn chunked_loader_streams_progress_chunks_and_final_decoded_text() {
    let path = temp_file_path("utf8-crlf-bom");
    let mut bytes = Vec::from(&b"\xef\xbb\xbfalpha\r\ncaf"[..]);
    bytes.extend_from_slice("é\r\nomega".as_bytes());
    fs::write(&path, &bytes).expect("write temp input");

    let document_id = DocumentId::new(41);
    let generation = DocumentLoadGeneration::next();
    let events = collect_load_events(FileLoadRequest {
        document_id,
        generation,
        path: path.clone(),
        chunk_size: 1,
    });

    let _ = fs::remove_file(&path);

    assert!(
        matches!(events.first(), Some(FileLoadEvent::Progress(progress))
        if progress.document_id == document_id
            && progress.generation == generation
            && progress.bytes_read == 0
            && progress.total_bytes == Some(bytes.len() as u64))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, FileLoadEvent::Chunk(chunk)
            if chunk.document_id == document_id
                && chunk.generation == generation
                && chunk.bytes_read > 0
                && chunk.total_bytes == Some(bytes.len() as u64)))
    );

    let finished = events
        .iter()
        .find_map(|event| match event {
            FileLoadEvent::Finished(Ok(finished)) => Some(finished),
            _ => None,
        })
        .expect("finished event");

    assert_eq!(finished.document_id, document_id);
    assert_eq!(finished.generation, generation);
    assert_eq!(finished.bytes_read, bytes.len() as u64);
    assert_eq!(finished.total_bytes, Some(bytes.len() as u64));
    assert_eq!(
        finished.encoding,
        fragile_notepad::core::TextEncoding::Utf8Bom
    );
    let streamed = events
        .iter()
        .filter_map(|event| match event {
            FileLoadEvent::Chunk(chunk) => Some(chunk.text.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(streamed.as_bytes(), &bytes[3..]);
}

#[test]
fn loading_document_applies_matching_progress_and_completion_only() {
    let document_id = DocumentId::new(42);
    let generation = DocumentLoadGeneration::next();
    let stale_generation = DocumentLoadGeneration::next();
    let mut document = Document::loading(document_id, "large.txt", generation);

    assert_eq!(document_status_label(&document, Some("Saved")), "indexing");
    assert!(document.update_load_progress(generation, 5, Some(20)));
    assert!(!document.replace_loading_preview(stale_generation, "stale", false, 5, Some(5)));
    assert_eq!(document.text(), "");
    assert!(document.replace_loading_preview(generation, "alpha\r", false, 6, Some(12)));
    assert!(document.replace_loading_preview(generation, "\nbeta", false, 11, Some(12)));
    assert_eq!(document.text(), "alpha\r\nbeta");
    assert_eq!(document.buffer.line(0).as_deref(), Some("alpha"));
    assert_eq!(document.buffer.line(1).as_deref(), Some("beta"));
    assert!(!document.complete_loading(
        stale_generation,
        fragile_notepad::core::decode_bytes(b"stale")
    ));
    assert_eq!(document.text(), "alpha\r\nbeta");
    assert!(document.is_loading_or_indexing());

    assert!(document.complete_loading(
        generation,
        fragile_notepad::core::decode_bytes(b"alpha\r\nbeta\n")
    ));

    assert_eq!(document.text(), "alpha\r\nbeta\n");
    assert_eq!(document.line_ending, Some(LineEnding::CrLf));
    assert!(!document.is_loading_or_indexing());
    assert_eq!(document_status_label(&document, Some("Saved")), "Saved");
}

#[test]
fn chunked_loader_finished_result_can_replace_lossy_preview() {
    let path = temp_file_path("split-multibyte");
    fs::write(&path, "AéB").expect("write temp input");

    let document_id = DocumentId::new(43);
    let generation = DocumentLoadGeneration::next();
    let mut document = Document::loading(document_id, path.clone(), generation);
    let events = collect_load_events(FileLoadRequest {
        document_id,
        generation,
        path: path.clone(),
        chunk_size: 1,
    });

    let _ = fs::remove_file(&path);

    for event in events {
        match event {
            FileLoadEvent::Progress(progress) => {
                document.update_load_progress(
                    progress.generation,
                    progress.bytes_read,
                    progress.total_bytes,
                );
            }
            FileLoadEvent::Chunk(chunk) => {
                document.replace_loading_preview(
                    chunk.generation,
                    &chunk.text,
                    chunk.reset,
                    chunk.bytes_read,
                    chunk.total_bytes,
                );
            }
            FileLoadEvent::Finished(Ok(finished)) => {
                assert!(document.complete_streaming_load(finished.generation, finished.encoding));
            }
            FileLoadEvent::Finished(Err(error)) => panic!("load failed: {error:?}"),
        }
    }

    assert_eq!(document.text(), "AéB");
    assert_eq!(
        document.bytes_for_save().expect("save bytes"),
        "AéB".as_bytes()
    );
}

#[test]
fn chunked_loader_streams_legacy_fallback_decoding() {
    let path = temp_file_path("windows-1252");
    let bytes = b"caf\xe9".to_vec();
    fs::write(&path, &bytes).expect("write temp input");

    let document_id = DocumentId::new(45);
    let generation = DocumentLoadGeneration::next();
    let events = collect_load_events(FileLoadRequest {
        document_id,
        generation,
        path: path.clone(),
        chunk_size: 1,
    });

    let _ = fs::remove_file(&path);

    let finished = events
        .iter()
        .find_map(|event| match event {
            FileLoadEvent::Finished(Ok(finished)) => Some(finished),
            _ => None,
        })
        .expect("finished event");

    assert_eq!(
        finished.encoding,
        fragile_notepad::core::TextEncoding::Windows1252
    );
    assert!(finished.had_errors);
    assert!(finished.fallback_contents.is_none());
    assert!(events.iter().any(|event| matches!(
        event,
        FileLoadEvent::Chunk(chunk) if chunk.reset
    )));
    assert_eq!(preview_from_chunks(&events), "café");
}

#[test]
fn chunked_loader_streams_utf16le_bom_decoding() {
    let path = temp_file_path("utf16le");
    let text = "a😀\r\nz";
    let mut bytes = vec![0xff, 0xfe];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(&path, &bytes).expect("write temp input");

    let document_id = DocumentId::new(47);
    let generation = DocumentLoadGeneration::next();
    let events = collect_load_events(FileLoadRequest {
        document_id,
        generation,
        path: path.clone(),
        chunk_size: 3,
    });

    let _ = fs::remove_file(&path);

    let finished = events
        .iter()
        .find_map(|event| match event {
            FileLoadEvent::Finished(Ok(finished)) => Some(finished),
            _ => None,
        })
        .expect("finished event");

    assert_eq!(
        finished.encoding,
        fragile_notepad::core::TextEncoding::Utf16LeBom
    );
    assert!(!finished.had_errors);
    assert!(finished.fallback_contents.is_none());
    assert_eq!(preview_from_chunks(&events), text);
}

#[test]
fn malformed_utf8_streams_reset_legacy_preview() {
    let path = temp_file_path("malformed-utf8");
    let bytes = vec![0xff; 4096];
    fs::write(&path, &bytes).expect("write temp input");

    let document_id = DocumentId::new(46);
    let generation = DocumentLoadGeneration::next();
    let events = collect_load_events(FileLoadRequest {
        document_id,
        generation,
        path: path.clone(),
        chunk_size: 1,
    });

    let _ = fs::remove_file(&path);

    let chunk_count = events
        .iter()
        .filter(|event| matches!(event, FileLoadEvent::Chunk(_)))
        .count();
    assert!(chunk_count > 1, "legacy fallback should stream chunks");

    let finished = events
        .iter()
        .find_map(|event| match event {
            FileLoadEvent::Finished(Ok(finished)) => Some(finished),
            _ => None,
        })
        .expect("finished event");

    assert_eq!(
        finished.encoding,
        fragile_notepad::core::TextEncoding::Windows1252
    );
    assert!(finished.had_errors);
    assert!(finished.fallback_contents.is_none());
    assert_eq!(preview_from_chunks(&events).chars().count(), bytes.len());
}

#[test]
fn binary_loader_streams_payload_as_normal_text() {
    let path = temp_file_path("binary");
    let bytes = vec![0; DEFAULT_CHUNK_SIZE * 16];
    fs::write(&path, &bytes).expect("write temp input");

    let document_id = DocumentId::new(48);
    let generation = DocumentLoadGeneration::next();
    let events = collect_load_events(FileLoadRequest {
        document_id,
        generation,
        path: path.clone(),
        chunk_size: DEFAULT_CHUNK_SIZE,
    });
    let _ = fs::remove_file(&path);

    let contents = preview_from_chunks(&events);
    let chunk_count = events
        .iter()
        .filter(|event| matches!(event, FileLoadEvent::Chunk(_)))
        .count();

    assert!(
        chunk_count > 1,
        "binary payloads should stream through the ordinary text path"
    );
    assert_eq!(contents.as_bytes(), bytes);

    let finished = events
        .iter()
        .find_map(|event| match event {
            FileLoadEvent::Finished(Ok(finished)) => Some(finished),
            _ => None,
        })
        .expect("finished event");

    assert_eq!(finished.document_id, document_id);
    assert_eq!(finished.generation, generation);
    assert_eq!(finished.bytes_read, bytes.len() as u64);
    assert_eq!(finished.total_bytes, Some(bytes.len() as u64));
    assert!(!finished.had_errors);
    assert!(finished.fallback_contents.is_none());
}

#[cfg(windows)]
#[test]
fn windows_ntdll_binary_load_applies_as_normal_text_without_chunk_stall() {
    let path = PathBuf::from(r"C:\Windows\System32\ntdll.dll");
    let Ok(metadata) = fs::metadata(&path) else {
        eprintln!("skipping ntdll.dll regression: file is unavailable");
        return;
    };

    let document_id = DocumentId::new(49);
    let generation = DocumentLoadGeneration::next();
    let events = collect_load_events(FileLoadRequest {
        document_id,
        generation,
        path: path.clone(),
        chunk_size: DEFAULT_CHUNK_SIZE,
    });
    let mut document = Document::loading(document_id, path.clone(), generation);
    let started = Instant::now();
    let mut chunk_count = 0;
    let mut slowest_chunk = Duration::ZERO;

    for event in events {
        match event {
            FileLoadEvent::Progress(progress) => {
                document.update_load_progress(
                    progress.generation,
                    progress.bytes_read,
                    progress.total_bytes,
                );
            }
            FileLoadEvent::Chunk(chunk) => {
                chunk_count += 1;
                let chunk_started = Instant::now();
                document.replace_loading_preview(
                    chunk.generation,
                    &chunk.text,
                    chunk.reset,
                    chunk.bytes_read,
                    chunk.total_bytes,
                );
                slowest_chunk = slowest_chunk.max(chunk_started.elapsed());
            }
            FileLoadEvent::Finished(Ok(finished)) => {
                assert!(document.complete_streaming_load(finished.generation, finished.encoding));
                assert_eq!(finished.bytes_read, metadata.len());
                assert!(finished.had_errors);
            }
            FileLoadEvent::Finished(Err(error)) => panic!("load failed: {error:?}"),
        }
    }

    let elapsed = started.elapsed();
    assert!(chunk_count > 1);
    assert_eq!(document.text().chars().count() as u64, metadata.len());
    assert_eq!(document.viewport.line_count(), document.buffer.line_count());
    assert_eq!(
        document.decorations.line_decorations.len(),
        document.buffer.line_count()
    );
    assert!(
        slowest_chunk < Duration::from_millis(250),
        "a streamed ntdll.dll chunk blocked document updates for {slowest_chunk:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "applying ntdll.dll loader events to the document took {elapsed:?}"
    );
}

#[test]
fn chunked_loader_delivers_finished_after_many_droppable_chunks() {
    let path = temp_file_path("many-chunks");
    let bytes = vec![b'x'; DEFAULT_CHUNK_SIZE * 32];
    fs::write(&path, &bytes).expect("write temp input");

    let document_id = DocumentId::new(44);
    let generation = DocumentLoadGeneration::next();
    let events = collect_load_events(FileLoadRequest {
        document_id,
        generation,
        path: path.clone(),
        chunk_size: 1,
    });

    let _ = fs::remove_file(&path);

    assert!(
        events.iter().any(|event| matches!(
            event,
            FileLoadEvent::Finished(Ok(finished))
                if finished.document_id == document_id
                    && finished.generation == generation
                    && finished.bytes_read == bytes.len() as u64
        )),
        "terminal finished event should not be dropped when the bounded stream is full"
    );
}
