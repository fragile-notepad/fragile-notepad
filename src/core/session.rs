//! Versioned recovery data, independent of live editor objects.

use super::TextEncoding;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub documents: Vec<SessionDocument>,
    pub active_index: usize,
    /// Version 2 layout. Document entries remain process-wide so recovery text
    /// is serialized once even while a tab is moving between native windows.
    #[serde(default)]
    pub windows: Vec<SessionWindow>,
    #[serde(default)]
    pub focused_window: Option<usize>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            version: 1,
            documents: Vec::new(),
            active_index: 0,
            windows: Vec::new(),
            focused_window: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionWindow {
    pub document_indices: Vec<usize>,
    /// Index within this window's `document_indices`.
    pub active_index: usize,
    /// Last normal logical bounds: x, y, width, height.
    pub bounds: Option<[f32; 4]>,
    pub maximized: bool,
    pub fullscreen: bool,
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
    /// Return a uniform layout without mutating legacy recovery metadata.
    pub fn normalized_windows(&self) -> Vec<SessionWindow> {
        if self.version == 1 {
            if self.documents.is_empty() {
                return Vec::new();
            }
            return vec![SessionWindow {
                document_indices: (0..self.documents.len()).collect(),
                active_index: self.active_index,
                ..SessionWindow::default()
            }];
        }
        self.windows.clone()
    }

    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.version, 1 | 2) {
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
        if self.version == 2 {
            if self.windows.len() > 1_000 {
                return Err("Session has too many windows".into());
            }
            if self
                .focused_window
                .is_some_and(|index| index >= self.windows.len())
            {
                return Err("Invalid focused session window".into());
            }
            let mut assigned = vec![false; self.documents.len()];
            for window in &self.windows {
                if window.document_indices.is_empty()
                    || window.active_index >= window.document_indices.len()
                {
                    return Err("Invalid active window document".into());
                }
                if let Some([x, y, width, height]) = window.bounds {
                    if ![x, y, width, height].iter().all(|value| value.is_finite())
                        || width <= 0.0
                        || height <= 0.0
                    {
                        return Err("Invalid session window placement".into());
                    }
                }
                for &index in &window.document_indices {
                    let Some(seen) = assigned.get_mut(index) else {
                        return Err("Window references an invalid session document".into());
                    };
                    if *seen {
                        return Err("Session document belongs to several windows".into());
                    }
                    *seen = true;
                }
            }
            if assigned.contains(&false) {
                return Err("Session document has no window".into());
            }
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
    use super::{Session, SessionDocument, SessionWindow};

    fn recovery_document(text: &str) -> SessionDocument {
        SessionDocument {
            text: Some(text.into()),
            ..Default::default()
        }
    }

    #[test]
    fn legacy_layout_migrates_to_one_window_without_losing_active_document() {
        let session = Session {
            documents: vec![recovery_document("first"), recovery_document("second")],
            active_index: 1,
            ..Default::default()
        };
        assert!(session.validate().is_ok());
        let windows = session.normalized_windows();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].document_indices, vec![0, 1]);
        assert_eq!(windows[0].active_index, 1);
    }

    #[test]
    fn multiwindow_session_round_trips_layout_and_native_placement() {
        let session = Session {
            version: 2,
            documents: vec![recovery_document("first"), recovery_document("second")],
            windows: vec![
                SessionWindow {
                    document_indices: vec![1],
                    bounds: Some([-500.0, 40.0, 800.0, 600.0]),
                    ..Default::default()
                },
                SessionWindow {
                    document_indices: vec![0],
                    maximized: true,
                    ..Default::default()
                },
            ],
            focused_window: Some(1),
            ..Default::default()
        };
        assert!(session.validate().is_ok());
        let restored: Session =
            serde_json::from_str(&serde_json::to_string(&session).unwrap()).unwrap();
        assert_eq!(restored, session);
    }

    #[test]
    fn multiwindow_session_rejects_duplicate_unowned_or_invalid_document_indices() {
        let mut session = Session {
            version: 2,
            documents: vec![recovery_document("first"), recovery_document("second")],
            windows: vec![SessionWindow {
                document_indices: vec![0, 0],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(session.validate().is_err());
        session.windows[0].document_indices = vec![0];
        assert!(session.validate().is_err());
        session.windows[0].document_indices = vec![0, 2];
        assert!(session.validate().is_err());
        session.windows[0].document_indices = vec![0, 1];
        session.windows[0].active_index = 2;
        assert!(session.validate().is_err());
    }

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
        TextEncoding::from_label(&label)
            .ok_or_else(|| serde::de::Error::custom("Unknown session encoding"))
    }
}
