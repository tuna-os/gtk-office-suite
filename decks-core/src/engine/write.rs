// write.rs — pptx writing: write_pptx_bytes + shape writers.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of engine.rs (issue #247).

use super::model::*;
use super::notes::notes_slide_xml;

use std::fs::File;
use std::io::{Read, Write};
use zip::write::SimpleFileOptions;
use quick_xml::events::{Event, BytesStart, BytesEnd, BytesDecl, BytesText};
use quick_xml::Writer;
use letters_core::model::{Run, RunStyle};

#[allow(clippy::too_many_arguments)]
fn write_text_box<W: std::io::Write>(
    writer: &mut Writer<W>,
    id: usize,
    name_idx: usize,
    x: f64, y: f64, w: f64, h: f64,
    text: &str,
    runs: &[Run],
) -> Result<(), quick_xml::Error> {
    writer.write_event(Event::Start(BytesStart::new("p:sp")))?;
    
    // nvSpPr
    writer.write_event(Event::Start(BytesStart::new("p:nvSpPr")))?;
    let mut c_nv_pr = BytesStart::new("p:cNvPr");
    c_nv_pr.push_attribute(("id", id.to_string().as_str()));
    c_nv_pr.push_attribute(("name", format!("TextBox {}", name_idx).as_str()));
    writer.write_event(Event::Empty(c_nv_pr))?;
    
    let mut c_nv_sp_pr = BytesStart::new("p:cNvSpPr");
    c_nv_sp_pr.push_attribute(("txBox", "1"));
    writer.write_event(Event::Empty(c_nv_sp_pr))?;
    
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvSpPr")))?;
    
    // spPr
    writer.write_event(Event::Start(BytesStart::new("p:spPr")))?;
    writer.write_event(Event::Start(BytesStart::new("a:xfrm")))?;
    
    let mut off = BytesStart::new("a:off");
    off.push_attribute(("x", ((x * 9525.0) as i64).to_string().as_str()));
    off.push_attribute(("y", ((y * 9525.0) as i64).to_string().as_str()));
    writer.write_event(Event::Empty(off))?;
    
    let mut ext = BytesStart::new("a:ext");
    ext.push_attribute(("cx", ((w * 9525.0) as i64).to_string().as_str()));
    ext.push_attribute(("cy", ((h * 9525.0) as i64).to_string().as_str()));
    writer.write_event(Event::Empty(ext))?;
    
    writer.write_event(Event::End(BytesEnd::new("a:xfrm")))?;
    
    let mut prst_geom = BytesStart::new("a:prstGeom");
    prst_geom.push_attribute(("prst", "rect"));
    writer.write_event(Event::Start(prst_geom))?;
    writer.write_event(Event::Empty(BytesStart::new("a:avLst")))?;
    writer.write_event(Event::End(BytesEnd::new("a:prstGeom")))?;
    
    writer.write_event(Event::End(BytesEnd::new("p:spPr")))?;
    
    // txBody
    writer.write_event(Event::Start(BytesStart::new("p:txBody")))?;
    writer.write_event(Event::Empty(BytesStart::new("a:bodyPr")))?;
    writer.write_event(Event::Empty(BytesStart::new("a:lstStyle")))?;
    
    // Emit styled runs when present (shared Run/RunStyle with Letters);
    // otherwise a single default-styled run with the plain text.
    let plain: Vec<Run>;
    let effective: &[Run] = if runs.is_empty() {
        plain = vec![Run { text: text.to_string(), style: RunStyle::default() }];
        &plain
    } else {
        runs
    };
    writer.write_event(Event::Start(BytesStart::new("a:p")))?;
    for run in effective {
        writer.write_event(Event::Start(BytesStart::new("a:r")))?;
        let mut r_pr = BytesStart::new("a:rPr");
        r_pr.push_attribute(("lang", "en-US"));
        let sz = run.style.font_size_hp.map(|hp| hp as u32 * 50).unwrap_or(1800);
        r_pr.push_attribute(("sz", sz.to_string().as_str()));
        if run.style.bold { r_pr.push_attribute(("b", "1")); }
        if run.style.italic { r_pr.push_attribute(("i", "1")); }
        if run.style.underline { r_pr.push_attribute(("u", "sng")); }
        if run.style.strikethrough { r_pr.push_attribute(("strike", "sngStrike")); }
        if let Some(color) = &run.style.color {
            writer.write_event(Event::Start(r_pr))?;
            writer.write_event(Event::Start(BytesStart::new("a:solidFill")))?;
            let mut clr = BytesStart::new("a:srgbClr");
            clr.push_attribute(("val", color.to_uppercase().as_str()));
            writer.write_event(Event::Empty(clr))?;
            writer.write_event(Event::End(BytesEnd::new("a:solidFill")))?;
            writer.write_event(Event::End(BytesEnd::new("a:rPr")))?;
        } else {
            writer.write_event(Event::Empty(r_pr))?;
        }
        writer.write_event(Event::Start(BytesStart::new("a:t")))?;
        let escaped = quick_xml::escape::escape(run.text.as_str());
        writer.write_event(Event::Text(BytesText::new(&escaped)))?;
        writer.write_event(Event::End(BytesEnd::new("a:t")))?;
        writer.write_event(Event::End(BytesEnd::new("a:r")))?;
    }
    writer.write_event(Event::End(BytesEnd::new("a:p")))?;
    writer.write_event(Event::End(BytesEnd::new("p:txBody")))?;
    
    writer.write_event(Event::End(BytesEnd::new("p:sp")))?;
    Ok(())
}

fn write_rect<W: std::io::Write>(
    writer: &mut Writer<W>,
    id: usize,
    name_idx: usize,
    x: f64, y: f64, w: f64, h: f64,
) -> Result<(), quick_xml::Error> {
    writer.write_event(Event::Start(BytesStart::new("p:sp")))?;
    
    // nvSpPr
    writer.write_event(Event::Start(BytesStart::new("p:nvSpPr")))?;
    let mut c_nv_pr = BytesStart::new("p:cNvPr");
    c_nv_pr.push_attribute(("id", id.to_string().as_str()));
    c_nv_pr.push_attribute(("name", format!("Rectangle {}", name_idx).as_str()));
    writer.write_event(Event::Empty(c_nv_pr))?;
    writer.write_event(Event::Empty(BytesStart::new("p:cNvSpPr")))?;
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvSpPr")))?;
    
    // spPr
    writer.write_event(Event::Start(BytesStart::new("p:spPr")))?;
    writer.write_event(Event::Start(BytesStart::new("a:xfrm")))?;
    
    let mut off = BytesStart::new("a:off");
    off.push_attribute(("x", ((x * 9525.0) as i64).to_string().as_str()));
    off.push_attribute(("y", ((y * 9525.0) as i64).to_string().as_str()));
    writer.write_event(Event::Empty(off))?;
    
    let mut ext = BytesStart::new("a:ext");
    ext.push_attribute(("cx", ((w * 9525.0) as i64).to_string().as_str()));
    ext.push_attribute(("cy", ((h * 9525.0) as i64).to_string().as_str()));
    writer.write_event(Event::Empty(ext))?;
    
    writer.write_event(Event::End(BytesEnd::new("a:xfrm")))?;
    
    let mut prst_geom = BytesStart::new("a:prstGeom");
    prst_geom.push_attribute(("prst", "rect"));
    writer.write_event(Event::Start(prst_geom))?;
    writer.write_event(Event::Empty(BytesStart::new("a:avLst")))?;
    writer.write_event(Event::End(BytesEnd::new("a:prstGeom")))?;
    
    writer.write_event(Event::Start(BytesStart::new("a:solidFill")))?;
    let mut srgb = BytesStart::new("a:srgbClr");
    srgb.push_attribute(("val", "4A90E2"));
    writer.write_event(Event::Empty(srgb))?;
    writer.write_event(Event::End(BytesEnd::new("a:solidFill")))?;
    
    writer.write_event(Event::End(BytesEnd::new("p:spPr")))?;
    
    writer.write_event(Event::End(BytesEnd::new("p:sp")))?;
    Ok(())
}

fn write_circle<W: std::io::Write>(
    writer: &mut Writer<W>,
    id: usize,
    name_idx: usize,
    x: f64, y: f64, r: f64,
) -> Result<(), quick_xml::Error> {
    writer.write_event(Event::Start(BytesStart::new("p:sp")))?;
    
    // nvSpPr
    writer.write_event(Event::Start(BytesStart::new("p:nvSpPr")))?;
    let mut c_nv_pr = BytesStart::new("p:cNvPr");
    c_nv_pr.push_attribute(("id", id.to_string().as_str()));
    c_nv_pr.push_attribute(("name", format!("Circle {}", name_idx).as_str()));
    writer.write_event(Event::Empty(c_nv_pr))?;
    writer.write_event(Event::Empty(BytesStart::new("p:cNvSpPr")))?;
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvSpPr")))?;
    
    // spPr
    writer.write_event(Event::Start(BytesStart::new("p:spPr")))?;
    writer.write_event(Event::Start(BytesStart::new("a:xfrm")))?;
    
    let mut off = BytesStart::new("a:off");
    off.push_attribute(("x", (((x - r) * 9525.0) as i64).to_string().as_str()));
    off.push_attribute(("y", (((y - r) * 9525.0) as i64).to_string().as_str()));
    writer.write_event(Event::Empty(off))?;
    
    let mut ext = BytesStart::new("a:ext");
    ext.push_attribute(("cx", ((2.0 * r * 9525.0) as i64).to_string().as_str()));
    ext.push_attribute(("cy", ((2.0 * r * 9525.0) as i64).to_string().as_str()));
    writer.write_event(Event::Empty(ext))?;
    
    writer.write_event(Event::End(BytesEnd::new("a:xfrm")))?;
    
    let mut prst_geom = BytesStart::new("a:prstGeom");
    prst_geom.push_attribute(("prst", "ellipse"));
    writer.write_event(Event::Start(prst_geom))?;
    writer.write_event(Event::Empty(BytesStart::new("a:avLst")))?;
    writer.write_event(Event::End(BytesEnd::new("a:prstGeom")))?;
    
    writer.write_event(Event::Start(BytesStart::new("a:solidFill")))?;
    let mut srgb = BytesStart::new("a:srgbClr");
    srgb.push_attribute(("val", "E04F32"));
    writer.write_event(Event::Empty(srgb))?;
    writer.write_event(Event::End(BytesEnd::new("a:solidFill")))?;
    
    writer.write_event(Event::End(BytesEnd::new("p:spPr")))?;
    
    writer.write_event(Event::End(BytesEnd::new("p:sp")))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_image<W: std::io::Write>(
    writer: &mut Writer<W>,
    id: usize,
    name_idx: usize,
    rel_id: &str,
    x: f64, y: f64, w: f64, h: f64,
) -> Result<(), quick_xml::Error> {
    writer.write_event(Event::Start(BytesStart::new("p:pic")))?;
    
    // nvPicPr
    writer.write_event(Event::Start(BytesStart::new("p:nvPicPr")))?;
    let mut c_nv_pr = BytesStart::new("p:cNvPr");
    c_nv_pr.push_attribute(("id", id.to_string().as_str()));
    c_nv_pr.push_attribute(("name", format!("Image {}", name_idx).as_str()));
    writer.write_event(Event::Empty(c_nv_pr))?;
    writer.write_event(Event::Empty(BytesStart::new("p:cNvPicPr")))?;
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvPicPr")))?;
    
    // blipFill
    writer.write_event(Event::Start(BytesStart::new("p:blipFill")))?;
    let mut blip = BytesStart::new("a:blip");
    blip.push_attribute(("r:embed", rel_id));
    writer.write_event(Event::Empty(blip))?;
    writer.write_event(Event::Start(BytesStart::new("a:stretch")))?;
    writer.write_event(Event::Empty(BytesStart::new("a:fillRect")))?;
    writer.write_event(Event::End(BytesEnd::new("a:stretch")))?;
    writer.write_event(Event::End(BytesEnd::new("p:blipFill")))?;
    
    // spPr
    writer.write_event(Event::Start(BytesStart::new("p:spPr")))?;
    writer.write_event(Event::Start(BytesStart::new("a:xfrm")))?;
    
    let mut off = BytesStart::new("a:off");
    off.push_attribute(("x", ((x * 9525.0) as i64).to_string().as_str()));
    off.push_attribute(("y", ((y * 9525.0) as i64).to_string().as_str()));
    writer.write_event(Event::Empty(off))?;
    
    let mut ext = BytesStart::new("a:ext");
    ext.push_attribute(("cx", ((w * 9525.0) as i64).to_string().as_str()));
    ext.push_attribute(("cy", ((h * 9525.0) as i64).to_string().as_str()));
    writer.write_event(Event::Empty(ext))?;
    
    writer.write_event(Event::End(BytesEnd::new("a:xfrm")))?;
    
    let mut prst_geom = BytesStart::new("a:prstGeom");
    prst_geom.push_attribute(("prst", "rect"));
    writer.write_event(Event::Start(prst_geom))?;
    writer.write_event(Event::Empty(BytesStart::new("a:avLst")))?;
    writer.write_event(Event::End(BytesEnd::new("a:prstGeom")))?;
    
    writer.write_event(Event::End(BytesEnd::new("p:spPr")))?;
    
    writer.write_event(Event::End(BytesEnd::new("p:pic")))?;
    Ok(())
}

pub fn write_pptx(path: &str, deck: &Deck) -> Result<(), String> {
    let bytes = write_pptx_bytes(deck)?;
    suite_common_core::atomic_save::atomic_write_bytes(std::path::Path::new(path), &bytes)
}

/// Render the deck to an in-memory .pptx buffer without touching disk —
/// shared by the real save path (above) and autosave snapshots.
pub fn write_pptx_bytes(deck: &Deck) -> Result<Vec<u8>, String> {
    // Built fully in memory, then placed atomically — see
    // suite_common_core::atomic_save and odp::write for why.
    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);

    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    // Track images to add to ppt/media/
    let mut images_to_add = Vec::new();

    // 1. Write [Content_Types].xml
    let mut content_types = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n\
           <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n\
           <Default Extension=\"xml\" ContentType=\"application/xml\"/>\n\
           <Default Extension=\"png\" ContentType=\"image/png\"/>\n\
           <Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\n\
           <Default Extension=\"jpg\" ContentType=\"image/jpeg\"/>\n\
           <Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>\n"
    );
    for i in 0..deck.slides.len() {
        content_types.push_str(&format!(
            "  <Override PartName=\"/ppt/slides/slide{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>\n",
            i + 1
        ));
        if !deck.slides[i].notes.is_empty() {
            content_types.push_str(&format!(
                "  <Override PartName=\"/ppt/notesSlides/notesSlide{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml\"/>\n",
                i + 1
            ));
        }
    }
    content_types.push_str("</Types>");
    zip.start_file("[Content_Types].xml", options).map_err(|e| e.to_string())?;
    zip.write_all(content_types.as_bytes()).map_err(|e| e.to_string())?;

    // 2. Write _rels/.rels
    let rels = 
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n\
           <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"ppt/presentation.xml\"/>\n\
         </Relationships>";
    zip.start_file("_rels/.rels", options).map_err(|e| e.to_string())?;
    zip.write_all(rels.as_bytes()).map_err(|e| e.to_string())?;

    // 3. Write ppt/presentation.xml
    let mut presentation = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <p:presentation xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"\n\
                         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"\n\
                         xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\n\
           <p:sldIdLst>\n"
    );
    for i in 0..deck.slides.len() {
        presentation.push_str(&format!(
            "    <p:sldId id=\"{}\" r:id=\"rId{}\"/>\n",
            256 + i,
            i + 1
        ));
    }
    presentation.push_str(
        "  </p:sldIdLst>\n\
           <p:sldSz cx=\"9144000\" cy=\"5143500\"/>\n\
           <p:notesSz cx=\"6858000\" cy=\"9144000\"/>\n\
         </p:presentation>"
    );
    zip.start_file("ppt/presentation.xml", options).map_err(|e| e.to_string())?;
    zip.write_all(presentation.as_bytes()).map_err(|e| e.to_string())?;

    // 4. Write ppt/_rels/presentation.xml.rels
    let mut pres_rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n"
    );
    for i in 0..deck.slides.len() {
        pres_rels.push_str(&format!(
            "  <Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{}.xml\"/>\n",
            format_args!("rId{}", i + 1),
            i + 1
        ));
    }
    pres_rels.push_str("</Relationships>");
    zip.start_file("ppt/_rels/presentation.xml.rels", options).map_err(|e| e.to_string())?;
    zip.write_all(pres_rels.as_bytes()).map_err(|e| e.to_string())?;

    // 5. Write each slide using quick-xml Writer
    for (i, slide) in deck.slides.iter().enumerate() {
        let mut slide_data = Vec::new();
        let mut slide_rels = Vec::new();
        {
            let mut writer = Writer::new(std::io::Cursor::new(&mut slide_data));
            
            // Write declaration
            writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), Some("yes")))).map_err(|e| e.to_string())?;

            // Open p:sld
            let mut sld = BytesStart::new("p:sld");
            sld.push_attribute(("xmlns:a", "http://schemas.openxmlformats.org/drawingml/2006/main"));
            sld.push_attribute(("xmlns:r", "http://schemas.openxmlformats.org/officeDocument/2006/relationships"));
            sld.push_attribute(("xmlns:p", "http://schemas.openxmlformats.org/presentationml/2006/main"));
            writer.write_event(Event::Start(sld)).map_err(|e| e.to_string())?;

            writer.write_event(Event::Start(BytesStart::new("p:cSld"))).map_err(|e| e.to_string())?;

            // Slide background (only when it differs from the default
            // white — Impress preserves an explicit p:bg).
            let bg = slide.background.trim_start_matches('#');
            if !bg.eq_ignore_ascii_case("ffffff") && bg.len() == 6 {
                writer.write_event(Event::Start(BytesStart::new("p:bg"))).map_err(|e| e.to_string())?;
                writer.write_event(Event::Start(BytesStart::new("p:bgPr"))).map_err(|e| e.to_string())?;
                writer.write_event(Event::Start(BytesStart::new("a:solidFill"))).map_err(|e| e.to_string())?;
                let mut clr = BytesStart::new("a:srgbClr");
                clr.push_attribute(("val", bg.to_uppercase().as_str()));
                writer.write_event(Event::Empty(clr)).map_err(|e| e.to_string())?;
                writer.write_event(Event::End(BytesEnd::new("a:solidFill"))).map_err(|e| e.to_string())?;
                writer.write_event(Event::Empty(BytesStart::new("a:effectLst"))).map_err(|e| e.to_string())?;
                writer.write_event(Event::End(BytesEnd::new("p:bgPr"))).map_err(|e| e.to_string())?;
                writer.write_event(Event::End(BytesEnd::new("p:bg"))).map_err(|e| e.to_string())?;
            }

            writer.write_event(Event::Start(BytesStart::new("p:spTree"))).map_err(|e| e.to_string())?;

            // Group properties
            writer.write_event(Event::Start(BytesStart::new("p:nvGrpSpPr"))).map_err(|e| e.to_string())?;
            let mut c_nv_pr = BytesStart::new("p:cNvPr");
            c_nv_pr.push_attribute(("id", "1"));
            c_nv_pr.push_attribute(("name", ""));
            writer.write_event(Event::Empty(c_nv_pr)).map_err(|e| e.to_string())?;
            writer.write_event(Event::Empty(BytesStart::new("p:cNvGrpSpPr"))).map_err(|e| e.to_string())?;
            writer.write_event(Event::Empty(BytesStart::new("p:nvPr"))).map_err(|e| e.to_string())?;
            writer.write_event(Event::End(BytesEnd::new("p:nvGrpSpPr"))).map_err(|e| e.to_string())?;

            writer.write_event(Event::Start(BytesStart::new("p:grpSpPr"))).map_err(|e| e.to_string())?;
            writer.write_event(Event::Start(BytesStart::new("a:xfrm"))).map_err(|e| e.to_string())?;
            
            let mut off = BytesStart::new("a:off");
            off.push_attribute(("x", "0"));
            off.push_attribute(("y", "0"));
            writer.write_event(Event::Empty(off)).map_err(|e| e.to_string())?;
            
            let mut ext = BytesStart::new("a:ext");
            ext.push_attribute(("cx", "0"));
            ext.push_attribute(("cy", "0"));
            writer.write_event(Event::Empty(ext)).map_err(|e| e.to_string())?;
            
            let mut ch_off = BytesStart::new("a:chOff");
            ch_off.push_attribute(("x", "0"));
            ch_off.push_attribute(("y", "0"));
            writer.write_event(Event::Empty(ch_off)).map_err(|e| e.to_string())?;
            
            let mut ch_ext = BytesStart::new("a:chExt");
            ch_ext.push_attribute(("cx", "0"));
            ch_ext.push_attribute(("cy", "0"));
            writer.write_event(Event::Empty(ch_ext)).map_err(|e| e.to_string())?;
            
            writer.write_event(Event::End(BytesEnd::new("a:xfrm"))).map_err(|e| e.to_string())?;
            writer.write_event(Event::End(BytesEnd::new("p:grpSpPr"))).map_err(|e| e.to_string())?;

            for (j, obj) in slide.objects.iter().enumerate() {
                let id = 2 + j;
                match obj {
                    SlideObject::TextBox { text, x, y, w, h, runs, .. } => {
                        write_text_box(&mut writer, id, j + 1, *x, *y, *w, *h, text, runs).map_err(|e| e.to_string())?;
                    }
                    SlideObject::Rect { x, y, w, h, .. } => {
                        write_rect(&mut writer, id, j + 1, *x, *y, *w, *h).map_err(|e| e.to_string())?;
                    }
                    SlideObject::Circle { x, y, r, .. } => {
                        write_circle(&mut writer, id, j + 1, *x, *y, *r).map_err(|e| e.to_string())?;
                    }
                    SlideObject::Image { path, x, y, w, h, .. } => {
                        let img_idx = images_to_add.len() + 1;
                        images_to_add.push(path.clone());

                        let rel_id = format!("rId{}", slide_rels.len() + 1);
                        slide_rels.push((rel_id.clone(), format!("../media/image{}.png", img_idx)));

                        write_image(&mut writer, id, j + 1, &rel_id, *x, *y, *w, *h).map_err(|e| e.to_string())?;
                    }
                }
            }

            writer.write_event(Event::End(BytesEnd::new("p:spTree"))).map_err(|e| e.to_string())?;
            writer.write_event(Event::End(BytesEnd::new("p:cSld"))).map_err(|e| e.to_string())?;
            writer.write_event(Event::End(BytesEnd::new("p:sld"))).map_err(|e| e.to_string())?;
        }

        let slide_path = format!("ppt/slides/slide{}.xml", i + 1);
        zip.start_file(&slide_path, options).map_err(|e| e.to_string())?;
        zip.write_all(&slide_data).map_err(|e| e.to_string())?;

        // Write slide relationships (images and/or speaker notes)
        let has_notes = !slide.notes.is_empty();
        if !slide_rels.is_empty() || has_notes {
            let mut rels_str = String::from(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
                 <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n"
            );
            let mut max_rel = 0usize;
            for (rel_id, target) in &slide_rels {
                if let Some(n) = rel_id.strip_prefix("rId").and_then(|n| n.parse::<usize>().ok()) {
                    max_rel = max_rel.max(n);
                }
                rels_str.push_str(&format!(
                    "  <Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"{}\"/>\n",
                    rel_id, target
                ));
            }
            if has_notes {
                rels_str.push_str(&format!(
                    "  <Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide\" Target=\"../notesSlides/notesSlide{}.xml\"/>\n",
                    max_rel + 1, i + 1
                ));
            }
            rels_str.push_str("</Relationships>");

            let rels_path = format!("ppt/slides/_rels/slide{}.xml.rels", i + 1);
            zip.start_file(&rels_path, options).map_err(|e| e.to_string())?;
            zip.write_all(rels_str.as_bytes()).map_err(|e| e.to_string())?;
        }

        if has_notes {
            let notes_path = format!("ppt/notesSlides/notesSlide{}.xml", i + 1);
            zip.start_file(&notes_path, options).map_err(|e| e.to_string())?;
            zip.write_all(notes_slide_xml(&slide.notes).as_bytes()).map_err(|e| e.to_string())?;
        }
    }

    // 6. Write image media files in ppt/media/
    for (idx, img_path) in images_to_add.iter().enumerate() {
        let zip_img_path = format!("ppt/media/image{}.png", idx + 1);
        let mut img_file = File::open(img_path)
            .map_err(|e| format!("Cannot open image {}: {}", img_path, e))?;
        let mut buffer = Vec::new();
        img_file.read_to_end(&mut buffer).map_err(|e| e.to_string())?;

        zip.start_file(&zip_img_path, options).map_err(|e| e.to_string())?;
        zip.write_all(&buffer).map_err(|e| e.to_string())?;
    }

    zip.finish().map_err(|e| e.to_string()).map(|c| c.into_inner())
}
