// decks-core — GTK-free presentation core for Decks.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Owns the Deck/Slide/SlideObject model, PPTX read/write, and undo
// commands. The `decks` binary renders (Cairo/Pango) and wires signals.

pub mod builds;
#[cfg(feature = "collab")]
pub mod collab;
pub mod controller;
pub mod engine;
pub mod format;
pub mod fragment;
pub mod guides;
pub mod image_px;
pub mod insert;
pub mod layouts;
pub mod loss;
pub mod magic_move;
pub mod odp;
mod odp_builds;
mod odp_graphics;
mod odp_layouts;
pub mod ops;
mod odp_text;
pub mod presenter;
pub mod snapshot;
pub mod templates;
pub mod undo;

pub use controller::DecksController;
pub use engine::{Deck, MasterSlide, Slide, SlideObject};

/// Read a presentation, dispatching on extension (.pptx or .odp).
pub fn read_deck(path: &str) -> Result<engine::Deck, String> {
    if path.to_lowercase().ends_with(".odp") {
        odp::read(path)
    } else {
        engine::read_pptx(path)
    }
}

/// Read a deck while retaining unsupported package members for a safe save.
pub fn read_deck_with_report(path: &str) -> Result<(engine::Deck, suite_common_core::interop::CompatibilityReport, suite_common_core::interop::OpaquePackage), String> {
    let deck = read_deck(path)?;
    let format = if path.to_lowercase().ends_with(".odp") { "odp" } else { "pptx" };
    let recognized: &[&str] = if format == "odp" {
        &["mimetype", "META-INF/manifest.xml", "content.xml", "styles.xml", "settings.xml"]
    } else {
        &["[Content_Types].xml", "_rels/.rels", "ppt/presentation.xml", "ppt/_rels/presentation.xml.rels"]
    };
    let opaque = suite_common_core::interop::OpaquePackage::capture(path, recognized)?;
    let mut report = suite_common_core::interop::CompatibilityReport::new(format);
    for name in opaque.part_names() {
        report.record(suite_common_core::interop::UnsupportedFeature::new("uninterpreted-package-part", "Uninterpreted package part", name, suite_common_core::interop::FeatureDisposition::OpaquePassThrough, "will be copied through on an opaque save"));
    }
    Ok((deck, report, opaque))
}

/// The formats Decks opens and saves, declared once (#1206): the Open
/// dialog's filter is built from this, and tests hold the desktop entry's
/// MIME types and docs/FORMATS.md to it.
pub const FORMATS: &[suite_common_core::file_formats::FileFormat] = {
    use suite_common_core::file_formats::FileFormat;
    &[
        FileFormat {
            label: "PowerPoint presentation",
            extensions: &["pptx"],
            mime: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            writable: true,
        },
        FileFormat {
            label: "OpenDocument Presentation",
            extensions: &["odp"],
            mime: "application/vnd.oasis.opendocument.presentation",
            writable: true,
        },
    ]
};

/// Write a presentation in the format its extension names (.pptx or .odp).
///
/// Any other name is refused before anything is written. It used to get
/// PowerPoint bytes whatever it was called, so a Save As to `talk.key` or
/// `talk` wrote a file its name misdescribed (#1206).
pub fn write_deck(path: &str, deck: &engine::Deck) -> Result<(), String> {
    match suite_common_core::file_formats::for_path(FORMATS, path).map(|format| format.extension()) {
        Some("odp") => odp::write(deck, path),
        Some(_) => engine::write_pptx(path, deck),
        None => Err(format!(
            "Decks cannot tell which format to write \"{path}\" in. Save it as .pptx or .odp."
        )),
    }
}

pub fn write_deck_with_opaque(path: &str, deck: &engine::Deck, opaque: &suite_common_core::interop::OpaquePackage) -> Result<(), String> {
    write_deck(path, deck)?;
    opaque.append_to(path)
}

/// Render a presentation to an in-memory buffer without touching disk, in
/// the format implied by `format_hint` ("odp" or anything else -> pptx) —
/// used for autosave snapshots, which have no real save path to dispatch on.
pub fn write_deck_bytes(format_hint: &str, deck: &engine::Deck) -> Result<Vec<u8>, String> {
    if format_hint.eq_ignore_ascii_case("odp") {
        odp::write_bytes(deck)
    } else {
        engine::write_pptx_bytes(deck)
    }
}


#[cfg(test)]
mod format_tests {
    #[test]
    fn the_declared_formats_agree_with_the_docs_and_the_desktop_entry() {
        let problems = suite_common_core::file_formats::disagreements(
            super::FORMATS,
            "Decks",
            include_str!("../../flatpak/org.tunaos.decks.desktop"),
            include_str!("../../docs/FORMATS.md"),
        );
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// A name that is neither .pptx nor .odp is refused and nothing is
    /// written; both declared formats, in any case, still save and reopen.
    #[test]
    fn a_save_under_an_unknown_extension_is_refused_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let deck = crate::Deck::new();
        for name in ["talk.key", "talk", "talk.ppt"] {
            let path = dir.path().join(name);
            let error = super::write_deck(path.to_str().unwrap(), &deck).expect_err(name);
            assert!(error.contains(".pptx or .odp"), "{error}");
            assert!(!path.exists(), "{name} was written");
        }
        for name in ["talk.pptx", "talk.ODP"] {
            let path = dir.path().join(name);
            super::write_deck(path.to_str().unwrap(), &deck).expect(name);
            super::read_deck(path.to_str().unwrap()).expect(name);
        }
    }
}
