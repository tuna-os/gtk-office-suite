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
use std::io::{Read, Write};

use crate::engine::{Deck, MasterSlide, Slide, SlideObject};

const MIMETYPE: &str = "application/vnd.oasis.opendocument.presentation";

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
                SlideObject::TextBox { text, x, y, w, h, runs } => {
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
                        "<draw:frame svg:x=\"{x:.2}pt\" svg:y=\"{y:.2}pt\" \
                         svg:width=\"{w:.2}pt\" svg:height=\"{h:.2}pt\">\
                         <draw:text-box>{inner}</draw:text-box></draw:frame>"
                    ));
                }
                SlideObject::Rect { x, y, w, h } => {
                    pages.push_str(&format!(
                        "<draw:rect svg:x=\"{x:.2}pt\" svg:y=\"{y:.2}pt\" \
                         svg:width=\"{w:.2}pt\" svg:height=\"{h:.2}pt\"/>"
                    ));
                }
                SlideObject::Circle { x, y, r } => {
                    let (cx, cy, d) = (x - r, y - r, r * 2.0);
                    pages.push_str(&format!(
                        "<draw:ellipse svg:x=\"{cx:.2}pt\" svg:y=\"{cy:.2}pt\" \
                         svg:width=\"{d:.2}pt\" svg:height=\"{d:.2}pt\"/>"
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

/// Write the deck as .odp.
pub fn write(deck: &Deck, path: &str) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipWriter::new(file);
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
    z.finish().map_err(|e| e.to_string())?;
    Ok(())
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
        if a.key.as_ref() == name.as_bytes() {
            Some(String::from_utf8_lossy(&a.value).to_string())
        } else {
            None
        }
    })
}

/// Read an .odp into a Deck.
pub fn read(path: &str) -> Result<Deck, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut content = String::new();
    zip.by_name("content.xml")
        .map_err(|_| "no content.xml — not an ODP?".to_string())?
        .read_to_string(&mut content)
        .map_err(|e| e.to_string())?;

    // First pass: text styles and drawing-page backgrounds.
    let mut text_styles: std::collections::HashMap<String, RunStyle> = Default::default();
    let mut page_bg: std::collections::HashMap<String, String> = Default::default();
    {
        let mut reader = Reader::from_str(&content);
        let mut cur: Option<(String, String)> = None; // (name, family)
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                    b"style:style" => {
                        cur = attr(&e, "style:name")
                            .zip(attr(&e, "style:family"));
                    }
                    b"style:text-properties" => {
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
                    b"style:drawing-page-properties" => {
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
                Ok(Event::End(e)) if e.name().as_ref() == b"style:style" => cur = None,
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
    reader.trim_text(false);
    let mut slide: Option<Slide> = None;
    let mut in_notes = false;
    // Current draw:frame geometry; taken by the text-box inside it.
    let mut frame: Option<(f64, f64, f64, f64)> = None;
    let mut textbox: Option<(Vec<String>, Vec<Run>)> = None; // (lines, runs)
    let mut span_style: Option<RunStyle> = None;
    let mut in_text = false;
    let mut shape_type: Option<String> = None;

    let geo = |e: &quick_xml::events::BytesStart| -> (f64, f64, f64, f64) {
        let g = |n: &str| attr(e, n).and_then(|v| parse_length_pt(&v)).unwrap_or(0.0);
        (g("svg:x"), g("svg:y"), g("svg:width"), g("svg:height"))
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                b"draw:page" => {
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
                b"presentation:notes" => in_notes = true,
                b"draw:frame" => frame = Some(geo(e)),
                b"draw:text-box" => textbox = Some((Vec::new(), Vec::new())),
                // Impress converts pptx text boxes to custom-shapes with
                // text:p directly inside (no draw:text-box wrapper).
                b"draw:custom-shape" => {
                    frame = Some(geo(e));
                    textbox = Some((Vec::new(), Vec::new()));
                    shape_type = None;
                }
                b"draw:enhanced-geometry" => {
                    if let Some(t) = attr(e, "draw:type") {
                        shape_type = Some(t);
                    }
                }
                b"text:p" => {
                    if let Some((lines, _)) = textbox.as_mut() {
                        lines.push(String::new());
                    }
                    in_text = true;
                }
                b"text:span" => {
                    span_style = attr(e, "text:style-name")
                        .and_then(|n| text_styles.get(&n).cloned());
                }
                b"draw:rect" => {
                    if let Some(s) = slide.as_mut() {
                        if !in_notes {
                            let (x, y, w, h) = geo(e);
                            s.objects.push(SlideObject::Rect { x, y, w, h });
                        }
                    }
                }
                b"draw:ellipse" | b"draw:circle" => {
                    if let Some(s) = slide.as_mut() {
                        if !in_notes {
                            let (x, y, w, h) = geo(e);
                            let r = (w.max(h)) / 2.0;
                            s.objects.push(SlideObject::Circle { x: x + w / 2.0, y: y + h / 2.0, r });
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(ref e)) => match e.name().as_ref() {
                b"draw:enhanced-geometry" => {
                    if let Some(t) = attr(e, "draw:type") {
                        shape_type = Some(t);
                    }
                }
                b"draw:rect" => {
                    if let (Some(s), false) = (slide.as_mut(), in_notes) {
                        let (x, y, w, h) = geo(e);
                        s.objects.push(SlideObject::Rect { x, y, w, h });
                    }
                }
                b"draw:ellipse" | b"draw:circle" => {
                    if let (Some(s), false) = (slide.as_mut(), in_notes) {
                        let (x, y, w, h) = geo(e);
                        let r = (w.max(h)) / 2.0;
                        s.objects.push(SlideObject::Circle { x: x + w / 2.0, y: y + h / 2.0, r });
                    }
                }
                _ => {}
            },
            Ok(Event::Text(ref t)) => {
                if in_text {
                    let txt = t.unescape().unwrap_or_default().to_string();
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
                b"text:p" => in_text = false,
                b"text:span" => span_style = None,
                b"draw:text-box" => {
                    if let (Some((lines, runs)), Some((x, y, w, h))) =
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
                                    runs: keep_runs,
                                });
                            }
                        }
                    }
                }
                b"draw:frame" => frame = None,
                b"draw:custom-shape" => {
                    if let (Some((lines, runs)), Some((x, y, w, h))) = (textbox.take(), frame.take()) {
                        let text = lines.join("\n");
                        if let Some(s) = slide.as_mut() {
                            if in_notes {
                                if !text.is_empty() {
                                    s.notes = text;
                                }
                            } else if !text.is_empty() {
                                let keep_runs = if lines.len() == 1 { runs } else { vec![] };
                                s.objects.push(SlideObject::TextBox { text, x, y, w, h, runs: keep_runs });
                            } else if shape_type.as_deref().is_some_and(|t| t.contains("ellipse")) {
                                let r = (w.max(h)) / 2.0;
                                s.objects.push(SlideObject::Circle { x: x + w / 2.0, y: y + h / 2.0, r });
                            } else {
                                s.objects.push(SlideObject::Rect { x, y, w, h });
                            }
                        }
                    }
                    shape_type = None;
                }
                b"presentation:notes" => in_notes = false,
                b"draw:page" => {
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

    #[test]
    fn geometry_round_trips() {
        let mut deck = Deck::new();
        deck.slides = vec![Slide {
            title: "g".into(),
            background: "#ffffff".into(),
            objects: vec![
                SlideObject::Rect { x: 240.0, y: 180.0, w: 320.0, h: 120.0 },
                SlideObject::Circle { x: 500.0, y: 300.0, r: 80.0 },
            ],
            notes: String::new(),
            master_idx: Some(0),
        }];
        let rt = round_trip(&deck);
        let close = |a: f64, b: f64| (a - b).abs() < 0.1;
        let Some(SlideObject::Rect { x, y, w, h }) = rt.slides[0]
            .objects
            .iter()
            .find(|o| matches!(o, SlideObject::Rect { .. }))
        else {
            panic!("rect lost: {:?}", rt.slides[0].objects)
        };
        assert!(close(*x, 240.0) && close(*y, 180.0) && close(*w, 320.0) && close(*h, 120.0));
        let Some(SlideObject::Circle { x, y, r }) = rt.slides[0]
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
}
