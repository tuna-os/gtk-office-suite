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
use super::shape_xml::{frame_tables, sp_styles, theme as read_theme, Theme};
use super::text_xml::{sp_texts, Inherited};

use std::fs::File;
use std::io::Write;
use std::path::Path;
use quick_xml::events::{Event, BytesStart, BytesRef, BytesText};
use quick_xml::Reader;
use letters_core::model::{Run, RunStyle};
use suite_common_core::zip_guard::{BoundedArchive, ZipBudget};

fn parse_coords(
    e: &BytesStart,
    k1: &str,
    k2: &str
) -> (Option<f64>, Option<f64>) {
    let mut v1 = None;
    let mut v2 = None;
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == k1 {
            if let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                v1 = val.parse::<f64>().ok();
            }
        } else if attr.key.as_ref() == k2 {
            if let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                v2 = val.parse::<f64>().ok();
            }
        }
    }
    (v1, v2)
}

fn parse_blip_embed(e: &BytesStart) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == "r:embed" {
            if let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                return Some(val.into_owned());
            }
        }
    }
    None
}

fn parse_prst_geom(e: &BytesStart) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == "prst" {
            if let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                return Some(val.into_owned());
            }
        }
    }
    None
}

fn is_tx_box_attr(e: &BytesStart) -> bool {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == "txBox" {
            if let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                return val.as_ref() == "1";
            }
        }
    }
    false
}

/// `a:xfrm`'s `rot`, in degrees. OOXML stores it in sixtieth-thousandths of
/// a degree and omits the attribute for an unrotated shape, so absent and
/// zero mean the same thing here.
///
/// The writer emitted no `rot` at all until the model's rotation was
/// threaded through it, and this reader hardcoded `rotation: 0.0` at every
/// construction site — so a rotated shape saved as pptx came back square,
/// in the format an unsaved deck (and therefore its crash snapshot) uses by
/// default.
fn parse_rotation(e: &BytesStart) -> Option<f64> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() != "rot" {
            continue;
        }
        let val = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?;
        let deg = val.trim().parse::<f64>().ok()? / 60_000.0;
        return if deg.is_finite() { Some(deg) } else { None };
    }
    None
}

/// How a document's own slide size maps onto the model's 960x540 space.
///
/// The model's units are abstract (ADR 0004: "960x540 model units"), so a
/// coordinate only means something relative to the slide it sits on. Both
/// readers used to divide by a fixed constant instead — 9525 EMU per unit
/// here, one point per unit in odp — which is the *same* thing only for a
/// slide of exactly the size our own writer emits. A deck from current
/// PowerPoint or Impress declares 13.333in x 7.5in
/// (`sldSz cx="12192000" cy="6858000"`), and a full-bleed shape on one read
/// back as 1280x720 in a 960x540 space: a third too large, running off the
/// canvas. Every test missed it because every test round-trips through our
/// own writer, which always emits the one size the constant assumed.
#[derive(Clone, Copy, Debug)]
pub struct SlideScale {
    pub x: f64,
    pub y: f64,
}

impl SlideScale {
    /// The slide size our own writer emits: 9144000 x 5143500 EMU
    /// (10in x 5.625in), which is what dividing by 9525 assumed.
    pub const DEFAULT_EMU: (f64, f64) = (9144000.0, 5143500.0);

    /// Each axis is normalised against that axis's own extent, so a
    /// full-bleed shape stays full-bleed whatever the source's aspect
    /// ratio. For any 16:9 source — both common PowerPoint sizes — the two
    /// factors are equal and this is a uniform scale; only a 4:3 import is
    /// stretched, which is the best a fixed 16:9 model space can do with
    /// one, and is still an improvement on today's silent overflow.
    pub fn from_emu(cx: f64, cy: f64) -> Self {
        let (dx, dy) = Self::DEFAULT_EMU;
        let cx = if cx > 0.0 { cx } else { dx };
        let cy = if cy > 0.0 { cy } else { dy };
        SlideScale { x: 960.0 / cx, y: 540.0 / cy }
    }
}

impl SlideScale {
    /// The factor text sizes take onto the model's slide: the same one the
    /// horizontal geometry takes. The model is always our writer's
    /// 10-inch-wide slide, so a 13.33-inch deck's shapes shrink to fit it;
    /// its point sizes, left alone, came out 1.33x too big beside them
    /// (render lab decks/text-styles, decks/table).
    pub fn text_factor(&self) -> f64 {
        self.x * 9525.0
    }

    /// `runs` with every size mapped onto the model's slide. A run with no
    /// size of its own has the 18 pt default, so on a slide of another size
    /// it gets the scaled default explicitly.
    pub fn text_runs(&self, runs: &[Run]) -> Vec<Run> {
        let k = self.text_factor();
        if (k - 1.0).abs() < 1e-6 {
            return runs.to_vec();
        }
        runs.iter()
            .map(|r| {
                let mut r = r.clone();
                let hp = r.style.font_size_hp.unwrap_or(36) as f64;
                r.style.font_size_hp = Some((hp * k).round().clamp(1.0, u16::MAX as f64) as u16);
                r
            })
            .collect()
    }
}

impl Default for SlideScale {
    fn default() -> Self {
        let (cx, cy) = Self::DEFAULT_EMU;
        Self::from_emu(cx, cy)
    }
}

/// The slide extent a package declares, from `p:sldSz` in presentation.xml.
pub fn parse_slide_size(xml: &str) -> Option<(f64, f64)> {
    if xml.is_empty() {
        return None;
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e))
                if e.name().as_ref() == "p:sldSz" =>
            {
                let (cx, cy) = parse_coords(e, "cx", "cy");
                if let (Some(cx), Some(cy)) = (cx, cy) {
                    if cx > 0.0 && cy > 0.0 {
                        return Some((cx, cy));
                    }
                }
                return None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}

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
/// carry it, so nothing is recorded for it and a wholly empty paragraph
/// (a blank line) is still dropped — as it was before this.
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
    /// Record one `a:t`'s text as a run, breaking the line first if a
    /// paragraph closed since the last one.
    fn push_run(&mut self, text: String) {
        let cur = self.paras_seen.saturating_sub(1);
        if self.runs.is_empty() {
            self.line_paras.push(cur);
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

struct PendingPicture {
    embed_id: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
    rotation: Option<f64>,
}

/// The placeholders of a slide layout and of the master it belongs to, for
/// resolving slide placeholders that carry no geometry of their own.
fn read_layout_placeholders(
    archive: &mut zip::ZipArchive<File>,
    budget: &mut ZipBudget,
    layout_path: &str,
    theme: &Theme,
) -> (Vec<Placeholder>, Vec<Placeholder>, Inherited) {
    let layout_xml = archive.optional_part_to_string(layout_path, budget);
    let dir = Path::new(layout_path).parent().unwrap_or(Path::new("ppt"));
    let file = Path::new(layout_path).file_name().unwrap_or_default().to_string_lossy();
    let rels = archive.optional_part_to_string(&format!("{}/_rels/{}.rels", dir.to_string_lossy(), file), budget);
    let master_xml = rels
        .split("Target=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .find(|t| t.contains("slideMaster"))
        .map(|t| archive.optional_part_to_string(&format!("ppt/{}", t.trim_start_matches("../")), budget))
        .unwrap_or_default();
    let inherited = Inherited::read(&layout_xml, &master_xml, theme);
    (parse_placeholders(&layout_xml), parse_placeholders(&master_xml), inherited)
}

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
    let scale = match parse_slide_size(&presentation_xml) {
        Some((cx, cy)) => SlideScale::from_emu(cx, cy),
        None => SlideScale::default(),
    };

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

    // The deck's theme colours, which shapes reference through p:style and
    // schemeClr. A package without a theme part gets the default theme's.
    let theme = read_theme(&archive.optional_part_to_string("ppt/theme/theme1.xml", &mut budget));

    let mut slides = Vec::new();
    // Layout part path per slide (slide → layout → master mapping is
    // resolved after the slide loop, when `archive` is free again).
    let mut slide_layout_paths: Vec<Option<String>> = Vec::new();
    #[allow(clippy::type_complexity)]
    let mut placeholder_cache: std::collections::HashMap<String, (Vec<Placeholder>, Vec<Placeholder>, Inherited)> =
        std::collections::HashMap::new();

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

        let mut objects = Vec::new();
        let mut object_ids: Vec<Option<u32>> = Vec::new();
        let sp_paint = sp_styles(&slide_xml, &theme, scale.x);
        let mut sp_index = 0usize;
        // Tables live in p:graphicFrame, which the walker used to skip.
        let mut tables = frame_tables(&slide_xml, &theme);
        for t in &mut tables {
            t.table.cell_margins = Some((91440.0 * scale.x, 45720.0 * scale.y));
            for cell in t.table.rows.iter_mut().flatten() {
                cell.runs = scale.text_runs(&cell.runs);
            }
        }
        let mut frame_index = 0usize;

        // The layout (and through it, the master) this slide's placeholders
        // inherit their geometry from. Read once per layout.
        let layout_path = slide_image_rels
            .values()
            .find(|t| t.contains("slideLayout"))
            .map(|t| format!("ppt/{}", t.trim_start_matches("../")));
        if let Some(lp) = &layout_path {
            if !placeholder_cache.contains_key(lp) {
                let found = read_layout_placeholders(&mut archive, &mut budget, lp, &theme);
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
        let sp_text = sp_texts(&slide_xml, &theme, inherited, scale);

        // Parse slide XML using quick-xml event reader
        let mut background = String::from("#ffffff");
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
            let mut in_rpr = false;
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
                            "p:sp" => {
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
                            "a:srgbClr" if in_rpr || in_bg => {
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
                                }
                            }
                            "a:srgbClr" if in_bg => {
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
                            _ => {}
                        }
                    }
                    Ok(Event::End(ref e)) => {
                        let name = e.name();
                        if name.as_ref() == "p:bg" {
                            in_bg = false;
                        }
                        if name.as_ref() == "a:rPr" {
                            in_rpr = false;
                        }
                        if name.as_ref() == "p:sp" {
                            // This shape's fill and outline, resolved by the
                            // shape_xml pass over the same part (Nth p:sp).
                            let paint = sp_paint.get(sp_index).cloned().unwrap_or_default();
                            let text_style = sp_text.get(sp_index);
                            sp_index += 1;
                            if let Some(mut shape) = current_shape.take() {
                                let text = text_of(&shape.runs);
                                let has_text = !text.trim().is_empty();
                                if let Some(key) = &shape.ph {
                                    // An empty placeholder is a prompt in the
                                    // editor, not content: LibreOffice doesn't
                                    // draw it, and neither do we.
                                    if !has_text && !shape.is_tx_box {
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
                                let x = shape.x.unwrap_or(0.0) * scale.x;
                                let y = shape.y.unwrap_or(0.0) * scale.y;
                                let w = shape.w.unwrap_or(0.0) * scale.x;
                                let h = shape.h.unwrap_or(0.0) * scale.y;
                                
                                let rotation = shape.rotation.unwrap_or(0.0);

                                if shape.is_tx_box || (shape.has_tx_body && has_text) {
                                    let (runs, body) = shape.resolve(text_style);
                                    objects.push(SlideObject::TextBox { text, x, y, w, h, rotation, runs: scale.text_runs(&runs), body });
                                } else {
                                    // The shape as the file draws it: its own
                                    // preset and paint. It used to become a
                                    // plain Rect or a Circle of the width's
                                    // diameter, painted in the app's colours.
                                    let prst = shape.prst.unwrap_or_else(|| "rect".to_string());
                                    let mut kind = ShapeKind::from_prst(&prst);
                                    if let (ShapeKind::RoundRect { radius }, Some(adj)) = (&mut kind, paint.round_adj) {
                                        *radius = adj.clamp(0.0, 0.5);
                                    }
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
                            }
                            frame_index += 1;
                        } else if name.as_ref() == "p:pic" {
                            if let Some(pic) = current_picture.take() {
                                if let Some(embed_id) = pic.embed_id {
                                    let x = pic.x.unwrap_or(0.0) * scale.x;
                                    let y = pic.y.unwrap_or(0.0) * scale.y;
                                    let w = pic.w.unwrap_or(0.0) * scale.x;
                                    let h = pic.h.unwrap_or(0.0) * scale.y;
                                    
                                    if let Some(obj) = resolve_and_extract_picture(
                                        &embed_id,
                                        PictureRect { x, y, w, h, rotation: pic.rotation.unwrap_or(0.0) },
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
            objects,
            notes,
            master_idx: Some(0),
            transition: parse_transition(&slide_xml),
            builds: super::timing::read_builds(&slide_xml, |spid| object_ids.iter().position(|id| *id == Some(spid))),
        });
    }

    // ── Masters: one entry per distinct layout (master decorations +
    // layout decorations, placeholders skipped). ────────────────────────
    let mut masters: Vec<MasterSlide> = Vec::new();
    {
        let mut layout_to_idx: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let read_part = |archive: &mut zip::ZipArchive<File>, budget: &mut ZipBudget, name: &str| -> String {
            archive.optional_part_to_string(name, budget)
        };
        for (i, layout_path) in slide_layout_paths.iter().enumerate() {
            let Some(layout_path) = layout_path else { continue };
            let idx = if let Some(&idx) = layout_to_idx.get(layout_path) {
                idx
            } else {
                let layout_xml = read_part(&mut archive, &mut budget, layout_path);
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
                    &mut budget,
                    &format!("{}/_rels/{}.rels", dir.to_string_lossy(), file),
                );
                let master_path = rels
                    .split("Target=\"")
                    .skip(1)
                    .filter_map(|s| s.split('"').next())
                    .find(|t| t.contains("slideMaster"))
                    .map(|t| format!("ppt/{}", t.trim_start_matches("../")));
                let master_xml = master_path
                    .as_deref()
                    .map(|p| read_part(&mut archive, &mut budget, p))
                    .unwrap_or_default();

                // Master rels → theme part → body font. A master with no
                // theme relationship is what this writer used to emit, so
                // the absence has to fall back rather than fail.
                let theme_font = master_path
                    .as_deref()
                    .and_then(|mp| {
                        let mdir = Path::new(mp).parent().unwrap_or(Path::new("ppt"));
                        let mfile =
                            Path::new(mp).file_name().unwrap_or_default().to_string_lossy().to_string();
                        let mrels = read_part(
                            &mut archive,
                            &mut budget,
                            &format!("{}/_rels/{}.rels", mdir.to_string_lossy(), mfile),
                        );
                        mrels
                            .split("Target=\"")
                            .skip(1)
                            .filter_map(|s| s.split('"').next())
                            .find(|t| t.contains("theme"))
                            .map(|t| format!("ppt/{}", t.trim_start_matches("../")))
                    })
                    .map(|tp| read_part(&mut archive, &mut budget, &tp))
                    .and_then(|tx| parse_theme_font(&tx));

                let (master_bg, mut shapes) = parse_master_shapes_scaled(&master_xml, scale);
                let (layout_bg, layout_shapes) = parse_master_shapes_scaled(&layout_xml, scale);
                shapes.extend(layout_shapes);
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
                masters.push(MasterSlide {
                    name,
                    background: layout_bg
                        .or(master_bg)
                        .unwrap_or_else(|| "#ffffff".into()),
                    default_font: theme_font
                        .unwrap_or_else(|| MasterSlide::DEFAULT_FONT.into()),
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
            default_font: MasterSlide::DEFAULT_FONT.into(),
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
            transition: Default::default(),
            builds: Vec::new(),
        });
    }

    Ok(Deck { slides, masters })
}

/// A shape's `p:cNvPr id`.
fn cnvpr_id(e: &BytesStart) -> Option<u32> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == "id")
        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.parse().ok())
}

/// A slide's `p:transition`. PowerPoint writes Morph inside
/// `mc:AlternateContent`: a `p159:morph` Choice and a plain Fallback
/// (usually a fade), so the first transition kind seen wins and Morph,
/// read first, is not overwritten by its fallback. A kind we don't draw
/// reads as a dissolve: the slide still changes with a transition.
pub fn parse_transition(xml: &str) -> Transition {
    let mut reader = Reader::from_str(xml);
    // Nesting below the open p:transition (0: not in one). Only its direct
    // children name the kind; p:sndAc, p:extLst and their insides don't.
    let mut level = 0usize;
    let mut found: Option<Transition> = None;
    let consider = |name: &str, level: usize, found: &mut Option<Transition>| {
        if level == 1 && found.is_none() && name != "p:sndAc" && name != "p:extLst" {
            *found = Some(transition_kind(name));
        }
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if e.name().as_ref() == "p:transition" {
                    level = 1;
                } else if level > 0 {
                    consider(e.name().as_ref(), level, &mut found);
                    level += 1;
                }
            }
            Ok(Event::Empty(e)) => {
                if level > 0 {
                    consider(e.name().as_ref(), level, &mut found);
                }
            }
            Ok(Event::End(_)) if level > 0 => level -= 1,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    found.unwrap_or_default()
}

fn transition_kind(name: &str) -> Transition {
    match name {
        n if n.ends_with(":morph") => Transition::MagicMove,
        "p:push" | "p:cover" | "p:pull" => Transition::Push,
        "p:wipe" | "p:wedge" | "p:split" => Transition::Wipe,
        _ => Transition::Fade,
    }
}

/// The `p:cSld/@name` of a slideMaster or slideLayout part.
///
/// This is the master's own name, and it is what the writer puts there.
/// Without it the name fell out of the part's file path — "slideLayout1"
/// for a deck we wrote, so a master called "House Style" came back under a
/// name nobody chose, and the sidebar showed it.
/// The body font a theme part names, from `a:fontScheme/a:minorFont/a:latin`.
///
/// `a:latin` appears under both `a:majorFont` (headings) and `a:minorFont`
/// (body), so a search for the first `a:latin` in the part returns the
/// heading font instead — right-looking and wrong whenever the two differ.
/// This tracks which scheme it is inside and only answers for the body one,
/// which is the font a reader applies to ordinary text.
pub fn parse_theme_font(xml: &str) -> Option<String> {
    if xml.is_empty() {
        return None;
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_minor = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) if e.name().as_ref() == "a:minorFont" => in_minor = true,
            Ok(Event::End(ref e)) if e.name().as_ref() == "a:minorFont" => in_minor = false,
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e))
                if in_minor && e.name().as_ref() == "a:latin" =>
            {
                return e
                    .attributes()
                    .filter_map(|a| a.ok())
                    .find(|a| a.key.as_ref() == "typeface")
                    .map(|a| a.value.to_string())
                    .filter(|f| !f.trim().is_empty());
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}

pub fn parse_c_sld_name(xml: &str) -> Option<String> {
    if xml.is_empty() {
        return None;
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    while let Ok(ev) = reader.read_event_into(&mut buf) {
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) if e.name().as_ref() == "p:cSld" => {
                return e
                    .attributes()
                    .filter_map(|a| a.ok())
                    .find(|a| a.key.as_ref() == "name")
                    // Normalised, not raw: a name containing `&` or `<` is
                    // escaped in the attribute, and reading the bytes back
                    // verbatim returns "R&amp;D" as the name itself.
                    .and_then(|a| {
                        a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .ok()
                            .map(|v| v.to_string())
                    })
                    .filter(|n| !n.is_empty());
            }
            Event::Eof => break,
            _ => {}
        }
    }
    None
}

/// Parse a slideMaster/slideLayout part: background color and
/// non-placeholder decoration shapes. Placeholder shapes (`p:ph` —
/// "Click to edit Master title style" and friends) are styling slots,
/// not content, and are skipped.
pub fn parse_master_shapes(xml: &str) -> (Option<String>, Vec<SlideObject>) {
    parse_master_shapes_scaled(xml, SlideScale::default())
}

/// As `parse_master_shapes`, against the slide size the package declares.
pub fn parse_master_shapes_scaled(
    xml: &str,
    scale: SlideScale,
) -> (Option<String>, Vec<SlideObject>) {
    if xml.is_empty() {
        return (None, Vec::new());
    }
    let mut reader = Reader::from_str(xml);
    // Untrimmed for the same reason as the slide walker above.
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut background: Option<String> = None;
    let mut shapes = Vec::new();
    let mut in_bg = false;
    let mut in_text = false;
    let mut in_rpr = false;
    // (x, y, w, h, prst, has_ph, text)
    struct Pending {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        prst: Option<String>,
        has_ph: bool,
        runs: Vec<Run>,
        pending_breaks: usize,
        cur_style: RunStyle,
    }
    impl Pending {
        fn push_run(&mut self, text: String) {
            close_paragraphs(&mut self.runs, self.pending_breaks);
            self.pending_breaks = 0;
            self.runs.push(Run { text, style: self.cur_style.clone() });
        }
    }
    let mut cur: Option<Pending> = None;
    while let Ok(ev) = reader.read_event_into(&mut buf) {
        match ev {
            // `<a:p/>` arrives as one Empty event rather than Start+End, so
            // the End arm below never sees it — and an empty paragraph is
            // exactly the blank line that must not be swallowed.
            Event::Empty(ref e) if e.name().as_ref() == "a:p" => {
                if let Some(p) = cur.as_mut() {
                    p.pending_breaks += 1;
                }
            }
            Event::Start(ref e) | Event::Empty(ref e) => match e.name().as_ref() {
                "p:bg" => in_bg = true,
                "p:sp" => {
                    cur = Some(Pending {
                        x: 0.0,
                        y: 0.0,
                        w: 0.0,
                        h: 0.0,
                        prst: None,
                        has_ph: false,
                        runs: Vec::new(),
                        pending_breaks: 0,
                        cur_style: RunStyle::default(),
                    });
                }
                "p:ph" => {
                    if let Some(p) = cur.as_mut() {
                        p.has_ph = true;
                    }
                }
                "a:off" => {
                    if let Some(p) = cur.as_mut() {
                        let (x, y) = parse_coords(e, "x", "y");
                        if let Some(x) = x {
                            p.x = x * scale.x;
                        }
                        if let Some(y) = y {
                            p.y = y * scale.y;
                        }
                    }
                }
                "a:ext" => {
                    if let Some(p) = cur.as_mut() {
                        let (w, h) = parse_coords(e, "cx", "cy");
                        if let Some(w) = w {
                            p.w = w * scale.x;
                        }
                        if let Some(h) = h {
                            p.h = h * scale.y;
                        }
                    }
                }
                "a:prstGeom" => {
                    if let Some(p) = cur.as_mut() {
                        p.prst = parse_prst_geom(e);
                    }
                }
                "a:t" => in_text = true,
                "a:rPr" => {
                    if let Some(p) = cur.as_mut() {
                        p.cur_style = parse_run_style(e);
                    }
                    in_rpr = true;
                }
                "a:srgbClr" if in_rpr => {
                    if let Some(p) = cur.as_mut() {
                        if let Some(val) = e
                            .attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == "val")
                        {
                            // Lowercase and without the '#', matching the
                            // slide walker and `RunStyle`'s documented form.
                            p.cur_style.color = Some(val.value.to_lowercase());
                        }
                    }
                }
                "a:srgbClr" if in_bg => {
                    if let Some(val) = e
                        .attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| a.key.as_ref() == "val")
                    {
                        background =
                            Some(format!("#{}", val.value.to_lowercase()));
                    }
                }
                _ => {}
            },
            Event::End(ref e) => match e.name().as_ref() {
                "p:bg" => in_bg = false,
                "a:t" => in_text = false,
                "a:rPr" => in_rpr = false,
                "a:p" => {
                    if let Some(p) = cur.as_mut() {
                        p.pending_breaks += 1;
                    }
                }
                "p:sp" => {
                    if let Some(p) = cur.take() {
                        if !p.has_ph && p.w > 0.0 && p.h > 0.0 {
                            let text = text_of(&p.runs);
                            let has_text = !text.trim().is_empty();
                            if has_text {
                                shapes.push(SlideObject::TextBox {
                                    text,
                                    x: p.x,
                                    y: p.y,
                                    w: p.w,
                                    h: p.h,
                                    rotation: 0.0,
                                    runs: p.runs,
                                    body: Default::default(),
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
                        p.push_run(unescape_text(t));
                    }
                }
            }
            Event::GeneralRef(ref r) => {
                if in_text {
                    if let Some(p) = cur.as_mut() {
                        p.push_run(resolve_general_ref(r));
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

/// Where a picture sits on the slide, in points. Four loose `f64`s in a
/// row were easy to transpose at the call site and pushed the argument
/// list past what clippy will accept once the read budget joined it.
struct PictureRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    rotation: f64,
}

fn resolve_and_extract_picture(
    embed_id: &str,
    rect: PictureRect,
    rels: &std::collections::HashMap<String, String>,
    archive: &mut zip::ZipArchive<File>,
    budget: &mut ZipBudget,
) -> Option<SlideObject> {
    let PictureRect { x, y, w, h, rotation } = rect;
    let target = rels.get(embed_id)?;
    let relative_path = target.trim_start_matches("../");
    let full_zip_path = format!("ppt/{}", relative_path);

    // An image is the largest part a real deck has and the easiest to lie
    // about, so it draws on the same budget as the XML parts.
    let buffer = archive.part_to_bytes(&full_zip_path, budget).ok()?;

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
        rotation,
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
            SlideObject::Rect { x, y, w, h, .. } => {
                assert!((x - 2.0).abs() < 0.01 && (y - 3.0).abs() < 0.01);
                assert!((w - 20.0).abs() < 0.01 && (h - 10.0).abs() < 0.01);
            }
            other => panic!("expected rect, got {other:?}"),
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
    use super::parse_theme_font;

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
        assert_eq!(parse_theme_font(xml).as_deref(), Some("Body Face"));
    }

    /// `a:cs` and `a:ea` sit beside `a:latin` inside the same scheme, and
    /// the writer emits them empty.
    #[test]
    fn the_latin_typeface_is_read_not_its_siblings() {
        let xml = "<a:fontScheme><a:minorFont>\
            <a:latin typeface=\"Body Face\"/><a:ea typeface=\"\"/>\
            <a:cs typeface=\"\"/></a:minorFont></a:fontScheme>";
        assert_eq!(parse_theme_font(xml).as_deref(), Some("Body Face"));
    }

    #[test]
    fn an_empty_typeface_is_no_answer_rather_than_an_empty_one() {
        let xml = "<a:fontScheme><a:minorFont><a:latin typeface=\"\"/>\
            </a:minorFont></a:fontScheme>";
        assert_eq!(parse_theme_font(xml), None);
    }

    /// A package written before the theme part existed — which is every
    /// pptx this writer produced until now — has to fall back, not fail.
    #[test]
    fn a_part_with_no_font_scheme_answers_nothing() {
        assert_eq!(parse_theme_font("<a:theme><a:themeElements/></a:theme>"), None);
        assert_eq!(parse_theme_font(""), None);
        assert_eq!(parse_theme_font("not xml at all <<<"), None);
    }
}

