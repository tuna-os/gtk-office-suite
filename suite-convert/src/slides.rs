// slides.rs — a deck as Typst source, so a presentation can become a PDF (and,
// through the rasteriser, a PNG) without Decks' cairo and pango renderer.
//
// This is a re-drawing, not Decks' own render: the PDF it makes is labelled
// approximate in the loss report, as Letters' Typst export is (ADR 0010). It
// draws what a slide is mostly made of — backgrounds, text, solid shapes — and
// says plainly, per slide, what it left out (pictures, tables, charts), so a
// loss budget can refuse a deck that depends on them.
//
// No GTK, no cairo: `decks_core::Deck` in, Typst source and losses out.

use decks_core::engine::shape::{ShapeKind, ShapeStyle};
use decks_core::engine::text_body::{Anchor, ParaAlign};
use decks_core::engine::{MasterSlide, Slide, SlideObject};
use decks_core::Deck;
use suite_common_core::document::Run;

use crate::engine::Loss;
use crate::LossLevel;

/// The model's coordinate space is 960 x 540 whatever the file's own slide size.
const MODEL_W: f64 = 960.0;
const MODEL_H: f64 = 540.0;
/// Text size when a run states none, in points.
const DEFAULT_PT: f64 = 18.0;

/// A string as a Typst string literal, so slide text can never be read as markup.
fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn pt(v: f64) -> String {
    // Typst reads a plain decimal; keep it short and finite.
    let v = if v.is_finite() { v } else { 0.0 };
    format!("{:.2}pt", v)
}

fn rgb(hex: &str) -> Option<String> {
    let c = suite_common_core::color::Color::from_hex(hex)?;
    Some(format!("rgb(\"#{}\")", c.to_hex()))
}

fn paint(style: &ShapeStyle) -> (String, String) {
    let fill = style
        .fill
        .map(|c| format!("rgb(\"#{}\")", c.to_hex()))
        .unwrap_or_else(|| "none".into());
    let stroke = style
        .stroke
        .map(|s| {
            format!(
                "(paint: rgb(\"#{}\"), thickness: {})",
                s.color.to_hex(),
                pt(s.width)
            )
        })
        .unwrap_or_else(|| "none".into());
    (fill, stroke)
}

struct Page {
    /// Points per model unit, vertically: the model is always 960 x 540, so a
    /// 4:3 deck was scaled onto it and is scaled back here.
    sy: f64,
}

/// One drawn thing: the Typst for it and whether it had to be approximated.
fn shape(
    kind: &ShapeKind,
    w: f64,
    h: f64,
    style: &ShapeStyle,
    approximated: &mut Option<String>,
) -> String {
    let (fill, stroke) = paint(style);
    let (wp, hp) = (pt(w), pt(h));
    match kind {
        ShapeKind::Rect => {
            format!("rect(width: {wp}, height: {hp}, fill: {fill}, stroke: {stroke})")
        }
        ShapeKind::RoundRect { radius } => format!(
            "rect(width: {wp}, height: {hp}, radius: {}, fill: {fill}, stroke: {stroke})",
            pt(radius * w.min(h))
        ),
        ShapeKind::Ellipse => {
            format!("ellipse(width: {wp}, height: {hp}, fill: {fill}, stroke: {stroke})")
        }
        ShapeKind::Triangle => format!(
            "polygon(({}, {}), ({}, {}), ({}, {}), fill: {fill}, stroke: {stroke})",
            pt(w / 2.0),
            pt(0.0),
            pt(w),
            pt(h),
            pt(0.0),
            pt(h)
        ),
        ShapeKind::Diamond => format!(
            "polygon(({}, {}), ({}, {}), ({}, {}), ({}, {}), fill: {fill}, stroke: {stroke})",
            pt(w / 2.0),
            pt(0.0),
            pt(w),
            pt(h / 2.0),
            pt(w / 2.0),
            pt(h),
            pt(0.0),
            pt(h / 2.0)
        ),
        ShapeKind::Other(name) => {
            *approximated = Some(format!("`{name}` is drawn as its bounding rectangle"));
            format!("rect(width: {wp}, height: {hp}, fill: {fill}, stroke: {stroke})")
        }
        ShapeKind::Freeform(_) => {
            *approximated = Some("a freeform shape is drawn as its bounding rectangle".into());
            format!("rect(width: {wp}, height: {hp}, fill: {fill}, stroke: {stroke})")
        }
    }
}

fn run(r: &Run) -> String {
    let mut args = vec![format!(
        "size: {}",
        pt(r.style
            .font_size_hp
            .map_or(DEFAULT_PT, |hp| f64::from(hp) / 2.0))
    )];
    if let Some(c) = r.style.color.as_deref().and_then(rgb) {
        args.push(format!("fill: {c}"));
    }
    if r.style.bold {
        args.push("weight: \"bold\"".into());
    }
    if r.style.italic {
        args.push("style: \"italic\"".into());
    }
    if let Some(f) = &r.style.font_family {
        args.push(format!("font: {}", string(f)));
    }
    let mut body = format!("#{}", string(&r.text));
    if r.style.underline {
        body = format!("#underline[{body}]");
    }
    if r.style.strikethrough {
        body = format!("#strike[{body}]");
    }
    format!("#text({})[{body}]", args.join(", "))
}

/// The runs of a text box split into paragraphs at newlines.
fn paragraphs(text: &str, runs: &[Run]) -> Vec<Vec<Run>> {
    let source: Vec<Run> = if runs.is_empty() {
        vec![Run::plain(text)]
    } else {
        runs.to_vec()
    };
    let mut out: Vec<Vec<Run>> = vec![Vec::new()];
    for r in source {
        let mut parts = r.text.split('\n').peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                out.last_mut().unwrap().push(Run {
                    text: part.into(),
                    style: r.style.clone(),
                });
            }
            if parts.peek().is_some() {
                out.push(Vec::new());
            }
        }
    }
    out
}

fn text_box(o: &SlideObject, page: &Page, approximated: &mut Option<String>) -> Option<String> {
    let SlideObject::TextBox {
        text,
        w,
        h,
        runs,
        body,
        ..
    } = o
    else {
        return None;
    };
    let (w, h) = (*w, *h * page.sy);
    let mut out = String::new();
    if let Some(frame) = &body.frame {
        out.push_str(&format!(
            "#place(top + left, {})\n",
            shape(&frame.kind, w, h, &frame.style, approximated)
        ));
    }
    let inset = body.insets.map_or(4.0, |i| i.left.max(0.0));
    let vertical = match body.anchor {
        Anchor::Top => "top",
        Anchor::Middle => "horizon",
        Anchor::Bottom => "bottom",
    };
    let mut paras = String::new();
    for (i, p) in paragraphs(text, runs).iter().enumerate() {
        let align = match body.paras.get(i).map(|s| s.align).unwrap_or_default() {
            ParaAlign::Left | ParaAlign::Justify => "left",
            ParaAlign::Center => "center",
            ParaAlign::Right => "right",
        };
        let content: String = p.iter().map(run).collect();
        // An empty paragraph still takes a line.
        let content = if content.is_empty() {
            "#h(0pt)".to_string()
        } else {
            content
        };
        paras.push_str(&format!("#align({align})[{content}]\n"));
    }
    out.push_str(&format!(
        "#box(width: {}, height: {}, inset: {}, clip: true)[#align({vertical})[\n{paras}]]\n",
        pt(w),
        pt(h),
        pt(inset)
    ));
    Some(out)
}

/// What an object is drawn as, or `None` when it is left out (and `left_out` says why).
fn object(
    o: &SlideObject,
    page: &Page,
    left_out: &mut Vec<(&'static str, &'static str, LossLevel)>,
    approximated: &mut Option<String>,
) -> Option<String> {
    let body = match o {
        SlideObject::TextBox { .. } => text_box(o, page, approximated)?,
        SlideObject::Rect { w, h, .. } => format!(
            "#rect(width: {}, height: {}, fill: rgb(\"#4472c4\"))\n",
            pt(*w),
            pt(*h * page.sy)
        ),
        SlideObject::Circle { r, .. } => format!(
            "#ellipse(width: {}, height: {}, fill: rgb(\"#4472c4\"))\n",
            pt(r * 2.0),
            pt(r * 2.0 * page.sy)
        ),
        SlideObject::Shape {
            kind, w, h, style, ..
        } => {
            format!("#{}\n", shape(kind, *w, *h * page.sy, style, approximated))
        }
        SlideObject::Image { .. } => {
            left_out.push(("picture-omitted", "Picture left out", LossLevel::Major));
            return None;
        }
        SlideObject::Table { .. } => {
            left_out.push(("table-omitted", "Table left out", LossLevel::Major));
            return None;
        }
        SlideObject::Chart { .. } => {
            left_out.push(("chart-omitted", "Chart left out", LossLevel::Major));
            return None;
        }
    };
    let rotation = o.rotation();
    let placed = if rotation.abs() > f64::EPSILON {
        format!("rotate({rotation}deg, origin: center + horizon)[\n{body}]")
    } else {
        format!("[\n{body}]")
    };
    Some(format!(
        "#place(top + left, dx: {}, dy: {}, {placed})\n",
        pt(o.x()),
        pt(o.y() * page.sy)
    ))
}

fn background<'a>(deck: &'a Deck, slide: &'a Slide, master: Option<&'a MasterSlide>) -> String {
    let layout = master.and_then(|m| {
        let id = slide.layout_id?;
        m.layouts.iter().find(|l| l.id == id)
    });
    let hex = decks_core::layouts::resolve_background(
        &slide.background,
        layout.and_then(|l| l.background.as_deref()),
        master.map_or("", |m| m.background.as_str()),
    );
    let _ = deck;
    rgb(hex).unwrap_or_else(|| "white".into())
}

/// A deck as Typst source plus what was left out or approximated. Hidden slides
/// are not drawn, as they are not presented.
pub fn deck_typst(deck: &Deck) -> Result<(String, Vec<Loss>), String> {
    let (pw, ph) = deck.page_emu();
    let width = MODEL_W;
    let height = if pw > 0.0 { MODEL_W * ph / pw } else { MODEL_H };
    let page = Page {
        sy: height / MODEL_H,
    };

    let mut src = String::new();
    let mut losses: Vec<Loss> = Vec::new();
    let mut drawn = 0usize;
    for (index, slide) in deck.slides.iter().enumerate() {
        if slide.hidden {
            continue;
        }
        let master = deck.master_of(slide);
        if drawn > 0 {
            src.push_str("#pagebreak()\n");
        }
        src.push_str(&format!(
            "#set page(width: {}, height: {}, margin: 0pt, fill: {})\n#h(0pt)\n",
            pt(width),
            pt(height),
            background(deck, slide, master)
        ));
        drawn += 1;

        let mut left_out = Vec::new();
        let mut approximated = None;
        // The master's and layout's decorations sit under the slide's own
        // objects. Their text placeholders hold prompt text ("Click to edit …"),
        // which is never shown on a slide.
        let layout = master.and_then(|m| {
            let id = slide.layout_id?;
            m.layouts.iter().find(|l| l.id == id)
        });
        let under = master
            .map(|m| m.shapes.iter())
            .into_iter()
            .flatten()
            .chain(layout.map(|l| l.shapes.iter()).into_iter().flatten())
            .filter(
                |o| !matches!(o, SlideObject::TextBox { body, .. } if body.placeholder.is_some()),
            );
        for o in under.chain(slide.objects.iter()) {
            if let Some(typst) = object(o, &page, &mut left_out, &mut approximated) {
                src.push_str(&typst);
            }
        }
        let location = format!("slide {}", index + 1);
        for (id, label, level) in left_out {
            losses.push(Loss {
                id: id.into(),
                label: label.into(),
                location: location.clone(),
                level,
                detail:
                    "The Typst slide renderer does not draw this yet; Decks' own PDF export does."
                        .into(),
            });
        }
        if let Some(detail) = approximated {
            losses.push(Loss {
                id: "shape-approximate".into(),
                label: "Shape drawn approximately".into(),
                location,
                level: LossLevel::Minor,
                detail,
            });
        }
        if slide.background_picture.is_some() {
            losses.push(Loss {
                id: "background-picture-omitted".into(),
                label: "Background picture left out".into(),
                location: format!("slide {}", index + 1),
                level: LossLevel::Minor,
                detail: "The slide's background is drawn as its colour only.".into(),
            });
        }
    }
    if drawn == 0 {
        return Err("the deck has no visible slides to draw".into());
    }
    losses.insert(
        0,
        Loss {
            id: "typst-approximate".into(),
            label: "Approximate PDF".into(),
            location: "whole deck".into(),
            level: LossLevel::Minor,
            detail: "The PDF is a Typst re-drawing of the slides, not Decks' own render.".into(),
        },
    );
    Ok((src, losses))
}

#[cfg(test)]
mod tests {
    use super::*;
    use decks_core::engine::text_body::TextBody;

    fn text_slide(text: &str) -> SlideObject {
        SlideObject::TextBox {
            text: text.into(),
            x: 40.0,
            y: 40.0,
            w: 400.0,
            h: 80.0,
            rotation: 0.0,
            runs: vec![Run::plain(text)],
            body: TextBody::default(),
        }
    }

    #[test]
    fn slide_text_is_a_string_so_markup_in_it_is_inert() {
        let mut deck = Deck::new();
        deck.slides[0]
            .objects
            .push(text_slide("#set page(width: 1pt) *x* \"quoted\" \\ back"));
        let (src, _) = deck_typst(&deck).unwrap();
        assert!(
            src.contains("#\"#set page(width: 1pt) *x* \\\"quoted\\\" \\\\ back\""),
            "{src}"
        );
    }

    #[test]
    fn each_visible_slide_is_a_page_and_a_hidden_one_is_not_drawn() {
        let mut deck = Deck::new();
        let mut second = deck.slides[0].clone();
        second.hidden = true;
        deck.slides.push(second);
        deck.slides.push(deck.slides[0].clone());
        let (src, _) = deck_typst(&deck).unwrap();
        assert_eq!(src.matches("#set page(").count(), 2);
        assert_eq!(src.matches("#pagebreak()").count(), 1);
    }

    #[test]
    fn what_cannot_be_drawn_is_named_per_slide_and_the_pdf_is_labelled_approximate() {
        let mut deck = Deck::new();
        deck.slides[0].objects.push(SlideObject::Image {
            path: "/nonexistent.png".into(),
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            rotation: 0.0,
            crop: Default::default(),
        });
        let (_, losses) = deck_typst(&deck).unwrap();
        assert_eq!(losses[0].id, "typst-approximate");
        let picture = losses
            .iter()
            .find(|l| l.id == "picture-omitted")
            .expect("a picture loss");
        assert_eq!(picture.location, "slide 1");
        assert_eq!(picture.level, LossLevel::Major);
    }

    #[test]
    fn a_deck_with_nothing_visible_is_an_error_not_an_empty_pdf() {
        let mut deck = Deck::new();
        deck.slides[0].hidden = true;
        assert!(deck_typst(&deck).is_err());
    }

    #[test]
    fn the_source_compiles_to_a_pdf_with_a_page_per_slide() {
        let mut deck = Deck::new();
        deck.slides[0].objects.push(text_slide("Quarterly review"));
        deck.slides[0].objects.push(SlideObject::Rect {
            x: 500.0,
            y: 100.0,
            w: 120.0,
            h: 60.0,
            rotation: 15.0,
        });
        deck.slides.push(deck.slides[0].clone());
        let (src, _) = deck_typst(&deck).unwrap();
        let pdf = suite_export::compile_pdf(&src).expect("typst compiles the slides");
        assert!(pdf.starts_with(b"%PDF-"));
        let pages = hayro::hayro_syntax::Pdf::new(pdf)
            .expect("a readable pdf")
            .pages()
            .len();
        assert_eq!(pages, 2);
    }
}
