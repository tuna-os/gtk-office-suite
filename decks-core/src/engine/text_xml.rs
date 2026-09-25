// text_xml.rs — what a pptx text body's paragraphs and runs look like,
// after everything they inherit.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// DrawingML resolves a paragraph's properties through a chain, most
// specific last (ECMA-376 Part 1, 19.3.1.36 and 21.1.2.4):
//
// 1. the master's `p:txStyles`: `titleStyle` for a title, `bodyStyle` for a
//    body, subtitle or content placeholder, `otherStyle` for anything else;
// 2. the master placeholder's `a:lstStyle`;
// 3. the layout placeholder's `a:lstStyle` (matched by idx, then type, as
//    placeholders.rs does for geometry);
// 4. the shape's own `a:lstStyle`;
// 5. the paragraph's `a:pPr`, and for a run its `a:rPr`.
//
// Each list-style level (`a:lvlNpPr`, picked by `a:pPr lvl`) carries
// alignment, margins, bullet and spacing, and `a:defRPr` for the runs.
// The python-pptx default template is the worked example: its title is
// 44 pt centred, its subtitle centred in tx1 at 75% tint, and its body
// levels are 32/28/24 pt with bullets •, – and • on hanging indents. None
// of that is written on the slide; all of it came out as 18 pt left-aligned
// plain text (render lab decks/title-layout, decks/bullets).
//
// The text box's `a:bodyPr` (vertical anchor, insets) inherits the same
// way through the placeholders.

use super::placeholders::{inherited_indices, PhKey};
use super::shape_xml::{first_color, parse_tree, Node, Theme};
use super::text_body::{Anchor, Autofit, Bullet, Insets, ParaAlign, ParaStyle, Spacing, TextBody};
use super::SlideScale;
use letters_core::model::RunStyle;

/// Spacing as the file states it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RawSpacing {
    /// Hundredths of a point (`a:spcPts`).
    Points(f64),
    /// Thousandths of a percent of a line (`a:spcPct`).
    Percent(f64),
}

/// Run properties, each absent unless some level states it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RunProps {
    /// Hundredths of a point.
    sz: Option<u32>,
    b: Option<bool>,
    i: Option<bool>,
    u: Option<bool>,
    strike: Option<bool>,
    /// `RRGGBB`, lower case.
    color: Option<String>,
    /// The Latin typeface, theme references resolved.
    latin: Option<String>,
}

impl RunProps {
    fn over(&mut self, top: &RunProps) {
        macro_rules! take { ($($f:ident),*) => { $( if top.$f.is_some() { self.$f = top.$f.clone(); } )* } }
        take!(sz, b, i, u, strike, color, latin);
    }

    fn of(rpr: &Node, theme: &Theme) -> RunProps {
        let flag = |k: &str| rpr.attr(k).map(|v| v == "1" || v == "true");
        RunProps {
            sz: rpr.attr("sz").and_then(|v| v.parse().ok()),
            b: flag("b"),
            i: flag("i"),
            u: rpr.attr("u").map(|v| v != "none" && !v.is_empty()),
            strike: rpr.attr("strike").map(|v| v != "noStrike" && !v.is_empty()),
            color: rpr
                .child("a:solidFill")
                .and_then(|f| first_color(f, theme, None))
                .map(|c| c.to_hex().to_lowercase()),
            latin: rpr.child("a:latin").and_then(|l| l.attr("typeface")).and_then(|t| theme.typeface(t)),
        }
    }

    /// The model's run style.
    pub(crate) fn style(&self) -> RunStyle {
        RunStyle {
            bold: self.b.unwrap_or(false),
            italic: self.i.unwrap_or(false),
            underline: self.u.unwrap_or(false),
            strikethrough: self.strike.unwrap_or(false),
            font_size_hp: self.sz.map(|s| (s / 50) as u16),
            color: self.color.clone(),
            font_family: self.latin.clone(),
            ..RunStyle::default()
        }
    }
}

/// One list-style level, or a paragraph's own `a:pPr`.
#[derive(Clone, Debug, Default, PartialEq)]
struct LevelProps {
    algn: Option<ParaAlign>,
    /// EMU.
    mar_l: Option<f64>,
    /// EMU.
    indent: Option<f64>,
    bullet: Option<Bullet>,
    spc_bef: Option<RawSpacing>,
    spc_aft: Option<RawSpacing>,
    run: RunProps,
}

impl LevelProps {
    fn over(&mut self, top: &LevelProps) {
        macro_rules! take { ($($f:ident),*) => { $( if top.$f.is_some() { self.$f = top.$f.clone(); } )* } }
        take!(algn, mar_l, indent, bullet, spc_bef, spc_aft);
        self.run.over(&top.run);
    }

    fn of(ppr: &Node, theme: &Theme) -> LevelProps {
        let num = |k: &str| ppr.attr(k).and_then(|v| v.parse::<f64>().ok());
        let spacing = |name: &str| {
            let s = ppr.child(name)?;
            if let Some(p) = s.child("a:spcPts") {
                return p.attr("val")?.parse().ok().map(RawSpacing::Points);
            }
            s.child("a:spcPct")?.attr("val")?.parse().ok().map(RawSpacing::Percent)
        };
        let mut bullet = None;
        for c in &ppr.children {
            match c.name.as_str() {
                "a:buNone" => bullet = Some(Bullet::None),
                "a:buChar" => bullet = c.attr("char").map(|ch| Bullet::Char(ch.to_string())),
                "a:buAutoNum" => {
                    bullet = Some(Bullet::AutoNum {
                        scheme: c.attr("type").unwrap_or("arabicPeriod").to_string(),
                        start: c.attr("startAt").and_then(|v| v.parse().ok()).unwrap_or(1),
                    })
                }
                _ => {}
            }
        }
        LevelProps {
            algn: ppr.attr("algn").and_then(ParaAlign::from_drawingml),
            mar_l: num("marL"),
            indent: num("indent"),
            bullet,
            spc_bef: spacing("a:spcBef"),
            spc_aft: spacing("a:spcAft"),
            run: ppr.child("a:defRPr").map(|r| RunProps::of(r, theme)).unwrap_or_default(),
        }
    }
}

/// Nine levels of a list style (`a:lstStyle`, `p:titleStyle`, …).
#[derive(Clone, Debug, Default, PartialEq)]
struct ListStyle([LevelProps; 9]);

impl ListStyle {
    fn of(node: Option<&Node>, theme: &Theme) -> ListStyle {
        let mut out = ListStyle::default();
        let Some(node) = node else { return out };
        for (n, level) in out.0.iter_mut().enumerate() {
            if let Some(p) = node.child(&format!("a:lvl{}pPr", n + 1)) {
                *level = LevelProps::of(p, theme);
            }
        }
        out
    }
    fn level(&self, lvl: usize) -> &LevelProps {
        &self.0[lvl.min(8)]
    }
}

/// `a:bodyPr` as far as the canvas draws it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct BodyProps {
    anchor: Option<Anchor>,
    /// lIns, tIns, rIns, bIns in EMU.
    ins: [Option<f64>; 4],
    /// `a:normAutofit` (Some) or `a:noAutofit`/`a:spAutoFit` (None), when
    /// stated.
    autofit: Option<Option<Autofit>>,
}

impl BodyProps {
    fn of(body_pr: Option<&Node>) -> BodyProps {
        let Some(b) = body_pr else { return BodyProps::default() };
        let num = |k: &str| b.attr(k).and_then(|v| v.parse::<f64>().ok());
        BodyProps {
            anchor: b.attr("anchor").and_then(Anchor::from_drawingml),
            ins: [num("lIns"), num("tIns"), num("rIns"), num("bIns")],
            autofit: if let Some(n) = b.child("a:normAutofit") {
                let frac = |k: &str| n.attr(k).and_then(|v| v.parse::<f64>().ok()).map(|v| v / 100_000.0);
                Some(Some(Autofit {
                    font_scale: frac("fontScale").unwrap_or(1.0).clamp(0.01, 1.0),
                    line_reduction: frac("lnSpcReduction").unwrap_or(0.0).clamp(0.0, 0.9),
                }))
            } else if b.child("a:noAutofit").is_some() || b.child("a:spAutoFit").is_some() {
                Some(None)
            } else {
                None
            },
        }
    }
    fn over(&mut self, top: &BodyProps) {
        if top.anchor.is_some() {
            self.anchor = top.anchor;
        }
        if top.autofit.is_some() {
            self.autofit = top.autofit;
        }
        for (a, b) in self.ins.iter_mut().zip(top.ins) {
            if b.is_some() {
                *a = b;
            }
        }
    }
}

/// A placeholder's text styling in a layout or master.
#[derive(Debug)]
struct PhText {
    key: PhKey,
    lst: ListStyle,
    body: BodyProps,
}

fn ph_key(sp: &Node) -> Option<PhKey> {
    let ph = sp.child("p:nvSpPr")?.child("p:nvPr")?.child("p:ph")?;
    Some(PhKey {
        ty: ph.attr("type").unwrap_or("obj").to_string(),
        idx: ph.attr("idx").and_then(|v| v.parse().ok()),
    })
}

fn placeholder_texts(root: &Node, theme: &Theme) -> Vec<PhText> {
    let mut sps = Vec::new();
    root.find_all("p:sp", &mut sps);
    sps.into_iter()
        .filter_map(|sp| {
            let key = ph_key(sp)?;
            let tx = sp.child("p:txBody");
            Some(PhText {
                key,
                lst: ListStyle::of(tx.and_then(|t| t.child("a:lstStyle")), theme),
                body: BodyProps::of(tx.and_then(|t| t.child("a:bodyPr"))),
            })
        })
        .collect()
}

/// What the text on a slide can inherit: its layout's and master's
/// placeholders, and the master's text styles.
#[derive(Debug, Default)]
pub(crate) struct Inherited {
    layout: Vec<PhText>,
    master: Vec<PhText>,
    title: ListStyle,
    body: ListStyle,
    other: ListStyle,
}

impl Inherited {
    pub(crate) fn read(layout_xml: &str, master_xml: &str, theme: &Theme) -> Inherited {
        let master = parse_tree(master_xml);
        let tx = master.find("p:txStyles");
        let style = |name: &str| ListStyle::of(tx.and_then(|t| t.child(name)), theme);
        Inherited {
            layout: placeholder_texts(&parse_tree(layout_xml), theme),
            master: placeholder_texts(&master, theme),
            title: style("p:titleStyle"),
            body: style("p:bodyStyle"),
            other: style("p:otherStyle"),
        }
    }

    /// The list styles and body properties a shape inherits, least
    /// specific first.
    fn chain(&self, key: Option<&PhKey>) -> (Vec<&ListStyle>, BodyProps) {
        let Some(key) = key else { return (vec![&self.other], BodyProps::default()) };
        let base = match key.text_style_family() {
            "title" => &self.title,
            "body" => &self.body,
            _ => &self.other,
        };
        let lk: Vec<&PhKey> = self.layout.iter().map(|p| &p.key).collect();
        let mk: Vec<&PhKey> = self.master.iter().map(|p| &p.key).collect();
        let (l, m) = inherited_indices(key, &lk, &mk);
        let mut lists = vec![base];
        let mut body = BodyProps::default();
        for ph in [m.map(|i| &self.master[i]), l.map(|i| &self.layout[i])].into_iter().flatten() {
            lists.push(&ph.lst);
            body.over(&ph.body);
        }
        (lists, body)
    }
}

/// One `p:sp`'s text, resolved.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SpText {
    /// Every `a:p` in the text body, in order, in model units.
    pub paras: Vec<ParaStyle>,
    /// Every run (`a:r` or `a:fld`), in order, with its paragraph's
    /// defaults under its own properties.
    pub runs: Vec<RunProps>,
    pub anchor: Anchor,
    pub insets: Option<Insets>,
    pub autofit: Option<Autofit>,
}

impl SpText {
    /// The text body for the lines a reader kept: `line_paras[k]` is the
    /// index of the `a:p` that line `k` came from.
    pub(crate) fn body_for(&self, line_paras: &[usize]) -> TextBody {
        TextBody {
            paras: line_paras.iter().map(|&p| self.paras.get(p).cloned().unwrap_or_default()).collect(),
            anchor: self.anchor,
            insets: self.insets,
            autofit: self.autofit,
        }
    }
}

/// The resolved text of every `p:sp` in a slide part, in document order.
/// `ph_of(n)` is the placeholder key of the nth shape (None for a plain
/// shape), which the caller already knows.
pub(crate) fn sp_texts(slide_xml: &str, theme: &Theme, inh: &Inherited, scale: SlideScale) -> Vec<SpText> {
    let root = parse_tree(slide_xml);
    let mut sps = Vec::new();
    root.find_all("p:sp", &mut sps);
    sps.into_iter().map(|sp| resolve_sp_text(sp, theme, inh, scale)).collect()
}

fn resolve_sp_text(sp: &Node, theme: &Theme, inh: &Inherited, scale: SlideScale) -> SpText {
    let key = ph_key(sp);
    let (mut lists, mut body) = inh.chain(key.as_ref());
    let tx = sp.child("p:txBody");
    let own = ListStyle::of(tx.and_then(|t| t.child("a:lstStyle")), theme);
    lists.push(&own);
    body.over(&BodyProps::of(tx.and_then(|t| t.child("a:bodyPr"))));

    let emu = |v: f64| v * scale.x;
    let mut out = SpText {
        anchor: body.anchor.unwrap_or_default(),
        // A master's plain `a:normAutofit` says "shrink if needed" with
        // nothing shrunk yet: only a stated scale changes the drawing.
        autofit: body.autofit.flatten().filter(|a| !a.is_identity()),
        // Only a placeholder or a box that states its insets gets them;
        // a plain box keeps the canvas's default so it draws as before.
        insets: if key.is_some() || body.ins.iter().any(Option::is_some) {
            let d = [91440.0, 45720.0, 91440.0, 45720.0];
            let i = |n: usize| body.ins[n].unwrap_or(d[n]);
            Some(Insets { left: emu(i(0)), top: i(1) * scale.y, right: emu(i(2)), bottom: i(3) * scale.y })
        } else {
            None
        },
        ..SpText::default()
    };
    let Some(tx) = tx else { return out };
    let pt = 96.0 / 72.0 * scale.text_factor();
    for p in tx.children_named("a:p") {
        let ppr = p.child("a:pPr");
        let lvl = ppr.and_then(|n| n.attr("lvl")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0).min(8);
        let mut props = LevelProps::default();
        for l in &lists {
            props.over(l.level(lvl));
        }
        if let Some(ppr) = ppr {
            props.over(&LevelProps::of(ppr, theme));
        }
        let spacing = |s: Option<RawSpacing>| match s {
            Some(RawSpacing::Points(h)) => Spacing::Units(h / 100.0 * pt),
            Some(RawSpacing::Percent(v)) => Spacing::Lines(v / 100_000.0),
            None => Spacing::default(),
        };
        out.paras.push(ParaStyle {
            align: props.algn.unwrap_or_default(),
            level: lvl as u8,
            bullet: props.bullet.clone().unwrap_or_default(),
            margin_left: emu(props.mar_l.unwrap_or(0.0)),
            indent: emu(props.indent.unwrap_or(0.0)),
            space_before: spacing(props.spc_bef),
            space_after: spacing(props.spc_aft),
        });
        for r in p.children.iter().filter(|c| c.name == "a:r" || c.name == "a:fld") {
            let mut run = props.run.clone();
            if let Some(rpr) = r.child("a:rPr") {
                run.over(&RunProps::of(rpr, theme));
            }
            out.runs.push(run);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The python-pptx default template's master, trimmed to what matters.
    const MASTER: &str = r#"<p:sldMaster xmlns:a="a" xmlns:p="p"><p:cSld><p:spTree>
        <p:sp><p:nvSpPr><p:cNvPr id="2" name="t"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/>
          <p:txBody><a:bodyPr lIns="91440" tIns="45720" rIns="91440" bIns="45720" anchor="ctr"/><a:lstStyle/><a:p/></p:txBody></p:sp>
        <p:sp><p:nvSpPr><p:cNvPr id="3" name="b"/><p:cNvSpPr/><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/>
          <p:txBody><a:bodyPr/><a:lstStyle/><a:p/></p:txBody></p:sp>
        </p:spTree></p:cSld>
        <p:txStyles>
          <p:titleStyle><a:lvl1pPr algn="ctr"><a:spcBef><a:spcPct val="0"/></a:spcBef><a:buNone/>
            <a:defRPr sz="4400"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill></a:defRPr></a:lvl1pPr></p:titleStyle>
          <p:bodyStyle>
            <a:lvl1pPr marL="342900" indent="-342900" algn="l"><a:spcBef><a:spcPct val="20000"/></a:spcBef><a:buChar char="•"/><a:defRPr sz="3200"/></a:lvl1pPr>
            <a:lvl2pPr marL="742950" indent="-285750" algn="l"><a:spcBef><a:spcPct val="20000"/></a:spcBef><a:buChar char="–"/><a:defRPr sz="2800"/></a:lvl2pPr>
          </p:bodyStyle>
          <p:otherStyle><a:lvl1pPr marL="0" algn="l"><a:defRPr sz="1800"/></a:lvl1pPr></p:otherStyle>
        </p:txStyles></p:sldMaster>"#;

    const LAYOUT: &str = r#"<p:sldLayout xmlns:a="a" xmlns:p="p"><p:cSld><p:spTree>
        <p:sp><p:nvSpPr><p:cNvPr id="2" name="t"/><p:cNvSpPr/><p:nvPr><p:ph type="ctrTitle"/></p:nvPr></p:nvSpPr><p:spPr/>
          <p:txBody><a:bodyPr/><a:lstStyle/><a:p/></p:txBody></p:sp>
        <p:sp><p:nvSpPr><p:cNvPr id="3" name="s"/><p:cNvSpPr/><p:nvPr><p:ph type="subTitle" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/>
          <p:txBody><a:bodyPr/><a:lstStyle><a:lvl1pPr marL="0" indent="0" algn="ctr"><a:buNone/>
            <a:defRPr><a:solidFill><a:schemeClr val="tx1"><a:tint val="75000"/></a:schemeClr></a:solidFill></a:defRPr></a:lvl1pPr></a:lstStyle><a:p/></p:txBody></p:sp>
        <p:sp><p:nvSpPr><p:cNvPr id="4" name="c"/><p:cNvSpPr/><p:nvPr><p:ph idx="2"/></p:nvPr></p:nvSpPr><p:spPr/>
          <p:txBody><a:bodyPr/><a:lstStyle/><a:p/></p:txBody></p:sp>
        </p:spTree></p:cSld></p:sldLayout>"#;

    fn slide(shapes: &str) -> String {
        format!(r#"<p:sld xmlns:a="a" xmlns:p="p"><p:cSld><p:spTree>{shapes}</p:spTree></p:cSld></p:sld>"#)
    }

    fn sp(ph: &str, body: &str) -> String {
        format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="9" name="x"/><p:cNvSpPr/><p:nvPr>{ph}</p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{body}</p:txBody></p:sp>"#
        )
    }

    fn resolve(shapes: &str) -> Vec<SpText> {
        let theme = Theme::default();
        let inh = Inherited::read(LAYOUT, MASTER, &theme);
        sp_texts(&slide(shapes), &theme, &inh, SlideScale::default())
    }

    #[test]
    fn a_centred_title_takes_the_masters_title_style_and_anchor() {
        let got = resolve(&sp(r#"<p:ph type="ctrTitle"/>"#, "<a:p><a:r><a:t>Hi</a:t></a:r></a:p>"));
        assert_eq!(got[0].paras[0].align, ParaAlign::Center);
        assert_eq!(got[0].paras[0].bullet, Bullet::None);
        assert_eq!(got[0].runs[0].style().font_size_hp, Some(88), "44 pt");
        assert_eq!(got[0].runs[0].style().color.as_deref(), Some("000000"));
        assert_eq!(got[0].anchor, Anchor::Middle, "from the master title's bodyPr");
        let ins = got[0].insets.expect("a placeholder has insets");
        assert!((ins.left - 9.6).abs() < 1e-9 && (ins.top - 4.8).abs() < 1e-9);
    }

    #[test]
    fn a_subtitle_is_centred_grey_body_text_without_a_bullet() {
        let got = resolve(&sp(r#"<p:ph type="subTitle" idx="1"/>"#, "<a:p><a:r><a:t>Sub</a:t></a:r></a:p>"));
        let p = &got[0].paras[0];
        assert_eq!((p.align, &p.bullet, p.margin_left, p.indent), (ParaAlign::Center, &Bullet::None, 0.0, 0.0));
        let st = got[0].runs[0].style();
        assert_eq!(st.font_size_hp, Some(64), "32 pt from the body style");
        // tx1 (black) at a 75% tint, in linear light: a mid grey.
        let c = st.color.expect("a colour");
        assert!(c != "000000" && c[..2] == c[2..4], "{c}");
        assert_eq!(got[0].anchor, Anchor::Top);
    }

    #[test]
    fn body_levels_take_their_bullets_indents_and_sizes() {
        let got = resolve(&sp(
            r#"<p:ph idx="2"/>"#,
            r#"<a:p><a:r><a:t>One</a:t></a:r></a:p><a:p><a:pPr lvl="1"/><a:r><a:t>Two</a:t></a:r></a:p>"#,
        ));
        let (a, b) = (&got[0].paras[0], &got[0].paras[1]);
        assert_eq!((a.level, &a.bullet), (0, &Bullet::Char("•".into())));
        assert_eq!((b.level, &b.bullet), (1, &Bullet::Char("–".into())));
        // 342900 EMU = 0.375in = 36 model units.
        assert!((a.margin_left - 36.0).abs() < 1e-9 && (a.indent + 36.0).abs() < 1e-9);
        assert_eq!(a.space_before, Spacing::Lines(0.2));
        assert_eq!(got[0].runs[0].style().font_size_hp, Some(64));
        assert_eq!(got[0].runs[1].style().font_size_hp, Some(56));
    }

    #[test]
    fn the_slide_overrides_what_it_inherits() {
        let got = resolve(&sp(
            r#"<p:ph idx="2"/>"#,
            r#"<a:p><a:pPr algn="r"><a:buNone/></a:pPr><a:r><a:rPr sz="1000" b="1"><a:solidFill><a:srgbClr val="C80000"/></a:solidFill></a:rPr><a:t>x</a:t></a:r><a:r><a:t>y</a:t></a:r></a:p>"#,
        ));
        assert_eq!(got[0].paras[0].align, ParaAlign::Right);
        assert_eq!(got[0].paras[0].bullet, Bullet::None);
        let (x, y) = (got[0].runs[0].style(), got[0].runs[1].style());
        assert_eq!((x.font_size_hp, x.bold, x.color.as_deref()), (Some(20), true, Some("c80000")));
        assert_eq!((y.font_size_hp, y.bold), (Some(64), false), "the next run is back to the level's defaults");
    }

    #[test]
    fn a_plain_text_box_takes_the_other_style_and_no_insets() {
        let got = resolve(&sp("", "<a:p><a:r><a:t>t</a:t></a:r></a:p>"));
        assert_eq!(got[0].paras[0], ParaStyle::default());
        assert_eq!(got[0].runs[0].style().font_size_hp, Some(36));
        assert_eq!(got[0].insets, None);
    }

    #[test]
    fn lines_map_to_the_paragraphs_they_came_from() {
        let got = resolve(&sp(
            r#"<p:ph idx="2"/>"#,
            r#"<a:p/><a:p><a:r><a:t>a</a:t></a:r></a:p><a:p><a:pPr lvl="1"/><a:r><a:t>b</a:t></a:r></a:p>"#,
        ));
        // The reader drops the leading empty paragraph: lines are paras 1, 2.
        let body = got[0].body_for(&[1, 2]);
        assert_eq!(body.paras.iter().map(|p| p.level).collect::<Vec<_>>(), [0, 1]);
    }

    #[test]
    fn a_content_box_the_layout_has_no_slot_for_takes_the_masters_body_not_the_subtitle() {
        // LAYOUT is python-pptx's Title Slide plus a content slot at idx 2;
        // idx 13 is on neither. The body style's bullets apply, not the
        // subtitle's centred, bullet-less level.
        let layout = LAYOUT.replace(r#"<p:ph idx="2"/>"#, r#"<p:ph type="dt" idx="10"/>"#);
        let theme = Theme::default();
        let inh = Inherited::read(&layout, MASTER, &theme);
        let got = sp_texts(&slide(&sp(r#"<p:ph idx="13"/>"#, "<a:p><a:r><a:t>a</a:t></a:r></a:p>")), &theme, &inh, SlideScale::default());
        assert_eq!(got[0].paras[0].bullet, Bullet::Char("•".into()));
        assert_eq!(got[0].paras[0].align, ParaAlign::Left);
    }

    #[test]
    fn a_shrunk_body_keeps_its_scale_and_a_plain_autofit_is_nothing() {
        let body = |pr: &str| {
            format!(r#"<p:sp><p:nvSpPr><p:cNvPr id="9" name="x"/><p:cNvSpPr/><p:nvPr><p:ph idx="2"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody>{pr}<a:lstStyle/><a:p><a:r><a:t>a</a:t></a:r></a:p></p:txBody></p:sp>"#)
        };
        let got = resolve(&(body(r#"<a:bodyPr><a:normAutofit fontScale="62500" lnSpcReduction="20000"/></a:bodyPr>"#)
            + &body(r#"<a:bodyPr><a:normAutofit/></a:bodyPr>"#)));
        assert_eq!(got[0].autofit, Some(Autofit { font_scale: 0.625, line_reduction: 0.2 }));
        assert_eq!(got[1].autofit, None);
        // The sizes themselves are the author's.
        assert_eq!(got[0].runs[0].style().font_size_hp, Some(64));
    }

    #[test]
    fn theme_font_references_resolve_to_the_themes_faces() {
        let theme = super::super::shape_xml::theme(
            r#"<a:theme xmlns:a="a"><a:themeElements><a:fontScheme name="x">
               <a:majorFont><a:latin typeface="Heading Face"/><a:ea typeface=""/></a:majorFont>
               <a:minorFont><a:latin typeface="Body Face"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#,
        );
        let master = MASTER.replace(
            r#"<a:defRPr sz="4400">"#,
            r#"<a:defRPr sz="4400"><a:latin typeface="+mj-lt"/>"#,
        ).replace(
            r#"<a:defRPr sz="3200"/>"#,
            r#"<a:defRPr sz="3200"><a:latin typeface="+mn-lt"/></a:defRPr>"#,
        );
        let inh = Inherited::read(LAYOUT, &master, &theme);
        let got = sp_texts(
            &slide(&(sp(r#"<p:ph type="title"/>"#, "<a:p><a:r><a:t>T</a:t></a:r></a:p>")
                + &sp(r#"<p:ph idx="2"/>"#, r#"<a:p><a:r><a:t>a</a:t></a:r><a:r><a:rPr><a:latin typeface="Own Face"/></a:rPr><a:t>b</a:t></a:r></a:p>"#))),
            &theme,
            &inh,
            SlideScale::default(),
        );
        assert_eq!(got[0].runs[0].style().font_family.as_deref(), Some("Heading Face"));
        assert_eq!(got[1].runs[0].style().font_family.as_deref(), Some("Body Face"));
        assert_eq!(got[1].runs[1].style().font_family.as_deref(), Some("Own Face"));
        // A reference the theme can't answer is no font, not "+mj-lt".
        assert_eq!(Theme::default().typeface("+mj-lt"), None);
        assert_eq!(Theme::default().typeface("+mn-ea"), None);
    }

    #[test]
    fn sizes_scale_onto_the_models_slide() {
        let theme = Theme::default();
        let inh = Inherited::read(LAYOUT, MASTER, &theme);
        let wide = SlideScale::from_emu(12_192_000.0, 6_858_000.0);
        let got = sp_texts(&slide(&sp(r#"<p:ph idx="2"/>"#, r#"<a:p><a:pPr><a:spcBef><a:spcPts val="1200"/></a:spcBef></a:pPr><a:r><a:t>a</a:t></a:r></a:p>"#)), &theme, &inh, wide);
        // 12 pt of space on a 13.33in slide is 9 pt on the model's: 12 units.
        assert_eq!(got[0].paras[0].space_before, Spacing::Units(12.0));
        // 342900 EMU on the wide slide is 0.375 * 0.75 in = 27 units.
        assert!((got[0].paras[0].margin_left - 27.0).abs() < 1e-9);
    }
}
