// empty_paragraphs.rs — an empty paragraph is as tall as its end mark.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A blank paragraph between two others has no run, so its height is its
// `a:endParaRPr` size. Decks used the run before it instead, so an 8 pt
// spacer line between 20 pt paragraphs pushed everything after it down a
// whole 20 pt line, and most real decks use such spacers.

use std::io::{Read, Write};

use decks_core::engine::{read_pptx, write_pptx, Deck, ParaStyle, SlideObject, TextBody};
use suite_common_core::document::{Run, RunStyle};

fn ends(d: &Deck) -> Vec<Option<u16>> {
    d.slides[0]
        .objects
        .iter()
        .find_map(|o| match o {
            SlideObject::TextBox { body, .. } => Some(
                body.paras
                    .iter()
                    .map(|p| p.end_run.as_ref().and_then(|r| r.font_size_hp))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text: {:?}", d.slides[0].objects))
}

/// A deck of one box: a 20 pt line, an 8 pt spacer, a 20 pt line.
fn spaced_deck() -> Deck {
    let big = RunStyle {
        font_size_hp: Some(40),
        ..Default::default()
    };
    let mut d = Deck::new();
    let spacer = ParaStyle {
        end_run: Some(RunStyle {
            font_size_hp: Some(16),
            ..Default::default()
        }),
        ..Default::default()
    };
    d.slides[0].objects = vec![SlideObject::TextBox {
        text: "a\n\nb".into(),
        x: 100.0,
        y: 100.0,
        w: 400.0,
        h: 200.0,
        rotation: 0.0,
        runs: vec![
            Run {
                text: "a\n\n".into(),
                style: big.clone(),
            },
            Run {
                text: "b".into(),
                style: big,
            },
        ],
        body: TextBody {
            paras: vec![ParaStyle::default(), spacer, ParaStyle::default()],
            ..Default::default()
        },
    }];
    d
}

#[test]
fn an_empty_paragraphs_size_survives_a_pptx_save() {
    let d = spaced_deck();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("e.pptx");
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    let back = ends(&read_pptx(path.to_str().unwrap()).unwrap());
    assert_eq!(back.get(1).copied().flatten(), Some(16), "{back:?}");
    assert_eq!(
        back[0], None,
        "a paragraph with text has no end look of its own"
    );
}

/// A copy of the package at `path` with `name` replaced by `f` of it.
fn rewrite(path: &std::path::Path, name: &str, f: impl Fn(&str) -> String) -> std::path::PathBuf {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let out = path.with_extension("patched.pptx");
    let mut w = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let entry_name = entry.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry_name == name {
            bytes = f(&String::from_utf8(bytes).unwrap()).into_bytes();
        }
        w.start_file(entry_name, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(&bytes).unwrap();
    }
    w.finish().unwrap();
    out
}

#[test]
fn an_empty_paragraph_scales_with_the_slide_like_its_text() {
    // On a 13.33-inch slide every run is drawn at 0.75 of its size on our
    // 10-inch model. The spacer's end mark was left at full size, so each
    // blank line between bullets was a third taller than the text round it
    // (render lab decks/real-2024-11-presentation-pay-centre-innovation).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("e.pptx");
    write_pptx(path.to_str().unwrap(), &spaced_deck()).unwrap();
    let wide = rewrite(&path, "ppt/presentation.xml", |x| {
        let start = x.find("<p:sldSz ").expect("a slide size");
        let end = start + x[start..].find("/>").unwrap() + 2;
        format!(
            r#"{}<p:sldSz cx="12192000" cy="6858000"/>{}"#,
            &x[..start],
            &x[end..]
        )
    });
    let d = read_pptx(wide.to_str().unwrap()).unwrap();
    let text = d.slides[0].objects.iter().find_map(|o| match o {
        SlideObject::TextBox { runs, .. } => runs.first().and_then(|r| r.style.font_size_hp),
        _ => None,
    });
    assert_eq!(text, Some(30), "20 pt text -> 15 pt");
    assert_eq!(
        ends(&d).get(1).copied().flatten(),
        Some(12),
        "the 8 pt spacer -> 6 pt, as its text scales"
    );
}
