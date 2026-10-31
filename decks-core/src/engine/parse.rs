// parse.rs — pptx reading: read_pptx + xml parse helpers.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of engine.rs (issue #247).

pub(crate) fn unescape_text(t: &BytesText) -> String {
    // quick-xml 0.42 dropped `decode()`: event payloads are already `&str`.
    quick_xml::escape::unescape(t)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| t.to_string())
}

pub(crate) fn resolve_general_ref(r: &BytesRef) -> String {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return c.to_string();
    }
    let name: &str = r;
    quick_xml::escape::resolve_predefined_entity(name)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("&{name};"))
}

use super::model::*;
use super::notes::{extract_notes_text, parse_run_style};
use super::placeholders::{inherited_rect, parse_placeholders, PhKey, Placeholder};
use super::shape::ShapeKind;
use super::shape_xml::{cxn_styles, frame_charts, frame_tables, sp_geometries, sp_styles, theme as read_theme, Theme};
use super::text_xml::{sp_texts, Inherited};
use crate::layouts::PlaceholderKey;

use std::fs::File;
use std::path::Path;
use quick_xml::events::{Event, BytesStart, BytesRef, BytesText};
use quick_xml::Reader;
use letters_core::model::{Run, RunStyle};
use suite_common_core::zip_guard::{BoundedArchive, ZipBudget};
#[path = "parse/geometry.rs"]
mod geometry;
#[path = "parse/layout.rs"]
mod layout;
#[path = "parse/master.rs"]
mod master;
#[path = "parse/pictures.rs"]
mod pictures;
#[path = "parse/transitions.rs"]
mod transitions;

pub use geometry::{parse_slide_size, SlideScale};
pub use master::{parse_master_shapes, parse_master_shapes_scaled};
use self::geometry::{
    in_groups, is_tx_box_attr, parse_blip_embed, parse_coords, parse_prst_geom, parse_rotation,
    slide_is_hidden, GroupFrame,
};
use self::layout::{
    layout_path_of, layout_theme, parse_color_map, parse_layout_background, read_layout,
    read_layout_placeholders,
};
use self::master::{master_shapes, parse_c_sld_name};
use self::pictures::{
    part_path, part_picture, parse_fill_rect, parse_src_rect, resolve_and_extract_picture,
    PendingPicture, PictureRect,
};
use self::transitions::parse_transition;


/// Close the paragraph a shape has been accumulating, so the next one starts
/// on a new line.
///
/// A text box's paragraphs are `a:p` elements and its runs are the `a:r`
/// inside them, and the two are *not* interchangeable: the runs of one
/// paragraph are consecutive pieces of a single line, while two paragraphs
/// are two lines. Both readers used to record one entry per `a:t` and join
/// the lot with `\n`, which is right only when every paragraph holds exactly
/// one run — so a decoration reading "Plain **Bold**" came back as two
/// lines, and the break was then written into the saved package.
///
/// The break is carried *inside* the runs rather than only in the derived
/// text, because `SlideObject::TextBox` requires concatenated run text to
/// equal `text`. A paragraph that contributed no run of its own cannot
/// carry it: a blank paragraph between runs becomes a second break on the
/// run before, and blank paragraphs ahead of the first run lead its text
/// (`push_run`).
fn close_paragraphs(runs: &mut [Run], count: usize) {
    if let Some(last) = runs.last_mut() {
        for _ in 0..count {
            last.text.push('\n');
        }
    }
}

/// The `(text, runs)` pair a `TextBox` is built from.
///
/// `text` is *derived* from the runs rather than accumulated beside them,
/// which is what makes the model's invariant structural instead of a thing
/// each walker has to remember.
fn text_of(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

struct PendingShape {
    is_tx_box: bool,
    /// A p:txBody element was seen. Impress adds an (empty) txBody to
    /// every shape, so this alone does not make it a text box.
    has_tx_body: bool,
    runs: Vec<Run>,
    /// Paragraphs that have closed since the last run, counted rather than
    /// flagged: an empty `<a:p/>` between two others is a blank line, and a
    /// flag would collapse the two breaks into one and lose it. Applied
    /// only when more text actually arrives, so trailing empty paragraphs
    /// cannot leave dangling breaks.
    pending_breaks: usize,
    cur_style: RunStyle,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
    rotation: Option<f64>,
    prst: Option<String>,
    /// Set when the shape is a placeholder (`p:ph`): its geometry may
    /// come from the layout or master instead of its own `a:xfrm`.
    ph: Option<PhKey>,
    /// `a:p` elements seen so far (the current one is `paras_seen - 1`).
    paras_seen: usize,
    /// `a:r`/`a:fld` elements seen so far.
    runs_seen: usize,
    /// For each line of text, the `a:p` it came from. Empty paragraphs at
    /// the start and end make no line, so this is not simply 0..n.
    line_paras: Vec<usize>,
    /// For each run, the `a:r`/`a:fld` it came from.
    run_refs: Vec<Option<usize>>,
}

impl PendingShape {
    /// A line break inside the paragraph (`a:br`): on the run before it,
    /// or a run of its own when the paragraph has none yet.
    fn line_break(&mut self) {
        if self.pending_breaks == 0 && !self.runs.is_empty() {
            if let Some(r) = self.runs.last_mut() {
                r.text.push(super::LINE_BREAK);
            }
        } else {
            self.push_run(super::LINE_BREAK.to_string());
        }
    }

    /// Record one `a:t`'s text as a run, breaking the line first if a
    /// paragraph closed since the last one.
    fn push_run(&mut self, mut text: String) {
        let cur = self.paras_seen.saturating_sub(1);
        if self.runs.is_empty() {
            // Blank paragraphs ahead of the first run are lines too: a
            // top-anchored tile that opens with two empty paragraphs sets
            // its figure two lines down (#1397), so they lead this run's
            // text as breaks of their own, sized like it.
            let lead = std::mem::take(&mut self.pending_breaks);
            for k in 0..lead {
                self.line_paras.push((cur + k).saturating_sub(lead));
            }
            self.line_paras.push(cur);
            text.insert_str(0, &"\n".repeat(lead));
        } else {
            // n closed paragraphs since the last run: the n-1 empty ones
            // between, then this one.
            for k in 0..self.pending_breaks {
                self.line_paras.push((cur + 1 + k).saturating_sub(self.pending_breaks));
            }
        }
        close_paragraphs(&mut self.runs, self.pending_breaks);
        self.pending_breaks = 0;
        self.runs.push(Run { text, style: self.cur_style.clone() });
        self.run_refs.push(self.runs_seen.checked_sub(1));
    }

    /// The runs styled as the file resolves them (inherited defaults under
    /// their own properties), and the text body for the lines kept.
    fn resolve(&self, st: Option<&super::text_xml::SpText>) -> (Vec<Run>, super::text_body::TextBody) {
        let Some(st) = st else { return (self.runs.clone(), Default::default()) };
        let runs = self
            .runs
            .iter()
            .zip(&self.run_refs)
            .map(|(r, i)| match i.and_then(|i| st.runs.get(i)) {
                Some(p) => Run { text: r.text.clone(), style: p.style() },
                None => r.clone(),
            })
            .collect();
        (runs, st.body_for(&self.line_paras))
    }
}


/// The placeholders of a slide layout and of the master it belongs to, for
/// resolving slide placeholders that carry no geometry of their own.
pub fn read_pptx(path: &str) -> Result<Deck, String> {
    let file = File::open(path).map_err(|e| format!("Cannot open file: {}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Invalid zip archive: {}", e))?;
    // A PPTX arrives from a download or an attachment. Every part below is
    // read under one budget, so neither a single member claiming to
    // decompress to gigabytes nor a hundred medium ones can exhaust memory
    // (#442). The budget is shared across the whole archive on purpose: a
    // per-part limit that resets would be no limit at all.
    let mut budget = ZipBudget::default();
    budget.check_entry_count(archive.len())?;

    // 1. Read presentation.xml to count slides and get their rIds
    // A missing part means "not a PPTX"; a part past the read budget means
    // "this file is too large to open". Telling the user the first when the
    // second happened sends them looking for the wrong problem.
    let presentation_xml = archive
        .part_to_string("ppt/presentation.xml", &mut budget)
        .map_err(|e| {
            if e.is_missing() {
                "Not a valid PPTX (missing ppt/presentation.xml)".to_string()
            } else {
                e.to_string()
            }
        })?;

    // Every coordinate in the package is relative to this, so it has to be
    // read before any of them. A package that declares no size gets the one
    // our own writer emits, which is what the old fixed divisor assumed.
    let declared = parse_slide_size(&presentation_xml);
    let scale = match declared {
        Some((cx, cy)) => SlideScale::from_emu(cx, cy),
        None => SlideScale::default(),
    };
    // Kept once on the deck (DCK-006) so a save writes the same size back.
    let page_size = declared.filter(|&(cx, cy)| cx > 0.0 && cy > 0.0 && (cx, cy) != SlideScale::DEFAULT_EMU);

    // 2. Read presentation.xml.rels to resolve slide relationship IDs to paths
    let rels_xml = archive
        .part_to_string("ppt/_rels/presentation.xml.rels", &mut budget)
        .map_err(|e| {
            if e.is_missing() {
                "Not a valid PPTX (missing ppt/_rels/presentation.xml.rels)".to_string()
            } else {
                e.to_string()
            }
        })?;

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
                    if name.as_ref() == "Relationship" {
                        let mut id = None;
                        let mut target = None;
                        let mut is_slide = false;
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                "Id" => {
                                    id = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned());
                                }
                                "Target" => {
                                    target = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned());
                                }
                                "Type" => {
                                    if let Ok(v) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
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
                    if name.as_ref() == "p:sldId" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == "r:id" {
                                if let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
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

    // Theme parts by part path, and the theme part per layout: every slide
    // resolves its shapes and text against its own master's theme
    // (DCK-006), not the package's first theme part. A chain that names
    // no theme resolves against the default theme.
    let mut themes: std::collections::HashMap<String, Theme> = std::collections::HashMap::new();
    let mut layout_theme_paths: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
    // The retained model themes (`Deck.themes`), in package order, and the
    // theme part path each id was read from: masters that share one part
    // share one entry. Built with the masters below.
    let mut model_themes: Vec<super::theme::Theme> = Vec::new();
    let mut theme_id_of: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    let mut slides = Vec::new();
    // Stable ids minted once at import (DCK-005): one counter for the
    // whole package, so slide, object, master, layout, placeholder and
    // theme ids are all distinct. Slides and objects are stamped as each
    // slide is read (the builds below name object ids); masters, layouts
    // and themes when their parts resolve afterwards.
    let mut fresh = 1u64;
    // Layout part path per slide (slide → layout → master mapping is
    // resolved after the slide loop, when `archive` is free again).
    let mut slide_layout_paths: Vec<Option<String>> = Vec::new();
    #[allow(clippy::type_complexity)]
    let mut placeholder_cache: std::collections::HashMap<String, (Vec<Placeholder>, Vec<Placeholder>, Inherited)> =
        std::collections::HashMap::new();

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

        let Ok(slide_xml) = archive.part_to_string(&target_path, &mut budget) else {
            continue;
        };

        // Check if there's a slide relationship file (for images)
        let slide_dir = Path::new(&target_path).parent().unwrap_or(Path::new("ppt/slides"));
        let slide_filename = Path::new(&target_path).file_name().unwrap_or_default().to_string_lossy();
        let slide_rels_path = format!("{}/_rels/{}.rels", slide_dir.to_string_lossy(), slide_filename);
        
        let slide_rels_xml = archive.optional_part_to_string(&slide_rels_path, &mut budget);

        let mut slide_image_rels = std::collections::HashMap::new();
        if !slide_rels_xml.is_empty() {
            let mut reader = Reader::from_str(&slide_rels_xml);
            reader.config_mut().trim_text(true);
            let mut buf = Vec::new();
            loop {
                match reader.read_event_into(&mut buf) {
                    Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                        let name = e.name();
                        if name.as_ref() == "Relationship" {
                            let mut id = None;
                            let mut target = None;
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == "Id" {
                                    id = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned());
                                } else if attr.key.as_ref() == "Target" {
                                    target = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned());
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

        // The layout this slide hangs off, and (through it, the master)
        // the theme its shapes and text answer to (DCK-006): the theme
        // part its layout's master names, shared with the master builder
        // below through the same cache.
        let layout_path = layout_path_of(&slide_image_rels);
        let (slide_theme, _) = match layout_path.as_deref() {
            Some(lp) => layout_theme(&mut archive, &mut budget, lp, &mut layout_theme_paths, &mut themes),
            None => (Theme::default(), None),
        };
        let mut objects = Vec::new();
        let mut object_ids: Vec<Option<u32>> = Vec::new();
        let sp_paint = sp_styles(&slide_xml, &slide_theme, scale.x);
        let sp_geom = sp_geometries(&slide_xml);
        let mut sp_index = 0usize;
        // Connectors (p:cxnSp) are counted apart, so the p:sp order the
        // text and paint passes share stays as it was.
        let cxn_paint = cxn_styles(&slide_xml, &slide_theme, scale.x);
        let mut cxn_index = 0usize;
        // Tables live in p:graphicFrame, which the walker used to skip.
        let mut tables = frame_tables(&slide_xml, &slide_theme);
        for t in &mut tables {
            t.table.cell_margins = Some((91440.0 * scale.x, 45720.0 * scale.y));
            for cell in t.table.rows.iter_mut().flatten() {
                cell.runs = scale.text_runs(&cell.runs);
            }
        }
        let charts = frame_charts(&slide_xml);
        let mut frame_index = 0usize;

        // The layout (and through it, the master) this slide's placeholders
        // inherit their geometry from. Read once per layout.
        if let Some(lp) = &layout_path {
            if !placeholder_cache.contains_key(lp) {
                let found = read_layout_placeholders(&mut archive, &mut budget, lp, &slide_theme);
                placeholder_cache.insert(lp.clone(), found);
            }
        }
        let no_inheritance = Inherited::default();
        let (layout_phs, master_phs, inherited): (&[Placeholder], &[Placeholder], &Inherited) = layout_path
            .as_ref()
            .and_then(|lp| placeholder_cache.get(lp))
            .map(|(l, m, i)| (l.as_slice(), m.as_slice(), i))
            .unwrap_or((&[], &[], &no_inheritance));
        // Paragraph and run styles of every p:sp, with what the layout and
        // master give them (Nth p:sp, like the paint).
        let sp_text = sp_texts(&slide_xml, &slide_theme, inherited, scale);

        // Parse slide XML using quick-xml event reader
        // The slide's own background, when it states one. Absence stays
        // empty (inherits the layout's, then the master's): never white
        // smuggled in at import (DCK-006).
        let mut background = String::new();
        // A picture the background is filled with (`p:bg`'s `a:blipFill`):
        // its relationship and where it is stretched to (`a:fillRect`).
        let mut background_blip: Option<String> = None;
        let mut background_fill = super::model::FillRect::default();
        {
            // Text inside `a:t` is significant: a run ending in a space
            // ("Plain " + "Bold") is one word boundary, and trimming it
            // silently welds the words together. The odp walker already
            // reads its text untrimmed for the same reason. Whitespace
            // *between* elements is still ignored, because a text event is
            // only consumed while inside an `a:t`.
            let mut reader = Reader::from_str(&slide_xml);
            reader.config_mut().trim_text(false);
            let mut buf = Vec::new();

            let mut current_shape: Option<PendingShape> = None;
            let mut current_picture: Option<PendingPicture> = None;
            let mut in_text_element = false;
            let mut in_bg = false;
            // Inside `p:bg`'s `a:blipFill`: a colour there (a duotone, a
            // recolour) belongs to the picture, not the background.
            let mut in_bg_blip = false;
            let mut in_rpr = false;
            // The groups (p:grpSp) the walker is inside, outermost first:
            // a shape in one is placed in the group's child frame.
            let mut groups: Vec<GroupFrame> = Vec::new();
            let mut in_grp_pr = false;
            // The cNvPr id of the last shape seen, and each kept object's:
            // builds (p:timing) target shapes by it.
            let mut last_id: Option<u32> = None;

            loop {
                while object_ids.len() < objects.len() {
                    object_ids.push(last_id);
                }
                match reader.read_event_into(&mut buf) {
                    Ok(Event::Start(ref e)) => {
                        let name = e.name();
                        match name.as_ref() {
                            "p:bg" => in_bg = true,
                            // A break with properties of its own (`a:rPr`
                            // inside) is an element, not an empty one.
                            "a:br" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.line_break();
                                }
                            }
                            "a:blipFill" if in_bg && current_shape.is_none() && current_picture.is_none() => in_bg_blip = true,
                            "a:fillRect" if in_bg_blip => background_fill = parse_fill_rect(e),
                            "p:grpSp" => groups.push(GroupFrame::default()),
                            "p:grpSpPr" if current_shape.is_none() && current_picture.is_none() => in_grp_pr = true,
                            "a:chOff" | "a:chExt" if in_grp_pr => {
                                if let Some(g) = groups.last_mut() {
                                    g.note(e);
                                }
                            }
                            // A connector (p:cxnSp) is read as a shape:
                            // its frame and preset, and no text.
                            "p:sp" | "p:cxnSp" => {
                                current_shape = Some(PendingShape {
                                    is_tx_box: false,
                                    has_tx_body: false,
                                    runs: Vec::new(),
                                    pending_breaks: 0,
                                    cur_style: RunStyle::default(),
                                    x: None,
                                    y: None,
                                    w: None,
                                    h: None,
                                    rotation: None,
                                    prst: None,
                                    ph: None,
                                    paras_seen: 0,
                                    runs_seen: 0,
                                    line_paras: Vec::new(),
                                    run_refs: Vec::new(),
                                });
                            }
                            "p:pic" => {
                                current_picture = Some(PendingPicture {
                                    embed_id: None,
                                    x: None,
                                    y: None,
                                    w: None,
                                    h: None,
                                    rotation: None,
                                    crop: Default::default(),
                                });
                            }
                            "a:xfrm" => {
                                let rot = parse_rotation(e);
                                if let Some(shape) = current_shape.as_mut() {
                                    if rot.is_some() { shape.rotation = rot; }
                                } else if let Some(pic) = current_picture.as_mut() {
                                    if rot.is_some() { pic.rotation = rot; }
                                }
                            }
                            "a:off" => {
                                let (x, y) = parse_coords(e, "x", "y");
                                if let Some(shape) = current_shape.as_mut() {
                                    if x.is_some() { shape.x = x; }
                                    if y.is_some() { shape.y = y; }
                                } else if let Some(pic) = current_picture.as_mut() {
                                    if x.is_some() { pic.x = x; }
                                    if y.is_some() { pic.y = y; }
                                } else if in_grp_pr {
                                    if let Some(g) = groups.last_mut() {
                                        g.note(e);
                                    }
                                }
                            }
                            "a:ext" => {
                                let (w, h) = parse_coords(e, "cx", "cy");
                                if let Some(shape) = current_shape.as_mut() {
                                    if w.is_some() { shape.w = w; }
                                    if h.is_some() { shape.h = h; }
                                } else if let Some(pic) = current_picture.as_mut() {
                                    if w.is_some() { pic.w = w; }
                                    if h.is_some() { pic.h = h; }
                                } else if in_grp_pr {
                                    if let Some(g) = groups.last_mut() {
                                        g.note(e);
                                    }
                                }
                            }
                            "a:prstGeom" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    if let Some(prst) = parse_prst_geom(e) {
                                        shape.prst = Some(prst);
                                    }
                                }
                            }
                            "p:ph" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.ph = Some(PhKey::from_ph(e));
                                }
                            }
                            "p:cNvPr" => last_id = cnvpr_id(e),
                            "p:cNvSpPr" => {
                                if is_tx_box_attr(e) {
                                    if let Some(shape) = current_shape.as_mut() {
                                        shape.is_tx_box = true;
                                    }
                                }
                            }
                            "p:txBody" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.has_tx_body = true;
                                }
                            }
                            "a:blip" => {
                                if let Some(pic) = current_picture.as_mut() {
                                    if let Some(embed) = parse_blip_embed(e) {
                                        pic.embed_id = Some(embed);
                                    }
                                } else if in_bg_blip {
                                    background_blip = parse_blip_embed(e);
                                }
                            }
                            "a:srcRect" => {
                                if let Some(pic) = current_picture.as_mut() {
                                    pic.crop = parse_src_rect(e);
                                }
                            }
                            "a:t" => {
                                in_text_element = true;
                            }
                            "a:p" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.paras_seen += 1;
                                }
                            }
                            "a:r" | "a:fld" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.runs_seen += 1;
                                }
                            }
                            "a:rPr" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.cur_style = parse_run_style(e);
                                }
                                in_rpr = true;
                            }
                            _ => {}
                        }
                    }
                    Ok(Event::Empty(ref e)) => {
                        let name = e.name();
                        match name.as_ref() {
                            "a:chOff" | "a:chExt" if in_grp_pr => {
                                if let Some(g) = groups.last_mut() {
                                    g.note(e);
                                }
                            }
                            "a:br" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.line_break();
                                }
                            }
                            // See the master walker: `<a:p/>` is a blank line.
                            "a:p" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.pending_breaks += 1;
                                    shape.paras_seen += 1;
                                }
                            }
                            "a:r" | "a:fld" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.runs_seen += 1;
                                }
                            }
                            "a:rPr" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.cur_style = parse_run_style(e);
                                }
                            }
                            "a:srgbClr" if in_rpr || (in_bg && !in_bg_blip) => {
                                if let Some(val) = e
                                    .attributes()
                                    .filter_map(|a| a.ok())
                                    .find(|a| a.key.as_ref() == "val")
                                {
                                    let hex =
                                        val.value.to_lowercase();
                                    if in_rpr {
                                        if let Some(shape) = current_shape.as_mut() {
                                            shape.cur_style.color = Some(hex);
                                        }
                                    } else {
                                        background = format!("#{hex}");
                                    }
                                }
                            }
                            "a:off" => {
                                let (x, y) = parse_coords(e, "x", "y");
                                if let Some(shape) = current_shape.as_mut() {
                                    if x.is_some() { shape.x = x; }
                                    if y.is_some() { shape.y = y; }
                                } else if let Some(pic) = current_picture.as_mut() {
                                    if x.is_some() { pic.x = x; }
                                    if y.is_some() { pic.y = y; }
                                } else if in_grp_pr {
                                    if let Some(g) = groups.last_mut() {
                                        g.note(e);
                                    }
                                }
                            }
                            "a:ext" => {
                                let (w, h) = parse_coords(e, "cx", "cy");
                                if let Some(shape) = current_shape.as_mut() {
                                    if w.is_some() { shape.w = w; }
                                    if h.is_some() { shape.h = h; }
                                } else if let Some(pic) = current_picture.as_mut() {
                                    if w.is_some() { pic.w = w; }
                                    if h.is_some() { pic.h = h; }
                                } else if in_grp_pr {
                                    if let Some(g) = groups.last_mut() {
                                        g.note(e);
                                    }
                                }
                            }
                            "a:prstGeom" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    if let Some(prst) = parse_prst_geom(e) {
                                        shape.prst = Some(prst);
                                    }
                                }
                            }
                            "p:ph" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.ph = Some(PhKey::from_ph(e));
                                }
                            }
                            "p:cNvPr" => last_id = cnvpr_id(e),
                            "p:cNvSpPr" => {
                                if is_tx_box_attr(e) {
                                    if let Some(shape) = current_shape.as_mut() {
                                        shape.is_tx_box = true;
                                    }
                                }
                            }
                            "a:blip" => {
                                if let Some(pic) = current_picture.as_mut() {
                                    if let Some(embed) = parse_blip_embed(e) {
                                        pic.embed_id = Some(embed);
                                    }
                                } else if in_bg_blip {
                                    background_blip = parse_blip_embed(e);
                                }
                            }
                            "a:fillRect" if in_bg_blip => background_fill = parse_fill_rect(e),
                            "a:srcRect" => {
                                if let Some(pic) = current_picture.as_mut() {
                                    pic.crop = parse_src_rect(e);
                                }
                            }
                            "a:srgbClr" if in_bg && !in_bg_blip => {
                                if let Some(val) = e
                                    .attributes()
                                    .filter_map(|a| a.ok())
                                    .find(|a| a.key.as_ref() == "val")
                                {
                                    background = format!(
                                        "#{}",
                                        val.value.to_lowercase()
                                    );
                                }
                            }
                            // A theme-slot background (`a:schemeClr`),
                            // resolved against the slide's own master's
                            // theme rather than the first theme part.
                            "a:schemeClr" if in_bg && !in_bg_blip => {
                                if let Some(val) = e
                                    .attributes()
                                    .filter_map(|a| a.ok())
                                    .find(|a| a.key.as_ref() == "val")
                                {
                                    if let Some(color) = slide_theme.slot(&val.value.to_lowercase()) {
                                        background = format!("#{}", color.to_hex());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(Event::End(ref e)) => {
                        let name = e.name();
                        if name.as_ref() == "p:bg" {
                            in_bg = false;
                            in_bg_blip = false;
                        }
                        if name.as_ref() == "a:blipFill" {
                            in_bg_blip = false;
                        }
                        if name.as_ref() == "p:grpSpPr" {
                            in_grp_pr = false;
                        }
                        if name.as_ref() == "p:grpSp" {
                            groups.pop();
                        }
                        if name.as_ref() == "a:rPr" {
                            in_rpr = false;
                        }
                        if name.as_ref() == "p:cxnSp" {
                            // A connector: a line (or bent line) between
                            // two shapes, which the walker used to skip, so
                            // a deck's separator rules were not drawn
                            // (#1397). Its outline is its whole look.
                            let paint = cxn_paint.get(cxn_index).cloned().unwrap_or_default();
                            cxn_index += 1;
                            if let Some(shape) = current_shape.take() {
                                let (x, y, w, h) = in_groups(&groups, (shape.x.unwrap_or(0.0), shape.y.unwrap_or(0.0), shape.w.unwrap_or(0.0), shape.h.unwrap_or(0.0)));
                                let (x, y, w, h) = (x * scale.x, y * scale.y, w * scale.x, h * scale.y);
                                let kind = ShapeKind::from_prst(shape.prst.as_deref().unwrap_or("line"));
                                objects.push(SlideObject::Shape { kind, x, y, w, h, rotation: shape.rotation.unwrap_or(0.0), style: paint.style });
                            }
                        } else if name.as_ref() == "p:sp" {
                            // This shape's fill and outline, resolved by the
                            // shape_xml pass over the same part (Nth p:sp).
                            let paint = sp_paint.get(sp_index).cloned().unwrap_or_default();
                            let text_style = sp_text.get(sp_index);
                            let geometry = sp_geom.get(sp_index).cloned().flatten();
                            sp_index += 1;
                            if let Some(mut shape) = current_shape.take() {
                                let text = text_of(&shape.runs);
                                let has_text = !text.trim().is_empty();
                                // A title, subtitle or body placeholder is a
                                // layout slot (decks_core::layouts): kept even
                                // empty, when the editor shows its prompt and
                                // a show draws nothing.
                                let role = shape.ph.as_ref().and_then(|k| crate::layouts::Placeholder::from_pptx(Some(&k.ty)));
                                if let Some(key) = &shape.ph {
                                    // Any other empty placeholder (a date, a
                                    // footer) is a prompt, not content:
                                    // LibreOffice doesn't draw it, and neither
                                    // do we.
                                    if !has_text && !shape.is_tx_box && role.is_none() {
                                        buf.clear();
                                        continue;
                                    }
                                    if [shape.x, shape.y, shape.w, shape.h].iter().any(Option::is_none) {
                                        if let Some(r) = inherited_rect(key, layout_phs, master_phs) {
                                            shape.x = shape.x.or(Some(r.x));
                                            shape.y = shape.y.or(Some(r.y));
                                            shape.w = shape.w.or(Some(r.w));
                                            shape.h = shape.h.or(Some(r.h));
                                        }
                                    }
                                }
                                // Its kind: its own paths (a custom geometry,
                                // a path of no size in the shape's EMU), or
                                // its preset with the corner the paint read.
                                let (raw_w, raw_h) = (shape.w.unwrap_or(0.0), shape.h.unwrap_or(0.0));
                                let kind_of = |prst: Option<String>| match geometry.clone() {
                                    Some(mut paths) => {
                                        for p in &mut paths {
                                            if p.w <= 0.0 || p.h <= 0.0 {
                                                (p.w, p.h) = (raw_w.max(1.0), raw_h.max(1.0));
                                            }
                                        }
                                        ShapeKind::Freeform(paths)
                                    }
                                    None => {
                                        let mut kind = ShapeKind::from_prst(&prst.unwrap_or_else(|| "rect".to_string()));
                                        if let (ShapeKind::RoundRect { radius }, Some(adj)) = (&mut kind, paint.round_adj) {
                                            *radius = adj.clamp(0.0, 0.5);
                                        }
                                        kind
                                    }
                                };
                                let (x, y, w, h) = in_groups(&groups, (shape.x.unwrap_or(0.0), shape.y.unwrap_or(0.0), shape.w.unwrap_or(0.0), shape.h.unwrap_or(0.0)));
                                let (x, y, w, h) = (x * scale.x, y * scale.y, w * scale.x, h * scale.y);
                                
                                let rotation = shape.rotation.unwrap_or(0.0);

                                if shape.is_tx_box || (shape.has_tx_body && has_text) || role.is_some() {
                                    let (runs, mut body) = shape.resolve(text_style);
                                    body.placeholder = role;
                                    // The source placeholder this box fills,
                                    // so a later layout application matches
                                    // it by stable identity (DCK-006).
                                    body.placeholder_key = shape.ph.as_ref().map(|k| PlaceholderKey { ty: k.ty.clone(), idx: k.idx });
                                    // A shape holding text keeps its own
                                    // paint under the text: a navy tile's
                                    // white figures used to lose the tile.
                                    let painted = paint.style.fill.is_some() || paint.style.gradient.is_some() || paint.style.stroke.is_some();
                                    if painted {
                                        body.frame = Some(super::text_body::Frame { kind: kind_of(shape.prst.clone()), style: paint.style.clone() });
                                    }
                                    let blank = role.is_some() && !has_text;
                                    let text = if blank { String::new() } else { text };
                                    let runs = if blank { Vec::new() } else { runs };
                                    objects.push(SlideObject::TextBox { text, x, y, w, h, rotation, runs: scale.text_runs(&runs), body });
                                } else {
                                    // The shape as the file draws it: its own
                                    // preset and paint. It used to become a
                                    // plain Rect or a Circle of the width's
                                    // diameter, painted in the app's colours.
                                    let kind = kind_of(shape.prst.clone());
                                    objects.push(SlideObject::Shape { kind, x, y, w, h, rotation, style: paint.style });
                                }
                            }
                        } else if name.as_ref() == "p:graphicFrame" {
                            if let Some(t) = tables.iter().find(|t| t.index == frame_index) {
                                objects.push(SlideObject::Table {
                                    x: t.x * scale.x,
                                    y: t.y * scale.y,
                                    w: t.w * scale.x,
                                    h: t.h * scale.y,
                                    rotation: 0.0,
                                    table: t.table.clone(),
                                });
                            } else if let Some(c) = charts.iter().find(|c| c.index == frame_index) {
                                // The chart part the frame's relationship
                                // names, read on the same budget as the
                                // slide; a part that isn't there or isn't a
                                // chart we draw leaves no object.
                                let chart = slide_image_rels
                                    .get(&c.rel_id)
                                    .map(|t| part_path(&slide_dir.to_string_lossy(), t))
                                    .map(|p| archive.optional_part_to_string(&p, &mut budget))
                                    .and_then(|xml| super::chart::parse_chart_space(&xml));
                                if let Some(chart) = chart {
                                    objects.push(SlideObject::Chart {
                                        x: c.x * scale.x,
                                        y: c.y * scale.y,
                                        w: c.w * scale.x,
                                        h: c.h * scale.y,
                                        rotation: 0.0,
                                        chart,
                                    });
                                }
                            }
                            frame_index += 1;
                        } else if name.as_ref() == "p:pic" {
                            if let Some(pic) = current_picture.take() {
                                if let Some(embed_id) = pic.embed_id {
                                    let (x, y, w, h) = in_groups(&groups, (pic.x.unwrap_or(0.0), pic.y.unwrap_or(0.0), pic.w.unwrap_or(0.0), pic.h.unwrap_or(0.0)));
                                    let (x, y, w, h) = (x * scale.x, y * scale.y, w * scale.x, h * scale.y);
                                    
                                    if let Some(obj) = resolve_and_extract_picture(
                                        &embed_id,
                                        PictureRect { x, y, w, h, rotation: pic.rotation.unwrap_or(0.0), crop: pic.crop },
                                        &slide_image_rels,
                                        &mut archive,
                                        &mut budget,
                                    ) {
                                        objects.push(obj);
                                    }
                                }
                            }
                        } else if name.as_ref() == "a:t" {
                            in_text_element = false;
                        } else if name.as_ref() == "a:p" {
                            if let Some(shape) = current_shape.as_mut() {
                                shape.pending_breaks += 1;
                            }
                        }
                    }
                    Ok(Event::Text(ref e)) => {
                        if in_text_element {
                            {
                                let t = unescape_text(e);
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.push_run(t);
                                }
                            }
                        }
                    }
                    Ok(Event::GeneralRef(ref r)) => {
                        if in_text_element {
                            let t = resolve_general_ref(r);
                            if let Some(shape) = current_shape.as_mut() {
                                shape.push_run(t);
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
                let notes_xml = archive.optional_part_to_string(&notes_path, &mut budget);
                if !notes_xml.is_empty() {
                    notes = extract_notes_text(&notes_xml);
                }
                break;
            }
        }

        slide_layout_paths.push(layout_path);

        // The slide's stable id and one per object, minted once at import
        // (DCK-005). The builds below name these ids, never positions, so
        // a later reorder keeps every reference.
        let slide_id = fresh;
        fresh += 1;
        let mut ids = crate::ops::SlideIds { slide: slide_id, objects: Vec::new(), deleted: Vec::new() };
        for _ in &objects {
            ids.objects.push(fresh);
            fresh += 1;
        }
        // The background picture, read into the media cache like a slide's
        // pictures. A relationship naming no part leaves the slide without
        // one; the save's loss report names that dangling picture
        // (`loss::pptx_drops`, missing-pictures).
        let background_picture = background_blip
            .as_deref()
            .and_then(|id| part_picture(&mut archive, &mut budget, &target_path, &slide_rels_xml, id))
            .map(|path| crate::engine::BackgroundPicture { path, fill: background_fill });
        slides.push(Slide {
            // The name the slide carries, falling back to a positional
            // label. The fallback is why the loss was invisible: the reader
            // synthesised a plausible "Slide 1" and never looked for the
            // real name, so a round trip produced a title that merely
            // looked right. `parse_c_sld_name` already existed and was
            // already used for masters.
            title: parse_c_sld_name(&slide_xml)
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("Slide {}", slide_index + 1)),
            background,
            background_picture,
            objects,
            notes,
            // Resolved to a master id once the master parts below are
            // read; `None` meanwhile falls back to the first master.
            master_id: None,
            transition: parse_transition(&slide_xml),
            builds: super::timing::read_builds(&slide_xml, |spid| object_ids.iter().position(|id| *id == Some(spid)).and_then(|i| ids.objects.get(i).copied())),
            ids,
            layout_id: None,
            hidden: slide_is_hidden(&slide_xml),
        });
    }

    // ── Masters: one per slideMaster part, with all its layouts
    // (decks_core::layouts): the master's decorations, and each layout's own
    // background, decorations and placeholder places. A slide names its
    // master and its layout. ────────────────────────────────────────────
    let mut masters: Vec<MasterSlide> = Vec::new();
    {
        // Layout part -> (master, layout); `None` for a master this writer
        // wrote without layouts (one empty blank layout, which is no layout).
        let mut slot_of: std::collections::HashMap<String, (usize, Option<usize>)> = std::collections::HashMap::new();
        let read_part = |archive: &mut zip::ZipArchive<File>, budget: &mut ZipBudget, name: &str| -> String {
            archive.optional_part_to_string(name, budget)
        };
        let targets = |rels: &str, kind: &str| -> Vec<String> {
            rels.split("Target=\"")
                .skip(1)
                .filter_map(|s| s.split('"').next())
                .filter(|t| t.contains(kind))
                .map(|t| format!("ppt/{}", t.trim_start_matches("../")))
                .collect()
        };
        let rels_of = |archive: &mut zip::ZipArchive<File>, budget: &mut ZipBudget, part: &str| -> String {
            let dir = Path::new(part).parent().unwrap_or(Path::new("ppt"));
            let file = Path::new(part).file_name().unwrap_or_default().to_string_lossy().to_string();
            archive.optional_part_to_string(&format!("{}/_rels/{}.rels", dir.to_string_lossy(), file), budget)
        };
        for (i, layout_path) in slide_layout_paths.iter().enumerate() {
            let Some(layout_path) = layout_path else { continue };
            if !slot_of.contains_key(layout_path) {
                let layout_xml = read_part(&mut archive, &mut budget, layout_path);
                if layout_xml.is_empty() {
                    continue;
                }
                let layout_rels = rels_of(&mut archive, &mut budget, layout_path);
                let master_path = targets(&layout_rels, "slideMaster").into_iter().next();
                let master_xml = master_path.as_deref().map(|p| read_part(&mut archive, &mut budget, p)).unwrap_or_default();
                let master_rels = master_path.as_deref().map(|p| rels_of(&mut archive, &mut budget, p)).unwrap_or_default();
                // The theme this master inherits from (DCK-006): the part
                // its relationships name, cached by part path so the slides
                // above resolved the same theme. A master with no theme
                // relationship is what this writer used to emit, so the
                // absence resolves against the default theme rather than
                // failing — and the master's colour map rides on the
                // resolving copy, not on the retained part.
                let theme_path = targets(&master_rels, "theme").into_iter().next();
                let base_theme = theme_path.as_deref().map(|tp| {
                    if !themes.contains_key(tp) {
                        let tx = read_part(&mut archive, &mut budget, tp);
                        themes.insert(tp.to_string(), read_theme(&tx));
                    }
                    themes.get(tp).cloned().unwrap_or_default()
                }).unwrap_or_default();
                let color_map = parse_color_map(&master_xml);
                let resolving = base_theme.with_remap(&color_map);
                // The body font the theme answers with, kept absent (not
                // Sans) when the theme names none.
                let theme_font = resolving.fonts().1;
                let (master_bg, shapes) = match master_path.as_deref() {
                    Some(mp) => master_shapes(&master_xml, scale, &resolving, &mut |id| part_picture(&mut archive, &mut budget, mp, &master_rels, id)),
                    None => master_shapes(&master_xml, scale, &resolving, &mut |_| None),
                };
                // The master's layouts in its own order; a layout its master
                // doesn't list (or with no master at all) still belongs.
                let mut layout_paths = targets(&master_rels, "slideLayout");
                if !layout_paths.contains(layout_path) {
                    layout_paths.push(layout_path.clone());
                }
                let master_phs = parse_placeholders(&master_xml);
                let mut layouts = Vec::new();
                for lp in &layout_paths {
                    let xml = if lp == layout_path { layout_xml.clone() } else { read_part(&mut archive, &mut budget, lp) };
                    let rels = rels_of(&mut archive, &mut budget, lp);
                    let mut pictures = |id: &str| part_picture(&mut archive, &mut budget, lp, &rels, id);
                    layouts.push(read_layout(&xml, lp, &master_phs, master_bg.as_deref(), scale, &resolving, &mut pictures));
                }
                // What this writer wrote for a master with no layouts: one
                // empty blank layout. It is no layout.
                let none = layouts.len() == 1
                    && layouts[0].kind == crate::layouts::LayoutKind::Blank
                    && layouts[0].placeholders.is_empty()
                    && layouts[0].shapes.is_empty()
                    && layouts[0].background.is_none();
                if none {
                    layouts.clear();
                }
                // The master's own name if either part records one;
                // the layout's file stem only as a last resort, which is
                // all a package written before this existed offers.
                let name = parse_c_sld_name(&master_xml)
                    .or_else(|| parse_c_sld_name(&layout_xml))
                    .unwrap_or_else(|| {
                        Path::new(layout_path)
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Master".into())
                    });
                let mi = masters.len();
                // A layout-less master keeps a layout's background, as the
                // one blank layout carried the master's.
                let layout_bg = if none { parse_layout_background(&layout_xml, scale, &resolving) } else { None };
                // The retained theme entry, shared by masters that name the
                // same part: one entry per part, ids from the session space.
                let theme_id = theme_path.as_deref().map(|tp| {
                    *theme_id_of.entry(tp.to_string()).or_insert_with(|| {
                        let id = fresh;
                        fresh += 1;
                        let entry = themes.get(tp).cloned().unwrap_or_default().to_model(id, tp);
                        model_themes.push(entry);
                        id
                    })
                });
                masters.push(MasterSlide {
                    // Stamped with the session id below; 0 is never stored.
                    id: 0,
                    name,
                    // Absence stays empty (inherits): never white at import.
                    background: master_bg.or(layout_bg).unwrap_or_default(),
                    // Absence stays empty (draw-time Sans): never Sans at import.
                    default_font: theme_font.unwrap_or_default(),
                    shapes,
                    theme_id,
                    color_map,
                    layouts,
                });
                // Stable master, layout and placeholder ids, minted once
                // at import from the same session space as the slides.
                {
                    let m = &mut masters[mi];
                    m.id = fresh;
                    fresh += 1;
                    for l in m.layouts.iter_mut() {
                        l.id = fresh;
                        fresh += 1;
                        for p in l.placeholders.iter_mut() {
                            p.id = fresh;
                            fresh += 1;
                        }
                    }
                }
                for (j, lp) in layout_paths.iter().enumerate() {
                    slot_of.insert(lp.clone(), (mi, (!none).then_some(j)));
                }
            }
            // Slides name their master and layout by stable id (DCK-005):
            // the (master, layout) positions above only live inside this
            // import. A slide whose layout part resolved to nothing keeps
            // `None` and falls back to the first master.
            if let (Some(s), Some(&(mi, layout))) = (slides.get_mut(i), slot_of.get(layout_path)) {
                s.master_id = Some(masters[mi].id);
                s.layout_id = layout.and_then(|j| masters[mi].layouts.get(j)).map(|l| l.id);
            }
        }
    }
    if masters.is_empty() {
        masters.push(MasterSlide {
            // Stamped by the ensure below, once, with everything else.
            id: 0,
            name: "Default".into(),
            background: "#ffffff".into(),
            default_font: MasterSlide::DEFAULT_FONT.into(),
            shapes: vec![],
            theme_id: None,
            color_map: Default::default(),
            layouts: Vec::new(),
        });
    }

    if slides.is_empty() {
        slides.push(Slide { hidden: false,
            title: "Slide 1".into(),
            background: "#ffffff".into(),
            background_picture: None,
            objects: vec![],
            notes: String::new(),
            master_id: None,
            transition: Default::default(),
            builds: Vec::new(),
            ids: Default::default(),
            layout_id: None,
        });
    }

    // Safety net: anything the paths above left at 0 (a reader branch a
    // later edit adds) gets a session id here, still once per document.
    crate::ops::ensure_all_ids(&mut slides, &mut masters);
    crate::ops::ensure_theme_ids(&slides, &masters, &mut model_themes);
    Ok(Deck { slides, masters, page_size, themes: model_themes })
}

/// A shape's `p:cNvPr id`.
fn cnvpr_id(e: &BytesStart) -> Option<u32> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == "id")
        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::*;

    /// Reopening a pptx reuses the picture it already unpacked instead of
    /// leaving another temp file per picture per open (#455).
    #[test]
    fn reopening_a_pptx_does_not_unpack_its_picture_again() {
        let dir = tempfile::tempdir().unwrap();
        let png: &[u8] = &[
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D',
            b'R', 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 0x90, 0x77, 0x53, 0xde, 0, 0, 0,
            12, b'I', b'D', b'A', b'T', 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0, 0, 0, 3, 0,
            1, 0x9e, 0xdd, 0x22, 0x71, 0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42,
            0x60, 0x82,
        ];
        let src = dir.path().join("pic.png");
        std::fs::write(&src, png).unwrap();
        let mut deck = Deck::new();
        deck.slides[0].objects.push(SlideObject::Image {
            path: src.to_string_lossy().to_string(),
            x: 10.0, y: 20.0, w: 30.0, h: 40.0,
            rotation: 0.0, crop: Default::default()
        });
        let path = dir.path().join("pic.pptx");
        write_pptx(path.to_str().unwrap(), &deck).unwrap();

        let unpacked = |deck: &Deck| {
            let found: Vec<String> = deck.slides[0].objects.iter().filter_map(|o| match o {
                SlideObject::Image { path, .. } => Some(path.clone()),
                _ => None,
            }).collect();
            assert_eq!(found.len(), 1, "expected one picture, got {found:?}");
            found[0].clone()
        };
        let first = unpacked(&read_pptx(path.to_str().unwrap()).unwrap());
        assert_eq!(std::fs::read(&first).unwrap(), png);
        for _ in 0..5 {
            assert_eq!(unpacked(&read_pptx(path.to_str().unwrap()).unwrap()), first);
        }
        let cache = suite_common_core::media_cache::process_dir().unwrap();
        assert!(Path::new(&first).starts_with(&cache), "{first} not in {cache:?}");
    }

    #[test]
    fn test_pptx_roundtrip() {
        let mut deck = Deck::new();
        deck.slides[0].objects.push(SlideObject::TextBox {
            text: "Hello Slide".into(),
            x: 100.0, y: 100.0, w: 300.0, h: 50.0,
            runs: vec![],
            rotation: 0.0,
            body: Default::default(),
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

        // A rect and a circle come back as what the file holds: preset
        // shapes, in the colours they were written in.
        use crate::engine::shape::{Color, ShapeKind};
        match &slide.objects[1] {
            SlideObject::Shape { kind: ShapeKind::Rect, x, y, w, h, style, .. } => {
                assert!((x - 150.0).abs() < 0.1);
                assert!((y - 200.0).abs() < 0.1);
                assert!((w - 200.0).abs() < 0.1);
                assert!((h - 100.0).abs() < 0.1);
                assert_eq!(style.fill, Some(Color(0x4A, 0x90, 0xE2)));
            }
            other => panic!("Expected a rect shape, got {other:?}"),
        }

        match &slide.objects[2] {
            SlideObject::Shape { kind: ShapeKind::Ellipse, x, y, w, h, style, .. } => {
                // Circle (400, 300) r 50 is the box (350, 250, 100, 100).
                assert!((x - 350.0).abs() < 0.1 && (y - 250.0).abs() < 0.1);
                assert!((w - 100.0).abs() < 0.1 && (h - 100.0).abs() < 0.1);
                assert_eq!(style.fill, Some(Color(0xE0, 0x4F, 0x32)));
            }
            other => panic!("Expected an ellipse shape, got {other:?}"),
        }

        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod slide_size_tests {

    #[test]
    fn transitions_are_read_including_powerpoints_morph() {
        let sld = |t: &str| format!("<p:sld xmlns:p=\"p\"><p:cSld><p:spTree/></p:cSld>{t}</p:sld>");
        assert_eq!(parse_transition(&sld("")), Transition::None);
        assert_eq!(parse_transition(&sld("<p:transition spd=\"med\"><p:fade/></p:transition>")), Transition::Fade);
        assert_eq!(parse_transition(&sld("<p:transition><p:push dir=\"u\"/></p:transition>")), Transition::Push);
        assert_eq!(parse_transition(&sld("<p:transition><p:wipe/></p:transition>")), Transition::Wipe);
        assert_eq!(parse_transition(&sld("<p:transition><p:dissolve/></p:transition>")), Transition::Fade, "one we don't draw");
        // As PowerPoint writes Morph: the Choice first, a fade Fallback.
        let morph = "<mc:AlternateContent xmlns:mc=\"mc\"><mc:Choice xmlns:p159=\"x\" Requires=\"p159\">\
            <p:transition spd=\"slow\"><p159:morph option=\"byObject\"/></p:transition></mc:Choice>\
            <mc:Fallback><p:transition spd=\"slow\"><p:fade/></p:transition></mc:Fallback></mc:AlternateContent>";
        assert_eq!(parse_transition(&sld(morph)), Transition::MagicMove);
        // Sound on a transition is not a kind.
        assert_eq!(parse_transition(&sld("<p:transition><p:sndAc/><p:push/></p:transition>")), Transition::Push);
        assert_eq!(
            parse_transition(&sld("<p:transition><p:sndAc><p:stSnd><p:snd/></p:stSnd></p:sndAc><p:wipe/></p:transition>")),
            Transition::Wipe
        );
    }


    #[test]
    fn text_sizes_scale_with_the_slide_like_its_geometry() {
        use letters_core::model::{Run, RunStyle};
        // Our own 10in slide: sizes are untouched, including "no size".
        let ours = SlideScale::default();
        let runs = vec![Run { text: "a".into(), style: RunStyle { font_size_hp: Some(48), ..Default::default() } }];
        assert_eq!(ours.text_runs(&runs)[0].style.font_size_hp, Some(48));
        // A 13.33in (12192000 EMU) deck maps onto the 10in model at 0.75.
        let wide = SlideScale::from_emu(12_192_000.0, 6_858_000.0);
        assert!((wide.text_factor() - 0.75).abs() < 1e-9);
        let plain = vec![Run { text: "b".into(), style: RunStyle::default() }];
        assert_eq!(wide.text_runs(&runs)[0].style.font_size_hp, Some(36), "24 pt -> 18 pt");
        assert_eq!(wide.text_runs(&plain)[0].style.font_size_hp, Some(27), "the 18 pt default -> 13.5 pt");
    }

    use super::*;

    const MODERN: &str = "<p:presentation xmlns:p=\"p\">\
        <p:sldSz cx=\"12192000\" cy=\"6858000\"/></p:presentation>";

    #[test]
    fn the_declared_slide_size_is_read() {
        assert_eq!(parse_slide_size(MODERN), Some((12192000.0, 6858000.0)));
    }

    #[test]
    fn a_package_declaring_no_size_reads_as_none() {
        assert_eq!(parse_slide_size("<p:presentation xmlns:p=\"p\"/>"), None);
        assert_eq!(parse_slide_size(""), None);
    }

    /// A full-bleed shape is full-bleed whatever size the slide declares.
    /// On the current PowerPoint default this used to come back as
    /// 1280x720 in a 960x540 space.
    #[test]
    fn a_full_bleed_shape_fills_the_model_space_on_any_slide_size() {
        let scale = SlideScale::from_emu(12192000.0, 6858000.0);
        assert!((12192000.0 * scale.x - 960.0).abs() < 1e-6);
        assert!((6858000.0 * scale.y - 540.0).abs() < 1e-6);
    }

    /// The default has to be exactly what dividing by 9525 did, or every
    /// package that declares no size shifts the day this landed.
    #[test]
    fn the_default_is_the_old_fixed_divisor() {
        let d = SlideScale::default();
        for emu in [0.0, 9525.0, 190500.0, 9144000.0] {
            assert!((emu * d.x - emu / 9525.0).abs() < 1e-9, "x at {emu}");
            assert!((emu * d.y - emu / 9525.0).abs() < 1e-9, "y at {emu}");
        }
    }

    /// Both axes are normalised independently, and only a non-16:9 slide
    /// can show it: on any 16:9 source — including both common PowerPoint
    /// sizes — `960/cx` and `540/cy` are equal, so a mutation using the
    /// width factor for both passes against every other fixture here.
    ///
    /// A 4:3 deck cannot be represented faithfully in a fixed 16:9 model
    /// space at all. Normalising per axis keeps a full-bleed shape
    /// full-bleed and everything on-slide, at the cost of stretching it;
    /// the alternative keeps shapes square and pushes content off the
    /// bottom, which is the behaviour this replaced.
    #[test]
    fn a_four_three_slide_is_normalised_on_both_axes() {
        // 10in x 7.5in, the classic 4:3 slide.
        let scale = SlideScale::from_emu(9144000.0, 6858000.0);
        assert!((9144000.0 * scale.x - 960.0).abs() < 1e-6, "width");
        assert!((6858000.0 * scale.y - 540.0).abs() < 1e-6, "height");
        assert!(
            (scale.x - scale.y).abs() > 1e-12,
            "a 4:3 slide must not use one factor for both axes"
        );
    }

    /// A nonsense or zero extent falls back rather than dividing by it.
    #[test]
    fn a_zero_extent_falls_back_to_the_default() {
        let z = SlideScale::from_emu(0.0, 0.0);
        let d = SlideScale::default();
        assert!((z.x - d.x).abs() < 1e-12 && (z.y - d.y).abs() < 1e-12);
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
            SlideObject::Shape { kind: ShapeKind::Rect, x, y, w, h, .. } => {
                assert!((x - 2.0).abs() < 0.01 && (y - 3.0).abs() < 0.01);
                assert!((w - 20.0).abs() < 0.01 && (h - 10.0).abs() < 0.01);
            }
            other => panic!("expected rect, got {other:?}"),
        }
    }

    #[test]
    fn a_masters_decorations_keep_their_preset_fill_gradient_and_outline() {
        use crate::engine::shape::{Color, GradientStop, LinearGradient, Stroke};
        let xml = r##"<p:sldMaster xmlns:p="x" xmlns:a="y"><p:cSld><p:spTree>
            <p:sp><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="95250" cy="95250"/></a:xfrm>
              <a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="E01B24"/></a:solidFill><a:ln><a:noFill/></a:ln></p:spPr></p:sp>
            <p:sp><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="95250" cy="95250"/></a:xfrm>
              <a:prstGeom prst="ellipse"/><a:gradFill><a:gsLst><a:gs pos="0"><a:srgbClr val="0B3D6B"/></a:gs>
              <a:gs pos="100000"><a:srgbClr val="138D9C"/></a:gs></a:gsLst><a:lin ang="5400000"/></a:gradFill>
              <a:ln w="19050"><a:solidFill><a:srgbClr val="B55A3C"/></a:solidFill></a:ln></p:spPr></p:sp>
            </p:spTree></p:cSld></p:sldMaster>"##;
        let (_, shapes) = parse_master_shapes(xml);
        match &shapes[0] {
            SlideObject::Shape { kind: ShapeKind::Rect, style, .. } => {
                assert_eq!(style.fill, Some(Color(0xE0, 0x1B, 0x24)));
                assert_eq!(style.stroke, None);
            }
            other => panic!("expected the red bar, got {other:?}"),
        }
        match &shapes[1] {
            SlideObject::Shape { kind: ShapeKind::Ellipse, style, .. } => {
                let want = LinearGradient {
                    stops: vec![GradientStop { pos: 0.0, color: Color(0x0B, 0x3D, 0x6B) }, GradientStop { pos: 1.0, color: Color(0x13, 0x8D, 0x9C) }],
                    angle: 90.0,
                };
                assert_eq!(style.gradient.as_ref(), Some(&want));
                assert_eq!(style.stroke, Some(Stroke { color: Color(0xB5, 0x5A, 0x3C), width: 2.0 }));
            }
            other => panic!("expected the gradient ellipse, got {other:?}"),
        }
    }

    // ── notes_slide_xml ↔ extract_notes_text ─────────────────────────────

    /// Impress's pptx exporter writes the notes into a shape with no
    /// placeholder at all — one `p:sp` whose `p:nvPr` is empty. Requiring
    /// `p:ph type="body"` dropped the speaker notes of every deck that had
    /// been through it, and no test saw it because our own writer always
    /// emits the body placeholder we were looking for.
    #[test]
    fn notes_in_a_shape_with_no_placeholder_are_read() {
        let xml = r##"<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:cNvPr id="8" name=""/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/>
<p:txBody><a:p><a:r><a:t>remember the demo</a:t></a:r></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:notes>"##;
        assert_eq!(extract_notes_text(xml), "remember the demo");
    }

    /// But a placeholder that is *not* the body one is never notes. A slide
    /// number read as speaker notes would be worse than losing them, so the
    /// fallback widens only to shapes that declare no placeholder.
    #[test]
    fn a_slide_number_placeholder_is_never_read_as_notes() {
        let xml = r##"<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:nvPr><p:ph type="sldNum" idx="10"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>7</a:t></a:r></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:notes>"##;
        assert_eq!(extract_notes_text(xml), "");
    }

    /// And when a body placeholder is present it still wins, so a notes
    /// part carrying both does not concatenate them.
    #[test]
    fn the_body_placeholder_wins_over_an_unplaceheld_shape() {
        let xml = r##"<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:nvPr/></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>slide furniture</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:nvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>the real notes</a:t></a:r></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:notes>"##;
        assert_eq!(extract_notes_text(xml), "the real notes");
    }


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
    fn notes_blank_lines_survive_round_trip() {
        // Blank lines are the presenter's; they used to collapse.
        assert_eq!(extract_notes_text(&notes_slide_xml("a\n\nb")), "a\n\nb");
        assert_eq!(extract_notes_text(&notes_slide_xml("a\n")), "a\n");
        // A shape of nothing but blank paragraphs holds no notes.
        assert_eq!(extract_notes_text(&notes_slide_xml("\n")), "");
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
                // The decoration's styling is read, not discarded: this
                // asserted `runs.is_empty()` while the master walker parsed
                // text as plain, which is what let a styled master
                // decoration lose its emphasis on every open.
                assert_eq!(runs.len(), 1);
                assert_eq!(runs[0].text, "Deck title");
                assert!((x - 2.0).abs() < 0.01 && (y - 3.0).abs() < 0.01);
                assert!((w - 20.0).abs() < 0.01 && (h - 10.0).abs() < 0.01);
            }
            other => panic!("expected text box, got {other:?}"),
        }
    }

    /// A shape with one geometry, so each case differs only in its text.
    fn master_with(txbody: &str) -> Vec<SlideObject> {
        let xml = format!(
            r##"<p:sldMaster xmlns:p="x" xmlns:a="y"><p:cSld><p:spTree>
<p:sp><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="190500" cy="95250"/></a:xfrm></p:spPr>
<p:txBody>{txbody}</p:txBody></p:sp>
</p:spTree></p:cSld></p:sldMaster>"##
        );
        parse_master_shapes(&xml).1
    }

    fn text_and_runs(shapes: &[SlideObject]) -> (String, Vec<Run>) {
        match shapes.first() {
            Some(SlideObject::TextBox { text, runs, .. }) => (text.clone(), runs.clone()),
            other => panic!("expected a text box, got {other:?}"),
        }
    }

    /// Two runs inside ONE paragraph are one line.
    ///
    /// This is the case that was wrong: the walker recorded one entry per
    /// `a:t` and joined them with `\n`, so a decoration reading
    /// "Plain **Bold**" came back as two lines — and, because masters are
    /// written back from this text, the break was then saved into the file.
    #[test]
    fn master_runs_in_one_paragraph_do_not_become_separate_lines() {
        let (text, runs) = text_and_runs(&master_with(
            "<a:p><a:r><a:t>Plain </a:t></a:r>\
             <a:r><a:rPr b=\"1\"/><a:t>Bold</a:t></a:r></a:p>",
        ));
        assert_eq!(text, "Plain Bold", "runs of one paragraph are one line");
        assert_eq!(runs.len(), 2);
        assert!(!runs[0].style.bold);
        assert!(runs[1].style.bold, "the master's own emphasis is read");
    }

    /// Separate paragraphs ARE separate lines — the half that was already
    /// right, and which a fix for the case above could easily break.
    #[test]
    fn master_separate_paragraphs_stay_separate_lines() {
        let (text, runs) = text_and_runs(&master_with(
            "<a:p><a:r><a:t>Line one</a:t></a:r></a:p>\
             <a:p><a:r><a:t>Line two</a:t></a:r></a:p>",
        ));
        assert_eq!(text, "Line one\nLine two");
        assert_eq!(
            runs.iter().map(|r| r.text.as_str()).collect::<String>(),
            text,
            "concatenated runs must equal `text` (SlideObject::TextBox's invariant)"
        );
    }

    /// `a:t` content is significant, so a run's trailing space survives.
    /// Trimming it welds the words together, which the newline above was
    /// masking: "Plain " + "Bold" read back as "PlainBold".
    #[test]
    fn master_keeps_the_space_a_run_ends_on() {
        let (text, _) = text_and_runs(&master_with(
            "<a:p><a:r><a:t>Plain </a:t></a:r><a:r><a:t>Bold</a:t></a:r></a:p>",
        ));
        assert_eq!(text, "Plain Bold");
    }

    /// An empty paragraph *between* two others is a blank line and has to
    /// survive. Only a foreign file produces one — our own writer puts a
    /// whole multi-line box in a single `a:t` — so this is import-only, and
    /// counting the closes rather than flagging them is what keeps it.
    #[test]
    fn master_keeps_a_blank_line_between_two_paragraphs() {
        let (text, runs) = text_and_runs(&master_with(
            "<a:p><a:r><a:t>a</a:t></a:r></a:p><a:p/>\
             <a:p><a:r><a:t>b</a:t></a:r></a:p>",
        ));
        assert_eq!(text, "a\n\nb", "the blank line was swallowed");
        assert_eq!(
            runs.iter().map(|r| r.text.as_str()).collect::<String>(),
            text
        );
    }

    /// A trailing empty paragraph must not leave a dangling break, which is
    /// why the newline is applied when the next run arrives rather than
    /// when the paragraph closes.
    #[test]
    fn master_trailing_empty_paragraph_adds_no_line() {
        let (text, _) = text_and_runs(&master_with(
            "<a:p><a:r><a:t>Only line</a:t></a:r></a:p><a:p/>",
        ));
        assert_eq!(text, "Only line");
    }

}

#[cfg(test)]
mod theme_font_tests {
    use super::super::shape_xml::theme;

    /// The heading and body fonts differ, which is the only arrangement
    /// that can tell a correct reader from one that returns the first
    /// `a:latin` it sees. With both set to the same face this test would
    /// pass against either.
    #[test]
    fn the_body_font_is_read_not_the_heading_font() {
        let xml = "<a:theme><a:themeElements><a:fontScheme>\
            <a:majorFont><a:latin typeface=\"Heading Face\"/></a:majorFont>\
            <a:minorFont><a:latin typeface=\"Body Face\"/></a:minorFont>\
            </a:fontScheme></a:themeElements></a:theme>";
        assert_eq!(theme(xml).fonts(), (Some("Heading Face".into()), Some("Body Face".into())));
    }

    /// `a:cs` and `a:ea` sit beside `a:latin` inside the same scheme, and
    /// the writer emits them empty.
    #[test]
    fn the_latin_typeface_is_read_not_its_siblings() {
        let xml = "<a:fontScheme><a:minorFont>\
            <a:latin typeface=\"Body Face\"/><a:ea typeface=\"\"/>\
            <a:cs typeface=\"\"/></a:minorFont></a:fontScheme>";
        assert_eq!(theme(xml).fonts().1.as_deref(), Some("Body Face"));
    }

    #[test]
    fn an_empty_typeface_is_no_answer_rather_than_an_empty_one() {
        let xml = "<a:fontScheme><a:minorFont><a:latin typeface=\"\"/>\
            </a:minorFont></a:fontScheme>";
        assert_eq!(theme(xml).fonts(), (None, None));
    }

    /// A package written before the theme part existed — which is every
    /// pptx this writer produced until now — has to fall back, not fail.
    #[test]
    fn a_part_with_no_font_scheme_answers_nothing() {
        assert_eq!(theme("<a:theme><a:themeElements/></a:theme>").fonts(), (None, None));
        assert_eq!(theme("").fonts(), (None, None));
        assert_eq!(theme("not xml at all <<<").fonts(), (None, None));
    }
}

