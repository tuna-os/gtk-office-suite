// parse.rs — pptx reading: read_pptx + xml parse helpers.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of engine.rs (issue #247).

pub(crate) fn unescape_text(t: &BytesText) -> String {
    let decoded = t.decode().unwrap_or_default();
    quick_xml::escape::unescape(&decoded)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| decoded.into_owned())
}

pub(crate) fn resolve_general_ref(r: &BytesRef) -> String {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return c.to_string();
    }
    let name = r.decode().unwrap_or_default();
    quick_xml::escape::resolve_predefined_entity(&name)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("&{name};"))
}

use super::model::*;
use super::notes::{extract_notes_text, parse_run_style};

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use quick_xml::events::{Event, BytesStart, BytesRef, BytesText};
use quick_xml::Reader;
use letters_core::model::{Run, RunStyle};

fn parse_coords<B: std::io::BufRead>(
    e: &BytesStart,
    reader: &Reader<B>,
    k1: &[u8],
    k2: &[u8]
) -> (Option<f64>, Option<f64>) {
    let mut v1 = None;
    let mut v2 = None;
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == k1 {
            if let Ok(val) = attr.decode_and_unescape_value(reader.decoder()) {
                v1 = val.parse::<f64>().ok();
            }
        } else if attr.key.as_ref() == k2 {
            if let Ok(val) = attr.decode_and_unescape_value(reader.decoder()) {
                v2 = val.parse::<f64>().ok();
            }
        }
    }
    (v1, v2)
}

fn parse_blip_embed<B: std::io::BufRead>(e: &BytesStart, reader: &Reader<B>) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"r:embed" {
            if let Ok(val) = attr.decode_and_unescape_value(reader.decoder()) {
                return Some(val.into_owned());
            }
        }
    }
    None
}

fn parse_prst_geom<B: std::io::BufRead>(e: &BytesStart, reader: &Reader<B>) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"prst" {
            if let Ok(val) = attr.decode_and_unescape_value(reader.decoder()) {
                return Some(val.into_owned());
            }
        }
    }
    None
}

fn is_tx_box_attr<B: std::io::BufRead>(e: &BytesStart, reader: &Reader<B>) -> bool {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"txBox" {
            if let Ok(val) = attr.decode_and_unescape_value(reader.decoder()) {
                return val.as_ref() == "1";
            }
        }
    }
    false
}

struct PendingShape {
    is_tx_box: bool,
    /// A p:txBody element was seen. Impress adds an (empty) txBody to
    /// every shape, so this alone does not make it a text box.
    has_tx_body: bool,
    text: Vec<String>,
    runs: Vec<Run>,
    cur_style: RunStyle,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
    prst: Option<String>,
}

struct PendingPicture {
    embed_id: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
}

pub fn read_pptx(path: &str) -> Result<Deck, String> {
    let file = File::open(path).map_err(|e| format!("Cannot open file: {}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Invalid zip archive: {}", e))?;

    // 1. Read presentation.xml to count slides and get their rIds
    let mut presentation_xml = String::new();
    if let Ok(mut file) = archive.by_name("ppt/presentation.xml") {
        file.read_to_string(&mut presentation_xml).unwrap_or(0);
    } else {
        return Err("Not a valid PPTX (missing ppt/presentation.xml)".into());
    }

    // 2. Read presentation.xml.rels to resolve slide relationship IDs to paths
    let mut rels_xml = String::new();
    if let Ok(mut file) = archive.by_name("ppt/_rels/presentation.xml.rels") {
        file.read_to_string(&mut rels_xml).unwrap_or(0);
    } else {
        return Err("Not a valid PPTX (missing ppt/_rels/presentation.xml.rels)".into());
    }

    // Scan relationships using quick-xml to map rId -> target
    let mut slide_paths = std::collections::BTreeMap::new();
    {
        let mut reader = Reader::from_str(&rels_xml);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let name = e.name();
                    if name.as_ref() == b"Relationship" {
                        let mut id = None;
                        let mut target = None;
                        let mut is_slide = false;
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"Id" => {
                                    id = attr.decode_and_unescape_value(reader.decoder()).ok().map(|v| v.into_owned());
                                }
                                b"Target" => {
                                    target = attr.decode_and_unescape_value(reader.decoder()).ok().map(|v| v.into_owned());
                                }
                                b"Type" => {
                                    if let Ok(v) = attr.decode_and_unescape_value(reader.decoder()) {
                                        if v.contains("relationships/slide") {
                                            is_slide = true;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        if is_slide {
                            if let (Some(id_val), Some(target_val)) = (id, target) {
                                slide_paths.insert(id_val, target_val);
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(format!("XML parsing error in presentation.xml.rels: {}", e)),
                _ => {}
            }
            buf.clear();
        }
    }

    // Scan slide ID list in presentation.xml using quick-xml to get their order
    let mut ordered_slide_rids = Vec::new();
    {
        let mut reader = Reader::from_str(&presentation_xml);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let name = e.name();
                    if name.as_ref() == b"p:sldId" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"r:id" {
                                if let Ok(val) = attr.decode_and_unescape_value(reader.decoder()) {
                                    ordered_slide_rids.push(val.into_owned());
                                }
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(format!("XML parsing error in presentation.xml: {}", e)),
                _ => {}
            }
            buf.clear();
        }
    }

    let mut slides = Vec::new();
    // Layout part path per slide (slide → layout → master mapping is
    // resolved after the slide loop, when `archive` is free again).
    let mut slide_layout_paths: Vec<Option<String>> = Vec::new();

    // 3. Parse each slide XML file
    for (slide_index, r_id) in ordered_slide_rids.iter().enumerate() {
        let target_path = match slide_paths.get(r_id) {
            Some(t) => {
                if t.starts_with('/') {
                    t.trim_start_matches('/').to_string()
                } else {
                    format!("ppt/{}", t)
                }
            }
            None => format!("ppt/slides/slide{}.xml", slide_index + 1),
        };

        let mut slide_xml = String::new();
        if let Ok(mut file) = archive.by_name(&target_path) {
            file.read_to_string(&mut slide_xml).unwrap_or(0);
        } else {
            continue;
        }

        // Check if there's a slide relationship file (for images)
        let slide_dir = Path::new(&target_path).parent().unwrap_or(Path::new("ppt/slides"));
        let slide_filename = Path::new(&target_path).file_name().unwrap_or_default().to_string_lossy();
        let slide_rels_path = format!("{}/_rels/{}.rels", slide_dir.to_string_lossy(), slide_filename);
        
        let mut slide_rels_xml = String::new();
        if let Ok(mut file) = archive.by_name(&slide_rels_path) {
            file.read_to_string(&mut slide_rels_xml).unwrap_or(0);
        }

        let mut slide_image_rels = std::collections::HashMap::new();
        if !slide_rels_xml.is_empty() {
            let mut reader = Reader::from_str(&slide_rels_xml);
            reader.config_mut().trim_text(true);
            let mut buf = Vec::new();
            loop {
                match reader.read_event_into(&mut buf) {
                    Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                        let name = e.name();
                        if name.as_ref() == b"Relationship" {
                            let mut id = None;
                            let mut target = None;
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"Id" {
                                    id = attr.decode_and_unescape_value(reader.decoder()).ok().map(|v| v.into_owned());
                                } else if attr.key.as_ref() == b"Target" {
                                    target = attr.decode_and_unescape_value(reader.decoder()).ok().map(|v| v.into_owned());
                                }
                            }
                            if let (Some(id_val), Some(target_val)) = (id, target) {
                                slide_image_rels.insert(id_val, target_val);
                            }
                        }
                    }
                    Ok(Event::Eof) => break,
                    _ => {}
                }
                buf.clear();
            }
        }

        let mut objects = Vec::new();

        // Parse slide XML using quick-xml event reader
        let mut background = String::from("#ffffff");
        {
            let mut reader = Reader::from_str(&slide_xml);
            reader.config_mut().trim_text(true);
            let mut buf = Vec::new();

            let mut current_shape: Option<PendingShape> = None;
            let mut current_picture: Option<PendingPicture> = None;
            let mut in_text_element = false;
            let mut in_bg = false;
            let mut in_rpr = false;

            loop {
                match reader.read_event_into(&mut buf) {
                    Ok(Event::Start(ref e)) => {
                        let name = e.name();
                        match name.as_ref() {
                            b"p:bg" => in_bg = true,
                            b"p:sp" => {
                                current_shape = Some(PendingShape {
                                    is_tx_box: false,
                                    has_tx_body: false,
                                    text: Vec::new(),
                                    runs: Vec::new(),
                                    cur_style: RunStyle::default(),
                                    x: None,
                                    y: None,
                                    w: None,
                                    h: None,
                                    prst: None,
                                });
                            }
                            b"p:pic" => {
                                current_picture = Some(PendingPicture {
                                    embed_id: None,
                                    x: None,
                                    y: None,
                                    w: None,
                                    h: None,
                                });
                            }
                            b"a:off" => {
                                let (x, y) = parse_coords(e, &reader, b"x", b"y");
                                if let Some(shape) = current_shape.as_mut() {
                                    if x.is_some() { shape.x = x; }
                                    if y.is_some() { shape.y = y; }
                                } else if let Some(pic) = current_picture.as_mut() {
                                    if x.is_some() { pic.x = x; }
                                    if y.is_some() { pic.y = y; }
                                }
                            }
                            b"a:ext" => {
                                let (w, h) = parse_coords(e, &reader, b"cx", b"cy");
                                if let Some(shape) = current_shape.as_mut() {
                                    if w.is_some() { shape.w = w; }
                                    if h.is_some() { shape.h = h; }
                                } else if let Some(pic) = current_picture.as_mut() {
                                    if w.is_some() { pic.w = w; }
                                    if h.is_some() { pic.h = h; }
                                }
                            }
                            b"a:prstGeom" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    if let Some(prst) = parse_prst_geom(e, &reader) {
                                        shape.prst = Some(prst);
                                    }
                                }
                            }
                            b"p:cNvSpPr" => {
                                if is_tx_box_attr(e, &reader) {
                                    if let Some(shape) = current_shape.as_mut() {
                                        shape.is_tx_box = true;
                                    }
                                }
                            }
                            b"p:txBody" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.has_tx_body = true;
                                }
                            }
                            b"a:blip" => {
                                if let Some(pic) = current_picture.as_mut() {
                                    if let Some(embed) = parse_blip_embed(e, &reader) {
                                        pic.embed_id = Some(embed);
                                    }
                                }
                            }
                            b"a:t" => {
                                in_text_element = true;
                            }
                            b"a:rPr" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.cur_style = parse_run_style(e, &reader);
                                }
                                in_rpr = true;
                            }
                            _ => {}
                        }
                    }
                    Ok(Event::Empty(ref e)) => {
                        let name = e.name();
                        match name.as_ref() {
                            b"a:rPr" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.cur_style = parse_run_style(e, &reader);
                                }
                            }
                            b"a:srgbClr" if in_rpr || in_bg => {
                                if let Some(val) = e
                                    .attributes()
                                    .filter_map(|a| a.ok())
                                    .find(|a| a.key.as_ref() == b"val")
                                {
                                    let hex =
                                        String::from_utf8_lossy(&val.value).to_lowercase();
                                    if in_rpr {
                                        if let Some(shape) = current_shape.as_mut() {
                                            shape.cur_style.color = Some(hex);
                                        }
                                    } else {
                                        background = format!("#{hex}");
                                    }
                                }
                            }
                            b"a:off" => {
                                let (x, y) = parse_coords(e, &reader, b"x", b"y");
                                if let Some(shape) = current_shape.as_mut() {
                                    if x.is_some() { shape.x = x; }
                                    if y.is_some() { shape.y = y; }
                                } else if let Some(pic) = current_picture.as_mut() {
                                    if x.is_some() { pic.x = x; }
                                    if y.is_some() { pic.y = y; }
                                }
                            }
                            b"a:ext" => {
                                let (w, h) = parse_coords(e, &reader, b"cx", b"cy");
                                if let Some(shape) = current_shape.as_mut() {
                                    if w.is_some() { shape.w = w; }
                                    if h.is_some() { shape.h = h; }
                                } else if let Some(pic) = current_picture.as_mut() {
                                    if w.is_some() { pic.w = w; }
                                    if h.is_some() { pic.h = h; }
                                }
                            }
                            b"a:prstGeom" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    if let Some(prst) = parse_prst_geom(e, &reader) {
                                        shape.prst = Some(prst);
                                    }
                                }
                            }
                            b"p:cNvSpPr" => {
                                if is_tx_box_attr(e, &reader) {
                                    if let Some(shape) = current_shape.as_mut() {
                                        shape.is_tx_box = true;
                                    }
                                }
                            }
                            b"a:blip" => {
                                if let Some(pic) = current_picture.as_mut() {
                                    if let Some(embed) = parse_blip_embed(e, &reader) {
                                        pic.embed_id = Some(embed);
                                    }
                                }
                            }
                            b"a:srgbClr" if in_bg => {
                                if let Some(val) = e
                                    .attributes()
                                    .filter_map(|a| a.ok())
                                    .find(|a| a.key.as_ref() == b"val")
                                {
                                    background = format!(
                                        "#{}",
                                        String::from_utf8_lossy(&val.value).to_lowercase()
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(Event::End(ref e)) => {
                        let name = e.name();
                        if name.as_ref() == b"p:bg" {
                            in_bg = false;
                        }
                        if name.as_ref() == b"a:rPr" {
                            in_rpr = false;
                        }
                        if name.as_ref() == b"p:sp" {
                            if let Some(shape) = current_shape.take() {
                                let x = shape.x.unwrap_or(0.0) / 9525.0;
                                let y = shape.y.unwrap_or(0.0) / 9525.0;
                                let w = shape.w.unwrap_or(0.0) / 9525.0;
                                let h = shape.h.unwrap_or(0.0) / 9525.0;
                                
                                let has_text =
                                    shape.text.iter().any(|t| !t.trim().is_empty());
                                if shape.is_tx_box || (shape.has_tx_body && has_text) {
                                    let text = shape.text.join("\n");
                                    objects.push(SlideObject::TextBox { text, x, y, w, h, rotation: 0.0, runs: shape.runs.clone() });
                                } else {
                                    let prst = shape.prst.unwrap_or_else(|| "rect".to_string());
                                    if prst == "ellipse" {
                                        objects.push(SlideObject::Circle {
                                            x: x + w / 2.0,
                                            y: y + h / 2.0,
                                            r: w / 2.0,
                                            rotation: 0.0,
                                        });
                                    } else {
                                        objects.push(SlideObject::Rect { x, y, w, h, rotation: 0.0 });
                                    }
                                }
                            }
                        } else if name.as_ref() == b"p:pic" {
                            if let Some(pic) = current_picture.take() {
                                if let Some(embed_id) = pic.embed_id {
                                    let x = pic.x.unwrap_or(0.0) / 9525.0;
                                    let y = pic.y.unwrap_or(0.0) / 9525.0;
                                    let w = pic.w.unwrap_or(0.0) / 9525.0;
                                    let h = pic.h.unwrap_or(0.0) / 9525.0;
                                    
                                    if let Some(obj) = resolve_and_extract_picture(&embed_id, x, y, w, h, &slide_image_rels, &mut archive) {
                                        objects.push(obj);
                                    }
                                }
                            }
                        } else if name.as_ref() == b"a:t" {
                            in_text_element = false;
                        }
                    }
                    Ok(Event::Text(ref e)) => {
                        if in_text_element {
                            {
                                let t = unescape_text(e);
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.runs.push(Run { text: t.clone(), style: shape.cur_style.clone() });
                                    shape.text.push(t);
                                }
                            }
                        }
                    }
                    Ok(Event::GeneralRef(ref r)) => {
                        if in_text_element {
                            let t = resolve_general_ref(r);
                            if let Some(shape) = current_shape.as_mut() {
                                shape.runs.push(Run { text: t.clone(), style: shape.cur_style.clone() });
                                shape.text.push(t);
                            }
                        }
                    }
                    Ok(Event::Eof) => break,
                    Err(e) => return Err(format!("XML parsing error in slide XML: {}", e)),
                    _ => {}
                }
                buf.clear();
            }
        }

        // Speaker notes: follow the notesSlide relationship if present.
        let mut notes = String::new();
        for target in slide_image_rels.values() {
            if target.contains("notesSlide") {
                let rel = target.trim_start_matches("../");
                let notes_path = format!("ppt/{}", rel);
                let mut notes_xml = String::new();
                if let Ok(mut f) = archive.by_name(&notes_path) {
                    f.read_to_string(&mut notes_xml).unwrap_or(0);
                }
                if !notes_xml.is_empty() {
                    notes = extract_notes_text(&notes_xml);
                }
                break;
            }
        }

        slide_layout_paths.push(
            slide_image_rels
                .values()
                .find(|t| t.contains("slideLayout"))
                .map(|t| format!("ppt/{}", t.trim_start_matches("../"))),
        );

        slides.push(Slide {
            title: format!("Slide {}", slide_index + 1),
            background,
            objects,
            notes,
            master_idx: Some(0),
        });
    }

    // ── Masters: one entry per distinct layout (master decorations +
    // layout decorations, placeholders skipped). ────────────────────────
    let mut masters: Vec<MasterSlide> = Vec::new();
    {
        let mut layout_to_idx: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let read_part = |archive: &mut zip::ZipArchive<File>, name: &str| -> String {
            let mut s = String::new();
            if let Ok(mut f) = archive.by_name(name) {
                f.read_to_string(&mut s).unwrap_or(0);
            }
            s
        };
        for (i, layout_path) in slide_layout_paths.iter().enumerate() {
            let Some(layout_path) = layout_path else { continue };
            let idx = if let Some(&idx) = layout_to_idx.get(layout_path) {
                idx
            } else {
                let layout_xml = read_part(&mut archive, layout_path);
                if layout_xml.is_empty() {
                    continue;
                }
                // Layout rels → its slideMaster part.
                let dir = Path::new(layout_path).parent().unwrap_or(Path::new("ppt"));
                let file = Path::new(layout_path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let rels = read_part(
                    &mut archive,
                    &format!("{}/_rels/{}.rels", dir.to_string_lossy(), file),
                );
                let master_xml = rels
                    .split("Target=\"")
                    .skip(1)
                    .filter_map(|s| s.split('"').next())
                    .find(|t| t.contains("slideMaster"))
                    .map(|t| format!("ppt/{}", t.trim_start_matches("../")))
                    .map(|p| read_part(&mut archive, &p))
                    .unwrap_or_default();

                let (master_bg, mut shapes) = parse_master_shapes(&master_xml);
                let (layout_bg, layout_shapes) = parse_master_shapes(&layout_xml);
                shapes.extend(layout_shapes);
                let name = Path::new(layout_path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Master".into());
                masters.push(MasterSlide {
                    name,
                    background: layout_bg
                        .or(master_bg)
                        .unwrap_or_else(|| "#ffffff".into()),
                    default_font: "Sans".into(),
                    shapes,
                });
                let idx = masters.len() - 1;
                layout_to_idx.insert(layout_path.clone(), idx);
                idx
            };
            if let Some(s) = slides.get_mut(i) {
                s.master_idx = Some(idx);
            }
        }
    }
    if masters.is_empty() {
        masters.push(MasterSlide {
            name: "Default".into(),
            background: "#ffffff".into(),
            default_font: "Sans".into(),
            shapes: vec![],
        });
    }

    if slides.is_empty() {
        slides.push(Slide {
            title: "Slide 1".into(),
            background: "#ffffff".into(),
            objects: vec![],
            notes: String::new(),
            master_idx: Some(0),
        });
    }

    Ok(Deck { slides, masters })
}

/// Parse a slideMaster/slideLayout part: background color and
/// non-placeholder decoration shapes. Placeholder shapes (`p:ph` —
/// "Click to edit Master title style" and friends) are styling slots,
/// not content, and are skipped.
pub fn parse_master_shapes(xml: &str) -> (Option<String>, Vec<SlideObject>) {
    if xml.is_empty() {
        return (None, Vec::new());
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut background: Option<String> = None;
    let mut shapes = Vec::new();
    let mut in_bg = false;
    let mut in_text = false;
    // (x, y, w, h, prst, has_ph, text)
    struct Pending {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        prst: Option<String>,
        has_ph: bool,
        text: Vec<String>,
    }
    let mut cur: Option<Pending> = None;
    while let Ok(ev) = reader.read_event_into(&mut buf) {
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => match e.name().as_ref() {
                b"p:bg" => in_bg = true,
                b"p:sp" => {
                    cur = Some(Pending {
                        x: 0.0,
                        y: 0.0,
                        w: 0.0,
                        h: 0.0,
                        prst: None,
                        has_ph: false,
                        text: Vec::new(),
                    });
                }
                b"p:ph" => {
                    if let Some(p) = cur.as_mut() {
                        p.has_ph = true;
                    }
                }
                b"a:off" => {
                    if let Some(p) = cur.as_mut() {
                        let (x, y) = parse_coords(e, &reader, b"x", b"y");
                        if let Some(x) = x {
                            p.x = x / 9525.0;
                        }
                        if let Some(y) = y {
                            p.y = y / 9525.0;
                        }
                    }
                }
                b"a:ext" => {
                    if let Some(p) = cur.as_mut() {
                        let (w, h) = parse_coords(e, &reader, b"cx", b"cy");
                        if let Some(w) = w {
                            p.w = w / 9525.0;
                        }
                        if let Some(h) = h {
                            p.h = h / 9525.0;
                        }
                    }
                }
                b"a:prstGeom" => {
                    if let Some(p) = cur.as_mut() {
                        p.prst = parse_prst_geom(e, &reader);
                    }
                }
                b"a:t" => in_text = true,
                b"a:srgbClr" if in_bg => {
                    if let Some(val) = e
                        .attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| a.key.as_ref() == b"val")
                    {
                        background =
                            Some(format!("#{}", String::from_utf8_lossy(&val.value).to_lowercase()));
                    }
                }
                _ => {}
            },
            Event::End(ref e) => match e.name().as_ref() {
                b"p:bg" => in_bg = false,
                b"a:t" => in_text = false,
                b"p:sp" => {
                    if let Some(p) = cur.take() {
                        if !p.has_ph && p.w > 0.0 && p.h > 0.0 {
                            let has_text = p.text.iter().any(|t| !t.trim().is_empty());
                            if has_text {
                                shapes.push(SlideObject::TextBox {
                                    text: p.text.join("\n"),
                                    x: p.x,
                                    y: p.y,
                                    w: p.w,
                                    h: p.h,
                                    rotation: 0.0,
                                    runs: vec![],
                                });
                            } else if p.prst.as_deref() == Some("ellipse") {
                                shapes.push(SlideObject::Circle {
                                    x: p.x + p.w / 2.0,
                                    y: p.y + p.h / 2.0,
                                    r: p.w / 2.0,
                                    rotation: 0.0,
                                });
                            } else {
                                shapes.push(SlideObject::Rect {
                                    x: p.x,
                                    y: p.y,
                                    w: p.w,
                                    h: p.h,
                                    rotation: 0.0,
                                });
                            }
                        }
                    }
                }
                _ => {}
            },
            Event::Text(ref t) => {
                if in_text {
                    if let Some(p) = cur.as_mut() {
                        p.text.push(unescape_text(t));
                    }
                }
            }
            Event::GeneralRef(ref r) => {
                if in_text {
                    if let Some(p) = cur.as_mut() {
                        p.text.push(resolve_general_ref(r));
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    (background, shapes)
}

fn resolve_and_extract_picture(
    embed_id: &str,
    x: f64, y: f64, w: f64, h: f64,
    rels: &std::collections::HashMap<String, String>,
    archive: &mut zip::ZipArchive<File>,
) -> Option<SlideObject> {
    let target = rels.get(embed_id)?;
    let relative_path = target.trim_start_matches("../");
    let full_zip_path = format!("ppt/{}", relative_path);

    let mut image_file = archive.by_name(&full_zip_path).ok()?;
    let mut buffer = Vec::new();
    image_file.read_to_end(&mut buffer).ok()?;

    // gh-268: the previous code wrote to a predictable /tmp/decks_img_<embed_id>.<ext>
    // path whose middle (embed_id) and suffix (extension) both came from the
    // untrusted document. A crafted PPTX could point the write anywhere via `..`
    // or a pre-created symlink. NamedTempFile gives O_EXCL + O_NOFOLLOW + an
    // unpredictable name in one step.
    let mut tmp = tempfile::NamedTempFile::new().ok()?;
    tmp.write_all(&buffer).ok()?;
    // Keep the temp file alive for the lifetime of the SlideObject; the model
    // reads it back later. NamedTempFile deletes on drop, so persist it.
    let (_, output_path) = tmp.keep().ok()?;

    Some(SlideObject::Image {
        path: output_path.to_string_lossy().to_string(),
        x,
        y,
        w,
        h,
        rotation: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::*;

    #[test]
    fn test_pptx_roundtrip() {
        let mut deck = Deck::new();
        deck.slides[0].objects.push(SlideObject::TextBox {
            text: "Hello Slide".into(),
            x: 100.0, y: 100.0, w: 300.0, h: 50.0,
            runs: vec![],
            rotation: 0.0,
        });
        deck.slides[0].objects.push(SlideObject::Rect {
            x: 150.0, y: 200.0, w: 200.0, h: 100.0,
            rotation: 0.0,
        });
        deck.slides[0].objects.push(SlideObject::Circle {
            x: 400.0, y: 300.0, r: 50.0,
            rotation: 0.0,
        });

        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_deck.pptx");
        let path_str = path.to_string_lossy();

        // Write
        let write_res = write_pptx(&path_str, &deck);
        assert!(write_res.is_ok(), "Write pptx failed: {:?}", write_res.err());

        // Read
        let read_res = read_pptx(&path_str);
        assert!(read_res.is_ok(), "Read pptx failed: {:?}", read_res.err());

        let read_deck = read_res.unwrap();
        assert_eq!(read_deck.slides.len(), 1);
        let slide = &read_deck.slides[0];
        assert_eq!(slide.objects.len(), 3);

        // Verify TextBox
        match &slide.objects[0] {
            SlideObject::TextBox { text, .. } => assert_eq!(text, "Hello Slide"),
            _ => panic!("Expected TextBox"),
        }

        // Verify Rect
        match &slide.objects[1] {
            SlideObject::Rect { x, y, w, h, .. } => {
                assert!((x - 150.0).abs() < 0.1);
                assert!((y - 200.0).abs() < 0.1);
                assert!((w - 200.0).abs() < 0.1);
                assert!((h - 100.0).abs() < 0.1);
            }
            _ => panic!("Expected Rect"),
        }

        // Verify Circle
        match &slide.objects[2] {
            SlideObject::Circle { x, y, r, .. } => {
                assert!((x - 400.0).abs() < 0.1);
                assert!((y - 300.0).abs() < 0.1);
                assert!((r - 50.0).abs() < 0.1);
            }
            _ => panic!("Expected Circle"),
        }

        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod master_tests {
    use super::*;
    use crate::engine::notes::{extract_notes_text, notes_slide_xml};

    #[test]
    fn master_parser_skips_placeholders_keeps_decorations() {
        let xml = r##"<p:sldMaster xmlns:p="x" xmlns:a="y"><p:cSld>
            <p:bg><p:bgPr><a:solidFill><a:srgbClr val="1A2B3C"/></a:solidFill></p:bgPr></p:bg>
            <p:spTree>
            <p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
              <p:spPr><a:xfrm><a:off x="9525" y="9525"/><a:ext cx="95250" cy="95250"/></a:xfrm></p:spPr>
              <p:txBody><a:p><a:r><a:t>Click to edit Master title style</a:t></a:r></a:p></p:txBody></p:sp>
            <p:sp><p:spPr><a:xfrm><a:off x="19050" y="28575"/><a:ext cx="190500" cy="95250"/></a:xfrm>
              <a:prstGeom prst="rect"/></p:spPr></p:sp>
            </p:spTree></p:cSld></p:sldMaster>"##;
        let (bg, shapes) = parse_master_shapes(xml);
        assert_eq!(bg.as_deref(), Some("#1a2b3c"));
        assert_eq!(shapes.len(), 1, "placeholder must be skipped: {shapes:?}");
        match &shapes[0] {
            SlideObject::Rect { x, y, w, h, .. } => {
                assert!((x - 2.0).abs() < 0.01 && (y - 3.0).abs() < 0.01);
                assert!((w - 20.0).abs() < 0.01 && (h - 10.0).abs() < 0.01);
            }
            other => panic!("expected rect, got {other:?}"),
        }
    }

    // ── notes_slide_xml ↔ extract_notes_text ─────────────────────────────

    /// Build a minimal notesSlide part around raw txBody XML so entity and
    /// break handling can be tested without going through `notes_slide_xml`.
    fn notes_with_txbody(txbody: &str) -> String {
        format!(
            r##"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Notes Placeholder"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr>
<p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/>
<p:txBody><a:bodyPr/><a:lstStyle/>{txbody}</p:txBody></p:sp>
</p:spTree></p:cSld></p:notes>"##
        )
    }

    #[test]
    fn notes_round_trip_plain_and_multiline() {
        assert_eq!(extract_notes_text(&notes_slide_xml("Hello")), "Hello");
        assert_eq!(
            extract_notes_text(&notes_slide_xml("line one\nline two")),
            "line one\nline two"
        );
        assert_eq!(extract_notes_text(&notes_slide_xml("")), "");
    }

    #[test]
    fn notes_round_trip_escapes_and_unescapes_entities() {
        let notes = "café & <b> > \"quotes\" 東京";
        assert_eq!(extract_notes_text(&notes_slide_xml(notes)), notes);
    }

    #[test]
    fn notes_blank_lines_do_not_survive_round_trip() {
        // Current behavior: empty paragraphs are dropped when captured, so
        // blank lines in speaker notes collapse on a write→read round trip.
        assert_eq!(extract_notes_text(&notes_slide_xml("a\n\nb")), "a\nb");
        assert_eq!(extract_notes_text(&notes_slide_xml("a\n")), "a");
    }

    #[test]
    fn notes_extract_resolves_numeric_char_refs() {
        // quick-xml 0.41 surfaces &#NN; as a GeneralRef event;
        // resolve_general_ref must turn it back into the character.
        let xml = notes_with_txbody("<a:p><a:r><a:t>&#65;&#x42;</a:t></a:r></a:p>");
        assert_eq!(extract_notes_text(&xml), "AB");
    }

    #[test]
    fn notes_extract_preserves_unknown_entities() {
        let xml = notes_with_txbody("<a:p><a:r><a:t>a &bogus; b</a:t></a:r></a:p>");
        assert_eq!(extract_notes_text(&xml), "a &bogus; b");
    }

    #[test]
    fn notes_extract_soft_break_becomes_newline() {
        let xml =
            notes_with_txbody("<a:p><a:r><a:t>one</a:t></a:r><a:br/><a:r><a:t>two</a:t></a:r></a:p>");
        assert_eq!(extract_notes_text(&xml), "one\ntwo");
    }

    // ── parse_master_shapes ──────────────────────────────────────────────

    #[test]
    fn master_parse_empty_xml_has_no_background_or_shapes() {
        let (bg, shapes) = parse_master_shapes("");
        assert!(bg.is_none());
        assert!(shapes.is_empty());
    }

    #[test]
    fn master_parse_missing_background_returns_none() {
        let xml = r##"<p:sldMaster xmlns:p="x"><p:cSld><p:spTree/></p:cSld></p:sldMaster>"##;
        let (bg, shapes) = parse_master_shapes(xml);
        assert!(bg.is_none());
        assert!(shapes.is_empty());
    }

    #[test]
    fn master_parse_background_without_shapes() {
        let xml = r##"<p:sldMaster xmlns:p="x" xmlns:a="y"><p:cSld>
<p:bg><p:bgPr><a:solidFill><a:srgbClr val="AABBCC"/></a:solidFill></p:bgPr></p:bg>
<p:spTree/></p:cSld></p:sldMaster>"##;
        let (bg, shapes) = parse_master_shapes(xml);
        assert_eq!(bg.as_deref(), Some("#aabbcc"));
        assert!(shapes.is_empty());
    }

    #[test]
    fn master_parse_text_box_shape() {
        let xml = r##"<p:sldMaster xmlns:p="x" xmlns:a="y"><p:cSld><p:spTree>
<p:sp><p:spPr><a:xfrm><a:off x="19050" y="28575"/><a:ext cx="190500" cy="95250"/></a:xfrm></p:spPr>
<p:txBody><a:p><a:r><a:t>Deck title</a:t></a:r></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:sldMaster>"##;
        let (_, shapes) = parse_master_shapes(xml);
        assert_eq!(shapes.len(), 1);
        match &shapes[0] {
            SlideObject::TextBox { text, x, y, w, h, runs, .. } => {
                assert_eq!(text, "Deck title");
                assert!(runs.is_empty());
                assert!((x - 2.0).abs() < 0.01 && (y - 3.0).abs() < 0.01);
                assert!((w - 20.0).abs() < 0.01 && (h - 10.0).abs() < 0.01);
            }
            other => panic!("expected text box, got {other:?}"),
        }
    }
}
