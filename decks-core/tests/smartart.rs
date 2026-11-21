// smartart.rs — a SmartArt graphic reads as the shapes PowerPoint drew.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The CNSC roundtable's "Key Insights" slide is six coloured boxes of
// SmartArt; the reader took the frame for nothing and the slide was a
// title over an empty background. PowerPoint saves the drawing it made
// (`ppt/diagrams/drawingN.xml`); its shapes are read as the slide's.

use decks_core::engine::{read_pptx, write_pptx, Deck, SlideObject};

const FRAME: &str = r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="40" name="Diagram 3"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="914400" y="914400"/><a:ext cx="3657600" cy="1828800"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/diagram"><dgm:relIds xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" r:dm="rId90" r:lo="rId91" r:qs="rId92" r:cs="rId93"/></a:graphicData></a:graphic></p:graphicFrame>"#;

const DRAWING: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<dsp:drawing xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:dsp="http://schemas.microsoft.com/office/drawing/2008/diagram" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><dsp:spTree><dsp:nvGrpSpPr><dsp:cNvPr id="0" name=""/><dsp:cNvGrpSpPr/></dsp:nvGrpSpPr><dsp:grpSpPr/><dsp:sp modelId="{1}"><dsp:nvSpPr><dsp:cNvPr id="0" name=""/><dsp:cNvSpPr/></dsp:nvSpPr><dsp:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1828800" cy="1828800"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="7F3F4F"/></a:solidFill></dsp:spPr><dsp:style><a:lnRef idx="2"><a:scrgbClr r="0" g="0" b="0"/></a:lnRef><a:fillRef idx="1"><a:scrgbClr r="0" g="0" b="0"/></a:fillRef><a:effectRef idx="0"><a:scrgbClr r="0" g="0" b="0"/></a:effectRef><a:fontRef idx="minor"><a:srgbClr val="FFFFFF"/></a:fontRef></dsp:style><dsp:txBody><a:bodyPr anchor="ctr"/><a:lstStyle/><a:p><a:r><a:rPr lang="en-CA" sz="2000"/><a:t>Communication</a:t></a:r></a:p></dsp:txBody><dsp:txXfrm><a:off x="10" y="10"/><a:ext cx="100" cy="100"/></dsp:txXfrm></dsp:sp></dsp:spTree></dsp:drawing>"#;

const DATA: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<dgm:dataModel xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><dgm:ptLst/><dgm:extLst><a:ext uri="http://schemas.microsoft.com/office/drawing/2008/diagram"><dsp:dataModelExt xmlns:dsp="http://schemas.microsoft.com/office/drawing/2008/diagram" relId="rId94" minVer="http://schemas.openxmlformats.org/drawingml/2006/diagram"/></a:ext></dgm:extLst></dgm:dataModel>"#;

const RELS: &str = r#"<Relationship Id="rId90" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/diagramData" Target="../diagrams/data1.xml"/><Relationship Id="rId94" Type="http://schemas.microsoft.com/office/2007/relationships/diagramDrawing" Target="../diagrams/drawing1.xml"/>"#;

/// A one-slide deck with a SmartArt frame and its parts added.
fn with_smartart(path: &std::path::Path) {
    write_pptx(path.to_str().unwrap(), &Deck::new()).unwrap();
    let bytes = std::fs::read(path).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let name = f.name().unwrap().into_owned();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut data).unwrap();
        let text = String::from_utf8_lossy(&data).into_owned();
        if name == "ppt/slides/slide1.xml" {
            data = text.replacen("</p:spTree>", &format!("{FRAME}</p:spTree>"), 1).into_bytes();
        } else if name == "ppt/slides/_rels/slide1.xml.rels" {
            data = text.replacen("</Relationships>", &format!("{RELS}</Relationships>"), 1).into_bytes();
        }
        out.start_file(name, opts).unwrap();
        std::io::Write::write_all(&mut out, &data).unwrap();
    }
    for (name, data) in [("ppt/diagrams/data1.xml", DATA), ("ppt/diagrams/drawing1.xml", DRAWING)] {
        out.start_file(name, opts).unwrap();
        std::io::Write::write_all(&mut out, data.as_bytes()).unwrap();
    }
    std::fs::write(path, out.finish().unwrap().into_inner()).unwrap();
}

#[test]
fn a_smartart_box_reads_as_a_filled_shape_with_its_white_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("smartart.pptx");
    with_smartart(&path);
    let deck = read_pptx(path.to_str().unwrap()).unwrap();
    let boxed = deck.slides[0]
        .objects
        .iter()
        .find_map(|o| match o {
            SlideObject::TextBox { text, x, y, w, h, runs, body, .. } if text.contains("Communication") => Some((*x, *y, *w, *h, runs.clone(), body.clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no diagram text: {:?}", deck.slides[0].objects));
    let (x, y, w, h, runs, body) = boxed;
    // At the frame's place (1 inch in), the box half the frame's width,
    // not the dropped text rectangle's 100 EMU.
    let frame = body.frame.expect("the box's paint under its text");
    assert_eq!(frame.style.fill.map(|c| c.to_hex()), Some("7F3F4F".to_string()));
    assert!(x > 0.0 && y > 0.0, "({x}, {y})");
    assert!((w - h).abs() < 1.0 && w > 50.0, "{w} x {h}");
    assert_eq!(runs[0].style.color.as_deref(), Some("ffffff"), "its style's fontRef white");
}
