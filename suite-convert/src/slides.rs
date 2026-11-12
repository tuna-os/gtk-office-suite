// slides.rs — a deck as Typst source, so a presentation can become a PDF (and,
// through the rasteriser, a PNG) without Decks' cairo and pango renderer.
//
// This is a re-drawing, not Decks' own render: the PDF it makes is labelled
// approximate in the loss report, as Letters' Typst export is (ADR 0010). It
// draws what a slide is mostly made of — backgrounds, text, solid shapes,
// pictures — and says plainly, per slide, what it left out (tables, charts,
// pictures it cannot read), so a
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
    files: &mut Files,
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
        SlideObject::Image { path, w, h, crop, .. } => match files.add(path) {
            Some(name) => picture(&name, *w, *h * page.sy, crop),
            None => {
                left_out.push(("picture-omitted", "Picture left out", LossLevel::Major));
                return None;
            }
        },
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

/// The picture files a deck refers to, by the name the Typst source uses.
#[derive(Default)]
struct Files {
    list: Vec<(String, Vec<u8>)>,
    by_path: std::collections::HashMap<String, Option<String>>,
}

impl Files {
    /// The name `path` is served under, or `None` when it cannot be read or is
    /// not a format Typst draws (png, jpeg, gif, webp, svg).
    fn add(&mut self, path: &str) -> Option<String> {
        if let Some(known) = self.by_path.get(path) {
            return known.clone();
        }
        let name = std::fs::read(path).ok().and_then(|bytes| {
            let ext = image_ext(&bytes)?;
            let name = format!("picture{}.{ext}", self.list.len());
            self.list.push((name.clone(), bytes));
            Some(name)
        });
        self.by_path.insert(path.to_string(), name.clone());
        name
    }
}

/// The file extension for the image formats Typst reads, from the bytes.
fn image_ext(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if b.starts_with(b"GIF8") {
        Some("gif")
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("webp")
    } else {
        let head = String::from_utf8_lossy(&b[..b.len().min(512)]);
        let head = head.trim_start();
        (head.starts_with("<svg") || (head.starts_with("<?xml") && head.contains("<svg"))).then_some("svg")
    }
}

/// A picture stretched to its `w` x `h` box (points), after the crop trims its
/// source: the whole source is scaled so the kept part fills the box, then
/// clipped to it.
fn picture(name: &str, w: f64, h: f64, crop: &decks_core::engine::Crop) -> String {
    let l = crop.left.clamp(0.0, 1.0);
    let t = crop.top.clamp(0.0, 1.0);
    let kept_w = (1.0 - l - crop.right.clamp(0.0, 1.0)).max(1e-6);
    let kept_h = (1.0 - t - crop.bottom.clamp(0.0, 1.0)).max(1e-6);
    let (full_w, full_h) = (w / kept_w, h / kept_h);
    format!(
        "#box(width: {}, height: {}, clip: true)[#place(top + left, dx: {}, dy: {}, image({}, width: {}, height: {}, fit: \"stretch\"))]\n",
        pt(w),
        pt(h),
        pt(-l * full_w),
        pt(-t * full_h),
        string(name),
        pt(full_w),
        pt(full_h),
    )
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

/// What drawing a deck produced: Typst source, what was left out or
/// approximated, and the picture files the source refers to by name.
pub struct Rendered {
    pub source: String,
    pub losses: Vec<Loss>,
    pub files: Vec<(String, Vec<u8>)>,
}

/// [`render`] without the picture files, for a deck with no pictures to draw
/// (the source of one that has pictures needs them to compile).
pub fn deck_typst(deck: &Deck) -> Result<(String, Vec<Loss>), String> {
    render(deck).map(|r| (r.source, r.losses))
}

/// A deck as Typst source plus what was left out or approximated. Hidden slides
/// are not drawn, as they are not presented.
pub fn render(deck: &Deck) -> Result<Rendered, String> {
    let (pw, ph) = deck.page_emu();
    let width = MODEL_W;
    let height = if pw > 0.0 { MODEL_W * ph / pw } else { MODEL_H };
    let page = Page {
        sy: height / MODEL_H,
    };

    let mut src = String::new();
    let mut losses: Vec<Loss> = Vec::new();
    let mut files = Files::default();
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
            if let Some(typst) = object(o, &page, &mut files, &mut left_out, &mut approximated) {
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
    Ok(Rendered {
        source: src,
        losses,
        files: files.list,
    })
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


    /// A 1x1 PNG.
    const PNG: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50, 0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82];

    fn picture_deck(path: &str, crop: decks_core::engine::Crop) -> Deck {
        let mut deck = Deck::new();
        deck.slides[0].objects.push(SlideObject::Image {
            path: path.into(),
            x: 10.0,
            y: 20.0,
            w: 200.0,
            h: 100.0,
            rotation: 0.0,
            crop,
        });
        deck
    }

    #[test]
    fn a_readable_picture_is_drawn_and_compiles_into_the_pdf() {
        let dir = std::env::temp_dir().join(format!("slides-pic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.png");
        std::fs::write(&path, PNG).unwrap();
        let deck = picture_deck(&path.to_string_lossy(), Default::default());
        let r = render(&deck).unwrap();
        assert!(r.source.contains("image(\"picture0.png\""), "{}", r.source);
        assert_eq!(r.files.len(), 1);
        assert!(r.losses.iter().all(|l| l.id != "picture-omitted"));
        let pdf = suite_export::compile_pdf_with_files(&r.source, &r.files).expect("compiles with the picture");
        assert!(pdf.starts_with(b"%PDF-"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_same_picture_twice_is_served_once() {
        let dir = std::env::temp_dir().join(format!("slides-pic2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.png");
        std::fs::write(&path, PNG).unwrap();
        let mut deck = picture_deck(&path.to_string_lossy(), Default::default());
        let again = deck.slides[0].objects[0].clone();
        deck.slides[0].objects.push(again);
        assert_eq!(render(&deck).unwrap().files.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_that_is_not_an_image_typst_reads_is_left_out_by_name() {
        let dir = std::env::temp_dir().join(format!("slides-pic3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clip.mp4");
        std::fs::write(&path, b"\0\0\0\x18ftypmp42").unwrap();
        let r = render(&picture_deck(&path.to_string_lossy(), Default::default())).unwrap();
        assert!(r.files.is_empty());
        assert!(r.losses.iter().any(|l| l.id == "picture-omitted"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_crop_scales_the_source_up_and_shifts_it_inside_a_clipped_box() {
        let crop = decks_core::engine::Crop { left: 0.25, top: 0.0, right: 0.25, bottom: 0.0 };
        let typst = picture("p.png", 200.0, 100.0, &crop);
        // Half the width is kept, so the whole source is drawn twice as wide
        // (400pt) and moved left by the cropped quarter (100pt).
        assert!(typst.contains("clip: true"), "{typst}");
        assert!(typst.contains("dx: -100.00pt"), "{typst}");
        assert!(typst.contains("width: 400.00pt"), "{typst}");
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
