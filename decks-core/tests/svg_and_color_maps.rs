// svg_and_color_maps.rs — SVG-only pictures and a layout's own colour map.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The CCRM deck's title slide artwork and its "Questions" icon are saved
// only as SVG (`asvg:svgBlip`, no PNG on the blip), and its section layout
// overrides the master's colour map (bg1 = dk1, tx1 = lt1) to set white
// text on navy. Decks drew neither picture, and the section slides came
// out white with navy text.

use std::io::{Read, Write};

use decks_core::engine::{read_pptx, write_pptx_bytes, Deck, SlideObject};

const SVG: &[u8] = br##"<?xml version="1.0" encoding="UTF-8"?><svg xmlns="http://www.w3.org/2000/svg" width="96" height="96" viewBox="0 0 96 96"><rect width="96" height="96" fill="#00c0f0"/></svg>"##;

fn rewrite(bytes: &[u8], edit: impl Fn(&str, String) -> String) -> Vec<u8> {
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).unwrap();
        let name = f.name().unwrap().to_string();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        if name.ends_with(".xml") {
            data = edit(&name, String::from_utf8(data).unwrap()).into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn read(bytes: &[u8]) -> Deck {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("d.pptx");
    std::fs::write(&path, bytes).unwrap();
    read_pptx(path.to_str().unwrap()).unwrap()
}

fn saved_with_picture() -> (tempfile::TempDir, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("icon");
    std::fs::write(&src, SVG).unwrap();
    let mut deck = Deck::new();
    deck.slides[0].objects.push(SlideObject::Image {
        path: src.to_str().unwrap().into(),
        x: 10.0,
        y: 10.0,
        w: 100.0,
        h: 100.0,
        rotation: 0.0,
        crop: Default::default(),
    });
    let bytes = write_pptx_bytes(&deck).unwrap();
    (dir, bytes)
}

#[test]
fn an_svg_picture_is_saved_as_svg_and_an_svg_only_blip_is_read() {
    let (_dir, bytes) = saved_with_picture();
    let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
    assert!(zip.file_names().filter_map(Result::ok).any(|n| n == "ppt/media/image1.svg"), "SVG bytes are named .svg, not .png");
    // As Office 2016+ saves an SVG-only graphic: no r:embed on the blip,
    // the part named on asvg:svgBlip in its extensions.
    let svg_only = rewrite(&bytes, |name, xml| {
        if !name.ends_with("slides/slide1.xml") {
            return xml;
        }
        xml.replace(
            r#"<a:blip r:embed="rId1"/>"#,
            r#"<a:blip><a:extLst><a:ext uri="{96DAC541-7B7A-43D3-8B79-37D633B846F1}"><asvg:svgBlip xmlns:asvg="http://schemas.microsoft.com/office/drawing/2016/SVG/main" r:embed="rId1"/></a:ext></a:extLst></a:blip>"#,
        )
    });
    let deck = read(&svg_only);
    let path = deck.slides[0]
        .objects
        .iter()
        .find_map(|o| match o {
            SlideObject::Image { path, .. } => Some(path.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the SVG-only picture was dropped: {:?}", deck.slides[0].objects));
    assert!(std::fs::read(path).unwrap().starts_with(b"<?xml"), "the picture is the SVG part");
}

#[test]
fn a_layouts_colour_map_recolours_its_background_and_its_slides() {
    let (_dir, bytes) = saved_with_picture();
    let section = rewrite(&bytes, |name, xml| {
        if name.ends_with("slideLayouts/slideLayout1.xml") {
            xml.replace(r#"<a:srgbClr val="FFFFFF"/>"#, r#"<a:schemeClr val="bg1"/>"#).replace(
                "<a:masterClrMapping/>",
                r#"<a:overrideClrMapping bg1="dk1" tx1="lt1" bg2="dk2" tx2="lt2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>"#,
            )
        } else if name.ends_with("slides/slide1.xml") {
            // A shape filled with bg1 on a slide that keeps its layout's map.
            xml.replace(
                "</p:spTree>",
                r#"<p:sp><p:nvSpPr><p:cNvPr id="9" name="Panel"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:schemeClr val="bg1"/></a:solidFill></p:spPr></p:sp></p:spTree>"#,
            )
        } else {
            xml
        }
    });
    let deck = read(&section);
    let dk1 = deck.masters[0].layouts[0].background.clone();
    assert!(dk1.as_deref().is_some_and(|c| !c.eq_ignore_ascii_case("ffffff")), "bg1 is dk1 on this layout: {dk1:?}");
    let fill = deck.slides[0].objects.iter().find_map(|o| match o {
        SlideObject::Shape { style, .. } => Some(style.fill),
        _ => None,
    });
    assert_eq!(
        fill.flatten().map(|c| format!("{:02x}{:02x}{:02x}", c.0, c.1, c.2)),
        dk1.map(|c| c.trim_start_matches('#').to_lowercase()),
        "a slide shape's bg1 follows its layout's map"
    );
}
