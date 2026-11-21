// SPDX-License-Identifier: GPL-3.0-or-later
//! Decks' runs are the suite's neutral runs (ADR 0012, #1662). Payloads
//! written while they were Letters' runs, with Letters' fields at their
//! defaults, still read; what Decks writes now carries only the shared
//! fields, under the same names.

use decks_core::engine::{Run, RunStyle};

/// A run as Decks serialized it while it used Letters' `RunStyle`.
const LETTERS_SHAPED: &str = r##"{"text":"Hi","style":{"bold":true,"italic":false,"underline":false,
"strikethrough":false,"highlight":false,"code":false,"link":null,"image":null,"image_extent_emu":null,
"font_family":"Cantarell","font_size_hp":48,"color":"#112233","vert_align":null,"footnote":null,
"html":false,"chip":null,"revision":null}}"##;

#[test]
fn a_run_saved_in_the_letters_shape_still_reads() {
    let run: Run = serde_json::from_str(LETTERS_SHAPED).expect("older payload reads");
    assert_eq!(
        run,
        Run {
            text: "Hi".into(),
            style: RunStyle {
                bold: true,
                font_family: Some("Cantarell".into()),
                font_size_hp: Some(48),
                color: Some("#112233".into()),
                ..Default::default()
            },
        }
    );
}

#[test]
fn a_run_writes_only_shared_fields_under_the_same_names() {
    let run: Run = serde_json::from_str(LETTERS_SHAPED).unwrap();
    let json: serde_json::Value = serde_json::to_value(&run).unwrap();
    let style = json["style"].as_object().unwrap();
    for gone in ["image", "image_extent_emu", "footnote", "html", "chip", "revision"] {
        assert!(!style.contains_key(gone), "{gone} written: {json}");
    }
    for kept in ["bold", "italic", "underline", "strikethrough", "font_family", "font_size_hp", "color"] {
        assert!(style.contains_key(kept), "{kept} missing: {json}");
    }
    let back: Run = serde_json::from_value(json).unwrap();
    assert_eq!(back, run);
}
