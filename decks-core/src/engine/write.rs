// write.rs — pptx writing: write_pptx_bytes + shape writers.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of engine.rs (issue #247).

use super::model::*;
use super::notes::notes_slide_xml;

use std::fs::File;
use std::io::{Read, Write};
use zip::write::SimpleFileOptions;
use quick_xml::events::{Event, BytesStart, BytesEnd, BytesDecl, BytesText};
use quick_xml::Writer;
use letters_core::model::{Run, RunStyle};
use super::text_body::{Anchor, Bullet, MarkerSize, ParaAlign, ParaStyle, Spacing, TextBody};

/// Where a shape sits and how far it is turned — the bounding box in points
/// plus a rotation in degrees.
///
/// Grouped rather than passed as five loose `f64`s because threading
/// rotation through made three of these writers exceed clippy's argument
/// limit, and the honest fix for "too many arguments" is fewer arguments,
/// not an `allow` (two of them carried one already). A named box also stops
/// the circle call site being read as a centre: `Placement::of_circle`
/// converts once, where the geometry is known.
#[derive(Clone, Copy)]
struct Placement {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    rotation: f64,
}

impl Placement {
    /// A circle's model form is a centre and a radius; its `a:xfrm` is the
    /// bounding box, like every other shape's.
    fn of_circle(cx: f64, cy: f64, r: f64, rotation: f64) -> Self {
        Placement { x: cx - r, y: cy - r, w: 2.0 * r, h: 2.0 * r, rotation }
    }
}

/// Emit a shape's `a:xfrm`: position, size, and rotation when there is any.
///
/// Four call sites wrote these six lines identically, which is how rotation
/// came to be missing from all of them at once — the model has carried a
/// `rotation` for every object since the rotate gesture landed, and the pptx
/// writer emitted none of it while the odp writer did. Rotating a shape and
/// saving as pptx (the format an unsaved deck defaults to, so also the format
/// its crash snapshot uses) silently discarded the rotation.
///
/// OOXML measures `rot` in sixtieth-thousandths of a degree, positive
/// clockwise, and omits the attribute entirely for an unrotated shape —
/// writing `rot="0"` everywhere would be valid but would rewrite every
/// existing file's shapes on the next save for no reason.
fn write_xfrm<W: std::io::Write>(
    writer: &mut Writer<W>,
    at: Placement,
) -> Result<(), quick_xml::Error> {
    let Placement { x, y, w, h, rotation } = at;
    let mut xfrm = BytesStart::new("a:xfrm");
    if rotation.is_finite() && rotation.abs() > f64::EPSILON {
        let rot = (rotation.rem_euclid(360.0) * 60_000.0).round() as i64;
        if rot != 0 {
            xfrm.push_attribute(("rot", rot.to_string().as_str()));
        }
    }
    writer.write_event(Event::Start(xfrm))?;

    let mut off = BytesStart::new("a:off");
    off.push_attribute(("x", ((x * 9525.0).round() as i64).to_string().as_str()));
    off.push_attribute(("y", ((y * 9525.0).round() as i64).to_string().as_str()));
    writer.write_event(Event::Empty(off))?;

    let mut ext = BytesStart::new("a:ext");
    ext.push_attribute(("cx", ((w * 9525.0).round() as i64).to_string().as_str()));
    ext.push_attribute(("cy", ((h * 9525.0).round() as i64).to_string().as_str()));
    writer.write_event(Event::Empty(ext))?;

    writer.write_event(Event::End(BytesEnd::new("a:xfrm")))?;
    Ok(())
}

/// The `a:p` paragraphs of a text body: styled runs when present (shared
/// Run/RunStyle with Letters), else one default-styled run of `text`.
/// EMU for a length in model units.
fn emu(v: f64) -> String {
    ((v * 9525.0).round() as i64).to_string()
}

/// `a:bodyPr` with the box's insets and anchor, where they aren't the
/// defaults.
fn write_body_pr<W: std::io::Write>(writer: &mut Writer<W>, body: &TextBody) -> Result<(), quick_xml::Error> {
    let mut b = BytesStart::new("a:bodyPr");
    if let Some(i) = body.insets {
        b.push_attribute(("lIns", emu(i.left).as_str()));
        b.push_attribute(("tIns", emu(i.top).as_str()));
        b.push_attribute(("rIns", emu(i.right).as_str()));
        b.push_attribute(("bIns", emu(i.bottom).as_str()));
    }
    if body.anchor != Anchor::Top {
        b.push_attribute(("anchor", body.anchor.to_drawingml()));
    }
    let Some(fit) = body.autofit else {
        writer.write_event(Event::Empty(b))?;
        return Ok(());
    };
    writer.write_event(Event::Start(b))?;
    let mut n = BytesStart::new("a:normAutofit");
    let per = |v: f64| ((v * 100_000.0).round() as i64).to_string();
    if (fit.font_scale - 1.0).abs() > 1e-9 {
        n.push_attribute(("fontScale", per(fit.font_scale).as_str()));
    }
    if fit.line_reduction.abs() > 1e-9 {
        n.push_attribute(("lnSpcReduction", per(fit.line_reduction).as_str()));
    }
    writer.write_event(Event::Empty(n))?;
    writer.write_event(Event::End(BytesEnd::new("a:bodyPr")))?;
    Ok(())
}

/// `a:pPr` for a paragraph whose style isn't the default. Everything is
/// written out, bullets included: the box is written as a plain text box,
/// which inherits none of the placeholder styles it may have been read
/// from.
fn write_para_pr<W: std::io::Write>(writer: &mut Writer<W>, st: &ParaStyle) -> Result<(), quick_xml::Error> {
    if *st == ParaStyle::default() {
        return Ok(());
    }
    let mut p = BytesStart::new("a:pPr");
    if st.margin_left != 0.0 {
        p.push_attribute(("marL", emu(st.margin_left).as_str()));
    }
    if st.level != 0 {
        p.push_attribute(("lvl", st.level.to_string().as_str()));
    }
    if st.indent != 0.0 {
        p.push_attribute(("indent", emu(st.indent).as_str()));
    }
    if st.align != ParaAlign::Left {
        p.push_attribute(("algn", st.align.to_drawingml()));
    }
    writer.write_event(Event::Start(p))?;
    for (name, sp) in [("a:spcBef", st.space_before), ("a:spcAft", st.space_after)] {
        let (el, val) = match sp {
            Spacing::Units(u) if u != 0.0 => ("a:spcPts", (u * 72.0 / 96.0 * 100.0).round() as i64),
            Spacing::Lines(f) if f != 0.0 => ("a:spcPct", (f * 100_000.0).round() as i64),
            _ => continue,
        };
        writer.write_event(Event::Start(BytesStart::new(name)))?;
        let mut e = BytesStart::new(el);
        e.push_attribute(("val", val.to_string().as_str()));
        writer.write_event(Event::Empty(e))?;
        writer.write_event(Event::End(BytesEnd::new(name)))?;
    }
    // CT_TextParagraphProperties order: buClr, buSz*, buFont, then the
    // bullet itself.
    if st.bullet != Bullet::None {
        let m = &st.marker;
        if let Some(c) = &m.color {
            writer.write_event(Event::Start(BytesStart::new("a:buClr")))?;
            let mut clr = BytesStart::new("a:srgbClr");
            clr.push_attribute(("val", c.to_uppercase().as_str()));
            writer.write_event(Event::Empty(clr))?;
            writer.write_event(Event::End(BytesEnd::new("a:buClr")))?;
        }
        match m.size {
            Some(MarkerSize::Relative(f)) => {
                let mut e = BytesStart::new("a:buSzPct");
                e.push_attribute(("val", ((f * 100_000.0).round() as i64).to_string().as_str()));
                writer.write_event(Event::Empty(e))?;
            }
            Some(MarkerSize::Points(p)) => {
                let mut e = BytesStart::new("a:buSzPts");
                e.push_attribute(("val", ((p * 100.0).round() as i64).to_string().as_str()));
                writer.write_event(Event::Empty(e))?;
            }
            None => {}
        }
        if let Some(f) = m.font.as_deref().filter(|f| !f.trim().is_empty()) {
            let mut e = BytesStart::new("a:buFont");
            e.push_attribute(("typeface", f));
            writer.write_event(Event::Empty(e))?;
        }
    }
    match &st.bullet {
        Bullet::None => writer.write_event(Event::Empty(BytesStart::new("a:buNone")))?,
        Bullet::Char(c) => {
            let mut e = BytesStart::new("a:buChar");
            e.push_attribute(("char", c.as_str()));
            writer.write_event(Event::Empty(e))?;
        }
        Bullet::AutoNum { scheme, start } => {
            let mut e = BytesStart::new("a:buAutoNum");
            e.push_attribute(("type", scheme.as_str()));
            if *start != 1 {
                e.push_attribute(("startAt", start.to_string().as_str()));
            }
            writer.write_event(Event::Empty(e))?;
        }
    }
    writer.write_event(Event::End(BytesEnd::new("a:pPr")))?;
    Ok(())
}

fn write_paragraphs<W: std::io::Write>(
    writer: &mut Writer<W>,
    text: &str,
    runs: &[Run],
    body: &TextBody,
) -> Result<(), quick_xml::Error> {
    // Emit styled runs when present (shared Run/RunStyle with Letters);
    // otherwise a single default-styled run with the plain text.
    let plain: Vec<Run>;
    let effective: &[Run] = if runs.is_empty() {
        plain = vec![Run { text: text.to_string(), style: RunStyle::default() }];
        &plain
    } else {
        runs
    };
    // One `a:p` per paragraph. A line break is a paragraph break in
    // DrawingML, not a character: writing the whole box as one `a:p` with a
    // newline inside its `a:t` is what the odp writer used to do, and ODF
    // collapsed it to a space. LibreOffice happens to read a newline here
    // as a break, so this survived a round trip either way — which is
    // precisely why it needed fixing rather than measuring once and
    // trusting.
    let mut para = 0usize;
    writer.write_event(Event::Start(BytesStart::new("a:p")))?;
    write_para_pr(writer, &body.para(0))?;
    for (run, piece, starts_paragraph) in effective.iter().flat_map(|run| {
        let mut parts = run.text.split('\n').enumerate().peekable();
        std::iter::from_fn(move || {
            parts.next().map(|(i, piece)| (run, piece.to_string(), i > 0))
        })
    }) {
        if starts_paragraph {
            writer.write_event(Event::End(BytesEnd::new("a:p")))?;
            writer.write_event(Event::Start(BytesStart::new("a:p")))?;
            para += 1;
            write_para_pr(writer, &body.para(para))?;
        }
        if piece.is_empty() {
            continue;
        }
        writer.write_event(Event::Start(BytesStart::new("a:r")))?;
        let mut r_pr = BytesStart::new("a:rPr");
        r_pr.push_attribute(("lang", "en-US"));
        let sz = run.style.font_size_hp.map(|hp| hp as u32 * 50).unwrap_or(1800);
        r_pr.push_attribute(("sz", sz.to_string().as_str()));
        if run.style.bold { r_pr.push_attribute(("b", "1")); }
        if run.style.italic { r_pr.push_attribute(("i", "1")); }
        if run.style.underline { r_pr.push_attribute(("u", "sng")); }
        if run.style.strikethrough { r_pr.push_attribute(("strike", "sngStrike")); }
        let family = run.style.font_family.as_deref().map(str::trim).filter(|f| !f.is_empty());
        if run.style.color.is_some() || family.is_some() {
            writer.write_event(Event::Start(r_pr))?;
            // CT_TextCharacterProperties order: the fill, then a:latin.
            if let Some(color) = &run.style.color {
                writer.write_event(Event::Start(BytesStart::new("a:solidFill")))?;
                let mut clr = BytesStart::new("a:srgbClr");
                clr.push_attribute(("val", color.to_uppercase().as_str()));
                writer.write_event(Event::Empty(clr))?;
                writer.write_event(Event::End(BytesEnd::new("a:solidFill")))?;
            }
            if let Some(family) = family {
                let mut latin = BytesStart::new("a:latin");
                latin.push_attribute(("typeface", family));
                writer.write_event(Event::Empty(latin))?;
            }
            writer.write_event(Event::End(BytesEnd::new("a:rPr")))?;
        } else {
            writer.write_event(Event::Empty(r_pr))?;
        }
        writer.write_event(Event::Start(BytesStart::new("a:t")))?;
        let escaped = quick_xml::escape::escape(piece.as_str());
        writer.write_event(Event::Text(BytesText::new(&escaped)))?;
        writer.write_event(Event::End(BytesEnd::new("a:t")))?;
        writer.write_event(Event::End(BytesEnd::new("a:r")))?;
    }
    writer.write_event(Event::End(BytesEnd::new("a:p")))?;
    Ok(())
}

fn write_text_box<W: std::io::Write>(
    writer: &mut Writer<W>,
    id: usize,
    name_idx: usize,
    at: Placement,
    text: &str,
    runs: &[Run],
    body: &TextBody,
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
    write_xfrm(writer, at)?;
    
    let mut prst_geom = BytesStart::new("a:prstGeom");
    prst_geom.push_attribute(("prst", "rect"));
    writer.write_event(Event::Start(prst_geom))?;
    writer.write_event(Event::Empty(BytesStart::new("a:avLst")))?;
    writer.write_event(Event::End(BytesEnd::new("a:prstGeom")))?;
    
    writer.write_event(Event::End(BytesEnd::new("p:spPr")))?;
    
    // txBody
    writer.write_event(Event::Start(BytesStart::new("p:txBody")))?;
    write_body_pr(writer, body)?;
    writer.write_event(Event::Empty(BytesStart::new("a:lstStyle")))?;
    
    write_paragraphs(writer, text, runs, body)?;
    writer.write_event(Event::End(BytesEnd::new("p:txBody")))?;
    
    writer.write_event(Event::End(BytesEnd::new("p:sp")))?;
    Ok(())
}

fn write_rect<W: std::io::Write>(
    writer: &mut Writer<W>,
    id: usize,
    name_idx: usize,
    at: Placement,
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
    write_xfrm(writer, at)?;
    
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
    at: Placement,
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
    write_xfrm(writer, at)?;
    
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

/// A preset shape with its own fill and outline (`SlideObject::Shape`).
fn write_shape<W: std::io::Write>(
    writer: &mut Writer<W>,
    id: usize,
    name_idx: usize,
    at: Placement,
    kind: &super::shape::ShapeKind,
    style: &super::shape::ShapeStyle,
) -> Result<(), quick_xml::Error> {
    use super::shape::ShapeKind;
    writer.write_event(Event::Start(BytesStart::new("p:sp")))?;
    writer.write_event(Event::Start(BytesStart::new("p:nvSpPr")))?;
    let mut c_nv_pr = BytesStart::new("p:cNvPr");
    c_nv_pr.push_attribute(("id", id.to_string().as_str()));
    c_nv_pr.push_attribute(("name", format!("Shape {}", name_idx).as_str()));
    writer.write_event(Event::Empty(c_nv_pr))?;
    writer.write_event(Event::Empty(BytesStart::new("p:cNvSpPr")))?;
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvSpPr")))?;

    writer.write_event(Event::Start(BytesStart::new("p:spPr")))?;
    write_xfrm(writer, at)?;
    let mut prst_geom = BytesStart::new("a:prstGeom");
    prst_geom.push_attribute(("prst", kind.prst()));
    writer.write_event(Event::Start(prst_geom))?;
    writer.write_event(Event::Start(BytesStart::new("a:avLst")))?;
    if let ShapeKind::RoundRect { radius } = kind {
        let mut gd = BytesStart::new("a:gd");
        gd.push_attribute(("name", "adj"));
        gd.push_attribute(("fmla", format!("val {}", (radius * 100_000.0).round() as i64).as_str()));
        writer.write_event(Event::Empty(gd))?;
    }
    writer.write_event(Event::End(BytesEnd::new("a:avLst")))?;
    writer.write_event(Event::End(BytesEnd::new("a:prstGeom")))?;

    let solid = |writer: &mut Writer<W>, color: super::shape::Color| -> Result<(), quick_xml::Error> {
        writer.write_event(Event::Start(BytesStart::new("a:solidFill")))?;
        let mut srgb = BytesStart::new("a:srgbClr");
        srgb.push_attribute(("val", color.to_hex().as_str()));
        writer.write_event(Event::Empty(srgb))?;
        writer.write_event(Event::End(BytesEnd::new("a:solidFill")))?;
        Ok(())
    };
    match (&style.gradient, style.fill) {
        (Some(g), _) => {
            writer.write_event(Event::Start(BytesStart::new("a:gradFill")))?;
            writer.write_event(Event::Start(BytesStart::new("a:gsLst")))?;
            for stop in &g.stops {
                let mut gs = BytesStart::new("a:gs");
                gs.push_attribute(("pos", ((stop.pos * 100_000.0).round() as i64).to_string().as_str()));
                writer.write_event(Event::Start(gs))?;
                let mut srgb = BytesStart::new("a:srgbClr");
                srgb.push_attribute(("val", stop.color.to_hex().as_str()));
                writer.write_event(Event::Empty(srgb))?;
                writer.write_event(Event::End(BytesEnd::new("a:gs")))?;
            }
            writer.write_event(Event::End(BytesEnd::new("a:gsLst")))?;
            let mut lin = BytesStart::new("a:lin");
            lin.push_attribute(("ang", ((g.angle * 60_000.0).round() as i64).to_string().as_str()));
            lin.push_attribute(("scaled", "0"));
            writer.write_event(Event::Empty(lin))?;
            writer.write_event(Event::End(BytesEnd::new("a:gradFill")))?;
        }
        (None, Some(c)) => solid(writer, c)?,
        (None, None) => writer.write_event(Event::Empty(BytesStart::new("a:noFill")))?,
    }
    let mut ln = BytesStart::new("a:ln");
    match style.stroke {
        Some(stroke) => {
            // Model units, as write_xfrm writes coordinates.
            ln.push_attribute(("w", ((stroke.width * 9525.0).round() as i64).to_string().as_str()));
            writer.write_event(Event::Start(ln))?;
            solid(writer, stroke.color)?;
            writer.write_event(Event::End(BytesEnd::new("a:ln")))?;
        }
        None => {
            writer.write_event(Event::Start(ln))?;
            writer.write_event(Event::Empty(BytesStart::new("a:noFill")))?;
            writer.write_event(Event::End(BytesEnd::new("a:ln")))?;
        }
    }
    writer.write_event(Event::End(BytesEnd::new("p:spPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:sp")))?;
    Ok(())
}

/// A table as a `p:graphicFrame` holding an `a:tbl`, in PowerPoint's
/// default table style ("Medium Style 2 - Accent 1"), which is the style
/// the model's `cell_paint` draws. Column widths and row heights are the
/// fitted ones, so the grid fills the frame exactly.
fn write_table<W: std::io::Write>(
    writer: &mut Writer<W>,
    id: usize,
    name_idx: usize,
    at: Placement,
    table: &super::table::TableData,
) -> Result<(), quick_xml::Error> {
    let emu = |v: f64| ((v * 9525.0).round() as i64).to_string();
    writer.write_event(Event::Start(BytesStart::new("p:graphicFrame")))?;
    writer.write_event(Event::Start(BytesStart::new("p:nvGraphicFramePr")))?;
    let mut c_nv_pr = BytesStart::new("p:cNvPr");
    c_nv_pr.push_attribute(("id", id.to_string().as_str()));
    c_nv_pr.push_attribute(("name", format!("Table {}", name_idx).as_str()));
    writer.write_event(Event::Empty(c_nv_pr))?;
    writer.write_event(Event::Start(BytesStart::new("p:cNvGraphicFramePr")))?;
    let mut locks = BytesStart::new("a:graphicFrameLocks");
    locks.push_attribute(("noGrp", "1"));
    writer.write_event(Event::Empty(locks))?;
    writer.write_event(Event::End(BytesEnd::new("p:cNvGraphicFramePr")))?;
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvGraphicFramePr")))?;

    writer.write_event(Event::Start(BytesStart::new("p:xfrm")))?;
    let mut off = BytesStart::new("a:off");
    off.push_attribute(("x", emu(at.x).as_str()));
    off.push_attribute(("y", emu(at.y).as_str()));
    writer.write_event(Event::Empty(off))?;
    let mut ext = BytesStart::new("a:ext");
    ext.push_attribute(("cx", emu(at.w).as_str()));
    ext.push_attribute(("cy", emu(at.h).as_str()));
    writer.write_event(Event::Empty(ext))?;
    writer.write_event(Event::End(BytesEnd::new("p:xfrm")))?;

    writer.write_event(Event::Start(BytesStart::new("a:graphic")))?;
    let mut data = BytesStart::new("a:graphicData");
    data.push_attribute(("uri", "http://schemas.openxmlformats.org/drawingml/2006/table"));
    writer.write_event(Event::Start(data))?;
    writer.write_event(Event::Start(BytesStart::new("a:tbl")))?;
    let mut tbl_pr = BytesStart::new("a:tblPr");
    if table.first_row {
        tbl_pr.push_attribute(("firstRow", "1"));
    }
    if table.band_row {
        tbl_pr.push_attribute(("bandRow", "1"));
    }
    writer.write_event(Event::Start(tbl_pr))?;
    writer.write_event(Event::Start(BytesStart::new("a:tableStyleId")))?;
    writer.write_event(Event::Text(BytesText::new("{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}")))?;
    writer.write_event(Event::End(BytesEnd::new("a:tableStyleId")))?;
    writer.write_event(Event::End(BytesEnd::new("a:tblPr")))?;

    let (cols, rows) = table.fitted(at.w, at.h);
    writer.write_event(Event::Start(BytesStart::new("a:tblGrid")))?;
    for w in &cols {
        let mut col = BytesStart::new("a:gridCol");
        col.push_attribute(("w", emu(*w).as_str()));
        writer.write_event(Event::Empty(col))?;
    }
    writer.write_event(Event::End(BytesEnd::new("a:tblGrid")))?;
    for (r, h) in rows.iter().enumerate() {
        let mut tr = BytesStart::new("a:tr");
        tr.push_attribute(("h", emu(*h).as_str()));
        writer.write_event(Event::Start(tr))?;
        for c in 0..cols.len() {
            let cell = table.rows.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default();
            writer.write_event(Event::Start(BytesStart::new("a:tc")))?;
            writer.write_event(Event::Start(BytesStart::new("a:txBody")))?;
            writer.write_event(Event::Empty(BytesStart::new("a:bodyPr")))?;
            writer.write_event(Event::Empty(BytesStart::new("a:lstStyle")))?;
            write_paragraphs(writer, &cell.text(), &cell.runs, &TextBody::default())?;
            writer.write_event(Event::End(BytesEnd::new("a:txBody")))?;
            match cell.fill {
                Some(fill) => {
                    writer.write_event(Event::Start(BytesStart::new("a:tcPr")))?;
                    writer.write_event(Event::Start(BytesStart::new("a:solidFill")))?;
                    let mut srgb = BytesStart::new("a:srgbClr");
                    srgb.push_attribute(("val", fill.to_hex().as_str()));
                    writer.write_event(Event::Empty(srgb))?;
                    writer.write_event(Event::End(BytesEnd::new("a:solidFill")))?;
                    writer.write_event(Event::End(BytesEnd::new("a:tcPr")))?;
                }
                None => writer.write_event(Event::Empty(BytesStart::new("a:tcPr")))?,
            }
            writer.write_event(Event::End(BytesEnd::new("a:tc")))?;
        }
        writer.write_event(Event::End(BytesEnd::new("a:tr")))?;
    }
    writer.write_event(Event::End(BytesEnd::new("a:tbl")))?;
    writer.write_event(Event::End(BytesEnd::new("a:graphicData")))?;
    writer.write_event(Event::End(BytesEnd::new("a:graphic")))?;
    writer.write_event(Event::End(BytesEnd::new("p:graphicFrame")))?;
    Ok(())
}

fn write_image<W: std::io::Write>(
    writer: &mut Writer<W>,
    id: usize,
    name_idx: usize,
    rel_id: &str,
    at: Placement,
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
    write_xfrm(writer, at)?;
    
    let mut prst_geom = BytesStart::new("a:prstGeom");
    prst_geom.push_attribute(("prst", "rect"));
    writer.write_event(Event::Start(prst_geom))?;
    writer.write_event(Event::Empty(BytesStart::new("a:avLst")))?;
    writer.write_event(Event::End(BytesEnd::new("a:prstGeom")))?;
    
    writer.write_event(Event::End(BytesEnd::new("p:spPr")))?;
    
    writer.write_event(Event::End(BytesEnd::new("p:pic")))?;
    Ok(())
}

// ── Slide masters ────────────────────────────────────────────────────
//
// Decks *reads* a master from an imported deck and renders it — the canvas
// and the sidebar thumbnails both consult it — but neither writer emitted
// one, so every save handed the reader a package with no master and the
// reader synthesised a white default in its place. An imported deck lost
// its design on the first save, not only on recovery.
//
// A pptx master is three parts, not one. `p:sldMaster` carries the
// decorations and the colour map; a `p:sldLayout` sits between it and the
// slides, and it is the layout a slide relates to. The reader walks exactly
// that chain (slide rels -> slideLayout -> its rels -> slideMaster), so all
// three have to be present and related for a master to come back.
//
// Decorations go on the master and the layout's shape tree is left empty:
// the reader concatenates both, so writing the shapes twice would double
// them on every save.

/// The empty group-shape prelude every `p:spTree` opens with.
fn write_group_prelude<W: std::io::Write>(
    writer: &mut Writer<W>,
) -> Result<(), quick_xml::Error> {
    writer.write_event(Event::Start(BytesStart::new("p:nvGrpSpPr")))?;
    let mut c_nv_pr = BytesStart::new("p:cNvPr");
    c_nv_pr.push_attribute(("id", "1"));
    c_nv_pr.push_attribute(("name", ""));
    writer.write_event(Event::Empty(c_nv_pr))?;
    writer.write_event(Event::Empty(BytesStart::new("p:cNvGrpSpPr")))?;
    writer.write_event(Event::Empty(BytesStart::new("p:nvPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:nvGrpSpPr")))?;

    writer.write_event(Event::Start(BytesStart::new("p:grpSpPr")))?;
    writer.write_event(Event::Start(BytesStart::new("a:xfrm")))?;
    for (tag, a, b) in [
        ("a:off", "x", "y"),
        ("a:ext", "cx", "cy"),
        ("a:chOff", "x", "y"),
        ("a:chExt", "cx", "cy"),
    ] {
        let mut e = BytesStart::new(tag);
        e.push_attribute((a, "0"));
        e.push_attribute((b, "0"));
        writer.write_event(Event::Empty(e))?;
    }
    writer.write_event(Event::End(BytesEnd::new("a:xfrm")))?;
    writer.write_event(Event::End(BytesEnd::new("p:grpSpPr")))?;
    Ok(())
}

/// Emit `p:bg` for an explicit colour. White and anything unparseable is
/// left out, so a deck that never set a background is not rewritten with
/// one.
fn write_background<W: std::io::Write>(
    writer: &mut Writer<W>,
    background: &str,
) -> Result<(), quick_xml::Error> {
    let bg = background.trim_start_matches('#');
    if bg.len() != 6 || bg.eq_ignore_ascii_case("ffffff") {
        return Ok(());
    }
    writer.write_event(Event::Start(BytesStart::new("p:bg")))?;
    writer.write_event(Event::Start(BytesStart::new("p:bgPr")))?;
    writer.write_event(Event::Start(BytesStart::new("a:solidFill")))?;
    let mut clr = BytesStart::new("a:srgbClr");
    clr.push_attribute(("val", bg.to_uppercase().as_str()));
    writer.write_event(Event::Empty(clr))?;
    writer.write_event(Event::End(BytesEnd::new("a:solidFill")))?;
    writer.write_event(Event::Empty(BytesStart::new("a:effectLst")))?;
    writer.write_event(Event::End(BytesEnd::new("p:bgPr")))?;
    writer.write_event(Event::End(BytesEnd::new("p:bg")))?;
    Ok(())
}

/// Write a master's decoration shapes.
///
/// Images are skipped: they would need a media part and a relationship per
/// master, and the master reader parses `p:sp` only, so writing one would
/// be a part nothing reads. Text, rects and circles are what it reads, and
/// they are what this writes.
fn write_master_shapes<W: std::io::Write>(
    writer: &mut Writer<W>,
    shapes: &[SlideObject],
) -> Result<(), quick_xml::Error> {
    for (j, obj) in shapes.iter().enumerate() {
        let id = 2 + j;
        match obj {
            SlideObject::TextBox { text, x, y, w, h, runs, rotation, body } => write_text_box(
                writer,
                id,
                j + 1,
                Placement { x: *x, y: *y, w: *w, h: *h, rotation: *rotation },
                text,
                runs,
                body,
            )?,
            SlideObject::Rect { x, y, w, h, rotation } => write_rect(
                writer,
                id,
                j + 1,
                Placement { x: *x, y: *y, w: *w, h: *h, rotation: *rotation },
            )?,
            SlideObject::Circle { x, y, r, rotation } => {
                write_circle(writer, id, j + 1, Placement::of_circle(*x, *y, *r, *rotation))?
            }
            SlideObject::Shape { kind, x, y, w, h, rotation, style } => write_shape(
                writer,
                id,
                j + 1,
                Placement { x: *x, y: *y, w: *w, h: *h, rotation: *rotation },
                kind,
                style,
            )?,
            SlideObject::Table { x, y, w, h, rotation, table } => write_table(
                writer,
                id,
                j + 1,
                Placement { x: *x, y: *y, w: *w, h: *h, rotation: *rotation },
                table,
            )?,
            SlideObject::Image { .. } => {}
        }
    }
    Ok(())
}

/// The `p:clrMap` a `p:sldMaster` is required to carry.
///
/// Identity mapping: it names the theme slots the master's colours come
/// from, and `theme_part_xml` writes those slots with the conventional
/// Office values, so mapping each to itself is what the two agree on.
/// (This said "the suite has no theme, so there is nothing to remap" until
/// the theme part existed.)
const CLR_MAP: &str = "<p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" \
     accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" \
     accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>";

const PART_NS: [(&str, &str); 3] = [
    ("xmlns:a", "http://schemas.openxmlformats.org/drawingml/2006/main"),
    ("xmlns:r", "http://schemas.openxmlformats.org/officeDocument/2006/relationships"),
    ("xmlns:p", "http://schemas.openxmlformats.org/presentationml/2006/main"),
];

/// `ppt/theme/themeN.xml` — where a pptx keeps a master's default font.
///
/// OOXML has no "default font" attribute on a master: the body font lives
/// in the theme's `a:fontScheme/a:minorFont`, and the heading font in
/// `a:majorFont`. Decks models one font per master, so both are written
/// from it — writing only `minorFont` would leave a reader that consults
/// the major font (headings) falling back to something else, which is a
/// silent half-carry of the kind this row keeps collecting.
///
/// A theme is also **required** rather than optional: ECMA-376 gives every
/// `p:sldMaster` exactly one theme relationship, and until now the writer
/// emitted masters with none. Impress opened those packages anyway, which
/// is why nothing caught it.
///
/// `a:clrScheme` and `a:fmtScheme` are present because a `a:themeElements`
/// missing either is rejected outright by stricter readers; their values
/// are the identity-ish defaults matching `CLR_MAP`.
fn theme_part_xml(master: &MasterSlide) -> String {
    let font = quick_xml::escape::escape(master.font_family());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
         name=\"{name}\"><a:themeElements>\
         <a:clrScheme name=\"Office\">\
         <a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1>\
         <a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1>\
         <a:dk2><a:srgbClr val=\"44546A\"/></a:dk2>\
         <a:lt2><a:srgbClr val=\"E7E6E6\"/></a:lt2>\
         <a:accent1><a:srgbClr val=\"4472C4\"/></a:accent1>\
         <a:accent2><a:srgbClr val=\"ED7D31\"/></a:accent2>\
         <a:accent3><a:srgbClr val=\"A5A5A5\"/></a:accent3>\
         <a:accent4><a:srgbClr val=\"FFC000\"/></a:accent4>\
         <a:accent5><a:srgbClr val=\"5B9BD5\"/></a:accent5>\
         <a:accent6><a:srgbClr val=\"70AD47\"/></a:accent6>\
         <a:hlink><a:srgbClr val=\"0563C1\"/></a:hlink>\
         <a:folHlink><a:srgbClr val=\"954F72\"/></a:folHlink>\
         </a:clrScheme>\
         <a:fontScheme name=\"Office\">\
         <a:majorFont><a:latin typeface=\"{font}\"/><a:ea typeface=\"\"/>\
         <a:cs typeface=\"\"/></a:majorFont>\
         <a:minorFont><a:latin typeface=\"{font}\"/><a:ea typeface=\"\"/>\
         <a:cs typeface=\"\"/></a:minorFont>\
         </a:fontScheme>\
         <a:fmtScheme name=\"Office\">\
         <a:fillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
         <a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
         <a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:fillStyleLst>\
         <a:lnStyleLst><a:ln><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln>\
         <a:ln><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln>\
         <a:ln><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln></a:lnStyleLst>\
         <a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle>\
         <a:effectStyle><a:effectLst/></a:effectStyle>\
         <a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>\
         <a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
         <a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
         <a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:bgFillStyleLst>\
         </a:fmtScheme></a:themeElements></a:theme>",
        name = quick_xml::escape::escape(master.name.as_str()),
        font = font,
    )
}

/// `ppt/slideMasters/slideMasterN.xml` — the decorations and the colour map.
fn master_part_xml(master: &MasterSlide) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut writer = Writer::new(std::io::Cursor::new(&mut out));
        writer
            .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), Some("yes"))))
            .map_err(|e| e.to_string())?;
        let mut root = BytesStart::new("p:sldMaster");
        for (k, v) in PART_NS {
            root.push_attribute((k, v));
        }
        writer.write_event(Event::Start(root)).map_err(|e| e.to_string())?;
        let mut c_sld = BytesStart::new("p:cSld");
        c_sld.push_attribute(("name", master.name.as_str()));
        writer.write_event(Event::Start(c_sld)).map_err(|e| e.to_string())?;
        write_background(&mut writer, &master.background).map_err(|e| e.to_string())?;
        writer.write_event(Event::Start(BytesStart::new("p:spTree"))).map_err(|e| e.to_string())?;
        write_group_prelude(&mut writer).map_err(|e| e.to_string())?;
        write_master_shapes(&mut writer, &master.shapes).map_err(|e| e.to_string())?;
        writer.write_event(Event::End(BytesEnd::new("p:spTree"))).map_err(|e| e.to_string())?;
        writer.write_event(Event::End(BytesEnd::new("p:cSld"))).map_err(|e| e.to_string())?;
        writer.write_event(Event::End(BytesEnd::new("p:sldMaster"))).map_err(|e| e.to_string())?;
    }
    let mut xml = out;
    // The colour map and the layout list are fixed text; splicing them in
    // after the tree keeps the event writer to the parts that vary.
    let close = b"</p:sldMaster>";
    let at = xml.len() - close.len();
    let tail = format!(
        "{CLR_MAP}<p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/>\
         </p:sldLayoutIdLst>"
    );
    xml.splice(at..at, tail.into_bytes());
    Ok(xml)
}

/// `ppt/slideLayouts/slideLayoutN.xml` — the part a slide actually relates
/// to. Its shape tree is empty on purpose (see the module note above); it
/// carries the name and the background so a reader that consults the layout
/// first still sees them.
fn layout_part_xml(master: &MasterSlide) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut writer = Writer::new(std::io::Cursor::new(&mut out));
        writer
            .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), Some("yes"))))
            .map_err(|e| e.to_string())?;
        let mut root = BytesStart::new("p:sldLayout");
        for (k, v) in PART_NS {
            root.push_attribute((k, v));
        }
        root.push_attribute(("type", "blank"));
        root.push_attribute(("preserve", "1"));
        writer.write_event(Event::Start(root)).map_err(|e| e.to_string())?;
        let mut c_sld = BytesStart::new("p:cSld");
        c_sld.push_attribute(("name", master.name.as_str()));
        writer.write_event(Event::Start(c_sld)).map_err(|e| e.to_string())?;
        write_background(&mut writer, &master.background).map_err(|e| e.to_string())?;
        writer.write_event(Event::Start(BytesStart::new("p:spTree"))).map_err(|e| e.to_string())?;
        write_group_prelude(&mut writer).map_err(|e| e.to_string())?;
        writer.write_event(Event::End(BytesEnd::new("p:spTree"))).map_err(|e| e.to_string())?;
        writer.write_event(Event::End(BytesEnd::new("p:cSld"))).map_err(|e| e.to_string())?;
        writer.write_event(Event::End(BytesEnd::new("p:sldLayout"))).map_err(|e| e.to_string())?;
    }
    let mut xml = out;
    let close = b"</p:sldLayout>";
    let at = xml.len() - close.len();
    xml.splice(at..at, b"<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>".to_vec());
    Ok(xml)
}

/// A relationships part carrying `(kind, target)` in order, numbered from
/// `rId1`.
///
/// Order is load-bearing for a master: `master_part_xml` names its layout
/// as `rId1` in fixed text, so the layout has to come first and the theme
/// after it.
fn rels_part(rels: &[(&str, &str)]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n",
    );
    for (i, (kind, target)) in rels.iter().enumerate() {
        out.push_str(&format!(
            "  <Relationship Id=\"rId{}\" \
             Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}\" \
             Target=\"{target}\"/>\n",
            i + 1
        ));
    }
    out.push_str("</Relationships>");
    out
}

/// A relationships part with one relationship, which is all a layout needs
/// to point back at its master.
fn one_rel(kind: &str, target: &str) -> String {
    rels_part(&[(kind, target)])
}

pub fn write_pptx(path: &str, deck: &Deck) -> Result<(), String> {
    let bytes = write_pptx_bytes(deck)?;
    suite_common_core::atomic_save::atomic_write_bytes(std::path::Path::new(path), &bytes)
}

/// Render the deck to an in-memory .pptx buffer without touching disk —
/// shared by the real save path (above) and autosave snapshots.
pub fn write_pptx_bytes(deck: &Deck) -> Result<Vec<u8>, String> {
    // Built fully in memory, then placed atomically — see
    // suite_common_core::atomic_save and odp::write for why.
    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);

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
    for k in 0..deck.masters.len() {
        content_types.push_str(&format!(
            "  <Override PartName=\"/ppt/slideMasters/slideMaster{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>\n\
             \x20 <Override PartName=\"/ppt/slideLayouts/slideLayout{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>\n\
             \x20 <Override PartName=\"/ppt/theme/theme{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>\n",
            n = k + 1
        ));
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
           <p:sldMasterIdLst>\n"
    );
    // The schema orders p:sldMasterIdLst before p:sldIdLst, and the master
    // relationship ids follow the slides' so the slide loop above keeps
    // rId1..rIdN.
    for k in 0..deck.masters.len() {
        presentation.push_str(&format!(
            "    <p:sldMasterId id=\"{}\" r:id=\"rId{}\"/>\n",
            2_147_483_648u64 + k as u64,
            deck.slides.len() + 1 + k,
        ));
    }
    presentation.push_str("  </p:sldMasterIdLst>\n  <p:sldIdLst>\n");
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
    for k in 0..deck.masters.len() {
        pres_rels.push_str(&format!(
            "  <Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"slideMasters/slideMaster{}.xml\"/>\n",
            deck.slides.len() + 1 + k,
            k + 1,
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

            // The slide's name, which is what `Slide::title` is. The master
            // and layout parts above have always written theirs here; the
            // slide's was left off, so a deck's slide names survived a .odp
            // save (`draw:page/@draw:name`) and were destroyed by a .pptx
            // one — and pptx is the format an unsaved deck is snapshotted
            // in, so crash recovery lost every one of them.
            //
            // Omitted rather than written empty when there is no name: the
            // attribute is optional, and `name=""` would be a claim that
            // the slide is called nothing rather than that it is unnamed.
            let mut c_sld = BytesStart::new("p:cSld");
            if !slide.title.trim().is_empty() {
                c_sld.push_attribute(("name", slide.title.as_str()));
            }
            writer.write_event(Event::Start(c_sld)).map_err(|e| e.to_string())?;

            // Slide background (only when it differs from the default
            // white — Impress preserves an explicit p:bg).
            let bg = slide.background.trim_start_matches('#');
            if !bg.eq_ignore_ascii_case("ffffff") && bg.len() == 6 {
                writer.write_event(Event::Start(BytesStart::new("p:bg"))).map_err(|e| e.to_string())?;
                writer.write_event(Event::Start(BytesStart::new("p:bgPr"))).map_err(|e| e.to_string())?;
                writer.write_event(Event::Start(BytesStart::new("a:solidFill"))).map_err(|e| e.to_string())?;
                let mut clr = BytesStart::new("a:srgbClr");
                clr.push_attribute(("val", bg.to_uppercase().as_str()));
                writer.write_event(Event::Empty(clr)).map_err(|e| e.to_string())?;
                writer.write_event(Event::End(BytesEnd::new("a:solidFill"))).map_err(|e| e.to_string())?;
                writer.write_event(Event::Empty(BytesStart::new("a:effectLst"))).map_err(|e| e.to_string())?;
                writer.write_event(Event::End(BytesEnd::new("p:bgPr"))).map_err(|e| e.to_string())?;
                writer.write_event(Event::End(BytesEnd::new("p:bg"))).map_err(|e| e.to_string())?;
            }

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
                    SlideObject::TextBox { text, x, y, w, h, runs, rotation, body } => {
                        write_text_box(&mut writer, id, j + 1, Placement { x: *x, y: *y, w: *w, h: *h, rotation: *rotation }, text, runs, body).map_err(|e| e.to_string())?;
                    }
                    SlideObject::Rect { x, y, w, h, rotation } => {
                        write_rect(&mut writer, id, j + 1, Placement { x: *x, y: *y, w: *w, h: *h, rotation: *rotation }).map_err(|e| e.to_string())?;
                    }
                    SlideObject::Circle { x, y, r, rotation } => {
                        write_circle(&mut writer, id, j + 1, Placement::of_circle(*x, *y, *r, *rotation)).map_err(|e| e.to_string())?;
                    }
                    SlideObject::Shape { kind, x, y, w, h, rotation, style } => {
                        write_shape(&mut writer, id, j + 1, Placement { x: *x, y: *y, w: *w, h: *h, rotation: *rotation }, kind, style).map_err(|e| e.to_string())?;
                    }
                    SlideObject::Table { x, y, w, h, rotation, table } => {
                        write_table(&mut writer, id, j + 1, Placement { x: *x, y: *y, w: *w, h: *h, rotation: *rotation }, table).map_err(|e| e.to_string())?;
                    }
                    SlideObject::Image { path, x, y, w, h, rotation } => {
                        let img_idx = images_to_add.len() + 1;
                        images_to_add.push(path.clone());

                        let rel_id = format!("rId{}", slide_rels.len() + 1);
                        slide_rels.push((rel_id.clone(), format!("../media/image{}.png", img_idx)));

                        write_image(&mut writer, id, j + 1, &rel_id, Placement { x: *x, y: *y, w: *w, h: *h, rotation: *rotation }).map_err(|e| e.to_string())?;
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

        // Write slide relationships (images, speaker notes, and the
        // slideLayout that leads to this slide's master). The layout
        // relationship is what makes the master reachable at all, so this
        // part is now written for every slide rather than only for slides
        // that carry an image or notes.
        let has_notes = !slide.notes.is_empty();
        let layout = slide
            .master_idx
            .filter(|k| *k < deck.masters.len())
            .or(if deck.masters.is_empty() { None } else { Some(0) });
        if !slide_rels.is_empty() || has_notes || layout.is_some() {
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
                max_rel += 1;
                rels_str.push_str(&format!(
                    "  <Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide\" Target=\"../notesSlides/notesSlide{}.xml\"/>\n",
                    max_rel, i + 1
                ));
            }
            if let Some(k) = layout {
                max_rel += 1;
                rels_str.push_str(&format!(
                    "  <Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout{}.xml\"/>\n",
                    max_rel, k + 1
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

    // 6. Write the masters and their layouts
    for (k, master) in deck.masters.iter().enumerate() {
        let n = k + 1;
        zip.start_file(format!("ppt/slideMasters/slideMaster{n}.xml"), options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&master_part_xml(master)?).map_err(|e| e.to_string())?;
        zip.start_file(format!("ppt/slideMasters/_rels/slideMaster{n}.xml.rels"), options)
            .map_err(|e| e.to_string())?;
        // Layout first: master_part_xml names it as rId1 in fixed text.
        zip.write_all(
            rels_part(&[
                ("slideLayout", &format!("../slideLayouts/slideLayout{n}.xml")),
                ("theme", &format!("../theme/theme{n}.xml")),
            ])
            .as_bytes(),
        )
        .map_err(|e| e.to_string())?;

        zip.start_file(format!("ppt/theme/theme{n}.xml"), options).map_err(|e| e.to_string())?;
        zip.write_all(theme_part_xml(master).as_bytes()).map_err(|e| e.to_string())?;

        zip.start_file(format!("ppt/slideLayouts/slideLayout{n}.xml"), options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&layout_part_xml(master)?).map_err(|e| e.to_string())?;
        zip.start_file(format!("ppt/slideLayouts/_rels/slideLayout{n}.xml.rels"), options)
            .map_err(|e| e.to_string())?;
        zip.write_all(
            one_rel("slideMaster", &format!("../slideMasters/slideMaster{n}.xml")).as_bytes(),
        )
        .map_err(|e| e.to_string())?;
    }

    // 7. Write image media files in ppt/media/
    for (idx, img_path) in images_to_add.iter().enumerate() {
        let zip_img_path = format!("ppt/media/image{}.png", idx + 1);
        let mut img_file = File::open(img_path)
            .map_err(|e| format!("Cannot open image {}: {}", img_path, e))?;
        let mut buffer = Vec::new();
        img_file.read_to_end(&mut buffer).map_err(|e| e.to_string())?;

        zip.start_file(&zip_img_path, options).map_err(|e| e.to_string())?;
        zip.write_all(&buffer).map_err(|e| e.to_string())?;
    }

    zip.finish().map_err(|e| e.to_string()).map(|c| c.into_inner())
}

#[cfg(test)]
mod emu_rounding_tests {
    use super::*;
    use crate::engine::parse::read_pptx;

    /// One slide holding a single text box at (`at`, `at`).
    fn deck_with_box(text: &str, at: f64, w: f64, h: f64) -> Deck {
        Deck {
            slides: vec![Slide {
                title: String::new(),
                background: String::new(),
                notes: String::new(),
                master_idx: None,
                objects: vec![SlideObject::TextBox {
                    text: text.into(),
                    x: at, y: at, w, h,
                    rotation: 0.0,
                    runs: vec![],
                    body: Default::default(),
                }],
            }],
            ..Default::default()
        }
    }

    /// PPTX stores geometry in EMU, which are integral, so a conversion from
    /// our floating-point points is inherently lossy. It must at least be
    /// *centred*: `as i64` truncates toward zero, which loses up to a whole
    /// EMU and always in the same direction, so a deck drifts a little further
    /// up and left every time it is saved.
    ///
    /// 2 cm is 75.5905511811 pt, which is 719999.9999… EMU. Truncated that is
    /// 719999 and reads back as 75.59044…; rounded it is 720000 and reads back
    /// exactly. The file-corpus journeys compare positions for equality after
    /// a save-and-reopen, so this showed up there as every Decks fixture
    /// failing at the reopen step (#447).
    #[test]
    fn a_two_centimetre_offset_survives_a_pptx_round_trip_exactly() {
        let two_cm = 75.590_551_181_1_f64;
        let deck = deck_with_box("geometry", two_cm, 200.0, 50.0);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("emu.pptx");
        write_pptx(path.to_str().unwrap(), &deck).expect("write");
        let read_back = read_pptx(path.to_str().unwrap()).expect("read");

        let (x, y) = crate::undo::obj_position(&read_back.slides[0].objects[0]);
        assert!(
            (x - two_cm).abs() < 1e-6 && (y - two_cm).abs() < 1e-6,
            "position drifted: wrote ({two_cm}, {two_cm}), read ({x}, {y})"
        );
    }

    /// Saving repeatedly must not walk the geometry anywhere. With truncation
    /// the error is one-directional, so it accumulates; with rounding it does
    /// not.
    #[test]
    fn geometry_does_not_drift_across_repeated_saves() {
        let start = 75.590_551_181_1_f64;
        let mut deck = deck_with_box("drift", start, 100.0, 40.0);

        let dir = tempfile::tempdir().unwrap();
        for generation in 0..8 {
            let path = dir.path().join(format!("gen{generation}.pptx"));
            write_pptx(path.to_str().unwrap(), &deck).expect("write");
            deck = read_pptx(path.to_str().unwrap()).expect("read");
        }
        let (x, y) = crate::undo::obj_position(&deck.slides[0].objects[0]);
        assert!(
            (x - start).abs() < 1e-6 && (y - start).abs() < 1e-6,
            "drifted over eight saves: {start} -> ({x}, {y})"
        );
    }
}
