//! Select installed families for Linux's generic text fonts.
use cosmic_text::fontdb::{self, Family, Query};

pub(super) fn configure(db: &mut fontdb::Database) {
    #[cfg(not(feature = "fira-sans"))]
    if let Some(family) = installed_family(
        db,
        Family::SansSerif,
        &[
            "Noto Sans",
            "DejaVu Sans",
            "Liberation Sans",
            "Ubuntu",
            "Cantarell",
            "Droid Sans",
            "FreeSans",
        ],
    ) {
        db.set_sans_serif_family(family);
    }

    if let Some(family) = installed_family(
        db,
        Family::Serif,
        &["Noto Serif", "DejaVu Serif", "Liberation Serif", "FreeSerif"],
    ) {
        db.set_serif_family(family);
    }

    if let Some(family) = installed_family(
        db,
        Family::Monospace,
        &[
            "Noto Sans Mono",
            "DejaVu Sans Mono",
            "Liberation Mono",
            "Ubuntu Mono",
            "Droid Sans Mono",
            "FreeMono",
        ],
    ) {
        db.set_monospace_family(family);
    }
}

fn installed_family(
    db: &fontdb::Database,
    generic: Family<'_>,
    families: &[&'static str],
) -> Option<&'static str> {
    // Cosmic's defaults name fonts that some distributions do not install.
    // Keep a usable alias; replace a missing one before shaping can fall back
    // to an unrelated family.
    if db
        .query(&Query {
            families: &[generic],
            ..Query::default()
        })
        .is_some()
    {
        return None;
    }

    families.iter().copied().find(|family| {
        db.query(&Query {
            families: &[Family::Name(family)],
            ..Query::default()
        })
        .is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn add_face(db: &mut fontdb::Database, family: &str, monospaced: bool) -> fontdb::ID {
        db.push_face_info(fontdb::FaceInfo {
            id: fontdb::ID::dummy(),
            source: fontdb::Source::Binary(Arc::new(Vec::<u8>::new())),
            index: 0,
            families: vec![(family.to_owned(), fontdb::Language::English_UnitedStates)],
            post_script_name: family.to_owned(),
            style: fontdb::Style::Normal,
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            monospaced,
        })
    }

    fn query(db: &fontdb::Database, family: Family<'_>) -> Option<fontdb::ID> {
        db.query(&Query {
            families: &[family],
            ..Query::default()
        })
    }

    #[test]
    fn missing_generic_families_select_installed_linux_fonts() {
        let mut db = fontdb::Database::new();
        db.set_sans_serif_family("Open Sans");
        db.set_serif_family("Missing Serif");
        db.set_monospace_family("Noto Sans Mono");
        let sans = add_face(&mut db, "DejaVu Sans", false);
        let serif = add_face(&mut db, "DejaVu Serif", false);
        let mono = add_face(&mut db, "DejaVu Sans Mono", true);

        configure(&mut db);

        #[cfg(not(feature = "fira-sans"))]
        assert_eq!(query(&db, Family::SansSerif), Some(sans));
        #[cfg(feature = "fira-sans")]
        let _ = sans;
        assert_eq!(query(&db, Family::Serif), Some(serif));
        assert_eq!(query(&db, Family::Monospace), Some(mono));
    }

    #[test]
    fn installed_generic_aliases_keep_their_selected_fonts() {
        let mut db = fontdb::Database::new();
        db.set_sans_serif_family("Custom UI");
        db.set_serif_family("Custom Serif");
        db.set_monospace_family("Custom Mono");
        let sans = add_face(&mut db, "Custom UI", false);
        let serif = add_face(&mut db, "Custom Serif", false);
        let mono = add_face(&mut db, "Custom Mono", true);
        let _ = add_face(&mut db, "Noto Sans", false);
        let _ = add_face(&mut db, "Noto Serif", false);
        let _ = add_face(&mut db, "Noto Sans Mono", true);

        configure(&mut db);

        assert_eq!(query(&db, Family::SansSerif), Some(sans));
        assert_eq!(query(&db, Family::Serif), Some(serif));
        assert_eq!(query(&db, Family::Monospace), Some(mono));
    }
}
