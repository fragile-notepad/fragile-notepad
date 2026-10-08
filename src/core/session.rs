//! Versioned recovery data, independent of live editor objects.

use super::TextEncoding;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub documents: Vec<SessionDocument>,
    pub active_index: usize,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            version: 1,
            documents: Vec::new(),
            active_index: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionDocument {
    #[serde(with = "native_path")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_revision: Option<super::FileRevision>,
    pub text: Option<String>,
    #[serde(with = "encoding")]
    pub encoding: TextEncoding,
    pub line_ending: Option<String>,
    pub is_pinned: bool,
    pub is_dirty: bool,
    pub anchor_line: usize,
    pub anchor_column: usize,
    pub cursor_line: usize,
    pub cursor_column: usize,
    pub first_visible_row: usize,
    /// Logical line and UTF-8 byte column at the top of the editor. Older
    /// sessions restore `first_visible_row` when this position is absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_visible_position: Option<(usize, usize)>,
    pub horizontal_offset: f32,
    pub syntax_token: Option<String>,
    /// None denotes older sessions which did not record language provenance.
    pub syntax_automatic: Option<bool>,
    pub collapsed_folds: Vec<(usize, usize)>,
}

impl Default for SessionDocument {
    fn default() -> Self {
        Self {
            path: None,
            disk_revision: None,
            text: None,
            encoding: TextEncoding::Utf8,
            line_ending: None,
            is_pinned: false,
            is_dirty: false,
            anchor_line: 0,
            anchor_column: 0,
            cursor_line: 0,
            cursor_column: 0,
            first_visible_row: 0,
            first_visible_position: None,
            horizontal_offset: 0.0,
            syntax_token: None,
            syntax_automatic: None,
            collapsed_folds: Vec::new(),
        }
    }
}

impl Session {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!("Unsupported session version: {}", self.version));
        }
        if self.documents.len() > 10_000 {
            return Err("Session has too many documents".into());
        }
        if (!self.documents.is_empty() && self.active_index >= self.documents.len())
            || (self.documents.is_empty() && self.active_index != 0)
        {
            return Err("Invalid active session document".into());
        }
        for doc in &self.documents {
            if doc.path.is_none() && doc.text.is_none() {
                return Err("Session document has neither a path nor recovery text".into());
            }
            if doc.is_dirty && doc.text.is_none() {
                return Err("Dirty session document has no recovery text".into());
            }
            if !doc.horizontal_offset.is_finite() || doc.horizontal_offset < 0.0 {
                return Err("Invalid session scroll offset".into());
            }
            if doc
                .line_ending
                .as_deref()
                .is_some_and(|ending| !matches!(ending, "\n" | "\r\n" | "\n\r" | "\r"))
            {
                return Err("Invalid session line ending".into());
            }
            if doc.collapsed_folds.iter().any(|(start, end)| start >= end) {
                return Err("Invalid session fold range".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SessionDocument;

    #[test]
    fn legacy_session_scroll_metadata_defaults_to_saved_screen_row() {
        let document: SessionDocument = serde_json::from_str(
            r#"{"text":"legacy","first_visible_row":12,"horizontal_offset":24.0}"#,
        )
        .unwrap();

        assert_eq!(document.first_visible_row, 12);
        assert_eq!(document.first_visible_position, None);
        assert_eq!(document.horizontal_offset, 24.0);
    }

    #[test]
    fn session_logical_top_position_survives_serialization() {
        let document = SessionDocument {
            text: Some("éééé".into()),
            first_visible_row: 3,
            first_visible_position: Some((0, 6)),
            ..Default::default()
        };
        let encoded = serde_json::to_string(&document).unwrap();
        let decoded: SessionDocument = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, document);
    }
}

mod native_path {
    use super::*;
    #[derive(Serialize, Deserialize)]
    #[serde(tag = "format", content = "value")]
    enum StoredPath {
        Utf8(String),
        Windows(Vec<u16>),
        Unix(Vec<u8>),
    }

    pub fn serialize<S: serde::Serializer>(
        path: &Option<PathBuf>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let encoded = path.as_ref().map(|path| {
            if let Some(text) = path.to_str() {
                return StoredPath::Utf8(text.to_owned());
            }
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStrExt;
                StoredPath::Windows(path.as_os_str().encode_wide().collect())
            }
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                StoredPath::Unix(path.as_os_str().as_bytes().to_vec())
            }
        });
        encoded.serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<PathBuf>, D::Error> {
        Option::<StoredPath>::deserialize(deserializer)?
            .map(|path| match path {
                StoredPath::Utf8(text) => Ok(PathBuf::from(text)),
                #[cfg(windows)]
                StoredPath::Windows(units) => {
                    use std::os::windows::ffi::OsStringExt;
                    Ok(PathBuf::from(std::ffi::OsString::from_wide(&units)))
                }
                #[cfg(unix)]
                StoredPath::Unix(bytes) => {
                    use std::os::unix::ffi::OsStringExt;
                    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
                }
                _ => Err(serde::de::Error::custom(
                    "Session path belongs to a different platform",
                )),
            })
            .transpose()
    }
}

mod encoding {
    use super::*;
    const ALL: &[TextEncoding] = &[
        TextEncoding::Utf8,
        TextEncoding::Utf8Bom,
        TextEncoding::Utf16BeBom,
        TextEncoding::Utf16LeBom,
        TextEncoding::Windows1250,
        TextEncoding::Windows1251,
        TextEncoding::Windows1252,
        TextEncoding::Windows1253,
        TextEncoding::Windows1254,
        TextEncoding::Windows1255,
        TextEncoding::Windows1256,
        TextEncoding::Windows1257,
        TextEncoding::Windows1258,
        TextEncoding::Iso8859_1,
        TextEncoding::Iso8859_2,
        TextEncoding::Iso8859_3,
        TextEncoding::Iso8859_4,
        TextEncoding::Iso8859_5,
        TextEncoding::Iso8859_6,
        TextEncoding::Iso8859_7,
        TextEncoding::Iso8859_8,
        TextEncoding::Iso8859_8I,
        TextEncoding::Iso8859_9,
        TextEncoding::Iso8859_10,
        TextEncoding::Iso8859_13,
        TextEncoding::Iso8859_14,
        TextEncoding::Iso8859_15,
        TextEncoding::Iso8859_16,
        TextEncoding::Koi8R,
        TextEncoding::Koi8U,
        TextEncoding::Macintosh,
        TextEncoding::Big5,
        TextEncoding::Gb18030,
        TextEncoding::ShiftJis,
        TextEncoding::EucJp,
        TextEncoding::EucKr,
        TextEncoding::Iso2022Jp,
        TextEncoding::Tis620,
        TextEncoding::Oem437,
        TextEncoding::Oem720,
        TextEncoding::Oem737,
        TextEncoding::Oem775,
        TextEncoding::Oem850,
        TextEncoding::Oem852,
        TextEncoding::Oem855,
        TextEncoding::Oem857,
        TextEncoding::Oem858,
        TextEncoding::Oem860,
        TextEncoding::Oem861,
        TextEncoding::Oem862,
        TextEncoding::Oem863,
        TextEncoding::Oem865,
        TextEncoding::Oem866,
        TextEncoding::Oem869,
    ];
    pub fn serialize<S: serde::Serializer>(
        encoding: &TextEncoding,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(encoding.label())
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<TextEncoding, D::Error> {
        let label = String::deserialize(deserializer)?;
        ALL.iter()
            .copied()
            .find(|value| value.label() == label)
            .ok_or_else(|| serde::de::Error::custom("Unknown session encoding"))
    }
}
