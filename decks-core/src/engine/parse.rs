// parse.rs — pptx reading: the `read_pptx` orchestration.
//
// Module contract (#1668): only this reader (and `package`, on part text it
// hands over) owns ZIP traversal. The helpers in `xml`, `geometry`, `layout`,
// `master`, `pictures` and `transitions` take XML text or events and return
// model-neutral values; none of them depends on GTK.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of engine.rs (issue #247).

use super::model::*;
use super::notes::{extract_notes_text, parse_run_style};
use super::placeholders::{inherited_rect, parse_placeholders, PhKey, Placeholder};
use super::shape::ShapeKind;
use super::shape_xml::{cxn_styles, frame_charts, frame_tables, sp_geometries, sp_styles, table_style_bold, theme as read_theme, Theme};
use super::text_xml::{sp_texts, Inherited};
use crate::layouts::PlaceholderKey;

use std::fs::File;
use std::path::Path;
use quick_xml::events::Event;
use quick_xml::Reader;
use suite_common_core::document::{Run, RunStyle};
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
#[path = "parse/package.rs"]
mod package;
#[path = "parse/xml.rs"]
mod xml;
#[cfg(test)]
#[path = "parse/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "parse/slide_size_tests.rs"]
mod slide_size_tests;
#[cfg(test)]
#[path = "parse/master_tests.rs"]
mod master_tests;
#[cfg(test)]
#[path = "parse/theme_font_tests.rs"]
mod theme_font_tests;

pub(crate) use self::xml::{resolve_general_ref, run_link, unescape_text};
use self::xml::{close_paragraphs, cnvpr_id, text_of};

pub use geometry::{parse_slide_size, SlideScale};
pub use master::{parse_master_shapes, parse_master_shapes_scaled};
use self::geometry::{
    in_groups, is_tx_box_attr, parse_blip_embed, parse_coords, parse_prst_geom, parse_rotation,
    slide_is_hidden, GroupFrame,
};
use self::layout::{
    layout_path_of, layout_theme, parse_color_map, parse_override_color_map, parse_layout_background, read_layout,
    read_layout_placeholders,
};
use self::master::{master_shapes, parse_c_sld_name};
use self::pictures::{
    part_path, part_picture, parse_fill_rect, parse_src_rect, resolve_and_extract_picture,
    PendingPicture, PictureRect,
};
use self::transitions::parse_transition;


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
        let resolved: Vec<Option<RunStyle>> = self.run_refs.iter().map(|i| i.and_then(|i| st.runs.get(i)).map(|p| p.style())).collect();
        let runs = self
            .runs
            .iter()
            .zip(&resolved)
            .enumerate()
            .map(|(k, (r, own))| match own {
                // The resolved look, with the run's own link: a link is
                // never inherited, so the resolver does not carry it.
                Some(style) => Run { text: r.text.clone(), style: RunStyle { link: r.style.link.clone(), ..style.clone() } },
                // A run of breaks alone (`a:br` with no run before it in
                // its paragraph, a title opening on an empty line) takes
                // the look of the text it breaks, the next run's, else the
                // one before: PowerPoint sizes the break as its paragraph's
                // text. Left at the default size, an 18pt line led a 40pt
                // title and the title's exact pitch was measured against
                // it, spacing every line two and a half times too far
                // apart (ccrm-dgpod's cover, shrunk to 75% where Impress
                // draws 90%).
                None if r.text.chars().all(|c| c == super::LINE_BREAK || c == '\n') => {
                    match resolved[k + 1..].iter().flatten().next().or_else(|| resolved[..k].iter().rev().flatten().next()) {
                        Some(style) => Run { text: r.text.clone(), style: style.clone() },
                        None => r.clone(),
                    }
                }
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
    // The package's own table style definitions, where it carries them.
    let table_styles = table_style_bold(&archive.optional_part_to_string("ppt/tableStyles.xml", &mut budget));
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

    let slide_paths = package::slide_targets(&rels_xml)?;
    let ordered_slide_rids = package::ordered_slide_rids(&presentation_xml)?;


    // Theme parts by part path, and the theme part per layout: every slide
    // resolves its shapes and text against its own master's theme
    // (DCK-006), not the package's first theme part. A chain that names
    // no theme resolves against the default theme.
    let mut themes: std::collections::HashMap<String, Theme> = std::collections::HashMap::new();
    let mut layout_theme_paths: std::collections::HashMap<String, (Option<String>, std::collections::BTreeMap<String, String>)> =
        std::collections::HashMap::new();
    // The retained model themes (`Deck.themes`), in package order, and the
    // theme part path each id was read from: masters that share one part
    // share one entry. Built with the masters below.
    let mut model_themes: Vec<super::theme::Theme> = Vec::new();
    let mut theme_id_of: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    let mut slides = Vec::new();
    // Parts of slides that could not be read, in presentation order
    // (#1977); carried out on the deck.
    let mut skipped_slides: Vec<super::model::SkippedSlide> = Vec::new();
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

        // A slide whose part cannot be read is skipped, but never
        // silently (#1977): the skip is recorded on the deck, so the open
        // report and the save-time loss check can name the slide the save
        // would otherwise drop without a trace.
        let slide_xml = match archive.part_to_string(&target_path, &mut budget) {
            Ok(xml) => xml,
            Err(e) => {
                skipped_slides.push(super::model::SkippedSlide { part: target_path.clone(), reason: e.to_string() });
                continue;
            }
        };

        // Check if there's a slide relationship file (for images)
        let slide_dir = Path::new(&target_path).parent().unwrap_or(Path::new("ppt/slides"));
        let slide_filename = Path::new(&target_path).file_name().unwrap_or_default().to_string_lossy();
        let slide_rels_path = format!("{}/_rels/{}.rels", slide_dir.to_string_lossy(), slide_filename);
        
        let mut slide_rels_xml = archive.optional_part_to_string(&slide_rels_path, &mut budget);
        // SmartArt as the shapes PowerPoint drew for it, before any pass
        // reads the slide (diagram.rs). The drawings' own relationships
        // (a picture fill) join the slide's under fresh ids.
        let (slide_xml, drawing_rels) = super::diagram::inline(&slide_xml, &slide_rels_xml, &slide_dir.to_string_lossy(), |part| {
            archive.optional_part_to_string(part, &mut budget)
        });
        if !drawing_rels.is_empty() {
            match slide_rels_xml.rfind("</Relationships>") {
                Some(at) => slide_rels_xml.insert_str(at, &drawing_rels),
                None => slide_rels_xml = format!("<Relationships>{drawing_rels}</Relationships>"),
            }
        }

        let mut slide_image_rels = std::collections::HashMap::new();
        // The slide's external hyperlink targets by relationship id, for
        // the `a:hlinkClick`s on its text runs (#1998).
        let mut slide_link_rels: std::collections::HashMap<String, String> = std::collections::HashMap::new();
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
                            let mut external = false;
                            let mut hyperlink = false;
                            for attr in e.attributes().flatten() {
                                let value = || attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned());
                                match attr.key.as_ref() {
                                    "Id" => id = value(),
                                    "Target" => target = value(),
                                    "TargetMode" => external = value().as_deref() == Some("External"),
                                    "Type" => hyperlink = value().is_some_and(|t| t.ends_with("/hyperlink")),
                                    _ => {}
                                }
                            }
                            if let (Some(id_val), Some(target_val)) = (id, target) {
                                if hyperlink && external {
                                    slide_link_rels.insert(id_val.clone(), target_val.clone());
                                }
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
        // The slide's own colour map, when it overrides its layout's.
        let slide_remap = parse_override_color_map(&slide_xml);
        let slide_theme = if slide_remap.is_empty() { slide_theme } else { slide_theme.with_remap(&slide_remap) };
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
        let mut tables = frame_tables(&slide_xml, &slide_theme, &slide_link_rels, scale);
        for t in &mut tables {
            t.table.file_bold = t.table.style_id.as_deref().and_then(|id| table_styles.get(&id.trim().to_ascii_uppercase())).copied();
            t.table.cell_margins = Some((91440.0 * scale.x, 45720.0 * scale.y));
            for cell in t.table.rows.iter_mut().flatten() {
                cell.runs = scale.text_runs(&cell.runs);
                cell.margins = cell.margins.map(|(l, t, r, b)| (l * scale.x, t * scale.y, r * scale.x, b * scale.y));
            }
        }
        let charts = frame_charts(&slide_xml);
        let mut frame_index = 0usize;

        // The layout (and through it, the master) this slide's placeholders
        // inherit their geometry from. Read once per layout.
        if let Some(lp) = &layout_path {
            if !placeholder_cache.contains_key(lp) {
                let found = read_layout_placeholders(&mut archive, &mut budget, lp, &slide_theme, scale.x);
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
                                    // A mirrored picture (a figure facing the
                                    // other way): drawn flipped in its box.
                                    let on = |key: &str| e.attributes().flatten().any(|a| a.key.as_ref() == key && matches!(a.value.as_ref(), "1" | "true"));
                                    pic.crop.flip_h = on("flipH");
                                    pic.crop.flip_v = on("flipV");
                                } else if in_grp_pr {
                                    // The group's own rotation and flips (#1989).
                                    if let Some(g) = groups.last_mut() {
                                        g.note(e);
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
                            // A picture saved only as SVG (Office 2016+) names
                            // its part on `asvg:svgBlip` inside the blip's
                            // extensions; a PNG fallback on the blip itself,
                            // when there is one, is kept.
                            n if n.ends_with(":svgBlip") => {
                                if let Some(pic) = current_picture.as_mut().filter(|p| p.embed_id.is_none()) {
                                    pic.embed_id = parse_blip_embed(e);
                                } else if in_bg_blip && background_blip.is_none() {
                                    background_blip = parse_blip_embed(e);
                                }
                            }
                            "a:srcRect" => {
                                if let Some(pic) = current_picture.as_mut() {
                                    pic.crop = crate::engine::Crop { flip_h: pic.crop.flip_h, flip_v: pic.crop.flip_v, ..parse_src_rect(e) };
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
                                    // A link is the run's own: a next run
                                    // with no properties is not linked.
                                    shape.cur_style.link = None;
                                }
                            }
                            "a:rPr" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.cur_style = parse_run_style(e);
                                }
                                in_rpr = true;
                            }
                            "a:hlinkClick" if in_rpr => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.cur_style.link = run_link(e, &slide_link_rels);
                                }
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
                                    shape.cur_style.link = None;
                                }
                            }
                            "a:rPr" => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.cur_style = parse_run_style(e);
                                }
                            }
                            "a:hlinkClick" if in_rpr => {
                                if let Some(shape) = current_shape.as_mut() {
                                    shape.cur_style.link = run_link(e, &slide_link_rels);
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
                            // A picture saved only as SVG (Office 2016+) names
                            // its part on `asvg:svgBlip` inside the blip's
                            // extensions; a PNG fallback on the blip itself,
                            // when there is one, is kept.
                            n if n.ends_with(":svgBlip") => {
                                if let Some(pic) = current_picture.as_mut().filter(|p| p.embed_id.is_none()) {
                                    pic.embed_id = parse_blip_embed(e);
                                } else if in_bg_blip && background_blip.is_none() {
                                    background_blip = parse_blip_embed(e);
                                }
                            }
                            "a:fillRect" if in_bg_blip => background_fill = parse_fill_rect(e),
                            "a:srcRect" => {
                                if let Some(pic) = current_picture.as_mut() {
                                    pic.crop = crate::engine::Crop { flip_h: pic.crop.flip_h, flip_v: pic.crop.flip_v, ..parse_src_rect(e) };
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
                                let ((x, y, w, h), rotation) = in_groups(&groups, (shape.x.unwrap_or(0.0), shape.y.unwrap_or(0.0), shape.w.unwrap_or(0.0), shape.h.unwrap_or(0.0)), shape.rotation.unwrap_or(0.0));
                                let (x, y, w, h) = (x * scale.x, y * scale.y, w * scale.x, h * scale.y);
                                let kind = ShapeKind::from_prst(shape.prst.as_deref().unwrap_or("line"));
                                objects.push(SlideObject::Shape { kind, x, y, w, h, rotation, style: paint.style });
                            }
                        } else if name.as_ref() == "p:sp" {
                            // This shape's fill and outline, resolved by the
                            // shape_xml pass over the same part (Nth p:sp).
                            let paint = sp_paint.get(sp_index).cloned().unwrap_or_default();
                            let text_style = sp_text.get(sp_index);
                            let geometry = sp_geom.get(sp_index).cloned().flatten();
                            sp_index += 1;
                            if let Some(mut shape) = current_shape.take() {
                                // A placeholder that paints nothing itself is
                                // drawn with its layout's (or master's)
                                // placeholder fill and outline.
                                let paint = super::shape_xml::placeholder_or_own(paint, shape.ph.as_ref().and_then(|k| inherited.paint(k)));
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
                                let ((x, y, w, h), rotation) = in_groups(&groups, (shape.x.unwrap_or(0.0), shape.y.unwrap_or(0.0), shape.w.unwrap_or(0.0), shape.h.unwrap_or(0.0)), shape.rotation.unwrap_or(0.0));
                                let (x, y, w, h) = (x * scale.x, y * scale.y, w * scale.x, h * scale.y);

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
                                    objects.push(SlideObject::TextBox { text, x, y, w, h, rotation, runs: scale.text_runs(&runs), body: scale.text_body(body) });
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
                                    .and_then(|xml| super::chart::parse_chart_space(&super::shape_xml::inline_scheme_colors(&xml, &slide_theme)));
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
                                    let ((x, y, w, h), rotation) = in_groups(&groups, (pic.x.unwrap_or(0.0), pic.y.unwrap_or(0.0), pic.w.unwrap_or(0.0), pic.h.unwrap_or(0.0)), pic.rotation.unwrap_or(0.0));
                                    let (x, y, w, h) = (x * scale.x, y * scale.y, w * scale.x, h * scale.y);

                                    if let Some(obj) = resolve_and_extract_picture(
                                        &embed_id,
                                        PictureRect { x, y, w, h, rotation, crop: pic.crop },
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
                    // A layout's own colour map recolours its background
                    // and decorations.
                    let remap = parse_override_color_map(&xml);
                    let layout_theme = base_theme.with_remap(if remap.is_empty() { &color_map } else { &remap });
                    layouts.push(read_layout(&xml, lp, &master_phs, master_bg.as_deref(), scale, &layout_theme, &mut pictures));
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
    Ok(Deck { slides, masters, page_size, themes: model_themes, skipped_slides })
}

