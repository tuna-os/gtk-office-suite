// odp.rs — OpenDocument Presentation (.odp) read/write for the Decks
// model (roadmap item 7; the LO-native format, like ODT for Letters).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Slide coordinates are the model's 960x540 units, written as points
// (960pt x 540pt is the standard 16:9 presentation size). Scope:
// text boxes (with per-run bold/italic/underline/size/color), rects,
// circles, speaker notes, slide backgrounds, multi-slide order.
// Fidelity is measured against the Impress oracle, never ported.

use letters_core::model::{Run, RunStyle};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::Write;

use crate::engine::{Deck, MasterSlide, Slide, SlideObject};
use suite_common_core::zip_guard::{BoundedArchive, ZipBudget};

const MIMETYPE: &str = "application/vnd.oasis.opendocument.presentation";

// ── Rotation, and the ODF transform convention ───────────────────────
//
// ODF has no rotation attribute. It spells the same thing OOXML spells as
// one `a:xfrm/@rot` as a `draw:transform` list, which entangles rotation
// with position — so writing it needs the exact convention, and the
// convention is not obvious from the spec text.
//
// It was settled by probing Impress rather than by reading the spec. A
// 300x100 rect at (100, 200) that our pptx writer gives `rot="1800000"`
// (30 degrees clockwise) comes back from an Impress pptx-to-odp conversion
// as, in Impress's own units,
//
//     svg:width="7.937cm" svg:height="2.645cm"
//     draw:transform="rotate (-0.523598775598299) translate (3.839cm 3.485cm)"
//
// with `svg:x`/`svg:y` *absent*. Three things follow, and all three were
// checked against a second shape with different geometry in the same file:
//
//   * ODF's angle is radians counter-clockwise where OOXML's `rot` is
//     hundred-thousandths of a degree clockwise, so the ODF angle is the
//     plain negation of our model's degrees — and Impress leaves it
//     negative rather than normalising it into [0, 2pi).
//   * The matrix for `rotate (a)` is
//         x' =  x * cos a + y * sin a
//         y' = -x * sin a + y * cos a
//     which is a counter-clockwise turn in a y-down space, not SVG's
//     `rotate`.
//   * The terms apply left to right, so `rotate (a) translate (t)` maps a
//     local point p to R(a) * p + t. (Right-to-left, SVG's order, puts both
//     probe shapes in the wrong place, so this is not a coin toss.)
//
// The shape's local box is (0, 0)-(w, h), and OOXML rotates about the
// shape's centre, so the translate is `centre - R(a) * (w/2, h/2)`.

/// Apply the ODF `rotate (a)` matrix to a point.
fn rotate_point(a: f64, x: f64, y: f64) -> (f64, f64) {
    let (s, c) = a.sin_cos();
    (x * c + y * s, -x * s + y * c)
}

/// Round to a millionth of a point, so a value that came back through the
/// trigonometry above is bit-identical on the next save. Without it
/// `odp_geometry_does_not_drift_across_repeated_saves` would fail on the
/// last digit or two of every rotated shape.
fn snap(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// The geometry attributes for a shape whose unrotated box is
/// `(x, y, w, h)` and which the model turns `rotation` degrees clockwise.
///
/// An unrotated shape keeps plain `svg:x`/`svg:y`, both because that is
/// what every reader handles and because it keeps the bytes of the decks
/// this suite has already written unchanged.
fn geometry(x: f64, y: f64, w: f64, h: f64, rotation: f64) -> String {
    if rotation == 0.0 {
        return format!(
            "svg:x=\"{x}pt\" svg:y=\"{y}pt\" svg:width=\"{w}pt\" svg:height=\"{h}pt\""
        );
    }
    let a = -rotation.to_radians();
    let (rx, ry) = rotate_point(a, w / 2.0, h / 2.0);
    let (tx, ty) = (x + w / 2.0 - rx, y + h / 2.0 - ry);
    format!(
        "svg:width=\"{w}pt\" svg:height=\"{h}pt\" \
         draw:transform=\"rotate ({a}) translate ({tx}pt {ty}pt)\""
    )
}

/// Split a `draw:transform` value into its `(name, args)` terms.
fn transform_terms(v: &str) -> Option<Vec<(String, Vec<String>)>> {
    let mut terms = Vec::new();
    let mut rest = v.trim();
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let close = rest.find(')')?;
        if close < open {
            return None;
        }
        let name = rest[..open].trim().to_string();
        let args = rest[open + 1..close]
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        terms.push((name, args));
        rest = rest[close + 1..].trim_start();
    }
    Some(terms)
}

/// Recover `(x, y, rotation-degrees)` from a `draw:transform`, given the
/// box size the same element carries in `svg:width`/`svg:height`.
///
/// Returns `None` for a transform this does not fully understand — a
/// `scale` or a `skewX`, which Impress writes for shapes we do not produce
/// — so the caller falls back to `svg:x`/`svg:y` rather than placing the
/// shape somewhere confidently wrong.
fn parse_transform(v: &str, w: f64, h: f64) -> Option<(f64, f64, f64)> {
    let mut angle = 0.0f64;
    // The local centre, carried through the list so its image is the
    // shape's centre on the page.
    let (mut px, mut py) = (w / 2.0, h / 2.0);
    for (name, args) in transform_terms(v)? {
        match (name.as_str(), args.len()) {
            ("rotate", 1) => {
                let a: f64 = args[0].parse().ok()?;
                (px, py) = rotate_point(a, px, py);
                angle += a;
            }
            ("translate", 1 | 2) => {
                px += parse_length_pt(&args[0])?;
                py += args.get(1).map_or(Some(0.0), |a| parse_length_pt(a))?;
            }
            _ => return None,
        }
    }
    Some((
        snap(px - w / 2.0),
        snap(py - h / 2.0),
        snap((-angle.to_degrees()).rem_euclid(360.0)),
    ))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── Writing ──────────────────────────────────────────────────────────

fn run_span(run: &Run, style_idx: usize) -> String {
    if run.style == RunStyle::default() {
        esc(&run.text)
    } else {
        format!("<text:span text:style-name=\"T{style_idx}\">{}</text:span>", esc(&run.text))
    }
}

fn text_style(st: &RunStyle) -> String {
    let mut props = String::new();
    if st.bold {
        props.push_str(" fo:font-weight=\"bold\"");
    }
    if st.italic {
        props.push_str(" fo:font-style=\"italic\"");
    }
    if st.underline {
        props.push_str(" style:text-underline-style=\"solid\"");
    }
    if st.strikethrough {
        props.push_str(" style:text-line-through-style=\"solid\"");
    }
    if let Some(hp) = st.font_size_hp {
        props.push_str(&format!(" fo:font-size=\"{}pt\"", hp as f32 / 2.0));
    }
    if let Some(c) = &st.color {
        props.push_str(&format!(" fo:color=\"#{c}\""));
    }
    props
}

fn content_xml(deck: &Deck) -> String {
    // Distinct run styles across the deck, in first-use order.
    let mut styles: Vec<RunStyle> = Vec::new();
    for slide in &deck.slides {
        for obj in &slide.objects {
            if let SlideObject::TextBox { runs, .. } = obj {
                for r in runs {
                    if r.style != RunStyle::default() && !styles.contains(&r.style) {
                        styles.push(r.style.clone());
                    }
                }
            }
        }
    }

    let mut auto = String::new();
    for (i, st) in styles.iter().enumerate() {
        auto.push_str(&format!(
            "<style:style style:name=\"T{}\" style:family=\"text\">\
             <style:text-properties{}/></style:style>",
            i + 1,
            text_style(st)
        ));
    }
    // Per-slide drawing-page styles carry the background fill.
    for (i, slide) in deck.slides.iter().enumerate() {
        let bg = slide.background.trim_start_matches('#');
        if bg.len() == 6 && !bg.eq_ignore_ascii_case("ffffff") {
            auto.push_str(&format!(
                "<style:style style:name=\"dp{}\" style:family=\"drawing-page\">\
                 <style:drawing-page-properties draw:fill=\"solid\" \
                 draw:fill-color=\"#{}\"/></style:style>",
                i + 1,
                bg.to_lowercase()
            ));
        }
    }

    let style_of = |st: &RunStyle| styles.iter().position(|s| s == st).map(|i| i + 1).unwrap_or(0);

    let mut pages = String::new();
    for (si, slide) in deck.slides.iter().enumerate() {
        let bg = slide.background.trim_start_matches('#');
        let dp_attr = if bg.len() == 6 && !bg.eq_ignore_ascii_case("ffffff") {
            format!(" draw:style-name=\"dp{}\"", si + 1)
        } else {
            String::new()
        };
        pages.push_str(&format!(
            "<draw:page draw:name=\"{}\"{dp_attr}>",
            esc(&slide.title)
        ));
        for obj in &slide.objects {
            match obj {
                SlideObject::TextBox { text, x, y, w, h, rotation, runs } => {
                    let inner: String = if runs.is_empty() {
                        text.split('\n')
                            .map(|l| format!("<text:p>{}</text:p>", esc(l)))
                            .collect()
                    } else {
                        format!(
                            "<text:p>{}</text:p>",
                            runs.iter().map(|r| run_span(r, style_of(&r.style))).collect::<String>()
                        )
                    };
                    pages.push_str(&format!(
                        "<draw:frame {}>\
                         <draw:text-box>{inner}</draw:text-box></draw:frame>",
                        geometry(*x, *y, *w, *h, *rotation)
                    ));
                }
                SlideObject::Rect { x, y, w, h, rotation } => {
                    pages.push_str(&format!(
                        "<draw:rect {}/>",
                        geometry(*x, *y, *w, *h, *rotation)
                    ));
                }
                SlideObject::Circle { x, y, r, rotation } => {
                    let (cx, cy, d) = (x - r, y - r, r * 2.0);
                    pages.push_str(&format!(
                        "<draw:ellipse {}/>",
                        geometry(cx, cy, d, d, *rotation)
                    ));
                }
                // Images need packaged media; deferred (matches pptx v1 scope
                // notes — the pptx path carries them).
                SlideObject::Image { .. } => {}
            }
        }
        if !slide.notes.is_empty() {
            let notes: String = slide
                .notes
                .split('\n')
                .map(|l| format!("<text:p>{}</text:p>", esc(l)))
                .collect();
            pages.push_str(&format!(
                "<presentation:notes><draw:frame presentation:class=\"notes\" \
                 svg:x=\"50pt\" svg:y=\"560pt\" svg:width=\"860pt\" svg:height=\"200pt\">\
                 <draw:text-box>{notes}</draw:text-box></draw:frame></presentation:notes>"
            ));
        }
        pages.push_str("</draw:page>");
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <office:document-content \
         xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" \
         xmlns:presentation=\"urn:oasis:names:tc:opendocument:xmlns:presentation:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
         xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
         xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" \
         office:version=\"1.2\">\
         <office:automatic-styles>{auto}</office:automatic-styles>\
         <office:body><office:presentation>{pages}</office:presentation></office:body>\
         </office:document-content>"
    )
}

const MANIFEST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.2\">\
<manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.presentation\"/>\
<manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
</manifest:manifest>";

/// Write the deck as .odp. Builds the whole archive in memory first, then
/// places it atomically — a rename before the ZipWriter flushes its central
/// directory would leave a corrupt archive, so the destination path is
/// never touched until the full buffer is ready (see suite_common_core::
/// atomic_save).
pub fn write(deck: &Deck, path: &str) -> Result<(), String> {
    let bytes = write_bytes(deck)?;
    suite_common_core::atomic_save::atomic_write_bytes(std::path::Path::new(path), &bytes)
}

/// Render the deck to an in-memory .odp buffer without touching disk —
/// shared by the real save path (above) and autosave snapshots.
pub fn write_bytes(deck: &Deck) -> Result<Vec<u8>, String> {
    let buf = std::io::Cursor::new(Vec::new());
    let mut z = zip::ZipWriter::new(buf);
    z.start_file(
        "mimetype",
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .map_err(|e| e.to_string())?;
    z.write_all(MIMETYPE.as_bytes()).map_err(|e| e.to_string())?;
    let opt = zip::write::SimpleFileOptions::default();
    z.start_file("META-INF/manifest.xml", opt).map_err(|e| e.to_string())?;
    z.write_all(MANIFEST.as_bytes()).map_err(|e| e.to_string())?;
    z.start_file("content.xml", opt).map_err(|e| e.to_string())?;
    z.write_all(content_xml(deck).as_bytes()).map_err(|e| e.to_string())?;
    z.finish().map_err(|e| e.to_string()).map(|c| c.into_inner())
}

// ── Reading ──────────────────────────────────────────────────────────

fn parse_length_pt(v: &str) -> Option<f64> {
    let v = v.trim();
    let split = v.find(|c: char| c.is_ascii_alphabetic())?;
    let (num, unit) = v.split_at(split);
    let n: f64 = num.parse().ok()?;
    Some(match unit {
        "pt" => n,
        "cm" => n * 72.0 / 2.54,
        "mm" => n * 72.0 / 25.4,
        "in" => n * 72.0,
        _ => return None,
    })
}

fn attr(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    e.attributes().filter_map(|a| a.ok()).find_map(|a| {
        if a.key.into_inner() == name {
            Some(a.value.to_string())
        } else {
            None
        }
    })
}

/// Read an .odp into a Deck.
pub fn read(path: &str) -> Result<Deck, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    // Bounded like every other package read: an ODP is an untrusted
    // download, and content.xml is the part a bomb would hide in (#442).
    let mut budget = ZipBudget::default();
    budget.check_entry_count(zip.len())?;
    let content = zip.part_to_string("content.xml", &mut budget).map_err(|e| {
        if e.is_missing() { "no content.xml — not an ODP?".to_string() } else { e.to_string() }
    })?;

    // First pass: text styles and drawing-page backgrounds.
    let mut text_styles: std::collections::HashMap<String, RunStyle> = Default::default();
    let mut page_bg: std::collections::HashMap<String, String> = Default::default();
    {
        let mut reader = Reader::from_str(&content);
        let mut cur: Option<(String, String)> = None; // (name, family)
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                    "style:style" => {
                        cur = attr(&e, "style:name")
                            .zip(attr(&e, "style:family"));
                    }
                    "style:text-properties" => {
                        if let Some((name, family)) = &cur {
                            if family == "text" {
                                let mut st = RunStyle::default();
                                if attr(&e, "fo:font-weight").as_deref() == Some("bold") {
                                    st.bold = true;
                                }
                                if attr(&e, "fo:font-style").as_deref() == Some("italic") {
                                    st.italic = true;
                                }
                                if attr(&e, "style:text-underline-style")
                                    .map(|v| v != "none")
                                    .unwrap_or(false)
                                {
                                    st.underline = true;
                                }
                                if attr(&e, "style:text-line-through-style")
                                    .map(|v| v != "none")
                                    .unwrap_or(false)
                                {
                                    st.strikethrough = true;
                                }
                                if let Some(sz) = attr(&e, "fo:font-size") {
                                    if let Ok(pt) = sz.trim_end_matches("pt").parse::<f32>() {
                                        st.font_size_hp = Some((pt * 2.0).round() as u16);
                                    }
                                }
                                if let Some(c) = attr(&e, "fo:color") {
                                    st.color =
                                        Some(c.trim_start_matches('#').to_lowercase());
                                }
                                text_styles.insert(name.clone(), st);
                            }
                        }
                    }
                    "style:drawing-page-properties" => {
                        if let Some((name, family)) = &cur {
                            if family == "drawing-page" {
                                if let Some(c) = attr(&e, "draw:fill-color") {
                                    page_bg.insert(name.clone(), c.to_lowercase());
                                }
                            }
                        }
                    }
                    _ => {}
                },
                // Reset at the style's end so text-properties nested in
                // later siblings (LO's text:list-style levels reuse the
                // same names) can't clobber the real definition.
                Ok(Event::End(e)) if e.name().as_ref() == "style:style" => cur = None,
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
        }
    }

    // Second pass: pages, frames, shapes, notes.
    let mut deck = Deck { slides: Vec::new(), masters: vec![MasterSlide {
        name: "Default".into(),
        background: "#ffffff".into(),
        default_font: "Sans".into(),
        shapes: vec![],
    }] };
    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(false);
    let mut slide: Option<Slide> = None;
    let mut in_notes = false;
    // Current draw:frame geometry and rotation; taken by the text-box
    // inside it.
    let mut frame: Option<(f64, f64, f64, f64, f64)> = None;
    let mut textbox: Option<(Vec<String>, Vec<Run>)> = None; // (lines, runs)
    let mut span_style: Option<RunStyle> = None;
    let mut in_text = false;
    let mut shape_type: Option<String> = None;

    // `svg:x`/`svg:y` place an unrotated shape; a rotated one carries
    // `draw:transform` instead and Impress omits them entirely, so the
    // transform is what has to be believed when both are present.
    let geo = |e: &quick_xml::events::BytesStart| -> (f64, f64, f64, f64, f64) {
        let g = |n: &str| attr(e, n).and_then(|v| parse_length_pt(&v)).unwrap_or(0.0);
        let (w, h) = (g("svg:width"), g("svg:height"));
        match attr(e, "draw:transform").and_then(|v| parse_transform(&v, w, h)) {
            Some((x, y, rotation)) => (x, y, w, h, rotation),
            None => (g("svg:x"), g("svg:y"), w, h, 0.0),
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                "draw:page" => {
                    let bg = attr(e, "draw:style-name")
                        .and_then(|n| page_bg.get(&n).cloned())
                        .unwrap_or_else(|| "#ffffff".into());
                    slide = Some(Slide {
                        title: attr(e, "draw:name").unwrap_or_default(),
                        background: bg,
                        objects: vec![],
                        notes: String::new(),
                        master_idx: Some(0),
                    });
                }
                "presentation:notes" => in_notes = true,
                "draw:frame" => frame = Some(geo(e)),
                "draw:text-box" => textbox = Some((Vec::new(), Vec::new())),
                // Impress converts pptx text boxes to custom-shapes with
                // text:p directly inside (no draw:text-box wrapper).
                "draw:custom-shape" => {
                    frame = Some(geo(e));
                    textbox = Some((Vec::new(), Vec::new()));
                    shape_type = None;
                }
                "draw:enhanced-geometry" => {
                    if let Some(t) = attr(e, "draw:type") {
                        shape_type = Some(t);
                    }
                }
                "text:p" => {
                    if let Some((lines, _)) = textbox.as_mut() {
                        lines.push(String::new());
                    }
                    in_text = true;
                }
                "text:span" => {
                    span_style = attr(e, "text:style-name")
                        .and_then(|n| text_styles.get(&n).cloned());
                }
                "draw:rect" => {
                    if let Some(s) = slide.as_mut() {
                        if !in_notes {
                            let (x, y, w, h, rotation) = geo(e);
                            s.objects.push(SlideObject::Rect { x, y, w, h, rotation });
                        }
                    }
                }
                "draw:ellipse" | "draw:circle" => {
                    if let Some(s) = slide.as_mut() {
                        if !in_notes {
                            let (x, y, w, h, rotation) = geo(e);
                            let r = (w.max(h)) / 2.0;
                            s.objects.push(SlideObject::Circle { x: x + w / 2.0, y: y + h / 2.0, r, rotation });
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(ref e)) => match e.name().as_ref() {
                "draw:enhanced-geometry" => {
                    if let Some(t) = attr(e, "draw:type") {
                        shape_type = Some(t);
                    }
                }
                "draw:rect" => {
                    if let (Some(s), false) = (slide.as_mut(), in_notes) {
                        let (x, y, w, h, rotation) = geo(e);
                        s.objects.push(SlideObject::Rect { x, y, w, h, rotation });
                    }
                }
                "draw:ellipse" | "draw:circle" => {
                    if let (Some(s), false) = (slide.as_mut(), in_notes) {
                        let (x, y, w, h, rotation) = geo(e);
                        let r = (w.max(h)) / 2.0;
                        s.objects.push(SlideObject::Circle { x: x + w / 2.0, y: y + h / 2.0, r, rotation });
                    }
                }
                _ => {}
            },
            Ok(Event::Text(ref t)) => {
                if in_text {
                    let txt = crate::engine::unescape_text(t);
                    if let Some((lines, runs)) = textbox.as_mut() {
                        if let Some(last) = lines.last_mut() {
                            last.push_str(&txt);
                        }
                        if !txt.is_empty() {
                            runs.push(Run {
                                text: txt,
                                style: span_style.clone().unwrap_or_default(),
                            });
                        }
                    }
                }
            }
            Ok(Event::GeneralRef(ref r)) => {
                if in_text {
                    let txt = crate::engine::resolve_general_ref(r);
                    if let Some((lines, runs)) = textbox.as_mut() {
                        if let Some(last) = lines.last_mut() {
                            last.push_str(&txt);
                        }
                        if !txt.is_empty() {
                            runs.push(Run {
                                text: txt,
                                style: span_style.clone().unwrap_or_default(),
                            });
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                "text:p" => in_text = false,
                "text:span" => span_style = None,
                "draw:text-box" => {
                    if let (Some((lines, runs)), Some((x, y, w, h, rotation))) =
                        (textbox.take(), frame)
                    {
                        let text = lines.join("\n");
                        if let Some(s) = slide.as_mut() {
                            if in_notes {
                                s.notes = text;
                            } else {
                                // Single-paragraph boxes keep their runs;
                                // multi-line falls back to plain (matches
                                // the pptx reader's behavior).
                                let keep_runs = if lines.len() == 1 { runs } else { vec![] };
                                s.objects.push(SlideObject::TextBox {
                                    text,
                                    x,
                                    y,
                                    w,
                                    h,
                                    rotation,
                                    runs: keep_runs,
                                });
                            }
                        }
                    }
                }
                "draw:frame" => frame = None,
                "draw:custom-shape" => {
                    if let (Some((lines, runs)), Some((x, y, w, h, rotation))) =
                        (textbox.take(), frame.take())
                    {
                        let text = lines.join("\n");
                        if let Some(s) = slide.as_mut() {
                            if in_notes {
                                if !text.is_empty() {
                                    s.notes = text;
                                }
                            } else if !text.is_empty() {
                                let keep_runs = if lines.len() == 1 { runs } else { vec![] };
                                s.objects.push(SlideObject::TextBox { text, x, y, w, h, rotation, runs: keep_runs });
                            } else if shape_type.as_deref().is_some_and(|t| t.contains("ellipse")) {
                                let r = (w.max(h)) / 2.0;
                                s.objects.push(SlideObject::Circle { x: x + w / 2.0, y: y + h / 2.0, r, rotation });
                            } else {
                                s.objects.push(SlideObject::Rect { x, y, w, h, rotation });
                            }
                        }
                    }
                    shape_type = None;
                }
                "presentation:notes" => in_notes = false,
                "draw:page" => {
                    if let Some(s) = slide.take() {
                        deck.slides.push(s);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error: {e}")),
            _ => {}
        }
    }

    if deck.slides.is_empty() {
        deck.slides.push(Slide {
            title: "Slide 1".into(),
            background: "#ffffff".into(),
            objects: vec![],
            notes: String::new(),
            master_idx: Some(0),
        });
    }
    Ok(deck)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(deck: &Deck) -> Deck {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.odp");
        write(deck, path.to_str().unwrap()).expect("write odp");
        read(path.to_str().unwrap()).expect("read odp")
    }

    fn text_slide(title: &str, text: &str, notes: &str) -> Slide {
        Slide {
            title: title.into(),
            background: "#ffffff".into(),
            objects: vec![SlideObject::TextBox {
                text: text.into(),
                x: 100.0,
                y: 100.0,
                w: 400.0,
                h: 60.0,
                runs: vec![],
                rotation: 0.0,
            }],
            notes: notes.into(),
            master_idx: Some(0),
        }
    }

    #[test]
    fn text_and_order_round_trip() {
        let mut deck = Deck::new();
        deck.slides = vec![text_slide("One", "first", ""), text_slide("Two", "second", "")];
        let rt = round_trip(&deck);
        assert_eq!(rt.slides.len(), 2);
        assert!(matches!(&rt.slides[0].objects[0],
            SlideObject::TextBox { text, .. } if text == "first"));
        assert!(matches!(&rt.slides[1].objects[0],
            SlideObject::TextBox { text, .. } if text == "second"));
    }

    /// What Impress wrote, verbatim, for the probe described on the test
    /// below: a 225x75pt rect at (75, 150)pt turned 30 degrees clockwise.
    const IMPRESS_ROTATED_RECT: &str =
        "rotate (-0.523598775598299) translate (3.839cm 3.485cm)";

    /// The probe that settled the ODF convention, kept as a test.
    ///
    /// Impress was given our pptx for a 300x100 rect at (100, 200) with
    /// `rot="1800000"` (30 degrees clockwise) and converted it to odp. In
    /// its own units — our pptx writer emits 9525 EMU per model unit, so
    /// Impress reads the shape as 225x75pt at (75, 150)pt — it wrote
    /// exactly the transform asserted below.
    ///
    /// Reproducing Impress's own output is what pins both the sign and the
    /// matrix. A round trip through our own reader cannot: a mirrored
    /// convention cancels itself out and passes. The tolerance is Impress's
    /// three decimal places of centimetres, a little under 0.03pt.
    #[test]
    fn the_transform_we_write_is_the_one_impress_wrote() {
        let got = geometry(75.0, 150.0, 225.0, 75.0, 30.0);
        let terms = transform_terms(
            got.split("draw:transform=\"").nth(1).expect("no transform emitted").trim_end_matches('"'),
        )
        .expect("our own transform must parse");
        let want = transform_terms(IMPRESS_ROTATED_RECT).expect("the recorded probe must parse");
        assert_eq!(terms[0].0, "rotate");
        let angle: f64 = terms[0].1[0].parse().unwrap();
        let wanted_angle: f64 = want[0].1[0].parse().unwrap();
        assert!(
            (angle - wanted_angle).abs() < 1e-9,
            "ODF wants radians counter-clockwise where our model is degrees \
             clockwise; Impress wrote {wanted_angle} and we wrote {angle}",
        );
        assert_eq!(terms[1].0, "translate");
        let cm = |v: &str| parse_length_pt(v).unwrap() * 2.54 / 72.0;
        assert!(
            (cm(&terms[1].1[0]) - cm(&want[1].1[0])).abs() < 0.001
                && (cm(&terms[1].1[1]) - cm(&want[1].1[1])).abs() < 0.001,
            "the translate must put the shape where Impress puts it; \
             we wrote ({}cm {}cm)",
            cm(&terms[1].1[0]),
            cm(&terms[1].1[1]),
        );
        assert!(
            !got.contains("svg:x"),
            "Impress omits svg:x/svg:y on a transformed shape, and a reader \
             that prefers them would place ours unrotated: {got}",
        );
    }

    /// The reading half of the same probe: Impress's own bytes must come
    /// back as the box and the clockwise angle we started from.
    #[test]
    fn a_transform_impress_wrote_reads_back_as_its_box_and_angle() {
        let (x, y, rotation) = parse_transform(IMPRESS_ROTATED_RECT, 225.0, 75.0)
            .expect("Impress's own transform must parse");
        assert!((x - 75.0).abs() < 0.05 && (y - 150.0).abs() < 0.05, "read as ({x}, {y})");
        assert!((rotation - 30.0).abs() < 0.01, "read as {rotation} degrees");
    }

    /// A shape that is not rotated keeps plain coordinates — every reader
    /// handles those, and the decks already written stay byte-identical.
    #[test]
    fn an_unrotated_shape_keeps_plain_svg_coordinates() {
        let got = geometry(10.0, 20.0, 30.0, 40.0, 0.0);
        assert_eq!(got, "svg:x=\"10pt\" svg:y=\"20pt\" svg:width=\"30pt\" svg:height=\"40pt\"");
    }

    /// Impress writes `scale` and `skewX` for shapes we do not produce.
    /// Declining them puts the shape at its `svg:x`/`svg:y` rather than
    /// somewhere confidently wrong.
    #[test]
    fn a_transform_term_we_do_not_understand_is_declined() {
        assert!(parse_transform("scale (2 2) translate (1cm 1cm)", 10.0, 10.0).is_none());
        assert!(parse_transform("skewX (0.3)", 10.0, 10.0).is_none());
        assert!(parse_transform("rotate (", 10.0, 10.0).is_none());
        // And a translate in a unit we cannot convert, rather than treating
        // it as zero and stacking the shape in the corner.
        assert!(parse_transform("translate (3parsec 1cm)", 10.0, 10.0).is_none());
    }

    /// Rotation past a half turn, because normalising with the wrong modulo
    /// turns 315 into -45 or 45.
    #[test]
    fn a_rotation_past_a_half_turn_round_trips() {
        for rotation in [30.0, 120.0, 315.0] {
            let got = geometry(40.0, 50.0, 60.0, 70.0, rotation);
            let transform =
                got.split("draw:transform=\"").nth(1).unwrap().trim_end_matches('"').to_string();
            let (x, y, back) = parse_transform(&transform, 60.0, 70.0).unwrap();
            assert!((back - rotation).abs() < 1e-6, "{rotation} came back as {back}");
            assert!((x - 40.0).abs() < 1e-6 && (y - 50.0).abs() < 1e-6, "moved to ({x}, {y})");
        }
    }

    #[test]
    fn geometry_round_trips() {
        let mut deck = Deck::new();
        deck.slides = vec![Slide {
            title: "g".into(),
            background: "#ffffff".into(),
            objects: vec![
                SlideObject::Rect { x: 240.0, y: 180.0, w: 320.0, h: 120.0, rotation: 0.0 },
                SlideObject::Circle { x: 500.0, y: 300.0, r: 80.0, rotation: 0.0 },
            ],
            notes: String::new(),
            master_idx: Some(0),
        }];
        let rt = round_trip(&deck);
        let close = |a: f64, b: f64| (a - b).abs() < 0.1;
        let Some(SlideObject::Rect { x, y, w, h, .. }) = rt.slides[0]
            .objects
            .iter()
            .find(|o| matches!(o, SlideObject::Rect { .. }))
        else {
            panic!("rect lost: {:?}", rt.slides[0].objects)
        };
        assert!(close(*x, 240.0) && close(*y, 180.0) && close(*w, 320.0) && close(*h, 120.0));
        let Some(SlideObject::Circle { x, y, r, .. }) = rt.slides[0]
            .objects
            .iter()
            .find(|o| matches!(o, SlideObject::Circle { .. }))
        else {
            panic!("circle lost")
        };
        assert!(close(*x, 500.0) && close(*y, 300.0) && close(*r, 80.0));
    }

    #[test]
    fn styled_runs_round_trip() {
        let mut deck = Deck::new();
        deck.slides = vec![Slide {
            title: "s".into(),
            background: "#ffffff".into(),
            objects: vec![SlideObject::TextBox {
                text: "plain bold".into(),
                x: 10.0,
                y: 10.0,
                w: 300.0,
                h: 50.0,
                runs: vec![
                    Run { text: "plain ".into(), style: RunStyle::default() },
                    Run {
                        text: "bold".into(),
                        style: RunStyle {
                            bold: true,
                            font_size_hp: Some(48),
                            color: Some("cc0000".into()),
                            ..Default::default()
                        },
                    },
                ],
                rotation: 0.0,
            }],
            notes: String::new(),
            master_idx: Some(0),
        }];
        let rt = round_trip(&deck);
        let SlideObject::TextBox { runs, .. } = &rt.slides[0].objects[0] else { panic!() };
        let bold = runs.iter().find(|r| r.style.bold).expect("bold run lost");
        assert_eq!(bold.text, "bold");
        assert_eq!(bold.style.font_size_hp, Some(48));
        assert_eq!(bold.style.color.as_deref(), Some("cc0000"));
    }

    #[test]
    fn notes_round_trip() {
        let mut deck = Deck::new();
        deck.slides = vec![text_slide("n", "body", "remember the joke")];
        let rt = round_trip(&deck);
        assert_eq!(rt.slides[0].notes, "remember the joke");
        // The notes frame must not leak into the slide's objects.
        assert_eq!(rt.slides[0].objects.len(), 1);
    }

    #[test]
    fn background_round_trips() {
        let mut deck = Deck::new();
        let mut s = text_slide("bg", "x", "");
        s.background = "#e8f0fe".into();
        deck.slides = vec![s];
        let rt = round_trip(&deck);
        assert_eq!(rt.slides[0].background, "#e8f0fe");
    }

    #[test]
    fn multiline_text_round_trips() {
        let mut deck = Deck::new();
        deck.slides = vec![text_slide("m", "line one\nline two", "")];
        let rt = round_trip(&deck);
        assert!(matches!(&rt.slides[0].objects[0],
            SlideObject::TextBox { text, .. } if text == "line one\nline two"));
    }
    // ── Pure helpers ─────────────────────────────────────────────────────

    #[test]
    fn esc_escapes_xml_specials() {
        assert_eq!(esc("a&b<c>d\"e"), "a&amp;b&lt;c&gt;d&quot;e");
        assert_eq!(esc("plain"), "plain");
        assert_eq!(esc(""), "");
    }

    #[test]
    fn text_style_emits_attribute_per_flag() {
        let st = RunStyle {
            bold: true,
            italic: true,
            underline: true,
            strikethrough: true,
            ..Default::default()
        };
        let out = text_style(&st);
        assert!(out.contains("fo:font-weight=\"bold\""), "got {out}");
        assert!(out.contains("fo:font-style=\"italic\""), "got {out}");
        assert!(
            out.contains("style:text-underline-style=\"solid\""),
            "got {out}"
        );
        assert!(
            out.contains("style:text-line-through-style=\"solid\""),
            "got {out}"
        );
    }

    #[test]
    fn text_style_size_color_and_default() {
        assert_eq!(text_style(&RunStyle::default()), "");
        let st = RunStyle {
            font_size_hp: Some(24), // 24 half-points = 12pt
            color: Some("ff0000".into()),
            ..Default::default()
        };
        let out = text_style(&st);
        assert!(out.contains("fo:font-size=\"12pt\""), "got {out}");
        assert!(out.contains("fo:color=\"#ff0000\""), "got {out}");
    }

    #[test]
    fn parse_length_pt_converts_supported_units() {
        assert_eq!(parse_length_pt("12pt"), Some(12.0));
        assert!((parse_length_pt("1cm").unwrap() - 72.0 / 2.54).abs() < 1e-9);
        assert!((parse_length_pt("10mm").unwrap() - 72.0 / 25.4 * 10.0).abs() < 1e-9);
        assert_eq!(parse_length_pt("1in"), Some(72.0));
        // Whitespace is trimmed before parsing.
        assert!((parse_length_pt(" 2.5cm ").unwrap() - 2.5 * 72.0 / 2.54).abs() < 1e-9);
    }

    #[test]
    fn parse_length_pt_rejects_garbage() {
        assert_eq!(parse_length_pt("12px"), None); // unknown unit
        assert_eq!(parse_length_pt("abc"), None); // non-numeric
        assert_eq!(parse_length_pt("pt"), None); // missing number
        assert_eq!(parse_length_pt(""), None);
        assert_eq!(parse_length_pt("1.5CM"), None); // units are case-sensitive
    }

    /// ODP wrote coordinates as `{x:.2}pt`, so 2 cm — 56.692913 pt — was
    /// stored as "56.69pt" and read back as 56.69. Every object shifted on
    /// every save, and the shift was one-directional, so it accumulated.
    ///
    /// Rust's default float formatting emits the shortest string that parses
    /// back to the identical f64, so full precision is also the shortest
    /// faithful representation — this is not a "write more digits" hack.
    #[test]
    fn a_two_centimetre_offset_survives_an_odp_round_trip_exactly() {
        let two_cm = 2.0 * 72.0 / 2.54;
        let deck = deck_with_box("geometry", two_cm);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("geometry.odp");
        write(&deck, path.to_str().unwrap()).expect("write odp");
        let back = read(path.to_str().unwrap()).expect("read odp");

        let (x, y) = crate::undo::obj_position(&back.slides[0].objects[0]);
        assert!(
            (x - two_cm).abs() < 1e-9 && (y - two_cm).abs() < 1e-9,
            "drifted: wrote ({two_cm}, {two_cm}), read ({x}, {y})"
        );
    }

    /// The accumulating half: repeated saves must not walk the geometry.
    #[test]
    fn odp_geometry_does_not_drift_across_repeated_saves() {
        let start = 2.0 * 72.0 / 2.54;
        let mut deck = deck_with_box("drift", start);
        let dir = tempfile::tempdir().unwrap();
        for generation in 0..8 {
            let path = dir.path().join(format!("gen{generation}.odp"));
            write(&deck, path.to_str().unwrap()).expect("write");
            deck = read(path.to_str().unwrap()).expect("read");
        }
        let (x, y) = crate::undo::obj_position(&deck.slides[0].objects[0]);
        assert!(
            (x - start).abs() < 1e-9 && (y - start).abs() < 1e-9,
            "drifted over eight saves: {start} -> ({x}, {y})"
        );
    }

    /// One slide holding a single text box at (`at`, `at`).
    fn deck_with_box(text: &str, at: f64) -> Deck {
        Deck {
            slides: vec![Slide {
                title: String::new(),
                background: String::new(),
                notes: String::new(),
                master_idx: None,
                objects: vec![SlideObject::TextBox {
                    text: text.into(),
                    x: at, y: at, w: 200.0, h: 50.0,
                    rotation: 0.0,
                    runs: vec![],
                }],
            }],
            ..Default::default()
        }
    }
}
