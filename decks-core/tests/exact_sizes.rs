// exact_sizes.rs — a run's size on a wide deck is kept exactly.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// On a 13.33-inch deck every run is drawn at 0.75 of its size on Decks'
// 10-inch model. A 19 pt run is 14.25 pt there, which half points round to
// 14.5: the HR policies deck's boxes of 19 pt text were drawn 1.7% large,
// and a save wrote them back as 19.33 pt (sz="1933").

use std::io::{Read, Write};

use decks_core::engine::{read_pptx, write_pptx, Deck, SlideObject};
use suite_common_core::document::{Run, RunStyle};

fn deck(hp: u16) -> Deck {
    let mut d = Deck::new();
    d.slides[0].objects = vec![SlideObject::TextBox {
        text: "nineteen".into(),
        x: 100.0,
        y: 100.0,
        w: 400.0,
        h: 100.0,
        rotation: 0.0,
        runs: vec![Run { text: "nineteen".into(), style: RunStyle { font_size_hp: Some(hp), ..Default::default() } }],
        body: Default::default(),
    }];
    d
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
        w.start_file(entry_name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(&bytes).unwrap();
    }
    w.finish().unwrap();
    out
}

fn part(path: &std::path::Path, name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

/// The deck as a 13.33-inch package holding a 19 pt run.
fn wide_19pt(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("n.pptx");
    write_pptx(path.to_str().unwrap(), &deck(38)).unwrap();
    rewrite(&path, "ppt/presentation.xml", |x| {
        let start = x.find("<p:sldSz ").expect("a slide size");
        let end = start + x[start..].find("/>").unwrap() + 2;
        format!(r#"{}<p:sldSz cx="12192000" cy="6858000"/>{}"#, &x[..start], &x[end..])
    })
}

#[test]
fn a_wide_decks_odd_size_is_drawn_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let wide = wide_19pt(dir.path());
    assert!(part(&wide, "ppt/slides/slide1.xml").contains(r#"sz="1900""#), "the fixture holds a 19 pt run");
    let d = read_pptx(wide.to_str().unwrap()).unwrap();
    let style = d.slides[0]
        .objects
        .iter()
        .find_map(|o| match o {
            SlideObject::TextBox { runs, .. } => runs.first().map(|r| r.style.clone()),
            _ => None,
        })
        .expect("the text");
    assert_eq!(style.font_size_hp, Some(29), "half points, rounded");
    assert_eq!(style.size_pt(), Some(14.25), "19 pt x 0.75, exactly");
}

#[test]
fn a_wide_decks_odd_size_survives_a_save() {
    let dir = tempfile::tempdir().unwrap();
    let wide = wide_19pt(dir.path());
    let d = read_pptx(wide.to_str().unwrap()).unwrap();
    let again = dir.path().join("again.pptx");
    write_pptx(again.to_str().unwrap(), &d).unwrap();
    let xml = part(&again, "ppt/slides/slide1.xml");
    assert!(xml.contains(r#"sz="1900""#), "saved back as 19 pt: {}", &xml[xml.find("sz=").unwrap_or(0)..][..12]);
}

/// A size the user sets replaces the read one, even where both round to
/// the same half point.
#[test]
fn a_set_size_wins_over_the_read_one() {
    let mut style = RunStyle { font_size_hp: Some(29), font_size_cpt: Some(1425), ..Default::default() };
    assert_eq!(style.size_pt(), Some(14.25));
    style.font_size_hp = Some(30);
    assert_eq!(style.size_pt(), Some(15.0), "a stale exact size is ignored");
}

/// A run with no size of its own, saved on a page wider than Decks' model
/// and read back, is the 18 pt it was: the file's sz is off the half-point
/// grid, and rounding it before scaling read it back as 18.14 pt.
#[test]
fn a_default_size_survives_a_save_on_a_wider_page() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = deck(36);
    if let SlideObject::TextBox { runs, .. } = &mut d.slides[0].objects[0] {
        runs[0].style.font_size_hp = None;
    }
    // A 28 cm x 15.75 cm ODP page.
    d.page_size = Some((10_080_000.0, 5_670_000.0));
    let path = dir.path().join("w.pptx");
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    let xml = part(&path, "ppt/slides/slide1.xml");
    let sz: u32 = xml[xml.find(r#"sz=""#).expect("a run size") + 4..].split('"').next().unwrap().parse().unwrap();
    assert!(!sz.is_multiple_of(50), "the fixture's size is off the half-point grid: sz={sz}");
    let back = read_pptx(path.to_str().unwrap()).unwrap();
    let style = back.slides[0]
        .objects
        .iter()
        .find_map(|o| match o {
            SlideObject::TextBox { runs, .. } => runs.first().map(|r| r.style.clone()),
            _ => None,
        })
        .expect("the text");
    assert_eq!(style.font_size_hp, Some(36));
    assert_eq!(style.font_size_cpt, None, "18 pt is on the half-point grid");
}
