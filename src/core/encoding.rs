use std::borrow::Cow;

const UTF8_BOM_BYTES: &[u8] = &[0xef, 0xbb, 0xbf];
const UTF16BE_BOM_BYTES: &[u8] = &[0xfe, 0xff];
const UTF16LE_BOM_BYTES: &[u8] = &[0xff, 0xfe];
const UTF8_BOM: &str = "\u{feff}";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf8Bom,
    Utf16BeBom,
    Utf16LeBom,
    Windows1250,
    Windows1251,
    Windows1252,
    Windows1253,
    Windows1254,
    Windows1255,
    Windows1256,
    Windows1257,
    Windows1258,
    Iso8859_1,
    Iso8859_2,
    Iso8859_3,
    Iso8859_4,
    Iso8859_5,
    Iso8859_6,
    Iso8859_7,
    Iso8859_8,
    Iso8859_8I,
    Iso8859_9,
    Iso8859_10,
    Iso8859_13,
    Iso8859_14,
    Iso8859_15,
    Iso8859_16,
    Koi8R,
    Koi8U,
    Macintosh,
    Big5,
    Gb18030,
    ShiftJis,
    EucJp,
    EucKr,
    Iso2022Jp,
    Tis620,
    Oem437,
    Oem720,
    Oem737,
    Oem775,
    Oem850,
    Oem852,
    Oem855,
    Oem857,
    Oem858,
    Oem860,
    Oem861,
    Oem862,
    Oem863,
    Oem865,
    Oem866,
    Oem869,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingError {
    Unsupported,
    MalformedInput,
    UnmappableCharacters,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedText {
    pub text: String,
    pub encoding: TextEncoding,
    pub had_errors: bool,
}

impl TextEncoding {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8-BOM",
            Self::Utf16BeBom => "UTF-16 BE BOM",
            Self::Utf16LeBom => "UTF-16 LE BOM",
            Self::Windows1250 => "Windows-1250",
            Self::Windows1251 => "Windows-1251",
            Self::Windows1252 => "Windows-1252",
            Self::Windows1253 => "Windows-1253",
            Self::Windows1254 => "Windows-1254",
            Self::Windows1255 => "Windows-1255",
            Self::Windows1256 => "Windows-1256",
            Self::Windows1257 => "Windows-1257",
            Self::Windows1258 => "Windows-1258",
            Self::Iso8859_1 => "ISO 8859-1",
            Self::Iso8859_2 => "ISO 8859-2",
            Self::Iso8859_3 => "ISO 8859-3",
            Self::Iso8859_4 => "ISO 8859-4",
            Self::Iso8859_5 => "ISO 8859-5",
            Self::Iso8859_6 => "ISO 8859-6",
            Self::Iso8859_7 => "ISO 8859-7",
            Self::Iso8859_8 => "ISO 8859-8",
            Self::Iso8859_8I => "ISO 8859-8-I",
            Self::Iso8859_9 => "ISO 8859-9",
            Self::Iso8859_10 => "ISO 8859-10",
            Self::Iso8859_13 => "ISO 8859-13",
            Self::Iso8859_14 => "ISO 8859-14",
            Self::Iso8859_15 => "ISO 8859-15",
            Self::Iso8859_16 => "ISO 8859-16",
            Self::Koi8R => "KOI8-R",
            Self::Koi8U => "KOI8-U",
            Self::Macintosh => "Macintosh",
            Self::Big5 => "Big5",
            Self::Gb18030 => "GB18030",
            Self::ShiftJis => "Shift-JIS",
            Self::EucJp => "EUC-JP",
            Self::EucKr => "EUC-KR",
            Self::Iso2022Jp => "ISO-2022-JP",
            Self::Tis620 => "TIS-620",
            Self::Oem437 => "OEM-US",
            Self::Oem720 => "OEM 720",
            Self::Oem737 => "OEM 737",
            Self::Oem775 => "OEM 775",
            Self::Oem850 => "OEM 850",
            Self::Oem852 => "OEM 852",
            Self::Oem855 => "OEM 855",
            Self::Oem857 => "OEM 857",
            Self::Oem858 => "OEM 858",
            Self::Oem860 => "OEM 860 : Portuguese",
            Self::Oem861 => "OEM 861 : Icelandic",
            Self::Oem862 => "OEM 862",
            Self::Oem863 => "OEM 863 : French",
            Self::Oem865 => "OEM 865 : Nordic",
            Self::Oem866 => "OEM 866",
            Self::Oem869 => "OEM 869",
        }
    }

    fn encoding_rs(self) -> Option<&'static encoding_rs::Encoding> {
        match self {
            Self::Utf8 | Self::Utf8Bom => Some(encoding_rs::UTF_8),
            Self::Utf16BeBom | Self::Utf16LeBom => None,
            Self::Windows1250 => Some(encoding_rs::WINDOWS_1250),
            Self::Windows1251 => Some(encoding_rs::WINDOWS_1251),
            Self::Windows1252 => Some(encoding_rs::WINDOWS_1252),
            Self::Windows1253 => Some(encoding_rs::WINDOWS_1253),
            Self::Windows1254 => Some(encoding_rs::WINDOWS_1254),
            Self::Windows1255 => Some(encoding_rs::WINDOWS_1255),
            Self::Windows1256 => Some(encoding_rs::WINDOWS_1256),
            Self::Windows1257 => Some(encoding_rs::WINDOWS_1257),
            Self::Windows1258 => Some(encoding_rs::WINDOWS_1258),
            Self::Iso8859_1 => None,
            Self::Iso8859_2 => Some(encoding_rs::ISO_8859_2),
            Self::Iso8859_3 => Some(encoding_rs::ISO_8859_3),
            Self::Iso8859_4 => Some(encoding_rs::ISO_8859_4),
            Self::Iso8859_5 => Some(encoding_rs::ISO_8859_5),
            Self::Iso8859_6 => Some(encoding_rs::ISO_8859_6),
            Self::Iso8859_7 => Some(encoding_rs::ISO_8859_7),
            Self::Iso8859_8 => Some(encoding_rs::ISO_8859_8),
            Self::Iso8859_8I => Some(encoding_rs::ISO_8859_8_I),
            Self::Iso8859_9 => Some(encoding_rs::WINDOWS_1254),
            Self::Iso8859_10 => Some(encoding_rs::ISO_8859_10),
            Self::Iso8859_13 => Some(encoding_rs::ISO_8859_13),
            Self::Iso8859_14 => Some(encoding_rs::ISO_8859_14),
            Self::Iso8859_15 => Some(encoding_rs::ISO_8859_15),
            Self::Iso8859_16 => Some(encoding_rs::ISO_8859_16),
            Self::Koi8R => Some(encoding_rs::KOI8_R),
            Self::Koi8U => Some(encoding_rs::KOI8_U),
            Self::Macintosh => Some(encoding_rs::MACINTOSH),
            Self::Big5 => Some(encoding_rs::BIG5),
            Self::Gb18030 => Some(encoding_rs::GB18030),
            Self::ShiftJis => Some(encoding_rs::SHIFT_JIS),
            Self::EucJp => Some(encoding_rs::EUC_JP),
            Self::EucKr => Some(encoding_rs::EUC_KR),
            Self::Iso2022Jp => Some(encoding_rs::ISO_2022_JP),
            Self::Tis620 => Some(encoding_rs::WINDOWS_874),
            Self::Oem437
            | Self::Oem720
            | Self::Oem737
            | Self::Oem775
            | Self::Oem850
            | Self::Oem852
            | Self::Oem855
            | Self::Oem857
            | Self::Oem858
            | Self::Oem860
            | Self::Oem861
            | Self::Oem862
            | Self::Oem863
            | Self::Oem865
            | Self::Oem866
            | Self::Oem869 => None,
        }
    }

    fn oem_code_page(self) -> Option<encoding_rs::oem::OemCodePage> {
        use encoding_rs::oem::OemCodePage;

        match self {
            Self::Oem437 => Some(OemCodePage::Cp437),
            Self::Oem720 => Some(OemCodePage::Cp720),
            Self::Oem737 => Some(OemCodePage::Cp737),
            Self::Oem775 => Some(OemCodePage::Cp775),
            Self::Oem850 => Some(OemCodePage::Cp850),
            Self::Oem852 => Some(OemCodePage::Cp852),
            Self::Oem855 => Some(OemCodePage::Cp855),
            Self::Oem857 => Some(OemCodePage::Cp857),
            Self::Oem858 => Some(OemCodePage::Cp858),
            Self::Oem860 => Some(OemCodePage::Cp860),
            Self::Oem861 => Some(OemCodePage::Cp861),
            Self::Oem862 => Some(OemCodePage::Cp862),
            Self::Oem863 => Some(OemCodePage::Cp863),
            Self::Oem865 => Some(OemCodePage::Cp865),
            Self::Oem866 => Some(OemCodePage::Cp866),
            Self::Oem869 => Some(OemCodePage::Cp869),
            _ => None,
        }
    }
}

pub fn decode_bytes(bytes: &[u8]) -> DecodedText {
    if bytes.starts_with(UTF8_BOM_BYTES) {
        let (text, had_errors) = decode_with_encoding(encoding_rs::UTF_8, &bytes[3..]);
        return DecodedText {
            text,
            encoding: TextEncoding::Utf8Bom,
            had_errors,
        };
    }

    if bytes.starts_with(UTF16BE_BOM_BYTES) {
        return decode_utf16(&bytes[2..], TextEncoding::Utf16BeBom, true);
    }

    if bytes.starts_with(UTF16LE_BOM_BYTES) {
        return decode_utf16(&bytes[2..], TextEncoding::Utf16LeBom, false);
    }

    let (text, had_errors) = decode_with_encoding(encoding_rs::UTF_8, bytes);

    if !had_errors {
        return DecodedText {
            text,
            encoding: TextEncoding::Utf8,
            had_errors: false,
        };
    }

    let (text, had_errors) = decode_with_encoding(encoding_rs::WINDOWS_1252, bytes);
    DecodedText {
        text,
        encoding: TextEncoding::Windows1252,
        had_errors,
    }
}

pub fn encode_text(text: &str, encoding: TextEncoding) -> Result<Vec<u8>, EncodingError> {
    match encoding {
        TextEncoding::Utf8 => Ok(strip_text_bom(text).as_bytes().to_vec()),
        TextEncoding::Utf8Bom => {
            let mut bytes = UTF8_BOM_BYTES.to_vec();
            bytes.extend_from_slice(strip_text_bom(text).as_bytes());
            Ok(bytes)
        }
        TextEncoding::Utf16BeBom => Ok(encode_utf16(text, true)),
        TextEncoding::Utf16LeBom => Ok(encode_utf16(text, false)),
        TextEncoding::Iso8859_1 => encode_iso_8859_1(text),
        other if other.oem_code_page().is_some() => {
            let code_page = other.oem_code_page().expect("checked code page");
            encoding_rs::oem::encode_oem(strip_text_bom(text), code_page)
                .map_err(|_| EncodingError::UnmappableCharacters)
        }
        other => {
            let encoding = other.encoding_rs().ok_or(EncodingError::Unsupported)?;
            let (encoded, _, had_errors) = encoding.encode(strip_text_bom(text));
            if had_errors {
                Err(EncodingError::UnmappableCharacters)
            } else {
                Ok(encoded.into_owned())
            }
        }
    }
}

pub fn encode_utf8_chunks_for_save<'a>(
    chunks: impl IntoIterator<Item = &'a str>,
    append_final_ending: Option<&str>,
    with_bom: bool,
) -> Vec<u8> {
    let mut bytes = if with_bom {
        UTF8_BOM_BYTES.to_vec()
    } else {
        Vec::new()
    };

    let mut first_chunk = true;
    for chunk in chunks {
        let chunk = if first_chunk {
            first_chunk = false;
            strip_text_bom(chunk)
        } else {
            chunk
        };
        bytes.extend_from_slice(chunk.as_bytes());
    }

    if let Some(ending) = append_final_ending {
        bytes.extend_from_slice(ending.as_bytes());
    }

    bytes
}

pub fn strip_text_bom(text: &str) -> &str {
    text.strip_prefix(UTF8_BOM).unwrap_or(text)
}

fn decode_with_encoding(encoding: &'static encoding_rs::Encoding, bytes: &[u8]) -> (String, bool) {
    let (text, _, had_errors) = encoding.decode(bytes);
    (text.into_owned(), had_errors)
}

fn decode_utf16(bytes: &[u8], encoding: TextEncoding, big_endian: bool) -> DecodedText {
    let mut had_errors = !bytes.len().is_multiple_of(2);
    let units = bytes.chunks_exact(2).map(|chunk| {
        if big_endian {
            u16::from_be_bytes([chunk[0], chunk[1]])
        } else {
            u16::from_le_bytes([chunk[0], chunk[1]])
        }
    });
    let text = std::char::decode_utf16(units)
        .map(|result| match result {
            Ok(ch) => ch,
            Err(_) => {
                had_errors = true;
                char::REPLACEMENT_CHARACTER
            }
        })
        .collect::<String>();

    DecodedText {
        text,
        encoding,
        had_errors,
    }
}

fn encode_utf16(text: &str, big_endian: bool) -> Vec<u8> {
    let mut bytes = if big_endian {
        UTF16BE_BOM_BYTES.to_vec()
    } else {
        UTF16LE_BOM_BYTES.to_vec()
    };

    for unit in strip_text_bom(text).encode_utf16() {
        let encoded = if big_endian {
            unit.to_be_bytes()
        } else {
            unit.to_le_bytes()
        };
        bytes.extend_from_slice(&encoded);
    }

    bytes
}

fn encode_iso_8859_1(text: &str) -> Result<Vec<u8>, EncodingError> {
    strip_text_bom(text)
        .chars()
        .map(|ch| {
            let code = ch as u32;
            u8::try_from(code).map_err(|_| EncodingError::UnmappableCharacters)
        })
        .collect()
}

impl From<Cow<'_, str>> for DecodedText {
    fn from(text: Cow<'_, str>) -> Self {
        Self {
            text: text.into_owned(),
            encoding: TextEncoding::Utf8,
            had_errors: false,
        }
    }
}
