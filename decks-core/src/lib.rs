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
pub mod provenance;
pub mod recovery;
pub mod rich_text;
pub mod snapshot;
pub mod templates;
pub mod undo;

pub use controller::{DecksController, SelectionMode, SelectionState, TextRange, TextSessionError, TxnOutcome};
pub use engine::{Deck, MasterSlide, Slide, SlideObject};
pub use provenance::{DeckSourceContext, MutationSet, ProvenanceFacts, SourceState};
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
    let opaque = suite_common_core::interop::OpaquePackage::capture(path, provenance::recognized_parts(format))?;
    let mut report = suite_common_core::interop::CompatibilityReport::new(format);
    for name in opaque.part_names() {
        report.record(suite_common_core::interop::UnsupportedFeature::new("uninterpreted-package-part", "Uninterpreted package part", name, suite_common_core::interop::FeatureDisposition::OpaquePassThrough, "will be copied through on an opaque save"));
    }
    // Slides the reader could not read (#1977): they are not in the deck,
    // so nothing else in this report can speak for them. Naming them here
    // is what stands between the user and a save that drops them forever;
    // the save-time loss check (`loss::content_a_save_drops`) makes the
    // same finding from the file under the same id.
    for skipped in &deck.skipped_slides {
        report.record(suite_common_core::interop::UnsupportedFeature::new("unreadable-slides", "Slides that could not be read", &skipped.part, suite_common_core::interop::FeatureDisposition::WarnOnLoss, format!("the slide was not read ({}); saving will drop it permanently", skipped.reason)));
    }
    Ok((deck, report, opaque))
}

/// Read a deck together with its session-owned source context (DCK-002):
/// the model, the import report, and the provenance pinned to the same
/// opened bytes. The caller installs the model and the context together
/// only after this returns `Ok` — never one without the other. This is
/// the only opener the window uses; `read_deck` stays for tests and
/// headless tooling with no session to own a context.
pub fn read_deck_with_context(path: &str) -> Result<(engine::Deck, DeckSourceContext), String> {
    let (deck, _, _) = read_deck_with_report(path)?;
    Ok((deck, DeckSourceContext::capture(std::path::Path::new(path))))
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

/// Write `deck` to `path` with the imported revision's safe parts, as one
/// staged-package transaction (DCK-002). This is the only live save path:
/// the old write-then-append helper is gone, so no caller can bypass the
/// staging, the carried-part hygiene, or the post-commit consistency
/// check. `carried` is the context's `carried_for_save` — pinned bytes
/// from the opened revision, never a re-read of the live source.
///
/// Validation runs before anything is written (unknown extension, hostile
/// carried names); the writers' output plus the carried parts are checked
/// for package consistency before this returns. A part the writer wrote
/// itself is left as written, and a cross-format target drops the carried
/// set whole (`Carried::apply`); the dialog, not this function, reports
/// those by name.
pub fn write_deck_with_context(
    path: &str,
    deck: &engine::Deck,
    carried: &suite_common_core::carry::Carried,
) -> Result<(), String> {
    let carried_names: Vec<String> = carried.names().map(str::to_string).collect();
    for name in &carried_names {
        if provenance::carried_name_is_hostile(name) {
            return Err(format!(
                "save blocked: the original file names a carried part that is not a safe relative path ({name})"
            ));
        }
    }
    carried.write_with(std::path::Path::new(path), |p| {
        write_deck(
            p.to_str().ok_or_else(|| "save path is not UTF-8".to_string())?,
            deck,
        )
    })?;
    // Tripwire, not a second safety boundary: `carry` maintains content
    // types/relationships/manifest entries itself, so a problem naming a
    // carried part means the save just wrote a half-consistent package.
    // Anything else is the writers' own business (DCK-001 row 9 pins a
    // clean Decks-written package separately).
    let problems: Vec<String> = suite_common_core::carry::problems(std::path::Path::new(path))
        .into_iter()
        .filter(|problem| carried_names.iter().any(|name| problem.contains(name)))
        .collect();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "save blocked: the saved package is inconsistent: {}",
            problems.join("; ")
        ))
    }
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
