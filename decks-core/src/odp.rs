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

pub(crate) fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── Writing ──────────────────────────────────────────────────────────

/// One run as a `text:span`, naming the automatic style it was assigned.
///
/// `prefix` namespaces that style name. `content.xml` and `styles.xml` each
/// carry their own `office:automatic-styles`, but the reader merges both
/// into one map keyed by name — so a master's `T1` in `styles.xml` would
/// quietly redefine a slide's `T1` from `content.xml` and restyle text on
/// an unrelated slide. Masters therefore use their own prefix.
/// Automatic text styles declared in `content.xml`, for slide runs.
const SLIDE_STYLE_PREFIX: &str = "T";

/// Automatic text styles declared in `styles.xml`, for one master's runs.
///
/// Per master rather than one shared set, so two masters using the same
/// index cannot collide either.
fn master_style_prefix(master_idx: usize) -> String {
    format!("MT{}_", master_idx + 1)
}

fn run_span(run: &Run, style_idx: usize, prefix: &str) -> String {
    if run.style == RunStyle::default() {
        esc(&run.text)
    } else {
        format!(
            "<text:span text:style-name=\"{prefix}{style_idx}\">{}</text:span>",
            esc(&run.text)
        )
    }
}

/// A styled text box's runs as `text:p` elements, one per paragraph.
///
/// The line break inside a run's text is a *paragraph* break and has to be
/// written as one. ODF collapses a literal newline in text content to a
/// space, so emitting one `text:p` containing "Bold one\nplain two" makes
/// Impress read back a single line reading "Bold one plain two" — the break
/// silently gone, which our own reader could not see because it put the
/// newline back when parsing its own single paragraph. The runs-empty path
/// above always split correctly; only the styled one did not.
fn styled_paragraphs(
    runs: &[Run],
    style_of: &dyn Fn(&RunStyle) -> usize,
    prefix: &str,
) -> String {
    let mut out = String::new();
    let mut para = String::new();
    for run in runs {
        let mut pieces = run.text.split('\n');
        if let Some(first) = pieces.next() {
            if !first.is_empty() {
                para.push_str(&run_span(
                    &Run { text: first.to_string(), style: run.style.clone() },
                    style_of(&run.style),
                    prefix,
                ));
            }
        }
        for piece in pieces {
            out.push_str(&format!("<text:p>{para}</text:p>"));
            para.clear();
            if !piece.is_empty() {
                para.push_str(&run_span(
                    &Run { text: piece.to_string(), style: run.style.clone() },
                    style_of(&run.style),
                    prefix,
                ));
            }
        }
    }
    out.push_str(&format!("<text:p>{para}</text:p>"));
    out
}

/// The distinct non-default run styles used by `shapes`, in first-use order.
/// Their position here is the index that names them.
fn distinct_run_styles(shapes: &[SlideObject]) -> Vec<RunStyle> {
    let mut styles: Vec<RunStyle> = Vec::new();
    let mut add = |runs: &[Run]| {
        for r in runs {
            if r.style != RunStyle::default() && !styles.contains(&r.style) {
                styles.push(r.style.clone());
            }
        }
    };
    for obj in shapes {
        match obj {
            SlideObject::TextBox { runs, .. } => add(runs),
            SlideObject::Table { table, .. } => {
                for cell in table.rows.iter().flatten() {
                    add(&cell.runs);
                }
            }
            _ => {}
        }
    }
    styles
}

/// Declare `styles` as `style:family="text"` automatic styles under
/// `prefix`, matching what `run_span` writes for the same prefix.
fn declare_run_styles(styles: &[RunStyle], prefix: &str) -> String {
    let mut out = String::new();
    for (i, st) in styles.iter().enumerate() {
        out.push_str(&format!(
            "<style:style style:name=\"{prefix}{}\" style:family=\"text\">\
             <style:text-properties{}/></style:style>",
            i + 1,
            text_style(st)
        ));
    }
    out
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
    if let Some(f) = st.font_family.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
        props.push_str(&format!(" fo:font-family=\"{}\"", esc(f)));
    }
    props
}

/// One picture the package has to carry: where it goes inside the archive,
/// what it is, and its bytes.
///
/// The odp writer dropped `SlideObject::Image` entirely before this — the
/// pptx path carried pictures and the odp path silently did not, so saving
/// a deck as odp lost every image in it. That is the same writer/reader
/// asymmetry as #716's, and like that one it cost the content on any
/// save-and-reopen, not only on recovery.
struct Media {
    zip_path: String,
    media_type: String,
    bytes: Vec<u8>,
}

/// The extension and media type for a picture, from its current extension.
///
/// ODF names the type in the manifest, so guessing wrong here is a
/// mislabelled part rather than a missing one. The pptx writer sidesteps
/// the question by naming every part `.png` whatever it actually is; this
/// keeps the real extension and labels it to match, and falls back to PNG
/// for an extension nobody recognises rather than refusing the save.
fn media_type_for(path: &str) -> (&'static str, &'static str) {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => ("jpg", "image/jpeg"),
        "gif" => ("gif", "image/gif"),
        "svg" => ("svg", "image/svg+xml"),
        "webp" => ("webp", "image/webp"),
        "bmp" => ("bmp", "image/bmp"),
        "tif" | "tiff" => ("tiff", "image/tiff"),
        _ => ("png", "image/png"),
    }
}

/// Emit a page's shapes. Shared by the slides in `content.xml` and the
/// master pages in `styles.xml`, so a shape kind cannot be written on one
/// and forgotten on the other.
///
/// `style_of` names the automatic text style for a run and `prefix`
/// namespaces it (see `run_span`). Both slides and master decorations pass
/// real ones: a master's runs are read back by both formats, so styling
/// them is carrying something that is honoured rather than decoration for
/// our own reader.
/// The automatic graphic styles (`style:family="graphic"`) the shapes in
/// one part use, named `{prefix}{n}` in first-use order.
pub(crate) struct GraphicStyles {
    prefix: String,
    styles: Vec<crate::engine::shape::ShapeStyle>,
    /// Text boxes' paragraph, list and frame styles, under the same prefix.
    text: crate::odp_text::TextStyles,
}

impl GraphicStyles {
    fn new(prefix: &str) -> Self {
        GraphicStyles { prefix: prefix.to_string(), styles: Vec::new(), text: crate::odp_text::TextStyles::new(prefix) }
    }

    fn name_of(&mut self, style: &crate::engine::shape::ShapeStyle) -> String {
        let i = match self.styles.iter().position(|s| s == style) {
            Some(i) => i,
            None => {
                self.styles.push(style.clone());
                self.styles.len() - 1
            }
        };
        format!("{}{}", self.prefix, i + 1)
    }

    /// The `<style:style>` declarations, for `office:automatic-styles`.
    /// A gradient is written as its mean colour: ODF gradients need named
    /// `draw:gradient` styles in office:styles, not yet written.
    fn declare(&self) -> String {
        let text = self.text.declare();
        self.styles
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let fill = match s.fill {
                    Some(c) => format!("draw:fill=\"solid\" draw:fill-color=\"#{}\"", c.to_hex().to_lowercase()),
                    None => "draw:fill=\"none\"".to_string(),
                };
                let stroke = match s.stroke {
                    Some(st) => format!(
                        "draw:stroke=\"solid\" svg:stroke-color=\"#{}\" svg:stroke-width=\"{}pt\"",
                        st.color.to_hex().to_lowercase(),
                        snap(st.width)
                    ),
                    None => "draw:stroke=\"none\"".to_string(),
                };
                format!(
                    "<style:style style:name=\"{}{}\" style:family=\"graphic\">\
                     <style:graphic-properties {fill} {stroke}/></style:style>",
                    self.prefix,
                    i + 1
                )
            })
            .collect::<String>()
            + &text
    }
}

fn shapes_xml(
    shapes: &[SlideObject],
    style_of: &dyn Fn(&RunStyle) -> usize,
    prefix: &str,
    media: &mut Vec<Media>,
    graphics: &mut GraphicStyles,
) -> Result<String, String> {
    let mut pages = String::new();
        for obj in shapes {
            match obj {
                SlideObject::TextBox { text, x, y, w, h, rotation, runs, body } if !body.is_plain() => {
                    // Paragraph styles, lists and the frame's anchor and
                    // padding (odp_text.rs).
                    let inner: Vec<String> = if runs.is_empty() {
                        text.split('\n').map(esc).collect()
                    } else {
                        crate::engine::text_body::paragraphs(runs)
                            .iter()
                            .map(|p| p.iter().map(|r| run_span(r, style_of(&r.style), prefix)).collect())
                            .collect()
                    };
                    let paras = graphics.text.paragraphs(body, &inner);
                    let style = graphics
                        .text
                        .frame_name(body)
                        .map(|n| format!("draw:style-name=\"{n}\" "))
                        .unwrap_or_default();
                    pages.push_str(&format!(
                        "<draw:frame {style}{}><draw:text-box>{paras}</draw:text-box></draw:frame>",
                        geometry(*x, *y, *w, *h, *rotation)
                    ));
                }
                SlideObject::TextBox { text, x, y, w, h, rotation, runs, .. } => {
                    let inner: String = if runs.is_empty() {
                        text.split('\n')
                            .map(|l| format!("<text:p>{}</text:p>", esc(l)))
                            .collect()
                    } else {
                        styled_paragraphs(runs, style_of, prefix)
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
                SlideObject::Table { x, y, w, h, rotation, table } => {
                    // A table in a frame, as Impress writes one. Its default
                    // look (header row, banding) isn't carried: ODF expresses
                    // it through a table template, not yet written.
                    let cols = table.rows.first().map_or(0, |r| r.len());
                    let mut t = format!("<draw:frame {}><table:table>", geometry(*x, *y, *w, *h, *rotation));
                    t.push_str(&"<table:table-column/>".repeat(cols));
                    for row in &table.rows {
                        t.push_str("<table:table-row>");
                        for cell in row {
                            let inner: String = if cell.runs.is_empty() {
                                "<text:p/>".to_string()
                            } else {
                                styled_paragraphs(&cell.runs, style_of, prefix)
                            };
                            t.push_str(&format!("<table:table-cell>{inner}</table:table-cell>"));
                        }
                        t.push_str("</table:table-row>");
                    }
                    t.push_str("</table:table></draw:frame>");
                    pages.push_str(&t);
                }
                SlideObject::Shape { kind, x, y, w, h, rotation, style } => {
                    use crate::engine::shape::ShapeKind;
                    let at = geometry(*x, *y, *w, *h, *rotation);
                    let gs = graphics.name_of(style);
                    pages.push_str(&match kind {
                        ShapeKind::Rect => format!("<draw:rect draw:style-name=\"{gs}\" {at}/>"),
                        ShapeKind::RoundRect { radius } => format!(
                            "<draw:rect draw:style-name=\"{gs}\" draw:corner-radius=\"{}pt\" {at}/>",
                            snap(radius.clamp(0.0, 0.5) * w.min(*h))
                        ),
                        ShapeKind::Ellipse => format!("<draw:ellipse draw:style-name=\"{gs}\" {at}/>"),
                        // Impress's names for the other DrawingML presets.
                        other => {
                            let ty = match other {
                                ShapeKind::Triangle => "isosceles-triangle".to_string(),
                                ShapeKind::Diamond => "diamond".to_string(),
                                _ => format!("ooxml-{}", other.prst()),
                            };
                            format!(
                                "<draw:custom-shape draw:style-name=\"{gs}\" {at}>\
                                 <draw:enhanced-geometry draw:type=\"{ty}\"/></draw:custom-shape>"
                            )
                        }
                    });
                }
                SlideObject::Image { path, x, y, w, h, rotation } => {
                    // Named by position in the package rather than after
                    // the source file: two decks can hold pictures called
                    // the same thing, and a name taken from the model is a
                    // name from a document we did not write.
                    let (ext, media_type) = media_type_for(path);
                    let zip_path = format!("Pictures/image{}.{ext}", media.len() + 1);
                    let bytes = std::fs::read(path)
                        .map_err(|e| format!("Cannot open image {path}: {e}"))?;
                    pages.push_str(&format!(
                        "<draw:frame {}>\
                         <draw:image xlink:href=\"{zip_path}\" xlink:type=\"simple\" \
                         xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>",
                        geometry(*x, *y, *w, *h, *rotation)
                    ));
                    media.push(Media { zip_path, media_type: media_type.to_string(), bytes });
                }
            }
        }
    Ok(pages)
}

fn content_xml(deck: &Deck, media: &mut Vec<Media>) -> Result<String, String> {
    // Distinct run styles across the deck, in first-use order.
    let mut styles: Vec<RunStyle> = Vec::new();
    for slide in &deck.slides {
        for st in distinct_run_styles(&slide.objects) {
            if !styles.contains(&st) {
                styles.push(st);
            }
        }
    }

    let mut auto = declare_run_styles(&styles, SLIDE_STYLE_PREFIX);
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
    let mut graphics = GraphicStyles::new("gr");
    for (si, slide) in deck.slides.iter().enumerate() {
        let bg = slide.background.trim_start_matches('#');
        let dp_attr = if bg.len() == 6 && !bg.eq_ignore_ascii_case("ffffff") {
            format!(" draw:style-name=\"dp{}\"", si + 1)
        } else {
            String::new()
        };
        // Omitted rather than written empty when unnamed, matching the
        // pptx writer: the attribute is optional, and `draw:name=""` is a
        // claim that the page is called nothing rather than unnamed.
        let name_attr = if slide.title.trim().is_empty() {
            String::new()
        } else {
            format!(" draw:name=\"{}\"", esc(&slide.title))
        };
        pages.push_str(&format!(
            "<draw:page{name_attr}{dp_attr}{}>",
            deck
                .masters
                .get(slide.master_idx.unwrap_or(0))
                .map(|m| format!(
                    " draw:master-page-name=\"{}\"",
                    esc(&encode_style_name(&m.name))
                ))
                .unwrap_or_default(),
        ));
        pages.push_str(&shapes_xml(&slide.objects, &style_of, SLIDE_STYLE_PREFIX, media, &mut graphics)?);
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
    auto.push_str(&graphics.declare());

    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <office:document-content \
         xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" \
         xmlns:presentation=\"urn:oasis:names:tc:opendocument:xmlns:presentation:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
         xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
         xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
         xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         office:version=\"1.2\">\
         <office:automatic-styles>{auto}</office:automatic-styles>\
         <office:body><office:presentation>{pages}</office:presentation></office:body>\
         </office:document-content>"
    ))
}

// ── Master pages ─────────────────────────────────────────────────────
//
// Decks reads a master from an imported deck and renders it — the canvas
// and the sidebar thumbnails both consult it — but neither writer emitted
// one, so every save handed the reader a package with no master and the
// reader synthesised a white default. An imported deck lost its design on
// the first save, not only on recovery.
//
// ODF puts masters in `styles.xml`, under `office:master-styles`, and each
// `draw:page` names the one it uses with `draw:master-page-name`. The
// writer had no `styles.xml` at all.

/// Escape a master's name into an ODF style token, the way LibreOffice
/// does: anything but an ASCII letter, digit or `-` becomes `_hh_`, so a
/// space is `_20_` and a literal underscore is `_5f_` (which is what keeps
/// the escape unambiguous to decode).
fn encode_style_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '-' {
            out.push(c);
        } else if (c as u32) < 0x100 {
            out.push_str(&format!("_{:02x}_", c as u32));
        } else {
            out.push_str(&format!("_{:x}_", c as u32));
        }
    }
    if out.is_empty() { "Default".into() } else { out }
}

/// The inverse, and tolerant: a token with no escapes, or a stray `_`, is
/// returned as-is rather than dropped.
fn decode_style_name(token: &str) -> String {
    let bytes: Vec<char> = token.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '_' {
            if let Some(end) = (i + 1..bytes.len()).find(|&j| bytes[j] == '_') {
                let hex: String = bytes[i + 1..end].iter().collect();
                if !hex.is_empty() && hex.len() <= 6 {
                    if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(c);
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// `styles.xml` — the page layout, one drawing-page style per master's
/// background, the master pages with their decorations, and the document's
/// default graphic font.
///
/// The font is **document-wide, not per master**, and that is ODF's shape
/// rather than a shortcut here. Converting a pptx that carries a per-master
/// theme font through LibreOffice Impress and reading back the `.odp` it
/// writes shows where Impress puts it:
///
/// ```text
/// <style:default-style style:family="graphic">
///   <style:text-properties style:font-name="Liberation Serif"/>
/// ```
///
/// One default for the document. Decks models one font per master, so a
/// deck whose masters name *different* fonts cannot keep them apart in
/// odp — the first master's font becomes the document's and the rest read
/// back as that one. `masters_keep_their_own_font_in_pptx_but_share_one_in_odp`
/// pins that, so the asymmetry is a recorded property rather than a
/// surprise. Writing per-master presentation styles instead was the
/// alternative, and it was not taken: nothing was found that reads them
/// back, so it would have round-tripped through this reader alone — the
/// exact shape of check this row keeps catching.
fn styles_xml(deck: &Deck, media: &mut Vec<Media>) -> Result<String, String> {
    let mut auto = String::from(
        "<style:page-layout style:name=\"PM1\">\
         <style:page-layout-properties fo:page-width=\"960pt\" fo:page-height=\"540pt\" \
         style:print-orientation=\"landscape\"/></style:page-layout>",
    );
    let mut pages = String::new();
    let mut master_graphics = GraphicStyles::new("mgr");
    for (i, master) in deck.masters.iter().enumerate() {
        let bg = master.background.trim_start_matches('#');
        let dp = if bg.len() == 6 {
            auto.push_str(&format!(
                "<style:style style:name=\"mdp{}\" style:family=\"drawing-page\">\
                 <style:drawing-page-properties draw:fill=\"solid\" draw:fill-color=\"#{}\"/>\
                 </style:style>",
                i + 1,
                bg.to_lowercase(),
            ));
            format!(" draw:style-name=\"mdp{}\"", i + 1)
        } else {
            String::new()
        };
        let mstyles = distinct_run_styles(&master.shapes);
        auto.push_str(&declare_run_styles(&mstyles, &master_style_prefix(i)));
        let style_of = |st: &RunStyle| {
            mstyles.iter().position(|s| s == st).map(|p| p + 1).unwrap_or(0)
        };
        pages.push_str(&format!(
            "<style:master-page style:name=\"{}\" style:page-layout-name=\"PM1\"{dp}>{}\
             </style:master-page>",
            esc(&encode_style_name(&master.name)),
            shapes_xml(&master.shapes, &style_of, &master_style_prefix(i), media, &mut master_graphics)?,
        ));
    }
    auto.push_str(&master_graphics.declare());
    // ODF wants a font it uses declared as well as referenced; Impress
    // writes both, and a reference to an undeclared face is what a
    // conforming reader is entitled to ignore.
    let font = deck
        .masters
        .first()
        .map(|m| m.font_family())
        .unwrap_or(MasterSlide::DEFAULT_FONT);
    let font_decls = format!(
        "<office:font-face-decls><style:font-face style:name=\"{f}\" \
         svg:font-family=\"&apos;{f}&apos;\"/></office:font-face-decls>",
        f = esc(font)
    );
    let default_style = format!(
        "<style:default-style style:family=\"graphic\">\
         <style:text-properties style:font-name=\"{}\"/></style:default-style>",
        esc(font)
    );
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <office:document-styles \
         xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" \
         xmlns:presentation=\"urn:oasis:names:tc:opendocument:xmlns:presentation:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
         xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
         xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
         xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         office:version=\"1.2\">\
         {font_decls}\
         <office:styles>{default_style}</office:styles>\
         <office:automatic-styles>{auto}</office:automatic-styles>\
         <office:master-styles>{pages}</office:master-styles>\
         </office:document-styles>"
    ))
}

/// The manifest, which has to name every part in the package — pictures
/// included. A picture in the archive the manifest does not list is the
/// failure this function exists to prevent: the bytes are there and a
/// conforming reader never looks at them.
fn manifest_xml(media: &[Media]) -> String {
    let mut entries = String::new();
    for m in media {
        entries.push_str(&format!(
            "<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"{}\"/>",
            esc(&m.zip_path),
            esc(&m.media_type),
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <manifest:manifest \
         xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" \
         manifest:version=\"1.2\">\
         <manifest:file-entry manifest:full-path=\"/\" \
         manifest:media-type=\"application/vnd.oasis.opendocument.presentation\"/>\
         <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
         <manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>\
         {entries}</manifest:manifest>"
    )
}

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
    // The XML is built first because building it is what discovers the
    // pictures: the manifest has to list them, and it is written before
    // them in the archive.
    let mut media: Vec<Media> = Vec::new();
    let content = content_xml(deck, &mut media)?;
    let styles = styles_xml(deck, &mut media)?;
    z.start_file("META-INF/manifest.xml", opt).map_err(|e| e.to_string())?;
    z.write_all(manifest_xml(&media).as_bytes()).map_err(|e| e.to_string())?;
    z.start_file("content.xml", opt).map_err(|e| e.to_string())?;
    z.write_all(content.as_bytes()).map_err(|e| e.to_string())?;
    z.start_file("styles.xml", opt).map_err(|e| e.to_string())?;
    z.write_all(styles.as_bytes()).map_err(|e| e.to_string())?;
    for m in &media {
        // Stored, not deflated: a PNG or JPEG is already compressed, and
        // deflating it again spends time to grow the archive.
        z.start_file(
            &m.zip_path,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .map_err(|e| e.to_string())?;
        z.write_all(&m.bytes).map_err(|e| e.to_string())?;
    }
    z.finish().map_err(|e| e.to_string()).map(|c| c.into_inner())
}

// ── Reading ──────────────────────────────────────────────────────────

pub(crate) fn parse_length_pt(v: &str) -> Option<f64> {
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

/// One attribute's value, with entities resolved.
///
/// Normalised rather than raw. Most attributes here are style names,
/// numbers and colours where an entity never appears, so reading the bytes
/// verbatim was latent — but a page named `R&D` is escaped in the
/// attribute, and the raw bytes give back `R&amp;D` as the name itself.
fn attr(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    e.attributes().filter_map(|a| a.ok()).find_map(|a| {
        if a.key.into_inner() == name {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|v| v.to_string())
        } else {
            None
        }
    })
}

/// The document's default graphic font, from
/// `style:default-style[@style:family="graphic"]/style:text-properties/@style:font-name`.
///
/// Scoped to the `graphic` family on purpose: `style:default-style` appears
/// once per family (paragraph, graphic, table…), and answering with the
/// first `style:text-properties` in the part would return whichever family
/// happens to come first in the file.
fn parse_default_graphic_font(xml: &str) -> Option<String> {
    if xml.is_empty() {
        return None;
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_graphic_default = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) if e.name().as_ref() == "style:default-style" => {
                in_graphic_default = attr(e, "style:family").as_deref() == Some("graphic");
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == "style:default-style" => {
                in_graphic_default = false;
            }
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e))
                if in_graphic_default && e.name().as_ref() == "style:text-properties" =>
            {
                if let Some(f) = attr(e, "style:font-name").filter(|f| !f.trim().is_empty()) {
                    return Some(f);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}

/// Collect the named text styles and drawing-page backgrounds a part
/// defines. Called for `content.xml` and again for `styles.xml`, because a
/// master page's background lives in the latter's automatic styles while
/// a slide's lives in the former's.
fn parse_styles(
    xml: &str,
    text_styles: &mut std::collections::HashMap<String, RunStyle>,
    page_bg: &mut std::collections::HashMap<String, String>,
) {
    {
        let mut reader = Reader::from_str(xml);
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
                                // LibreOffice names a face declared in
                                // office:font-face-decls, whose name is
                                // the family (or the family plus a
                                // number when two differ in pitch).
                                st.font_family = attr(&e, "fo:font-family")
                                    .or_else(|| attr(&e, "style:font-name"))
                                    .map(|f| f.trim().trim_matches('\'').to_string())
                                    .filter(|f| !f.is_empty());
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
}

/// One `draw:page` or `style:master-page`, with the master it names.
struct Page {
    slide: Slide,
    uses_master: Option<String>,
}

/// Walk a part's pages, collecting frames, shapes and notes.
///
/// `page_tag` is `draw:page` for `content.xml` and `style:master-page` for
/// the master pages in `styles.xml` — the two have the same children, so
/// one walker reads both. Reading masters through the same code as slides
/// is the point: a master's decorations came back as nothing while this
/// logic existed only for slides, and any shape the slide walker learns to
/// read the master walker now learns too.
/// The page extent `styles.xml` declares for the *slides*, in points.
///
/// The odp counterpart of `p:sldSz`: coordinates only mean something
/// relative to it, and reading them as points outright is right only for a
/// page of exactly the size our own writer emits (960pt x 540pt). See
/// `SlideScale` in the pptx reader for what that cost.
///
/// A document has several page layouts and taking the first is not good
/// enough: Impress writes the notes layout too, and that one is A4
/// *portrait*, which read a landscape slide as 1.21x wider and 0.48x
/// shorter — non-uniformly wrong, where the bug being fixed was at least
/// uniformly wrong. So the layout is looked up by the name the first
/// `style:master-page` points at, which is the one the slides use.
fn parse_page_size_pt(xml: &str) -> Option<(f64, f64)> {
    if xml.is_empty() {
        return None;
    }
    let mut layouts: std::collections::HashMap<String, (f64, f64)> = Default::default();
    let mut wanted: Option<String> = None;
    let mut cur: Option<String> = None;
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                match e.name().as_ref() {
                    "style:page-layout" => cur = attr(e, "style:name"),
                    "style:page-layout-properties" => {
                        if let Some(name) = cur.clone() {
                            let w = attr(e, "fo:page-width").and_then(|v| parse_length_pt(&v));
                            let h = attr(e, "fo:page-height").and_then(|v| parse_length_pt(&v));
                            if let (Some(w), Some(h)) = (w, h) {
                                if w > 0.0 && h > 0.0 {
                                    layouts.insert(name, (w, h));
                                }
                            }
                        }
                    }
                    // Guarded rather than a nested `if`: the first
                    // master-page wins, and later ones must fall through to
                    // `_` untouched.
                    "style:master-page" if wanted.is_none() => {
                        wanted = attr(e, "style:page-layout-name");
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == "style:page-layout" => cur = None,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    wanted.and_then(|n| layouts.get(&n).copied())
}

/// The page size our own writer emits, and what reading a length as model
/// units outright assumed.
const DEFAULT_PAGE_PT: (f64, f64) = (960.0, 540.0);

fn parse_pages(
    xml: &str,
    page_tag: &str,
    page_bg: &std::collections::HashMap<String, String>,
    text_styles: &std::collections::HashMap<String, RunStyle>,
    text_defs: &crate::odp_text::TextDefs,
    scale: (f64, f64),
    resolve_image: &mut dyn FnMut(&str) -> Option<String>,
) -> Result<Vec<Page>, String> {
    let mut pages: Vec<Page> = Vec::new();
    let mut uses_master: Option<String> = None;
    let mut reader = Reader::from_str(xml);
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
    // Paragraph layout of the box being read: the frame's graphic style,
    // the open `text:list`s' style names, and one style per `text:p`.
    let mut frame_style: Option<String> = None;
    let mut lists: Vec<Option<String>> = Vec::new();
    let mut paras: Vec<crate::engine::ParaStyle> = Vec::new();
    let body_of = |frame_style: &Option<String>, paras: &mut Vec<crate::engine::ParaStyle>| {
        let (anchor, insets) = text_defs.frame(frame_style.as_deref(), scale);
        crate::engine::TextBody { paras: std::mem::take(paras), anchor, insets, autofit: None }
    };

    // `svg:x`/`svg:y` place an unrotated shape; a rotated one carries
    // `draw:transform` instead and Impress omits them entirely, so the
    // transform is what has to be believed when both are present.
    // Lengths arrive in points and leave in model units; `scale` is what
    // the page this document declares makes one point worth. The transform
    // is parsed in points and scaled after, so its rotation is untouched.
    let geo = |e: &quick_xml::events::BytesStart| -> (f64, f64, f64, f64, f64) {
        let g = |n: &str| attr(e, n).and_then(|v| parse_length_pt(&v)).unwrap_or(0.0);
        let (w_pt, h_pt) = (g("svg:width"), g("svg:height"));
        let (x_pt, y_pt, rotation) =
            match attr(e, "draw:transform").and_then(|v| parse_transform(&v, w_pt, h_pt)) {
                Some((x, y, rotation)) => (x, y, rotation),
                None => (g("svg:x"), g("svg:y"), 0.0),
            };
        (x_pt * scale.0, y_pt * scale.1, w_pt * scale.0, h_pt * scale.1, rotation)
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                tag if tag == page_tag => {
                    let bg = attr(e, "draw:style-name")
                        .and_then(|n| page_bg.get(&n).cloned())
                        .unwrap_or_else(|| "#ffffff".into());
                    // A slide is named by draw:name; a master page by its
                    // style:name, which is an ODF style token and so is
                    // escaped (LibreOffice writes "Title_20_Slide").
                    uses_master = attr(e, "draw:master-page-name");
                    slide = Some(Slide {
                        title: attr(e, "draw:name")
                            .or_else(|| attr(e, "style:name").map(|n| decode_style_name(&n)))
                            .unwrap_or_default(),
                        background: bg,
                        objects: vec![],
                        notes: String::new(),
                        master_idx: Some(0),
                        transition: Default::default(),
                    });
                }
                "presentation:notes" => in_notes = true,
                "draw:frame" => {
                    frame = Some(geo(e));
                    frame_style = attr(e, "draw:style-name");
                }
                "text:list" => lists.push(attr(e, "text:style-name")),
                // A picture takes its geometry from the frame around it,
                // the way a text box does. `xlink:href` names a part inside
                // this package; the resolver turns it into something the
                // model can point at, and it is injected so that unpacking
                // a picture stays the caller's business rather than this
                // walker's.
                //
                // We write `<draw:image/>` empty and Impress writes it with
                // a caption paragraph inside, so it arrives as a Start
                // there and as an Empty here. Handling only one of the two
                // is how a picture survives our own round trip and vanishes
                // out of an Impress-written deck, which is why both arms
                // carry this.
                "draw:image" => {
                    if let (Some(s2), false, Some((x, y, w, h, rotation))) =
                        (slide.as_mut(), in_notes, frame)
                    {
                        if let Some(path) =
                            attr(e, "xlink:href").and_then(|href| resolve_image(&href))
                        {
                            s2.objects.push(SlideObject::Image { path, x, y, w, h, rotation });
                        }
                    }
                }
                "draw:text-box" => {
                    textbox = Some((Vec::new(), Vec::new()));
                    paras.clear();
                }
                // Impress converts pptx text boxes to custom-shapes with
                // text:p directly inside (no draw:text-box wrapper).
                "draw:custom-shape" => {
                    frame = Some(geo(e));
                    frame_style = attr(e, "draw:style-name");
                    textbox = Some((Vec::new(), Vec::new()));
                    paras.clear();
                    shape_type = None;
                }
                "draw:enhanced-geometry" => {
                    if let Some(t) = attr(e, "draw:type") {
                        shape_type = Some(t);
                    }
                }
                "text:p" => {
                    if let Some((lines, runs)) = textbox.as_mut() {
                        // The break between two paragraphs goes inside the
                        // runs as well as between the lines, so concatenated
                        // run text still equals the box's `text` — the
                        // invariant `SlideObject::TextBox` documents, and the
                        // one the pptx walker keeps the same way.
                        if let Some(last) = runs.last_mut() {
                            last.text.push('\n');
                        }
                        lines.push(String::new());
                        let list = lists.iter().flatten().next().map(String::as_str);
                        paras.push(text_defs.para(attr(e, "text:style-name").as_deref(), list, lists.len(), scale.0));
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
                // The empty form, which is what this writer emits — see the
                // Start arm above for why both are needed.
                "draw:image" => {
                    if let (Some(s2), false, Some((x, y, w, h, rotation))) =
                        (slide.as_mut(), in_notes, frame)
                    {
                        if let Some(path) =
                            attr(e, "xlink:href").and_then(|href| resolve_image(&href))
                        {
                            s2.objects.push(SlideObject::Image { path, x, y, w, h, rotation });
                        }
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
                "text:list" => {
                    lists.pop();
                }
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
                                // Every box keeps its runs. This used to
                                // drop them for anything multi-paragraph,
                                // "matching the pptx reader" — which is to
                                // say both readers lost a styled multi-line
                                // box's styling, one by discarding it and
                                // the other by never reading it.
                                s.objects.push(SlideObject::TextBox {
                                    text,
                                    x,
                                    y,
                                    w,
                                    h,
                                    rotation,
                                    runs,
                                    body: body_of(&frame_style, &mut paras),
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
                                s.objects.push(SlideObject::TextBox { text, x, y, w, h, rotation, runs, body: body_of(&frame_style, &mut paras) });
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
                tag if tag == page_tag => {
                    if let Some(s) = slide.take() {
                        pages.push(Page { slide: s, uses_master: uses_master.take() });
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error: {e}")),
            _ => {}
        }
    }
    Ok(pages)
}

/// Unpack one picture from the package and hand back a path the model can
/// point at.
///
/// It goes to an unpredictable temporary file, deliberately. The pptx
/// reader had this wrong once (gh-268): it wrote to
/// `/tmp/decks_img_<id>.<ext>` where both the middle and the suffix came
/// from the document, so a crafted package could steer the write through
/// `..` or a pre-created symlink. `NamedTempFile` gives O_EXCL, O_NOFOLLOW
/// and an unguessable name in one step, and `keep()` leaves it in place
/// because the model reads it back later.
///
/// `href` is document-controlled, so it may only name an entry of *this*
/// archive: the bytes come from `part_to_bytes`, which resolves inside the
/// zip and draws on the same read budget as the XML parts, so a picture
/// cannot be the hole a decompression bomb comes through (#442).
fn extract_picture(
    href: &str,
    zip: &mut zip::ZipArchive<std::fs::File>,
    budget: &mut ZipBudget,
) -> Option<String> {
    if href.is_empty() || href.starts_with("../") || href.contains("://") {
        return None;
    }
    let bytes = zip.part_to_bytes(href, budget).ok()?;
    let mut tmp = tempfile::NamedTempFile::new().ok()?;
    tmp.write_all(&bytes).ok()?;
    let (_, kept) = tmp.keep().ok()?;
    Some(kept.to_string_lossy().to_string())
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

    // First pass: text styles and drawing-page backgrounds, from both
    // parts — styles.xml is optional, and a deck we wrote before masters
    // existed has none.
    let styles = zip.optional_part_to_string("styles.xml", &mut budget);
    let mut text_styles: std::collections::HashMap<String, RunStyle> = Default::default();
    let mut page_bg: std::collections::HashMap<String, String> = Default::default();
    parse_styles(&content, &mut text_styles, &mut page_bg);
    parse_styles(&styles, &mut text_styles, &mut page_bg);
    let mut text_defs = crate::odp_text::TextDefs::default();
    text_defs.read(&content);
    text_defs.read(&styles);

    // One font for the whole document — see `styles_xml` for why ODF has
    // no per-master one to read. Every master gets it, so a deck saved and
    // reopened as odp renders in the font it was saved with even though
    // masters can no longer differ.
    // Read before any coordinate, since every one of them is relative to it.
    let scale = {
        let (w, h) = parse_page_size_pt(&styles).unwrap_or(DEFAULT_PAGE_PT);
        (960.0 / w, 540.0 / h)
    };

    let doc_font = parse_default_graphic_font(&styles)
        .unwrap_or_else(|| MasterSlide::DEFAULT_FONT.into());

    // Second pass: the master pages, then the slides that name them.
    let mut masters: Vec<MasterSlide> = Vec::new();
    let mut master_idx_by_name: std::collections::HashMap<String, usize> = Default::default();
    let master_pages = {
        let mut resolve = |href: &str| extract_picture(href, &mut zip, &mut budget);
        parse_pages(&styles, "style:master-page", &page_bg, &text_styles, &text_defs, scale, &mut resolve)?
    };
    for page in master_pages {
        master_idx_by_name.insert(page.slide.title.clone(), masters.len());
        masters.push(MasterSlide {
            name: page.slide.title,
            background: page.slide.background,
            default_font: doc_font.clone(),
            shapes: page.slide.objects,
        });
    }
    if masters.is_empty() {
        masters.push(MasterSlide {
            name: "Default".into(),
            background: "#ffffff".into(),
            default_font: doc_font.clone(),
            shapes: vec![],
        });
    }

    let mut deck = Deck { slides: Vec::new(), masters };
    let slide_pages = {
        let mut resolve = |href: &str| extract_picture(href, &mut zip, &mut budget);
        parse_pages(&content, "draw:page", &page_bg, &text_styles, &text_defs, scale, &mut resolve)?
    };
    for (i, page) in slide_pages.into_iter().enumerate() {
        let mut slide = page.slide;
        // A page that names nothing gets a positional label, the same one
        // the pptx reader falls back to, so an unnamed slide does not come
        // back nameless in one format and labelled in the other. Applied
        // here rather than in `parse_pages`, which also walks master pages
        // — a master's title is its name, and "Slide 3" is not a master.
        if slide.title.trim().is_empty() {
            slide.title = format!("Slide {}", i + 1);
        }
        slide.master_idx = page
            .uses_master
            .as_deref()
            .map(decode_style_name)
            .and_then(|n| master_idx_by_name.get(&n).copied())
            .or(Some(0));
        deck.slides.push(slide);
    }

    if deck.slides.is_empty() {
        deck.slides.push(Slide {
            title: "Slide 1".into(),
            background: "#ffffff".into(),
            objects: vec![],
            notes: String::new(),
            master_idx: Some(0),
            transition: Default::default(),
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
                body: Default::default(),
            }],
            notes: notes.into(),
            master_idx: Some(0),
            transition: Default::default(),
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

    /// An ODF style token is not a free-text name: LibreOffice writes a
    /// space as `_20_`, and a master called "House Style" comes back as
    /// "House_20_Style" to a reader that does not unescape. The underscore
    /// escaping its own self is what keeps the decode unambiguous.
    #[test]
    fn a_master_name_survives_the_style_token_escaping() {
        for name in ["House Style", "Default", "a_b", "x-y", "déjà vu", "2026 Q1"] {
            let token = encode_style_name(name);
            assert!(
                token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "{name:?} encoded to {token:?}, which is not a style token",
            );
            assert_eq!(decode_style_name(&token), name, "via {token:?}");
        }
        assert_eq!(encode_style_name("House Style"), "House_20_Style");
        assert_eq!(encode_style_name("a_b"), "a_5f_b");
        assert_eq!(encode_style_name(""), "Default", "an unnamed master still needs a token");
    }

    /// A token from elsewhere that is not escaped at all, or is escaped in
    /// a way we do not recognise, has to come back as itself rather than
    /// losing characters.
    #[test]
    fn an_unescaped_style_token_decodes_to_itself() {
        assert_eq!(decode_style_name("Standard"), "Standard");
        assert_eq!(decode_style_name("a_b"), "a_b");
        assert_eq!(decode_style_name("trailing_"), "trailing_");
    }

    /// The master page has to name the background style it defines, and
    /// the slide has to name the master — three references that have to
    /// agree or the deck comes back with a white default.
    /// Impress writes a notes layout beside the slide one, and the notes
    /// layout is A4 *portrait*. Taking the first `style:page-layout` read a
    /// landscape slide as 1.21x wider and 0.48x shorter — non-uniformly
    /// wrong, where the bug it was meant to fix was at least uniform. The
    /// layout the first `style:master-page` names is the slides'.
    #[test]
    fn the_page_size_comes_from_the_layout_the_master_names() {
        let xml = "<office:document-styles xmlns:office=\"o\" xmlns:style=\"s\" \
             xmlns:fo=\"f\">\
             <office:automatic-styles>\
             <style:page-layout style:name=\"PM2\">\
             <style:page-layout-properties fo:page-width=\"595pt\" \
             fo:page-height=\"842pt\"/></style:page-layout>\
             <style:page-layout style:name=\"PM1\">\
             <style:page-layout-properties fo:page-width=\"720pt\" \
             fo:page-height=\"405pt\"/></style:page-layout>\
             </office:automatic-styles>\
             <office:master-styles>\
             <style:master-page style:name=\"Default\" \
             style:page-layout-name=\"PM1\"/>\
             </office:master-styles></office:document-styles>";
        assert_eq!(
            parse_page_size_pt(xml),
            Some((720.0, 405.0)),
            "the A4 notes layout was read instead of the slide's"
        );
    }

    /// No master, no size: the caller falls back rather than guessing one.
    #[test]
    fn a_document_naming_no_layout_reads_as_none() {
        assert_eq!(parse_page_size_pt("<office:document-styles xmlns:office=\"o\"/>"), None);
        assert_eq!(parse_page_size_pt(""), None);
    }

    /// Our own page size must map one point to one model unit, or every
    /// document we wrote before this shifts.
    #[test]
    fn our_own_page_size_is_the_identity() {
        let (w, h) = DEFAULT_PAGE_PT;
        assert!((960.0 / w - 1.0).abs() < 1e-12);
        assert!((540.0 / h - 1.0).abs() < 1e-12);
    }

    #[test]
    fn styles_xml_ties_the_master_its_background_and_the_slide_together() {
        let deck = Deck {
            masters: vec![MasterSlide {
                name: "House Style".into(),
                background: "#204060".into(),
                default_font: "Sans".into(),
                shapes: vec![],
            }],
            slides: vec![Slide {
                title: "one".into(),
                background: String::new(),
                objects: vec![],
                notes: String::new(),
                master_idx: Some(0),
                transition: Default::default(),
            }],
        };
        let styles = styles_xml(&deck, &mut Vec::new()).unwrap();
        assert!(styles.contains("draw:fill-color=\"#204060\""), "{styles}");
        assert!(styles.contains("style:name=\"House_20_Style\""), "{styles}");
        assert!(styles.contains("draw:style-name=\"mdp1\""), "{styles}");
        assert!(
            content_xml(&deck, &mut Vec::new())
                .unwrap()
                .contains("draw:master-page-name=\"House_20_Style\""),
            "the slide must name its master, or every slide lands on the first",
        );
    }

    /// A tiny valid PNG, inline so the tests need no fixture file.
    fn a_png() -> &'static [u8] {
        &[
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a,
            0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0, 0, 0,
            0xfd, 0xd4, 0x9a, 0x73,
            0, 0, 0, 21, b'I', b'D', b'A', b'T', 0x78, 0x9c, 0x62, 0xfa, 0xcf, 0xc0, 0xc0,
            0xf0, 0x1f, 0x88, 0xff, 0x33, 0x30, 0x30, 0x00, 0x00, 0x00, 0xff, 0xff,
            0x03, 0x00, 0x2b, 0x11, 0x04, 0xf9,
            0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
        ]
    }

    fn deck_with_picture(path: &str) -> Deck {
        Deck {
            masters: vec![MasterSlide {
                name: "Default".into(),
                background: "#ffffff".into(),
                default_font: "Sans".into(),
                shapes: vec![],
            }],
            slides: vec![Slide {
                title: "p".into(),
                background: String::new(),
                objects: vec![SlideObject::Image {
                    path: path.to_string(),
                    x: 10.0,
                    y: 20.0,
                    w: 30.0,
                    h: 40.0,
                    rotation: 0.0,
                }],
                notes: String::new(),
                master_idx: Some(0),
                transition: Default::default(),
            }],
        }
    }

    /// The manifest has to name the picture. Bytes in the archive that the
    /// manifest does not list are a part a conforming reader never opens,
    /// which is indistinguishable from having dropped the picture — and it
    /// is the failure a round-trip test through our own reader would miss,
    /// because our reader goes straight to the href.
    #[test]
    fn a_picture_is_listed_in_the_manifest_and_present_in_the_archive() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("dot.png");
        std::fs::write(&src, a_png()).unwrap();
        let bytes = write_bytes(&deck_with_picture(src.to_str().unwrap())).unwrap();

        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let names: Vec<String> = zip.file_names().map(str::to_string).collect();
        assert!(
            names.iter().any(|n| n == "Pictures/image1.png"),
            "the picture is not in the archive: {names:?}",
        );

        let mut manifest = String::new();
        std::io::Read::read_to_string(
            &mut zip.by_name("META-INF/manifest.xml").unwrap(),
            &mut manifest,
        )
        .unwrap();
        assert!(
            manifest.contains("manifest:full-path=\"Pictures/image1.png\"")
                && manifest.contains("manifest:media-type=\"image/png\""),
            "the manifest does not declare the picture: {manifest}",
        );
    }

    /// The extension and the declared type have to agree, or the package
    /// says PNG about a JPEG. The writer keeps the source's extension
    /// rather than calling everything `.png` the way the pptx writer does.
    #[test]
    fn a_pictures_declared_type_matches_its_extension() {
        for (name, ext, media_type) in [
            ("a.png", "png", "image/png"),
            ("a.jpg", "jpg", "image/jpeg"),
            ("a.JPEG", "jpg", "image/jpeg"),
            ("a.gif", "gif", "image/gif"),
            ("a.svg", "svg", "image/svg+xml"),
            // Not a picture extension we know: labelled PNG rather than
            // refusing the save, since the alternative is losing the deck.
            ("a.qqq", "png", "image/png"),
            ("noextension", "png", "image/png"),
        ] {
            assert_eq!(media_type_for(name), (ext, media_type), "for {name}");
        }
    }

    /// A second picture must not overwrite the first. Naming parts after
    /// the source file would collide the moment two pictures shared a
    /// name, which is why they are numbered by position instead.
    #[test]
    fn two_pictures_get_two_parts() {
        let dir = tempfile::tempdir().unwrap();
        // Deliberately the same file name in different directories, which
        // is the collision a source-derived name would hit.
        let (a, b) = (dir.path().join("one"), dir.path().join("two"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let (p1, p2) = (a.join("dot.png"), b.join("dot.png"));
        std::fs::write(&p1, a_png()).unwrap();
        std::fs::write(&p2, a_png()).unwrap();

        let mut deck = deck_with_picture(p1.to_str().unwrap());
        deck.slides[0].objects.push(SlideObject::Image {
            path: p2.to_string_lossy().to_string(),
            x: 50.0,
            y: 60.0,
            w: 30.0,
            h: 40.0,
            rotation: 0.0,
        });
        let bytes = write_bytes(&deck).unwrap();
        let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let pictures: Vec<&str> =
            zip.file_names().filter(|n| n.starts_with("Pictures/")).collect();
        assert_eq!(pictures.len(), 2, "expected two picture parts, got {pictures:?}");
    }

    /// A missing source file fails the save rather than writing a deck with
    /// a picture-shaped hole in it. That matches the pptx writer, which has
    /// always refused, and it is the honest answer for a format whose
    /// promise is that a save keeps what the deck had.
    #[test]
    fn a_picture_whose_file_is_gone_fails_the_save() {
        let err = write_bytes(&deck_with_picture("/nonexistent/dot.png")).unwrap_err();
        assert!(err.contains("Cannot open image"), "unhelpful error: {err}");
    }

    /// Where an unpacked picture lands must not be steerable by the
    /// document — the gh-268 failure, where the pptx reader built
    /// `/tmp/decks_img_<id>.<ext>` out of document-supplied pieces.
    ///
    /// This asserts the property rather than the guard. Rejecting an href
    /// with `../` in it reads like the security check and is not one: a
    /// zip lookup is a name lookup, so `../` resolves to "no such entry"
    /// with or without the guard, and a test feeding it paths that are not
    /// in the package would pass whether or not the guard existed. What
    /// actually keeps this safe is that the *destination* is a fresh
    /// temporary file and the href only ever selects which archive entry
    /// to read — so that is what is checked: the path we hand back carries
    /// nothing from the document, and it is not the source file either.
    #[test]
    fn an_unpacked_picture_lands_where_the_document_cannot_choose() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("distinctive-name.png");
        std::fs::write(&src, a_png()).unwrap();
        let path = dir.path().join("deck.odp");
        write(&deck_with_picture(src.to_str().unwrap()), path.to_str().unwrap()).unwrap();

        let back = read(path.to_str().unwrap()).unwrap();
        let [SlideObject::Image { path: unpacked, .. }] = back.slides[0].objects.as_slice()
        else {
            panic!("expected one picture, got {:?}", back.slides[0].objects)
        };
        assert_ne!(unpacked, &src.to_string_lossy().to_string(), "the source was reused");
        assert!(
            !unpacked.contains("distinctive-name") && !unpacked.contains("image1"),
            "the unpacked path is derived from the document: {unpacked}",
        );
        assert!(
            std::path::Path::new(unpacked).starts_with(std::env::temp_dir()),
            "the picture was unpacked outside the temp dir: {unpacked}",
        );
        assert_eq!(std::fs::read(unpacked).unwrap(), a_png(), "wrong bytes unpacked");
    }

    /// An href naming a part the package does not contain yields nothing,
    /// rather than a picture-shaped object pointing at an empty file.
    #[test]
    fn an_href_naming_no_part_yields_no_picture() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("dot.png");
        std::fs::write(&src, a_png()).unwrap();
        let path = dir.path().join("deck.odp");
        write(&deck_with_picture(src.to_str().unwrap()), path.to_str().unwrap()).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut budget = ZipBudget::default();
        assert!(extract_picture("Pictures/absent.png", &mut zip, &mut budget).is_none());
        assert!(extract_picture("", &mut zip, &mut budget).is_none());
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
            transition: Default::default(),
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
                body: Default::default(),
            }],
            notes: String::new(),
            master_idx: Some(0),
            transition: Default::default(),
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
                    body: Default::default(),
                }],
                transition: Default::default(),
            }],
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod default_font_tests {
    use super::parse_default_graphic_font;

    /// Two families carry a font and the graphic one is second, so a reader
    /// that answers with the first `style:text-properties` in the part
    /// returns the paragraph font instead. Ordering them this way is the
    /// whole point of the fixture.
    #[test]
    fn the_graphic_family_is_read_not_whichever_comes_first() {
        let xml = "<office:styles>\
            <style:default-style style:family=\"paragraph\">\
            <style:text-properties style:font-name=\"Paragraph Face\"/>\
            </style:default-style>\
            <style:default-style style:family=\"graphic\">\
            <style:text-properties style:font-name=\"Graphic Face\"/>\
            </style:default-style></office:styles>";
        assert_eq!(parse_default_graphic_font(xml).as_deref(), Some("Graphic Face"));
    }

    #[test]
    fn a_document_with_no_graphic_default_answers_nothing() {
        let xml = "<office:styles><style:default-style style:family=\"paragraph\">\
            <style:text-properties style:font-name=\"Paragraph Face\"/>\
            </style:default-style></office:styles>";
        assert_eq!(parse_default_graphic_font(xml), None);
        assert_eq!(parse_default_graphic_font(""), None);
    }

    #[test]
    fn a_blank_font_name_is_no_answer_rather_than_a_blank_one() {
        let xml = "<style:default-style style:family=\"graphic\">\
            <style:text-properties style:font-name=\"\"/></style:default-style>";
        assert_eq!(parse_default_graphic_font(xml), None);
    }

    /// The writer has to declare the face it references: a reference to an
    /// undeclared font is something a conforming reader may ignore, and
    /// Impress declares one in `office:font-face-decls` for exactly this
    /// reason.
    #[test]
    fn the_written_styles_declare_the_font_they_reference() {
        use crate::engine::{Deck, MasterSlide};
        let deck = Deck {
            slides: vec![],
            masters: vec![MasterSlide {
                name: "M".into(),
                background: "#ffffff".into(),
                default_font: "Liberation Serif".into(),
                shapes: vec![],
            }],
        };
        let xml = super::styles_xml(&deck, &mut Vec::new()).unwrap();
        assert!(
            xml.contains("<office:font-face-decls>")
                && xml.contains("style:font-face style:name=\"Liberation Serif\""),
            "the face should be declared: {xml}"
        );
        assert!(
            xml.contains("style:font-name=\"Liberation Serif\""),
            "and referenced: {xml}"
        );
    }

    /// A master naming nothing must not write `style:font-name=""` — an
    /// empty family is a different request from no family, and it is what
    /// the raw field would have produced.
    #[test]
    fn a_master_with_no_font_writes_the_fallback_not_an_empty_name() {
        use crate::engine::{Deck, MasterSlide};
        let deck = Deck {
            slides: vec![],
            masters: vec![MasterSlide {
                name: "M".into(),
                background: "#ffffff".into(),
                default_font: "   ".into(),
                shapes: vec![],
            }],
        };
        let xml = super::styles_xml(&deck, &mut Vec::new()).unwrap();
        assert!(!xml.contains("style:font-name=\"\""), "no empty font name: {xml}");
        assert!(
            xml.contains(&format!("style:font-name=\"{}\"", MasterSlide::DEFAULT_FONT)),
            "the fallback should be written: {xml}"
        );
    }
}
