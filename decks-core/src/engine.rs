// engine.rs — Presentation engine: PPTX I/O using zip and XML.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;
use quick_xml::events::{Event, BytesStart, BytesEnd, BytesDecl, BytesText};
use quick_xml::Reader;
use quick_xml::Writer;
use letters_core::model::{Run, RunStyle};

#[derive(Clone, Debug)]
pub struct Deck {
    pub slides: Vec<Slide>,
    pub masters: Vec<MasterSlide>,
}

#[derive(Clone, Debug)]
pub struct Slide {
    pub title: String,
    pub background: String,
    pub objects: Vec<SlideObject>,
    pub notes: String,
    pub master_idx: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct MasterSlide {
    pub name: String,
    pub background: String,
    pub default_font: String,
    pub shapes: Vec<SlideObject>,
}

#[derive(Clone, Debug)]
pub enum SlideObject {
    TextBox {
        text: String,
        x: f64, y: f64, w: f64, h: f64,
        /// Styled runs (shared WYSIWYG primitive with Letters). When
        /// non-empty, concatenated run text equals `text`.
        runs: Vec<Run>,
    },
    Rect { x: f64, y: f64, w: f64, h: f64 },
    Circle { x: f64, y: f64, r: f64 },
    Image { path: String, x: f64, y: f64, w: f64, h: f64 },
}

impl Default for Deck {
    fn default() -> Self {
        Self::new()
    }
}

impl Deck {
    pub fn new() -> Self {
        let default_master = MasterSlide {
            name: "Default".into(),
            background: "#ffffff".into(),
            default_font: "Sans".into(),
            shapes: vec![],
        };
        Self {
            slides: vec![Slide {
                title: "Slide 1".into(),
                background: "#ffffff".into(),
                objects: vec![],
                notes: String::new(),
                master_idx: Some(0),
            }],
            masters: vec![default_master],
        }
    }
}

// ── Attributes and position helpers ──────────────────────────────────

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
            if let Ok(val) = attr.decode_and_unescape_value(reader) {
                v1 = val.parse::<f64>().ok();
            }
        } else if attr.key.as_ref() == k2 {
            if let Ok(val) = attr.decode_and_unescape_value(reader) {
                v2 = val.parse::<f64>().ok();
            }
        }
    }
    (v1, v2)
}

fn parse_blip_embed<B: std::io::BufRead>(e: &BytesStart, reader: &Reader<B>) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"r:embed" {
            if let Ok(val) = attr.decode_and_unescape_value(reader) {
                return Some(val.into_owned());
            }
        }
    }
    None
}

fn parse_prst_geom<B: std::io::BufRead>(e: &BytesStart, reader: &Reader<B>) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"prst" {
            if let Ok(val) = attr.decode_and_unescape_value(reader) {
                return Some(val.into_owned());
            }
        }
    }
    None
}

fn is_tx_box_attr<B: std::io::BufRead>(e: &BytesStart, reader: &Reader<B>) -> bool {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"txBox" {
            if let Ok(val) = attr.decode_and_unescape_value(reader) {
                return val.as_ref() == "1";
            }
        }
    }
    false
}

struct PendingShape {
    is_tx_box: bool,
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

// ── Read PPTX ────────────────────────────────────────────────────────

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
        reader.trim_text(true);
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
                                    id = attr.decode_and_unescape_value(&reader).ok().map(|v| v.into_owned());
                                }
                                b"Target" => {
                                    target = attr.decode_and_unescape_value(&reader).ok().map(|v| v.into_owned());
                                }
                                b"Type" => {
                                    if let Ok(v) = attr.decode_and_unescape_value(&reader) {
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
        reader.trim_text(true);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let name = e.name();
                    if name.as_ref() == b"p:sldId" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"r:id" {
                                if let Ok(val) = attr.decode_and_unescape_value(&reader) {
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
            reader.trim_text(true);
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
                                    id = attr.decode_and_unescape_value(&reader).ok().map(|v| v.into_owned());
                                } else if attr.key.as_ref() == b"Target" {
                                    target = attr.decode_and_unescape_value(&reader).ok().map(|v| v.into_owned());
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
        {
            let mut reader = Reader::from_str(&slide_xml);
            reader.trim_text(true);
            let mut buf = Vec::new();

            let mut current_shape: Option<PendingShape> = None;
            let mut current_picture: Option<PendingPicture> = None;
            let mut in_text_element = false;

            loop {
                match reader.read_event_into(&mut buf) {
                    Ok(Event::Start(ref e)) => {
                        let name = e.name();
                        match name.as_ref() {
                            b"p:sp" => {
                                current_shape = Some(PendingShape {
                                    is_tx_box: false,
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
                                    shape.is_tx_box = true;
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
                            _ => {}
                        }
                    }
                    Ok(Event::End(ref e)) => {
                        let name = e.name();
                        if name.as_ref() == b"p:sp" {
                            if let Some(shape) = current_shape.take() {
                                let x = shape.x.unwrap_or(0.0) / 9525.0;
                                let y = shape.y.unwrap_or(0.0) / 9525.0;
                                let w = shape.w.unwrap_or(0.0) / 9525.0;
                                let h = shape.h.unwrap_or(0.0) / 9525.0;
                                
                                if shape.is_tx_box {
                                    let text = shape.text.join("\n");
                                    objects.push(SlideObject::TextBox { text, x, y, w, h, runs: shape.runs.clone() });
                                } else {
                                    let prst = shape.prst.unwrap_or_else(|| "rect".to_string());
                                    if prst == "ellipse" {
                                        objects.push(SlideObject::Circle {
                                            x: x + w / 2.0,
                                            y: y + h / 2.0,
                                            r: w / 2.0,
                                        });
                                    } else {
                                        objects.push(SlideObject::Rect { x, y, w, h });
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
                            if let Ok(unesc) = e.unescape() {
                                if let Some(shape) = current_shape.as_mut() {
                                    let t = unesc.into_owned();
                                    shape.runs.push(Run { text: t.clone(), style: shape.cur_style.clone() });
                                    shape.text.push(t);
                                }
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

        slides.push(Slide {
            title: format!("Slide {}", slide_index + 1),
            background: "#ffffff".into(),
            objects,
            notes,
            master_idx: Some(0),
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

    Ok(Deck { slides, masters: vec![] })
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

    let extension = Path::new(&full_zip_path).extension()?.to_str()?;
    let temp_dir = std::env::temp_dir();
    let unique_name = format!("decks_img_{}.{}", embed_id, extension);
    let output_path = temp_dir.join(unique_name);
    
    let mut out = File::create(&output_path).ok()?;
    out.write_all(&buffer).ok()?;

    Some(SlideObject::Image {
        path: output_path.to_string_lossy().to_string(),
        x,
        y,
        w,
        h,
    })
}

// ── Write PPTX ───────────────────────────────────────────────────────

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
        r_pr.push_attribute(("sz", "1800"));
        if run.style.bold { r_pr.push_attribute(("b", "1")); }
        if run.style.italic { r_pr.push_attribute(("i", "1")); }
        if run.style.underline { r_pr.push_attribute(("u", "sng")); }
        if run.style.strikethrough { r_pr.push_attribute(("strike", "sngStrike")); }
        writer.write_event(Event::Empty(r_pr))?;
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
    let file = File::create(path).map_err(|e| format!("Cannot create file: {}", e))?;
    let mut zip = zip::ZipWriter::new(file);

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
                    SlideObject::TextBox { text, x, y, w, h, runs } => {
                        write_text_box(&mut writer, id, j + 1, *x, *y, *w, *h, text, runs).map_err(|e| e.to_string())?;
                    }
                    SlideObject::Rect { x, y, w, h } => {
                        write_rect(&mut writer, id, j + 1, *x, *y, *w, *h).map_err(|e| e.to_string())?;
                    }
                    SlideObject::Circle { x, y, r } => {
                        write_circle(&mut writer, id, j + 1, *x, *y, *r).map_err(|e| e.to_string())?;
                    }
                    SlideObject::Image { path, x, y, w, h } => {
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

    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pptx_roundtrip() {
        let mut deck = Deck::new();
        deck.slides[0].objects.push(SlideObject::TextBox {
            text: "Hello Slide".into(),
            x: 100.0, y: 100.0, w: 300.0, h: 50.0,
            runs: vec![],
        });
        deck.slides[0].objects.push(SlideObject::Rect {
            x: 150.0, y: 200.0, w: 200.0, h: 100.0,
        });
        deck.slides[0].objects.push(SlideObject::Circle {
            x: 400.0, y: 300.0, r: 50.0,
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
            SlideObject::Rect { x, y, w, h } => {
                assert!((x - 150.0).abs() < 0.1);
                assert!((y - 200.0).abs() < 0.1);
                assert!((w - 200.0).abs() < 0.1);
                assert!((h - 100.0).abs() < 0.1);
            }
            _ => panic!("Expected Rect"),
        }

        // Verify Circle
        match &slide.objects[2] {
            SlideObject::Circle { x, y, r } => {
                assert!((x - 400.0).abs() < 0.1);
                assert!((y - 300.0).abs() < 0.1);
                assert!((r - 50.0).abs() < 0.1);
            }
            _ => panic!("Expected Circle"),
        }

        let _ = std::fs::remove_file(&path);
    }
}

/// Minimal notesSlide part with the notes text in a body placeholder.
fn notes_slide_xml(notes: &str) -> String {
    let mut paras = String::new();
    for line in notes.split('\n') {
        let escaped = line
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        paras.push_str(&format!("<a:p><a:r><a:t>{}</a:t></a:r></a:p>", escaped));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <p:notes xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
<p:cSld><p:spTree>\
<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>\
<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Notes Placeholder\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr>\
<p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/>\
<p:txBody><a:bodyPr/><a:lstStyle/>{}</p:txBody></p:sp>\
</p:spTree></p:cSld></p:notes>",
        paras
    )
}

/// Extract the body-placeholder text from a notesSlide part.
fn extract_notes_text(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut in_sp = false;
    let mut sp_is_body = false;
    let mut in_t = false;
    let mut current = String::new();
    let mut parts: Vec<String> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                b"p:sp" => { in_sp = true; sp_is_body = false; }
                b"a:t" if sp_is_body => in_t = true,
                _ => {}
            },
            Ok(Event::Empty(ref e)) if e.name().as_ref() == b"p:ph" && in_sp => {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"type" {
                        if let Ok(v) = attr.decode_and_unescape_value(&reader) {
                            if v == "body" { sp_is_body = true; }
                        }
                    }
                }
            }
            Ok(Event::Text(ref t)) if in_t => {
                current.push_str(&t.unescape().unwrap_or_default());
            }
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"a:t" => in_t = false,
                b"a:p" if sp_is_body => {
                    if !current.is_empty() { parts.push(std::mem::take(&mut current)); }
                }
                b"p:sp" => { in_sp = false; sp_is_body = false; }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    parts.join("\n")
}

/// Parse b/i/u/strike attributes from an a:rPr element into the shared
/// RunStyle (same WYSIWYG primitive Letters uses).
fn parse_run_style(e: &BytesStart, reader: &Reader<&[u8]>) -> RunStyle {
    let mut st = RunStyle::default();
    for attr in e.attributes().flatten() {
        let val = attr.decode_and_unescape_value(reader).unwrap_or_default();
        match attr.key.as_ref() {
            b"b" => st.bold = val == "1" || val == "true",
            b"i" => st.italic = val == "1" || val == "true",
            b"u" => st.underline = val != "none" && !val.is_empty(),
            b"strike" => st.strikethrough = val != "noStrike" && !val.is_empty(),
            _ => {}
        }
    }
    st
}
