// group_transform.rs — a group's rotation and flips move its shapes (#1989).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The reader placed a group's children through the group's offset and
// scale only, so a rotated callout cluster or a mirrored diagram came in
// as if the group were upright. It now turns and mirrors each child about
// the group's centre, as PowerPoint draws it.

use std::io::{Read, Write};

use decks_core::engine::shape::ShapeKind;
use decks_core::engine::{read_pptx, write_pptx, Deck, SlideObject};

fn shape(x: f64, rotation: f64) -> SlideObject {
    SlideObject::Shape { kind: ShapeKind::Ellipse, x, y: 100.0, w: 100.0, h: 50.0, rotation, style: Default::default() }
}

/// A copy of the deck at `path` whose shapes sit in one group spanning
/// x 100..400, y 100..150 (model units), with `xfrm_attrs` on its `a:xfrm`.
fn grouped(path: &std::path::Path, xfrm_attrs: &str) -> Deck {
    let emu = |u: f64| (u * 9_525.0).round() as i64;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let out = path.with_extension("grouped.pptx");
    let mut w = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if name == "ppt/slides/slide1.xml" {
            let xml = String::from_utf8(bytes).unwrap();
            let start = xml.find("<p:sp>").unwrap();
            let end = xml.rfind("</p:sp>").unwrap() + "</p:sp>".len();
            let (x, y, cx, cy) = (emu(100.0), emu(100.0), emu(300.0), emu(50.0));
            bytes = format!(
                "{}<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"90\" name=\"Group\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
                 <p:grpSpPr><a:xfrm{xfrm_attrs}><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/>\
                 <a:chOff x=\"{x}\" y=\"{y}\"/><a:chExt cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm></p:grpSpPr>{}</p:grpSp>{}",
                &xml[..start],
                &xml[start..end],
                &xml[end..],
            )
            .into_bytes();
        }
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(&bytes).unwrap();
    }
    w.finish().unwrap();
    read_pptx(out.to_str().unwrap()).unwrap()
}

fn placed(d: &Deck) -> Vec<(f64, f64, f64, f64, f64)> {
    d.slides[0]
        .objects
        .iter()
        .filter_map(|o| match o {
            SlideObject::Shape { x, y, w, h, rotation, .. } => Some((*x, *y, *w, *h, *rotation)),
            _ => None,
        })
        .collect()
}

fn assert_near(got: (f64, f64, f64, f64, f64), want: (f64, f64, f64, f64, f64), what: &str) {
    let close = |a: f64, b: f64| (a - b).abs() < 0.5;
    assert!(
        close(got.0, want.0) && close(got.1, want.1) && close(got.2, want.2) && close(got.3, want.3) && close(got.4, want.4),
        "{what}: got {got:?}, want {want:?}"
    );
}

fn deck_at(dir: &std::path::Path, shapes: Vec<SlideObject>) -> std::path::PathBuf {
    let mut d = Deck::new();
    d.slides[0].objects = shapes;
    let path = dir.join("shapes.pptx");
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    path
}

#[test]
fn a_quarter_turned_group_turns_its_shapes_about_its_centre() {
    let dir = tempfile::tempdir().unwrap();
    let path = deck_at(dir.path(), vec![shape(100.0, 0.0), shape(300.0, 0.0)]);
    let got = placed(&grouped(&path, " rot=\"5400000\""));
    assert_eq!(got.len(), 2, "{got:?}");
    // The group's centre is (250, 125). Turned 90 degrees clockwise, the
    // left shape goes above it and the right one below, each turned too.
    assert_near(got[0], (200.0, 0.0, 100.0, 50.0, 90.0), "left shape");
    assert_near(got[1], (200.0, 200.0, 100.0, 50.0, 90.0), "right shape");
}

#[test]
fn a_mirrored_group_swaps_its_shapes_and_mirrors_their_turn() {
    let dir = tempfile::tempdir().unwrap();
    let path = deck_at(dir.path(), vec![shape(100.0, 30.0), shape(300.0, 0.0)]);
    let got = placed(&grouped(&path, " flipH=\"1\""));
    assert_near(got[0], (300.0, 100.0, 100.0, 50.0, 330.0), "left shape, now on the right");
    assert_near(got[1], (100.0, 100.0, 100.0, 50.0, 0.0), "right shape, now on the left");
}

#[test]
fn a_group_flipped_both_ways_keeps_its_shapes_turn() {
    let dir = tempfile::tempdir().unwrap();
    let path = deck_at(dir.path(), vec![shape(100.0, 30.0), shape(300.0, 0.0)]);
    let got = placed(&grouped(&path, " flipH=\"1\" flipV=\"1\""));
    assert_near(got[0], (300.0, 100.0, 100.0, 50.0, 30.0), "left shape");
}

#[test]
fn an_upright_group_places_its_shapes_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let path = deck_at(dir.path(), vec![shape(100.0, 30.0), shape(300.0, 0.0)]);
    let got = placed(&grouped(&path, ""));
    assert_near(got[0], (100.0, 100.0, 100.0, 50.0, 30.0), "left shape");
    assert_near(got[1], (300.0, 100.0, 100.0, 50.0, 0.0), "right shape");
}
