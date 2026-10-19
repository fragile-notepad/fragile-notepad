//! Native font selection and variation settings for macOS text.
use crate::core::font;
use cosmic_text::{Family, FontSystem, Weight, fontdb};
use std::sync::OnceLock;

static UI_WEIGHTS: OnceLock<[Weight; 9]> = OnceLock::new();

pub(super) fn configure(font_system: &mut FontSystem) {
    #[cfg(not(feature = "fira-sans"))]
    if let Some(family) = installed_family(
        font_system.db(),
        &[
            ".SF NS Text",
            ".SF NS",
            "SF Pro Text",
            "SF Pro",
            "Helvetica Neue",
            "Lucida Grande",
        ],
    ) {
        configure_sans_serif(font_system, family);
    }

    font_system.db_mut().set_serif_family("Times New Roman");
    if let Some(family) = installed_family(font_system.db(), &[".SF NS Mono", "SF Mono", "Menlo"]) {
        font_system.db_mut().set_monospace_family(family);
    }
}

#[cfg(not(feature = "fira-sans"))]
fn configure_sans_serif(font_system: &mut FontSystem, family: &'static str) {
    font_system.db_mut().set_sans_serif_family(family);
    if matches!(family, "Lucida Grande" | "Helvetica Neue") {
        // Cosmic's fallback iterator requires an exact static weight. Lucida's
        // regular/bold faces are 500/600, so a normal 400 request falls through.
        let weights = std::array::from_fn(|index| {
            let weight = Weight((index as u16 + 1) * 100);
            font_system
                .db()
                .query(&fontdb::Query {
                    families: &[Family::Name(family)],
                    weight,
                    ..fontdb::Query::default()
                })
                .and_then(|id| font_system.db().face(id))
                .map_or(weight, |face| face.weight)
        });
        let _ = UI_WEIGHTS.set(weights);
    } else {
        // Select SF's text design instead of its default 28pt display design.
        // Shaping and rasterization share these axes independently of DPI.
        let faces: Vec<_> = font_system
            .db()
            .faces()
            .filter(|face| face.families.iter().any(|(name, _)| name == family))
            .map(|face| face.id)
            .collect();
        for id in faces {
            font_system.set_font_optical_size(id, Some(17.0));
        }
    }
}

fn installed_family(db: &fontdb::Database, families: &[&'static str]) -> Option<&'static str> {
    // Resolve an installed family so generic text never falls through to an
    // arbitrary face on systems with a different set of native fonts.
    families.iter().copied().find(|family| {
        db.query(&fontdb::Query {
            families: &[Family::Name(family)],
            ..fontdb::Query::default()
        })
        .is_some()
    })
}

pub(super) fn ui_weight(family: font::Family, weight: Weight) -> Weight {
    if family == font::Family::SansSerif {
        UI_WEIGHTS
            .get()
            .map_or(weight, |weights| weights[usize::from(weight.0 / 100 - 1)])
    } else {
        weight
    }
}
