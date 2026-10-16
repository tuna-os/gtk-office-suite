// connectors.rs — a connector (p:cxnSp) is a line on the slide.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// DCK-004, reduced from #1397: the pay centre impact assessment's figure
// tiles are split by white separator rules, each a `p:cxnSp` with the `line`
// preset and the theme's thin line (`a:lnRef idx="1"`). The reader skipped
// connectors, so the rules were not drawn.

use std::io::{Read, Write};

use decks_core::engine::shape::ShapeKind;
use decks_core::engine::{read_pptx, write_pptx, Deck, SlideObject};

fn part(path: &std::path::Path, name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

/// A copy of the package at `path` with `name` replaced by `content`.
fn replace(path: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let out = path.with_extension("patched.pptx");
    let mut w = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let entry_name = entry.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry_name == name {
            bytes = content.as_bytes().to_vec();
        }
        w.start_file(entry_name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(&bytes).unwrap();
    }
    w.finish().unwrap();
    out
}

fn tile(id: u32, x: u64, color: &str) -> String {
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Tile {id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="1600200"/><a:ext cx="1828800" cy="1371600"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="{color}"/></a:solidFill><a:ln><a:noFill/></a:ln></p:spPr></p:sp>"#
    )
}

/// Two navy and teal tiles with PowerPoint's separator between them: the
/// connector, then the second tile, so a connector that took a tile's
/// place in the shape order would give that tile the wrong paint.
fn deck() -> Deck {
    let dir = tempfile::tempdir().unwrap();
    let ours = dir.path().join("ours.pptx");
    write_pptx(ours.to_str().unwrap(), &Deck::new()).unwrap();
    let connector = r#"<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="13" name="Straight Connector 12"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr><a:xfrm><a:off x="762000" y="2286000"/><a:ext cx="1342761" cy="11496"/></a:xfrm><a:prstGeom prst="line"><a:avLst/></a:prstGeom><a:ln><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:ln></p:spPr><p:style><a:lnRef idx="1"><a:srgbClr val="4472C4"/></a:lnRef><a:fillRef idx="0"><a:srgbClr val="4472C4"/></a:fillRef><a:effectRef idx="0"><a:srgbClr val="4472C4"/></a:effectRef><a:fontRef idx="minor"><a:srgbClr val="000000"/></a:fontRef></p:style></p:cxnSp>"#;
    let slide = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{}{connector}{}</p:spTree></p:cSld></p:sld>"#,
        tile(4, 609600, "172A45"),
        tile(5, 3048000, "0E7C7B"),
    );
    let path = replace(&ours, "ppt/slides/slide1.xml", &slide);
    read_pptx(path.to_str().unwrap()).unwrap()
}

fn shapes(d: &Deck) -> Vec<(ShapeKind, f64, f64, f64, f64, decks_core::engine::shape::ShapeStyle)> {
    d.slides[0]
        .objects
        .iter()
        .filter_map(|o| match o {
            SlideObject::Shape { kind, x, y, w, h, style, .. } => Some((kind.clone(), *x, *y, *w, *h, style.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_connector_is_read_as_a_white_line_between_the_tiles() {
    let d = deck();
    let s = shapes(&d);
    assert_eq!(s.len(), 3, "two tiles and the separator, in file order: {s:?}");
    let (kind, x, y, w, h, style) = &s[1];
    assert_eq!(*kind, ShapeKind::Other("line".into()));
    assert!(decks_core::engine::shape::is_line(kind));
    // Placed and sized as the file says, on the model's 960-wide slide:
    // the scale is the one the first tile was read at.
    let k = s[0].1 / 609_600.0;
    assert!((x - 762_000.0 * k).abs() < 0.01 && (w - 1_342_761.0 * k).abs() < 0.01, "{x} {w}");
    assert!((y - 2_286_000.0 * k).abs() < 0.01 && (h - 11_496.0 * k).abs() < 0.01, "{y} {h}");
    let stroke = style.stroke.as_ref().expect("the separator's outline");
    assert_eq!(stroke.color.to_f64(), (1.0, 1.0, 1.0), "white, the a:ln over the lnRef colour");
    assert!(stroke.width > 0.0);
}

#[test]
fn the_tile_after_a_connector_keeps_its_own_paint() {
    let d = deck();
    let s = shapes(&d);
    let fill = |i: usize| s[i].5.fill.map(|c| c.to_f64());
    let navy = (0x17 as f64 / 255.0, 0x2A as f64 / 255.0, 0x45 as f64 / 255.0);
    let teal = (0x0E as f64 / 255.0, 0x7C as f64 / 255.0, 0x7B as f64 / 255.0);
    assert_eq!(fill(0), Some(navy));
    assert_eq!(fill(2), Some(teal), "the second tile's own fill, not the connector's or the first's");
}

#[test]
fn a_connector_survives_our_pptx_as_a_line() {
    let d = deck();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("back.pptx");
    write_pptx(out.to_str().unwrap(), &d).unwrap();
    let xml = part(&out, "ppt/slides/slide1.xml");
    assert!(xml.contains(r#"prst="line""#), "{xml}");
    let back = shapes(&read_pptx(out.to_str().unwrap()).unwrap());
    assert_eq!(back.len(), 3, "{back:?}");
    assert_eq!(back[1].0, ShapeKind::Other("line".into()));
    assert_eq!(back[1].5.stroke.as_ref().map(|s| s.color.to_f64()), Some((1.0, 1.0, 1.0)));
}
