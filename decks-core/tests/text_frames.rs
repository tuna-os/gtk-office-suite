// text_frames.rs — a shape holding text keeps its own shape and paint,
// and the shapes of a group are where the group puts them.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The pay centre impact assessment's figures sit in navy tiles: white text
// on a filled shape, two of them in a group filled `a:grpFill`. The reader
// kept the text and dropped the tile (white on white), placed the group's
// shapes in the group's own frame, and filled them with the theme accent.

use std::io::{Read, Write};

use decks_core::engine::shape::{Color, ShapeKind, ShapeStyle, Stroke};
use decks_core::engine::text_body::Frame;
use decks_core::engine::{read_pptx, write_pptx, Deck, SlideObject};

fn navy() -> ShapeStyle {
    ShapeStyle { fill: Some(Color(0x09, 0x3B, 0x5C)), gradient: None, stroke: Some(Stroke { color: Color(0xFF, 0xFF, 0xFF), width: 3.0 }) }
}

fn tile(text: &str, x: f64) -> SlideObject {
    let body = decks_core::engine::TextBody {
        frame: Some(Frame { kind: ShapeKind::RoundRect { radius: 0.03 }, style: navy() }),
        ..Default::default()
    };
    SlideObject::TextBox { text: text.into(), x, y: 100.0, w: 120.0, h: 90.0, rotation: 0.0, runs: Vec::new(), body }
}

/// A copy of the package at `path` with `name` replaced by `f` of it.
fn rewrite(path: &std::path::Path, name: &str, f: impl Fn(&str) -> String) -> std::path::PathBuf {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let out = path.with_extension("patched.pptx");
    let mut w = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let entry_name = entry.name().to_string();
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

fn frames(d: &Deck) -> Vec<(String, Option<Frame>)> {
    d.slides[0]
        .objects
        .iter()
        .filter_map(|o| match o {
            SlideObject::TextBox { text, body, .. } => Some((text.clone(), body.frame.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_text_boxs_shape_and_paint_survive_a_save() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Deck::new();
    d.slides[0].objects = vec![tile("6,046", 100.0)];
    let path = dir.path().join("tile.pptx");
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    let back = read_pptx(path.to_str().unwrap()).unwrap();
    let got = frames(&back);
    assert_eq!(got.len(), 1, "{got:?}");
    let frame = got[0].1.clone().expect("the tile");
    assert!(matches!(frame.kind, ShapeKind::RoundRect { .. }), "{frame:?}");
    assert_eq!(frame.style.fill, Some(Color(0x09, 0x3B, 0x5C)));
    assert_eq!(frame.style.stroke.map(|s| s.color), Some(Color(0xFF, 0xFF, 0xFF)));
}

#[test]
fn a_groups_shapes_are_placed_and_filled_by_the_group() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Deck::new();
    d.slides[0].objects = vec![tile("Measure 1", 100.0), tile("6,046", 300.0)];
    let path = dir.path().join("group.pptx");
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    let flat = read_pptx(path.to_str().unwrap()).unwrap();
    let rect = |d: &Deck, i: usize| match &d.slides[0].objects[i] {
        SlideObject::TextBox { x, y, w, h, .. } => (*x, *y, *w, *h),
        other => panic!("{other:?}"),
    };
    let (ax, ay, aw, ah) = rect(&flat, 0);
    // Wrap both tiles in a group drawn at half size, 200 units to the right,
    // whose children take its fill.
    // A model unit is 9525 EMU on the default 10in-wide slide.
    let emu = |u: f64| (u * 9_525.0).round() as i64;
    let patched = rewrite(&path, "ppt/slides/slide1.xml", |xml| {
        let start = xml.find("<p:sp>").unwrap();
        let end = xml.rfind("</p:sp>").unwrap() + "</p:sp>".len();
        let children = xml[start..end].replace("<a:solidFill><a:srgbClr val=\"093B5C\"/></a:solidFill>", "<a:grpFill/>");
        let (cx, cy, cw, ch) = (emu(ax), emu(ay), emu(560.0), emu(ah));
        format!(
            "{}<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"90\" name=\"Group\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
             <p:grpSpPr><a:xfrm><a:off x=\"{}\" y=\"{cy}\"/><a:ext cx=\"{}\" cy=\"{}\"/><a:chOff x=\"{cx}\" y=\"{cy}\"/><a:chExt cx=\"{cw}\" cy=\"{ch}\"/></a:xfrm>\
             <a:solidFill><a:srgbClr val=\"C0504D\"/></a:solidFill></p:grpSpPr>{children}</p:grpSp>{}",
            &xml[..start],
            cx + emu(200.0),
            cw / 2,
            ch / 2,
            &xml[end..],
        )
    });
    let grouped = read_pptx(patched.to_str().unwrap()).unwrap();
    let (gx, gy, gw, gh) = rect(&grouped, 0);
    assert!((gx - (ax + 200.0)).abs() < 0.5 && (gy - ay).abs() < 0.5, "the group's offset: {:?}", (gx, gy));
    assert!((gw - aw / 2.0).abs() < 0.5 && (gh - ah / 2.0).abs() < 0.5, "the group's scale: {:?}", (gw, gh));
    let (bx, _, _, _) = rect(&grouped, 1);
    assert!((bx - (ax + 200.0 + 100.0)).abs() < 0.5, "the second tile, 200 units in, at half scale: {bx}");
    for (text, frame) in frames(&grouped) {
        assert_eq!(frame.and_then(|f| f.style.fill), Some(Color(0xC0, 0x50, 0x4D)), "{text}: the group's fill");
    }
}
