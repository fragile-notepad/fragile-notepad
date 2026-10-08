//! Chunked file loading for responsive open/drop flows.

use super::types::{
    FileError, FileLoadChunk, FileLoadEvent, FileLoadFailure, FileLoadFinished, FileLoadProgress,
    FileLoadRequest,
};
use crate::core::{FileRevision, TextEncoding};

use futures::{SinkExt, Stream, StreamExt, channel::mpsc, executor::block_on, stream};
use std::fs::File;
use std::io::{self, Read};
use std::sync::Arc;

pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;
const UTF8_BOM_BYTES: &[u8] = &[0xef, 0xbb, 0xbf];
const UTF16BE_BOM_BYTES: &[u8] = &[0xfe, 0xff];
const UTF16LE_BOM_BYTES: &[u8] = &[0xff, 0xfe];

struct HashedFile {
    file: File,
    hasher: blake3::Hasher,
}

impl HashedFile {
    fn open(path: &std::path::Path) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path)?,
            hasher: blake3::Hasher::new(),
        })
    }

    fn revision(&self) -> FileRevision {
        FileRevision(*self.hasher.finalize().as_bytes())
    }
}

impl Read for HashedFile {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = self.file.read(buffer)?;
        self.hasher.update(&buffer[..length]);
        Ok(length)
    }
}

pub fn load_file_chunks(request: FileLoadRequest) -> impl Stream<Item = FileLoadEvent> {
    load_file_chunks_with_encoding(request, None)
}

pub fn load_file_chunks_with_encoding(
    request: FileLoadRequest,
    encoding: Option<TextEncoding>,
) -> impl Stream<Item = FileLoadEvent> {
    let (sender, receiver) = mpsc::channel(8);
    let start = stream::once(async move {
        static LOAD_SLOTS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
            std::sync::OnceLock::new();
        let slots = LOAD_SLOTS
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(4)))
            .clone();
        let Ok(permit) = slots.acquire_owned().await else {
            return None;
        };
        if sender.is_closed() {
            return None;
        }
        std::thread::spawn(move || {
            let _permit = permit;
            load_file_on_thread(request, sender, encoding);
        });
        None::<FileLoadEvent>
    })
    .filter_map(futures::future::ready);
    stream::select(receiver, start)
}

fn load_file_on_thread(
    request: FileLoadRequest,
    mut sender: mpsc::Sender<FileLoadEvent>,
    encoding_override: Option<TextEncoding>,
) {
    if sender.is_closed() {
        return;
    }
    let total_bytes = std::fs::metadata(&request.path)
        .ok()
        .map(|metadata| metadata.len());

    send_progress(&mut sender, &request, 0, total_bytes);

    let mut file = match HashedFile::open(&request.path) {
        Ok(file) => file,
        Err(error) => {
            send_failure(&mut sender, request, FileError::Io(error.kind()));
            return;
        }
    };

    let chunk_size = request.chunk_size.max(1);
    let mut first_read = vec![0; chunk_size];
    let read = match file.read(&mut first_read) {
        Ok(read) => read,
        Err(error) => {
            send_failure(&mut sender, request, FileError::Io(error.kind()));
            return;
        }
    };

    if read == 0 {
        send_terminal(
            &mut sender,
            FileLoadEvent::Finished(Ok(FileLoadFinished {
                disk_revision: file.revision(),
                document_id: request.document_id,
                generation: request.generation,
                path: request.path,
                encoding: encoding_override.unwrap_or(TextEncoding::Utf8),
                had_errors: false,
                fallback_contents: None,
                bytes_read: 0,
                total_bytes,
            })),
        );
        return;
    }

    first_read.truncate(read);
    while first_read.len() < UTF8_BOM_BYTES.len() {
        if sender.is_closed() {
            return;
        }
        let mut byte = [0; 1];
        let read = match file.read(&mut byte) {
            Ok(read) => read,
            Err(error) => {
                send_failure(&mut sender, request, FileError::Io(error.kind()));
                return;
            }
        };
        if read == 0 {
            break;
        }
        first_read.push(byte[0]);
    }
    let encoding = encoding_override.unwrap_or_else(|| detect_initial_encoding(&first_read));
    load_text_chunks(
        request,
        sender,
        file,
        first_read,
        total_bytes,
        DecodingOptions {
            encoding,
            allow_legacy_fallback: encoding_override.is_none() && encoding == TextEncoding::Utf8,
            had_errors: false,
            reset_first_chunk: false,
        },
    );
}

struct DecodingOptions {
    encoding: TextEncoding,
    allow_legacy_fallback: bool,
    had_errors: bool,
    reset_first_chunk: bool,
}

enum ChunkDecoder {
    EncodingRs(encoding_rs::Decoder),
    Utf16(Utf16ChunkDecoder),
    Latin1,
    Oem(encoding_rs::oem::OemCodePage),
}

impl ChunkDecoder {
    fn new(encoding: TextEncoding) -> Self {
        match encoding {
            TextEncoding::Utf16BeBom | TextEncoding::Utf16LeBom => {
                Self::Utf16(Utf16ChunkDecoder::new(encoding == TextEncoding::Utf16BeBom))
            }
            TextEncoding::Iso8859_1 => Self::Latin1,
            other => {
                if let Some(code_page) = other.oem_code_page() {
                    Self::Oem(code_page)
                } else {
                    Self::EncodingRs(
                        other
                            .encoding_rs()
                            .expect("supported text encoding")
                            .new_decoder_without_bom_handling(),
                    )
                }
            }
        }
    }

    fn decode(&mut self, bytes: &[u8], last: bool) -> (String, bool) {
        match self {
            Self::EncodingRs(decoder) => {
                let max_output = decoder
                    .max_utf8_buffer_length(bytes.len())
                    .unwrap_or(bytes.len().saturating_mul(3).saturating_add(16));
                let mut output = String::with_capacity(max_output);
                let (_, _, malformed) = decoder.decode_to_string(bytes, &mut output, last);
                (output, malformed)
            }
            Self::Utf16(decoder) => (decoder.decode(bytes, last), decoder.had_errors()),
            Self::Latin1 => (bytes.iter().copied().map(char::from).collect(), false),
            Self::Oem(code_page) => (encoding_rs::oem::decode_oem(bytes, *code_page), false),
        }
    }
}

fn load_text_chunks(
    request: FileLoadRequest,
    mut sender: mpsc::Sender<FileLoadEvent>,
    mut file: HashedFile,
    first_read: Vec<u8>,
    total_bytes: Option<u64>,
    options: DecodingOptions,
) {
    let mut buffer = vec![0; request.chunk_size.max(1)];
    let mut bytes_read = first_read.len() as u64;
    let mut decoder = ChunkDecoder::new(options.encoding);
    let mut pending = strip_initial_bom(&first_read, options.encoding);
    let mut had_errors = options.had_errors;
    let mut reset_next_chunk = options.reset_first_chunk;
    let mut last = false;

    loop {
        if sender.is_closed() {
            return;
        }
        let (output, malformed) = decoder.decode(pending, last);
        had_errors |= malformed;
        if malformed && options.allow_legacy_fallback {
            load_legacy_from_start(request, sender, total_bytes);
            return;
        }
        if !output.is_empty() || reset_next_chunk {
            send_chunk(
                &mut sender,
                &request,
                output,
                bytes_read,
                total_bytes,
                reset_next_chunk,
            );
            reset_next_chunk = false;
        }
        if last {
            break;
        }
        let read = match file.read(&mut buffer) {
            Ok(read) => read,
            Err(error) => {
                send_failure(&mut sender, request, FileError::Io(error.kind()));
                return;
            }
        };
        pending = &buffer[..read];
        bytes_read += read as u64;
        last = read == 0;
    }
    send_terminal(
        &mut sender,
        FileLoadEvent::Finished(Ok(FileLoadFinished {
            disk_revision: file.revision(),
            document_id: request.document_id,
            generation: request.generation,
            path: request.path,
            encoding: options.encoding,
            had_errors,
            fallback_contents: None,
            bytes_read,
            total_bytes,
        })),
    );
}

fn load_legacy_from_start(
    request: FileLoadRequest,
    mut sender: mpsc::Sender<FileLoadEvent>,
    total_bytes: Option<u64>,
) {
    let file = match HashedFile::open(&request.path) {
        Ok(file) => file,
        Err(error) => {
            send_failure(&mut sender, request, FileError::Io(error.kind()));
            return;
        }
    };
    load_text_chunks(
        request,
        sender,
        file,
        Vec::new(),
        total_bytes,
        DecodingOptions {
            encoding: TextEncoding::Windows1252,
            allow_legacy_fallback: false,
            had_errors: true,
            reset_first_chunk: true,
        },
    );
}

fn detect_initial_encoding(bytes: &[u8]) -> TextEncoding {
    if bytes.starts_with(UTF8_BOM_BYTES) {
        TextEncoding::Utf8Bom
    } else if bytes.starts_with(UTF16BE_BOM_BYTES) {
        TextEncoding::Utf16BeBom
    } else if bytes.starts_with(UTF16LE_BOM_BYTES) {
        TextEncoding::Utf16LeBom
    } else {
        TextEncoding::Utf8
    }
}

fn strip_initial_bom(bytes: &[u8], encoding: TextEncoding) -> &[u8] {
    let marker = match encoding {
        TextEncoding::Utf8 | TextEncoding::Utf8Bom => UTF8_BOM_BYTES,
        TextEncoding::Utf16BeBom => UTF16BE_BOM_BYTES,
        TextEncoding::Utf16LeBom => UTF16LE_BOM_BYTES,
        _ => return bytes,
    };
    bytes.strip_prefix(marker).unwrap_or(bytes)
}

struct Utf16ChunkDecoder {
    big_endian: bool,
    pending_byte: Option<u8>,
    pending_high_surrogate: Option<u16>,
    had_errors: bool,
}

impl Utf16ChunkDecoder {
    const fn new(big_endian: bool) -> Self {
        Self {
            big_endian,
            pending_byte: None,
            pending_high_surrogate: None,
            had_errors: false,
        }
    }

    const fn had_errors(&self) -> bool {
        self.had_errors
    }

    fn decode(&mut self, bytes: &[u8], last: bool) -> String {
        let mut output = String::new();
        let mut index = 0;

        if let Some(first) = self.pending_byte.take() {
            if let Some(second) = bytes.first().copied() {
                self.push_unit(unit_from_bytes(first, second, self.big_endian), &mut output);
                index = 1;
            } else if last {
                self.had_errors = true;
                output.push(char::REPLACEMENT_CHARACTER);
            } else {
                self.pending_byte = Some(first);
            }
        }

        while index + 1 < bytes.len() {
            self.push_unit(
                unit_from_bytes(bytes[index], bytes[index + 1], self.big_endian),
                &mut output,
            );
            index += 2;
        }

        if index < bytes.len() {
            if last {
                self.had_errors = true;
                output.push(char::REPLACEMENT_CHARACTER);
            } else {
                self.pending_byte = Some(bytes[index]);
            }
        }

        if last && let Some(high) = self.pending_high_surrogate.take() {
            self.had_errors = true;
            let _ = high;
            output.push(char::REPLACEMENT_CHARACTER);
        }

        output
    }

    fn push_unit(&mut self, unit: u16, output: &mut String) {
        if let Some(high) = self.pending_high_surrogate.take() {
            if (0xdc00..=0xdfff).contains(&unit) {
                let high = u32::from(high) - 0xd800;
                let low = u32::from(unit) - 0xdc00;
                if let Some(ch) = char::from_u32(0x10000 + ((high << 10) | low)) {
                    output.push(ch);
                    return;
                }
            }

            self.had_errors = true;
            output.push(char::REPLACEMENT_CHARACTER);
        }

        match unit {
            0xd800..=0xdbff => self.pending_high_surrogate = Some(unit),
            0xdc00..=0xdfff => {
                self.had_errors = true;
                output.push(char::REPLACEMENT_CHARACTER);
            }
            _ => {
                if let Some(ch) = char::from_u32(u32::from(unit)) {
                    output.push(ch);
                }
            }
        }
    }
}

fn unit_from_bytes(first: u8, second: u8, big_endian: bool) -> u16 {
    if big_endian {
        u16::from_be_bytes([first, second])
    } else {
        u16::from_le_bytes([first, second])
    }
}

fn send_chunk(
    sender: &mut mpsc::Sender<FileLoadEvent>,
    request: &FileLoadRequest,
    text: String,
    bytes_read: u64,
    total_bytes: Option<u64>,
    reset: bool,
) {
    let _ = block_on(sender.send(FileLoadEvent::Chunk(FileLoadChunk {
        document_id: request.document_id,
        generation: request.generation,
        path: request.path.clone(),
        text: Arc::new(text),
        reset,
        bytes_read,
        total_bytes,
    })));
}

fn send_progress(
    sender: &mut mpsc::Sender<FileLoadEvent>,
    request: &FileLoadRequest,
    bytes_read: u64,
    total_bytes: Option<u64>,
) {
    let _ = sender.try_send(FileLoadEvent::Progress(FileLoadProgress {
        document_id: request.document_id,
        generation: request.generation,
        path: request.path.clone(),
        bytes_read,
        total_bytes,
    }));
}

fn send_failure(
    sender: &mut mpsc::Sender<FileLoadEvent>,
    request: FileLoadRequest,
    error: FileError,
) {
    send_terminal(
        sender,
        FileLoadEvent::Finished(Err(FileLoadFailure {
            document_id: request.document_id,
            generation: request.generation,
            path: request.path,
            error,
        })),
    );
}

fn send_terminal(sender: &mut mpsc::Sender<FileLoadEvent>, event: FileLoadEvent) {
    let _ = block_on(sender.send(event));
}
