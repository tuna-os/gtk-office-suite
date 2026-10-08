// freeform.rs — a custom geometry keeps its paths.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The overpayment dashboards' pie and stacked columns, the People and
// Nature infographics' icons: charts pasted as shapes and drawings made of
// `a:custGeom` paths, nearly 600 shapes across the real decks. The reader
// took each for its bounding rectangle.

use decks_core::engine::freeform::{FreePath, PathCmd};
use decks_core::engine::shape::{Color, ShapeKind, ShapeStyle};
use decks_core::engine::{read_pptx, write_pptx, Deck, SlideObject};

/// A pie wedge, filled, and an open polyline, outlined and not filled.
fn paths() -> Vec<FreePath> {
    vec![
        FreePath {
            w: 1000.0,
            h: 1000.0,
            cmds: vec![PathCmd::Move(500.0, 500.0), PathCmd::Line(1000.0, 500.0), PathCmd::Arc { wr: 500.0, hr: 500.0, start: 0.0, swing: 90.0 }, PathCmd::Close],
            fill: true,
            stroke: true,
        },
        FreePath { w: 1000.0, h: 1000.0, cmds: vec![PathCmd::Move(0.0, 0.0), PathCmd::Line(300.0, 100.0), PathCmd::Line(600.0, 0.0)], fill: false, stroke: true },
    ]
}

fn deck() -> Deck {
    let mut d = Deck::new();
    d.slides[0].objects = vec![SlideObject::Shape {
        kind: ShapeKind::Freeform(paths()),
        x: 100.0,
        y: 100.0,
        w: 200.0,
        h: 200.0,
        rotation: 0.0,
        style: ShapeStyle { fill: Some(Color(0x44, 0x72, 0xC4)), gradient: None, stroke: None },
    }];
    d
}

fn freeform(d: &Deck) -> Vec<FreePath> {
    d.slides[0]
        .objects
        .iter()
        .find_map(|o| match o {
            SlideObject::Shape { kind: ShapeKind::Freeform(p), .. } => Some(p.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no freeform: {:?}", d.slides[0].objects))
}

#[test]
fn a_freeform_survives_a_pptx_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.pptx");
    write_pptx(path.to_str().unwrap(), &deck()).unwrap();
    assert_eq!(freeform(&read_pptx(path.to_str().unwrap()).unwrap()), paths());
}

/// Through an odp the paths come back with the arc as Béziers, over the
/// view box the writer used: the same outline, the same fill and stroke.
#[test]
fn a_freeform_survives_an_odp_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.odp");
    decks_core::odp::write(&deck(), path.to_str().unwrap()).unwrap();
    let back = freeform(&decks_core::odp::read(path.to_str().unwrap()).unwrap());
    assert_eq!(back.len(), 2);
    assert_eq!(back.iter().map(|p| (p.fill, p.stroke)).collect::<Vec<_>>(), [(true, true), (false, true)]);
    let scale = back[0].w / 1000.0;
    assert_eq!(back[0].cmds[0], PathCmd::Move(500.0 * scale, 500.0 * scale));
    assert!(matches!(back[0].cmds[2], PathCmd::Cubic(_, _, _, _, x, y) if (x - 500.0 * scale).abs() < 1.0 && (y - 1000.0 * scale).abs() < 1.0));
    assert_eq!(back[0].cmds.last(), Some(&PathCmd::Close));
}

/// A path of no size of its own is in the shape's EMU: it is drawn over
/// the shape's box, not lost.
#[test]
fn a_path_of_no_size_is_in_the_shapes_units() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("emu.pptx");
    write_pptx(path.to_str().unwrap(), &deck()).unwrap();
    // Rewrite the paths' sizes away.
    let bytes = std::fs::read(&path).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let name = f.name().to_string();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut data).unwrap();
        if name == "ppt/slides/slide1.xml" {
            data = String::from_utf8(data).unwrap().replace(" w=\"1000\" h=\"1000\"", "").into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut out, &data).unwrap();
    }
    std::fs::write(&path, out.finish().unwrap().into_inner()).unwrap();
    let back = freeform(&read_pptx(path.to_str().unwrap()).unwrap());
    // 200 model units of 9525 EMU.
    assert_eq!((back[0].w, back[0].h), (1_905_000.0, 1_905_000.0));
}
