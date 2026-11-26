// SPDX-License-Identifier: GPL-3.0-or-later
//! Style inheritance: resolving paragraph/run properties through the style chain.

#![allow(unused_imports)]
use super::*;

/// How headings 1-6 look, from styles.xml's `Heading_20_N` (#1297), each
/// with what it inherits from its parent styles; empty when the file
/// defines none, as the docx reader does.
/// The document's body font: the default paragraph style's font and size,
/// with the "Standard" (Default Paragraph Style) style's own over them, as
/// Writer resolves them. A font is named by its font-face declaration.
/// Without it, a Calibri document drew in the application's serif face.
pub(super) fn read_base_font(styles: &str) -> crate::model::BaseFont {
    let mut faces: std::collections::HashMap<String, String> = Default::default();
    // (font name, size in points) from the default style, then Standard,
    // then Word's "Normal": a Word conversion has no Standard, and every
    // paragraph style inherits Normal's font, not the default style's.
    let mut found: [(Option<String>, Option<f64>); 3] = Default::default();
    let mut slot: Option<usize> = None;
    let mut reader = Reader::from_str(styles);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                "style:font-face" => {
                    if let (Some(n), Some(f)) = (attr_val(&e, "style:name"), attr_val(&e, "svg:font-family")) {
                        faces.insert(n, f.trim_matches(|c| c == '\'' || c == '"').to_string());
                    }
                }
                "style:default-style" => {
                    slot = (attr_val(&e, "style:family").as_deref() == Some("paragraph")).then_some(0);
                }
                "style:style" => {
                    let paragraph = attr_val(&e, "style:family").as_deref() == Some("paragraph");
                    slot = match attr_val(&e, "style:name").as_deref() {
                        Some("Standard") if paragraph => Some(1),
                        Some("Normal") if paragraph => Some(2),
                        _ => None,
                    };
                }
                "style:text-properties" => {
                    if let Some(i) = slot {
                        if let Some(n) = attr_val(&e, "style:font-name") {
                            found[i].0 = Some(n);
                        }
                        if let Some(pt) = attr_val(&e, "fo:font-size").and_then(|v| parse_length_pt(&v)) {
                            found[i].1 = Some(pt);
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if matches!(e.name().as_ref(), "style:default-style" | "style:style") => slot = None,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    // Normal stands in for Standard only when the file has no Standard.
    let base = if found[1].0.is_some() || found[1].1.is_some() { 1 } else { 2 };
    let name = found[base].0.clone().or_else(|| found[0].0.clone());
    let size = found[base].1.or(found[0].1);
    crate::model::BaseFont {
        family: name.map(|n| faces.get(&n).cloned().unwrap_or(n)),
        size_hp: size.map(|pt| (pt * 2.0).round() as u16).filter(|hp| *hp > 0),
    }
}

/// The space before, space after and line height paragraph style `name`
/// has: its own where it sets them, else the nearest style up its parent
/// chain (automatic styles in `auto`, named ones in `named`) that does.
/// None where no style in the chain sets one.
pub(super) fn inherited_spacing(name: &str, auto: &AutoStyles, named: &AutoStyles) -> (Option<f64>, Option<f64>, Option<f32>) {
    let (mut before, mut after, mut line) = (None, None, None);
    // Writer keeps a style's contextual spacing in the same setting as its
    // space above and below: a style that sets the one and not the others
    // takes the document default's spacing, not its parent's. The W1
    // notes' List Paragraph sets contextual spacing under a Normal of 6pt
    // above, and Writer opens its lists with no space above and closes
    // them with the default's 8pt below.
    let mut spacing_settled = false;
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let Some(n) = cur else { break };
        if let Some(st) = auto.para.get(&n).or_else(|| named.para.get(&n)) {
            if !spacing_settled {
                if st.sets[0] && before.is_none() {
                    before = Some(st.space_before_pt);
                }
                if st.sets[1] && after.is_none() {
                    after = Some(st.space_after_pt);
                }
                // A style that states its space above or below settles
                // both: what it leaves out is the document default's, not
                // its parent's. Measured in LibreOffice 24.2: a 6pt
                // `fo:margin-top` under a named parent's 20pt
                // `fo:margin-bottom` has nothing below, under only the
                // default style's 20pt it has 20pt below. The data
                // dictionary's blank line between its tables states 6pt
                // above and keeps the default's 8pt below, which was lost.
                spacing_settled = st.sets[0] || st.sets[1] || auto.contextual.contains_key(&n) || named.contextual.contains_key(&n);
            }
            if st.sets[2] && line.is_none() {
                line = Some(st.line_spacing);
            }
        }
        cur = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
    }
    // Every chain ends at the document's default paragraph style.
    if let Some(st) = named.para.get(DEFAULT_PARA) {
        if st.sets[0] && before.is_none() {
            before = Some(st.space_before_pt);
        }
        if st.sets[1] && after.is_none() {
            after = Some(st.space_after_pt);
        }
        if st.sets[2] && line.is_none() {
            line = Some(st.line_spacing);
        }
    }
    (before, after, line)
}

/// The left, right and first-line indents paragraph style `name` has:
/// its own where it sets them, else the nearest style up its parent chain
/// that does ("Table Paragraph"'s small left indent in every cell of the
/// vessel declaration). None where no style in the chain sets one.
pub(super) fn inherited_indents(name: &str, auto: &AutoStyles, named: &AutoStyles) -> [Option<f64>; 3] {
    let mut out = [None; 3];
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let Some(n) = cur else { break };
        if let Some(st) = auto.para.get(&n).or_else(|| named.para.get(&n)) {
            let own = [st.left_indent_pt, st.right_indent_pt, st.first_line_indent_pt];
            for k in 0..3 {
                if st.sets_indent[k] && out[k].is_none() {
                    out[k] = Some(own[k]);
                }
            }
        }
        cur = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
    }
    out
}

/// What paragraph style `name` inherits from its parent chain for the
/// keep and decoration properties: the nearest style up the chain
/// (automatic styles in `auto`, named ones in `named`) that sets each
/// one. Writer keeps these on the named style — a Heading's
/// keep-with-next, a quote's border — while the automatic style on the
/// paragraph carries only what that paragraph overrides.
#[derive(Default)]
pub(super) struct InheritedPara {
    pub(super) keep_with_next: bool,
    pub(super) keep_lines: bool,
    pub(super) widow_control: Option<bool>,
    pub(super) line_height: Option<LineHeight>,
    pub(super) shading: Option<String>,
    pub(super) borders: ParaBorders,
}

pub(super) fn inherited_para(name: &str, auto: &AutoStyles, named: &AutoStyles) -> InheritedPara {
    let mut out = InheritedPara::default();
    let mut line_decided = false;
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let Some(n) = cur else { break };
        if let Some(st) = auto.para.get(&n).or_else(|| named.para.get(&n)) {
            out.keep_with_next |= st.keep_with_next;
            out.keep_lines |= st.keep_lines;
            if out.widow_control.is_none() {
                out.widow_control = st.widow_control;
            }
            // The nearest style that sets a line height decides it: a
            // heading's 100% over Normal's exact one.
            if !line_decided && (st.sets[2] || st.line_height.is_some()) {
                (out.line_height, line_decided) = (st.line_height, true);
            }
            if out.shading.is_none() {
                out.shading.clone_from(&st.shading);
            }
            if out.borders.top.is_none() {
                out.borders.top.clone_from(&st.borders.top);
            }
            if out.borders.left.is_none() {
                out.borders.left.clone_from(&st.borders.left);
            }
            if out.borders.bottom.is_none() {
                out.borders.bottom.clone_from(&st.borders.bottom);
            }
            if out.borders.right.is_none() {
                out.borders.right.clone_from(&st.borders.right);
            }
        }
        cur = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
    }
    out
}

/// How a paragraph style's text looks: its own text properties, then what
/// it leaves unset from the named styles it is based on, as Writer draws a
/// form's "Coloured Box Headline" (bold, 14pt, based on Normal). The base
/// paragraph style (Standard, Word's Normal, or one based on nothing) is
/// the document's base font (`read_base_font`), not its runs'.
pub(super) fn para_text_style(name: &str, auto: &AutoStyles, named: &AutoStyles) -> RunStyle {
    let mut cur = auto.para_parent.get(name).or_else(|| named.para_parent.get(name)).cloned();
    let base = cur.is_none() || matches!(name, "Standard" | "Normal");
    let own = auto.para_text.get(name).or_else(|| named.para_text.get(name).filter(|_| !base));
    let mut style = own.cloned().unwrap_or_default();
    let mut depth = 0;
    while let Some(n) = cur {
        let parent = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
        if depth >= 8 || parent.is_none() || matches!(n.as_str(), "Standard" | "Normal") {
            break;
        }
        if let Some(own) = auto.para_text.get(&n).or_else(|| named.para_text.get(&n)) {
            style.bold |= own.bold;
            if !style.upright {
                style.italic |= own.italic;
            }
            if !style.italic {
                style.upright |= own.upright;
            }
            style.caps |= own.caps;
            style.font_size_hp = style.font_size_hp.or(own.font_size_hp);
            style.color = style.color.clone().or_else(|| own.color.clone());
            style.font_family = style.font_family.clone().or_else(|| own.font_family.clone());
        }
        cur = parent;
        depth += 1;
    }
    style
}

pub(super) fn read_heading_styles(styles: &str) -> Vec<RunStyle> {
    let defs = parse_auto_styles(styles);
    let names = heading_style_names(styles);
    let look = |n: u8| -> Option<RunStyle> {
        let mut chain = Vec::new();
        let mut name = names[usize::from(n) - 1].clone();
        while chain.len() < 8 {
            let own = defs.para_text.get(&name);
            let parent = defs.para_parent.get(&name).cloned();
            if own.is_none() && parent.is_none() {
                break;
            }
            chain.push(own.cloned().unwrap_or_default());
            match parent {
                Some(p) => name = p,
                None => break,
            }
        }
        (!chain.is_empty()).then(|| {
            chain.into_iter().rev().fold(RunStyle::default(), |base, own| RunStyle {
                bold: base.bold || own.bold,
                italic: base.italic || own.italic,
                font_size_hp: own.font_size_hp.or(base.font_size_hp),
                color: own.color.or(base.color),
                font_family: own.font_family.or(base.font_family),
                ..Default::default()
            })
        })
    };
    if !names.iter().any(|n| defs.para_text.contains_key(n)) {
        return Vec::new();
    }
    (1..=6).map(|n| look(n).unwrap_or_default()).collect()
}

/// Each heading level's paragraph style: Writer's own `Heading_20_N`, else
/// the paragraph style declaring that outline level
/// (`style:default-outline-level`), as a Word conversion names them
/// ("Heading2", "Heading 2").
pub(super) fn heading_style_names(styles: &str) -> [String; 6] {
    let mut by_level: [Option<String>; 6] = Default::default();
    let mut reader = Reader::from_str(styles);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == "style:style" => {
                let level = attr_val(&e, "style:default-outline-level").and_then(|l| l.parse::<usize>().ok()).filter(|l| (1..=6).contains(l));
                if let (Some(l), Some(name), Some("paragraph")) = (level, attr_val(&e, "style:name"), attr_val(&e, "style:family").as_deref()) {
                    by_level[l - 1].get_or_insert(name);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    std::array::from_fn(|i| {
        let own = format!("Heading_20_{}", i + 1);
        if styles.contains(&format!("style:name=\"{own}\"")) { own } else { by_level[i].clone().unwrap_or(own) }
    })
}

/// A paragraph's named style, and whether its spacing gives way beside a
/// paragraph of the same style (`style:contextual-spacing`, the nearest
/// style in its chain that says): an automatic style is its parent's.
/// Writer keeps the flag in one setting with the space above and below
/// (`inherited_spacing`), so a style that states its space above or below
/// and not the flag settles it off: the W1 notes' bullets are automatic
/// styles giving 8pt below under a contextual List Paragraph, and Writer
/// leaves the 8pt between them.
pub(super) fn contextual_spacing(name: &str, auto: &AutoStyles, named: &AutoStyles) -> (String, bool) {
    let is_auto = |n: &str| auto.para.contains_key(n) || auto.para_parent.contains_key(n) || auto.para_text.contains_key(n);
    let mut style = None;
    let mut on = None;
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let Some(n) = cur else { break };
        if style.is_none() && !is_auto(&n) {
            style = Some(n.clone());
        }
        if on.is_none() {
            on = auto.contextual.get(&n).or_else(|| named.contextual.get(&n)).copied();
        }
        if on.is_none() && auto.para.get(&n).or_else(|| named.para.get(&n)).is_some_and(|st| st.sets[0] || st.sets[1]) {
            on = Some(false);
        }
        cur = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
    }
    (style.unwrap_or_else(|| name.to_string()), on.unwrap_or(false))
}

/// A span's text style over the text style around it: what the span
/// does not set, it takes from its paragraph (or outer span).
/// A text style's look: an automatic style's or a named one's (styles.xml's
/// "Responses", blue), over what its parents give it.
/// Whether a paragraph style's text is kerned: the nearest
/// `style:letter-kerning` up its chain, then the default style's. Absent
/// everywhere, Writer draws it unkerned.
pub(super) fn para_kerned(name: &str, auto: &AutoStyles, named: &AutoStyles) -> bool {
    let own = |n: &str| auto.kerning.get(n).or_else(|| named.kerning.get(n)).copied();
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let Some(n) = cur else { break };
        if let Some(k) = own(&n) {
            return k;
        }
        cur = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
    }
    own(DEFAULT_PARA).unwrap_or(false)
}

/// A text style's own `style:letter-kerning`, the nearest up its chain;
/// None leaves its paragraph's.
pub(super) fn text_kerning(name: &str, auto: &AutoStyles, named: &AutoStyles) -> Option<bool> {
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let n = cur?;
        if let Some(&k) = auto.kerning.get(&n).or_else(|| named.kerning.get(&n)) {
            return Some(k);
        }
        cur = auto.text_parent.get(&n).or_else(|| named.text_parent.get(&n)).cloned();
    }
    None
}

/// A text style's own `style:text-position`, the nearest up its chain:
/// None where no style sets one; else the shift it makes (None for super-
/// and subscript).
pub(super) fn text_shift(name: &str, auto: &AutoStyles, named: &AutoStyles) -> Option<Option<f64>> {
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let n = cur?;
        if let Some(&s) = auto.text_shift.get(&n).or_else(|| named.text_shift.get(&n)) {
            return Some(s);
        }
        cur = auto.text_parent.get(&n).or_else(|| named.text_parent.get(&n)).cloned();
    }
    None
}

/// A paragraph style's text shift as a percentage of its font size: the
/// nearest `style:text-position` up its chain, then the default style's.
pub(super) fn para_shift(name: Option<&str>, auto: &AutoStyles, named: &AutoStyles) -> Option<f64> {
    let own = |n: &str| auto.para_shift.get(n).or_else(|| named.para_shift.get(n)).copied();
    let mut cur = name.map(str::to_string);
    for _ in 0..16 {
        let Some(n) = cur else { break };
        if let Some(s) = own(&n) {
            return s;
        }
        cur = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
    }
    own(DEFAULT_PARA).flatten()
}

/// The shift of a `style:text-position` value of the form `"<n>% [100%]"`,
/// in percent of the font size. Super, sub and a value that also resizes
/// the text (`"33% 58%"`) are not one.
pub(super) fn parse_text_position(v: &str) -> Option<f64> {
    let percent = |t: &str| t.strip_suffix('%')?.parse::<f64>().ok().filter(|n| n.is_finite());
    let mut parts = v.split_whitespace();
    let shift = percent(parts.next()?)?;
    match parts.next() {
        None => {}
        Some(size) if percent(size) == Some(100.0) => {}
        Some(_) => return None,
    }
    parts.next().is_none().then_some(shift)
}

pub(super) fn text_style(name: &str, auto: &AutoStyles, named: &AutoStyles) -> RunStyle {
    let mut chain = Vec::new();
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let Some(n) = cur else { break };
        if let Some(st) = auto.text.get(&n).or_else(|| named.text.get(&n)) {
            chain.push(st.clone());
        }
        cur = auto.text_parent.get(&n).or_else(|| named.text_parent.get(&n)).cloned();
    }
    chain.into_iter().rev().fold(None, |outer: Option<RunStyle>, own| Some(inherit(outer.as_ref(), own))).unwrap_or_default()
}

pub(super) fn inherit(outer: Option<&RunStyle>, own: RunStyle) -> RunStyle {
    let Some(outer) = outer else { return own };
    RunStyle {
        bold: own.bold || (outer.bold && !own.regular),
        regular: own.regular || (outer.regular && !own.bold),
        italic: own.italic || (outer.italic && !own.upright),
        upright: own.upright || (outer.upright && !own.italic),
        caps: own.caps || outer.caps,
        underline: own.underline || outer.underline,
        strikethrough: own.strikethrough || outer.strikethrough,
        highlight: own.highlight || outer.highlight,
        font_family: own.font_family.or_else(|| outer.font_family.clone()),
        font_size_hp: own.font_size_hp.or(outer.font_size_hp),
        color: own.color.or_else(|| outer.color.clone()),
        vert_align: own.vert_align.or(outer.vert_align),
        ..own
    }
}
