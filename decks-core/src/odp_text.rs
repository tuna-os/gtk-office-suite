//! odp_text.rs — a text box's paragraph layout in ODF: paragraph styles,
//! lists, and the frame's anchor and padding.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! The pptx side is `engine/text_xml.rs` (read) and `engine/write.rs`
//! (write). In ODF the same things live in three kinds of automatic style:
//!
//! - a `style:family="paragraph"` style: `fo:text-align`, `fo:margin-left`,
//!   `fo:text-indent`, `fo:margin-top`/`-bottom`;
//! - a `text:list-style`, one level per list depth: a bullet character
//!   (`text:list-level-style-bullet`) or a number format
//!   (`text:list-level-style-number`), and the level's indents. A
//!   paragraph's list level is how deeply its `text:list` elements nest;
//! - a `style:family="graphic"` style on the frame:
//!   `draw:textarea-vertical-align` and `fo:padding-*`.
//!
//! Lengths are written in points; the odp writer's page is 960 x 540 pt, so
//! one model unit is one point, as for geometry.
//!
//! Not carried: spacing given as a fraction of a line (`Spacing::Lines`),
//! which ODF paragraph margins can't express. It is written as nothing and
//! read back as zero.

use crate::engine::text_body::{Anchor, Bullet, Insets, MarkerSize, MarkerStyle, ParaAlign, ParaStyle, Spacing, TextBody};
use std::collections::HashMap;

fn pt(v: f64) -> String {
    format!("{}pt", (v * 1e4).round() / 1e4)
}

/// One list level's marker and indents.
#[derive(Clone, Debug, PartialEq)]
struct Level {
    level: u8,
    bullet: Bullet,
    margin_left: f64,
    indent: f64,
    marker: MarkerStyle,
}

/// The automatic styles a part's text boxes need, named under a prefix in
/// first-use order.
#[derive(Default)]
pub(crate) struct TextStyles {
    prefix: String,
    paras: Vec<ParaStyle>,
    lists: Vec<Vec<Level>>,
    frames: Vec<(Anchor, Option<Insets>)>,
}

fn index_of<T: PartialEq + Clone>(list: &mut Vec<T>, item: &T) -> usize {
    match list.iter().position(|x| x == item) {
        Some(i) => i + 1,
        None => {
            list.push(item.clone());
            list.len()
        }
    }
}

/// ODF's `style:num-format`/`num-prefix`/`num-suffix` for a DrawingML
/// auto-number scheme.
fn num_format(scheme: &str) -> (&'static str, &'static str, &'static str) {
    let fmt = if scheme.starts_with("alphaLc") {
        "a"
    } else if scheme.starts_with("alphaUc") {
        "A"
    } else if scheme.starts_with("romanLc") {
        "i"
    } else if scheme.starts_with("romanUc") {
        "I"
    } else {
        "1"
    };
    let (prefix, suffix) = if scheme.ends_with("ParenBoth") {
        ("(", ")")
    } else if scheme.ends_with("ParenR") {
        ("", ")")
    } else if scheme.ends_with("Plain") {
        ("", "")
    } else {
        ("", ".")
    };
    (fmt, prefix, suffix)
}

/// The DrawingML scheme for an ODF number format, the inverse of
/// `num_format` for the schemes it produces.
fn scheme_of(fmt: &str, prefix: &str, suffix: &str) -> String {
    let base = match fmt {
        "a" => "alphaLc",
        "A" => "alphaUc",
        "i" => "romanLc",
        "I" => "romanUc",
        _ => "arabic",
    };
    let tail = match (prefix, suffix) {
        ("(", ")") => "ParenBoth",
        (_, ")") => "ParenR",
        (_, "") => "Plain",
        _ => "Period",
    };
    format!("{base}{tail}")
}

impl TextStyles {
    pub(crate) fn new(prefix: &str) -> Self {
        TextStyles { prefix: prefix.to_string(), ..Default::default() }
    }

    fn para_name(&mut self, st: &ParaStyle) -> Option<String> {
        // Level and bullet belong to the list; the rest to the paragraph.
        let key = ParaStyle { level: 0, bullet: Bullet::None, ..st.clone() };
        if key == ParaStyle::default() {
            return None;
        }
        Some(format!("{}P{}", self.prefix, index_of(&mut self.paras, &key)))
    }

    fn list_name(&mut self, levels: Vec<Level>) -> String {
        format!("{}L{}", self.prefix, index_of(&mut self.lists, &levels))
    }

    /// The graphic style for a text frame, when its body states an anchor
    /// or insets.
    pub(crate) fn frame_name(&mut self, body: &TextBody) -> Option<String> {
        if body.anchor == Anchor::Top && body.insets.is_none() {
            return None;
        }
        Some(format!("{}F{}", self.prefix, index_of(&mut self.frames, &(body.anchor, body.insets))))
    }

    /// The `text:p`s (and lists) of a box. `inner[i]` is paragraph `i`'s
    /// already-written content.
    pub(crate) fn paragraphs(&mut self, body: &TextBody, inner: &[String]) -> String {
        let styles: Vec<ParaStyle> = (0..inner.len()).map(|i| body.para(i)).collect();
        let mut out = String::new();
        let mut i = 0;
        while i < inner.len() {
            if styles[i].bullet == Bullet::None {
                out.push_str(&self.p(&styles[i], &inner[i]));
                i += 1;
                continue;
            }
            // A block of consecutive list paragraphs is one list.
            let end = (i..inner.len()).find(|&j| styles[j].bullet == Bullet::None).unwrap_or(inner.len());
            let mut levels: Vec<Level> = Vec::new();
            for st in &styles[i..end] {
                if !levels.iter().any(|l| l.level == st.level) {
                    levels.push(Level {
                        level: st.level,
                        bullet: st.bullet.clone(),
                        margin_left: st.margin_left,
                        indent: st.indent,
                        marker: st.marker.clone(),
                    });
                }
            }
            levels.sort_by_key(|l| l.level);
            let name = self.list_name(levels);
            out.push_str(&format!("<text:list text:style-name=\"{name}\">"));
            // item_open[d]: the list at depth d+1 has an open list-item.
            let mut item_open: Vec<bool> = vec![false];
            for j in i..end {
                let target = styles[j].level as usize + 1;
                while item_open.len() > target {
                    if item_open.pop() == Some(true) {
                        out.push_str("</text:list-item>");
                    }
                    out.push_str("</text:list>");
                }
                if item_open.len() == target && item_open.last() == Some(&true) {
                    out.push_str("</text:list-item>");
                    if let Some(open) = item_open.last_mut() {
                        *open = false;
                    }
                }
                while item_open.len() < target {
                    if let Some(open) = item_open.last_mut() {
                        if !*open {
                            out.push_str("<text:list-item>");
                            *open = true;
                        }
                    }
                    out.push_str("<text:list>");
                    item_open.push(false);
                }
                out.push_str("<text:list-item>");
                out.push_str(&self.p(&styles[j], &inner[j]));
                if let Some(open) = item_open.last_mut() {
                    *open = true;
                }
            }
            while let Some(open) = item_open.pop() {
                if open {
                    out.push_str("</text:list-item>");
                }
                out.push_str("</text:list>");
            }
            i = end;
        }
        out
    }

    fn p(&mut self, st: &ParaStyle, inner: &str) -> String {
        match self.para_name(st) {
            Some(n) => format!("<text:p text:style-name=\"{n}\">{inner}</text:p>"),
            None => format!("<text:p>{inner}</text:p>"),
        }
    }

    /// The declarations, for `office:automatic-styles`.
    pub(crate) fn declare(&self) -> String {
        let mut out = String::new();
        for (i, st) in self.paras.iter().enumerate() {
            let mut props = String::new();
            if st.align != ParaAlign::Left {
                props.push_str(&format!(" fo:text-align=\"{}\"", st.align.to_odf()));
            }
            if st.margin_left != 0.0 {
                props.push_str(&format!(" fo:margin-left=\"{}\"", pt(st.margin_left)));
            }
            if st.indent != 0.0 {
                props.push_str(&format!(" fo:text-indent=\"{}\"", pt(st.indent)));
            }
            for (attr, sp) in [("fo:margin-top", st.space_before), ("fo:margin-bottom", st.space_after)] {
                if let Spacing::Units(u) = sp {
                    if u != 0.0 {
                        props.push_str(&format!(" {attr}=\"{}\"", pt(u)));
                    }
                }
            }
            out.push_str(&format!(
                "<style:style style:name=\"{}P{}\" style:family=\"paragraph\"><style:paragraph-properties{props}/></style:style>",
                self.prefix,
                i + 1
            ));
        }
        for (i, levels) in self.lists.iter().enumerate() {
            out.push_str(&format!("<text:list-style style:name=\"{}L{}\">", self.prefix, i + 1));
            for l in levels {
                let props = format!(
                    "<style:list-level-properties text:list-level-position-and-space-mode=\"label-alignment\">\
                     <style:list-level-label-alignment text:label-followed-by=\"listtab\" \
                     fo:margin-left=\"{}\" fo:text-indent=\"{}\"/></style:list-level-properties>",
                    pt(l.margin_left),
                    pt(l.indent)
                );
                let n = l.level + 1;
                // The bullet's own font and colour, and its size relative
                // to the text (ODF has no absolute bullet size: points are
                // written as nothing).
                let mut tp = String::new();
                if let Some(f) = l.marker.font.as_deref() {
                    tp.push_str(&format!(" fo:font-family=\"{}\"", crate::odp::esc(f)));
                }
                if let Some(c) = l.marker.color.as_deref() {
                    tp.push_str(&format!(" fo:color=\"#{c}\""));
                }
                let text_props = if tp.is_empty() { String::new() } else { format!("<style:text-properties{tp}/>") };
                let rel = match l.marker.size {
                    Some(MarkerSize::Relative(f)) => format!(" text:bullet-relative-size=\"{}%\"", (f * 100.0).round()),
                    _ => String::new(),
                };
                match &l.bullet {
                    Bullet::Char(c) => out.push_str(&format!(
                        "<text:list-level-style-bullet text:level=\"{n}\" text:bullet-char=\"{}\"{rel}>{props}{text_props}</text:list-level-style-bullet>",
                        crate::odp::esc(c)
                    )),
                    Bullet::AutoNum { scheme, start } => {
                        let (fmt, prefix, suffix) = num_format(scheme);
                        out.push_str(&format!(
                            "<text:list-level-style-number text:level=\"{n}\" style:num-format=\"{fmt}\" \
                             style:num-prefix=\"{prefix}\" style:num-suffix=\"{suffix}\" text:start-value=\"{start}\">{props}</text:list-level-style-number>"
                        ))
                    }
                    Bullet::None => {}
                }
            }
            out.push_str("</text:list-style>");
        }
        for (i, (anchor, insets)) in self.frames.iter().enumerate() {
            let mut props = String::new();
            let va = match anchor {
                Anchor::Top => "top",
                Anchor::Middle => "middle",
                Anchor::Bottom => "bottom",
            };
            props.push_str(&format!(" draw:textarea-vertical-align=\"{va}\""));
            if let Some(ins) = insets {
                props.push_str(&format!(
                    " fo:padding-left=\"{}\" fo:padding-top=\"{}\" fo:padding-right=\"{}\" fo:padding-bottom=\"{}\"",
                    pt(ins.left),
                    pt(ins.top),
                    pt(ins.right),
                    pt(ins.bottom)
                ));
            }
            out.push_str(&format!(
                "<style:style style:name=\"{}F{}\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"none\" draw:stroke=\"none\"{props}/></style:style>",
                self.prefix,
                i + 1
            ));
        }
        out
    }
}

// ── Reading ──────────────────────────────────────────────────────────

/// What a paragraph style says, as far as the model carries it.
#[derive(Clone, Debug, Default, PartialEq)]
struct ParaDef {
    align: Option<ParaAlign>,
    margin_left: Option<f64>,
    indent: Option<f64>,
    top: Option<f64>,
    bottom: Option<f64>,
}

/// Text-layout styles a document defines, by name.
#[derive(Debug, Default)]
pub(crate) struct TextDefs {
    paras: HashMap<String, ParaDef>,
    /// List style → level (0-based) → level definition.
    lists: HashMap<String, HashMap<u8, Level>>,
    /// Graphic style → anchor and padding (pt).
    frames: HashMap<String, (Option<Anchor>, [Option<f64>; 4])>,
}

fn attr(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
}

fn len(e: &quick_xml::events::BytesStart, name: &str) -> Option<f64> {
    attr(e, name).and_then(|v| crate::odp::parse_length_pt(&v))
}

impl TextDefs {
    /// Collect the definitions in one part (`content.xml` or `styles.xml`).
    pub(crate) fn read(&mut self, xml: &str) {
        use quick_xml::events::Event;
        let mut reader = quick_xml::Reader::from_str(xml);
        // (name, family) of the style:style being read, or the list style.
        let mut style: Option<(String, String)> = None;
        let mut list: Option<String> = None;
        let mut level: Option<Level> = None;
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                    "style:style" => style = attr(&e, "style:name").zip(attr(&e, "style:family")),
                    "text:list-style" => list = attr(&e, "style:name"),
                    "style:paragraph-properties" => {
                        if let Some((name, "paragraph")) = style.as_ref().map(|(n, f)| (n, f.as_str())) {
                            self.paras.insert(
                                name.clone(),
                                ParaDef {
                                    align: attr(&e, "fo:text-align").and_then(|v| ParaAlign::from_odf(&v)),
                                    margin_left: len(&e, "fo:margin-left"),
                                    indent: len(&e, "fo:text-indent"),
                                    top: len(&e, "fo:margin-top"),
                                    bottom: len(&e, "fo:margin-bottom"),
                                },
                            );
                        }
                    }
                    "style:graphic-properties" => {
                        if let Some((name, "graphic")) = style.as_ref().map(|(n, f)| (n, f.as_str())) {
                            let anchor = attr(&e, "draw:textarea-vertical-align").and_then(|v| match v.as_str() {
                                "top" => Some(Anchor::Top),
                                "middle" => Some(Anchor::Middle),
                                "bottom" => Some(Anchor::Bottom),
                                _ => None,
                            });
                            let pad = [
                                len(&e, "fo:padding-left"),
                                len(&e, "fo:padding-top"),
                                len(&e, "fo:padding-right"),
                                len(&e, "fo:padding-bottom"),
                            ];
                            if anchor.is_some() || pad.iter().any(Option::is_some) {
                                self.frames.insert(name.clone(), (anchor, pad));
                            }
                        }
                    }
                    tag @ ("text:list-level-style-bullet" | "text:list-level-style-number") if list.is_some() => {
                        let n = attr(&e, "text:level").and_then(|v| v.parse::<u8>().ok()).unwrap_or(1);
                        let bullet = if tag == "text:list-level-style-bullet" {
                            Bullet::Char(attr(&e, "text:bullet-char").unwrap_or_else(|| "•".into()))
                        } else {
                            Bullet::AutoNum {
                                scheme: scheme_of(
                                    &attr(&e, "style:num-format").unwrap_or_else(|| "1".into()),
                                    &attr(&e, "style:num-prefix").unwrap_or_default(),
                                    &attr(&e, "style:num-suffix").unwrap_or_default(),
                                ),
                                start: attr(&e, "text:start-value").and_then(|v| v.parse().ok()).unwrap_or(1),
                            }
                        };
                        let marker = MarkerStyle {
                            size: attr(&e, "text:bullet-relative-size")
                                .and_then(|v| v.trim_end_matches('%').parse::<f64>().ok())
                                .map(|p| MarkerSize::Relative(p / 100.0)),
                            ..MarkerStyle::default()
                        };
                        let l = Level { level: n.saturating_sub(1), bullet, margin_left: 0.0, indent: 0.0, marker };
                        if let Some(list) = &list {
                            self.lists.entry(list.clone()).or_default().insert(l.level, l.clone());
                        }
                        level = Some(l);
                    }
                    "style:text-properties" if level.is_some() => {
                        if let (Some(list), Some(l)) = (&list, level.as_mut()) {
                            l.marker.font = attr(&e, "fo:font-family")
                                .or_else(|| attr(&e, "style:font-name"))
                                .map(|f| f.trim().trim_matches('\'').to_string())
                                .filter(|f| !f.is_empty());
                            l.marker.color = attr(&e, "fo:color").map(|c| c.trim_start_matches('#').to_lowercase());
                            self.lists.entry(list.clone()).or_default().insert(l.level, l.clone());
                        }
                    }
                    "style:list-level-label-alignment" => {
                        if let (Some(list), Some(l)) = (&list, level.as_mut()) {
                            l.margin_left = len(&e, "fo:margin-left").unwrap_or(0.0);
                            l.indent = len(&e, "fo:text-indent").unwrap_or(0.0);
                            self.lists.entry(list.clone()).or_default().insert(l.level, l.clone());
                        }
                    }
                    _ => {}
                },
                Ok(Event::End(e)) => match e.name().as_ref() {
                    "style:style" => style = None,
                    "text:list-style" => list = None,
                    "text:list-level-style-bullet" | "text:list-level-style-number" => level = None,
                    _ => {}
                },
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
    }

    /// A paragraph's style: its paragraph style name, the list style it is
    /// in (the outermost `text:list`'s) and how deep it is (0 = not in a
    /// list). `k` scales points to model units.
    pub(crate) fn para(&self, style: Option<&str>, list: Option<&str>, depth: usize, k: f64) -> ParaStyle {
        let d = style.and_then(|s| self.paras.get(s)).cloned().unwrap_or_default();
        let mut st = ParaStyle {
            align: d.align.unwrap_or_default(),
            margin_left: d.margin_left.unwrap_or(0.0) * k,
            indent: d.indent.unwrap_or(0.0) * k,
            space_before: Spacing::Units(d.top.unwrap_or(0.0) * k),
            space_after: Spacing::Units(d.bottom.unwrap_or(0.0) * k),
            ..ParaStyle::default()
        };
        if depth > 0 {
            st.level = (depth - 1).min(8) as u8;
            if let Some(l) = list.and_then(|n| self.lists.get(n)).and_then(|ls| ls.get(&st.level)) {
                st.bullet = l.bullet.clone();
                st.marker = l.marker.clone();
                // The paragraph's own indents win; the level's apply when
                // it states none.
                if d.margin_left.is_none() {
                    st.margin_left = l.margin_left * k;
                }
                if d.indent.is_none() {
                    st.indent = l.indent * k;
                }
            }
        }
        st
    }

    /// A frame's anchor and insets, from its graphic style.
    pub(crate) fn frame(&self, style: Option<&str>, k: (f64, f64)) -> (Anchor, Option<Insets>) {
        let Some((anchor, pad)) = style.and_then(|s| self.frames.get(s)) else { return (Anchor::Top, None) };
        let insets = if pad.iter().any(Option::is_some) {
            let p = |i: usize, d: f64| pad[i].unwrap_or(d);
            let dflt = Insets::DRAWINGML;
            Some(Insets {
                left: p(0, dflt.left) * k.0,
                top: p(1, dflt.top) * k.1,
                right: p(2, dflt.right) * k.0,
                bottom: p(3, dflt.bottom) * k.1,
            })
        } else {
            None
        };
        (anchor.unwrap_or_default(), insets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bulleted(level: u8, c: &str) -> ParaStyle {
        ParaStyle {
            level,
            bullet: Bullet::Char(c.into()),
            marker: MarkerStyle {
                font: Some("Arial".into()),
                size: Some(MarkerSize::Relative(0.75)),
                color: Some("c00000".into()),
            },
            margin_left: 36.0 * (level as f64 + 1.0),
            indent: -18.0,
            ..Default::default()
        }
    }

    #[test]
    fn nested_levels_become_nested_lists_and_read_back() {
        let body = TextBody {
            paras: vec![
                ParaStyle { align: ParaAlign::Center, ..Default::default() },
                bulleted(0, "•"),
                bulleted(1, "–"),
                bulleted(0, "•"),
            ],
            anchor: Anchor::Middle,
            insets: Some(Insets { left: 9.6, top: 4.8, right: 9.6, bottom: 4.8 }),
            autofit: None,
        };
        let mut w = TextStyles::new("T");
        let inner: Vec<String> = ["t", "a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let xml = w.paragraphs(&body, &inner);
        assert_eq!(
            xml,
            "<text:p text:style-name=\"TP1\">t</text:p>\
             <text:list text:style-name=\"TL1\">\
             <text:list-item><text:p text:style-name=\"TP2\">a</text:p>\
             <text:list><text:list-item><text:p text:style-name=\"TP3\">b</text:p></text:list-item></text:list>\
             </text:list-item>\
             <text:list-item><text:p text:style-name=\"TP2\">c</text:p></text:list-item>\
             </text:list>"
        );
        let frame = w.frame_name(&body).expect("a frame style");

        let mut defs = TextDefs::default();
        defs.read(&format!("<office:automatic-styles>{}</office:automatic-styles>", w.declare()));
        let got: Vec<ParaStyle> = [(Some("TP1"), None, 0), (Some("TP2"), Some("TL1"), 1), (Some("TP3"), Some("TL1"), 2)]
            .iter()
            .map(|(s, l, d)| defs.para(*s, *l, *d, 1.0))
            .collect();
        assert_eq!(got[0].align, ParaAlign::Center);
        assert_eq!(got[1], body.paras[1]);
        assert_eq!(got[2], body.paras[2]);
        let (anchor, insets) = defs.frame(Some(&frame), (1.0, 1.0));
        assert_eq!(anchor, Anchor::Middle);
        assert_eq!(insets, body.insets);
    }

    #[test]
    fn numbering_schemes_round_trip_through_odf_formats() {
        for scheme in ["arabicPeriod", "arabicParenR", "alphaLcParenBoth", "romanUcPeriod", "alphaUcPlain"] {
            let (f, p, s) = num_format(scheme);
            assert_eq!(scheme_of(f, p, s), scheme);
        }
    }

    #[test]
    fn a_plain_body_writes_plain_paragraphs() {
        let mut w = TextStyles::new("T");
        let xml = w.paragraphs(&TextBody::default(), &["a".into(), "b".into()]);
        assert_eq!(xml, "<text:p>a</text:p><text:p>b</text:p>");
        assert_eq!(w.frame_name(&TextBody::default()), None);
        assert_eq!(w.declare(), "");
    }
}
