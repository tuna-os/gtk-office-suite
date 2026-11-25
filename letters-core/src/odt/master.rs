// SPDX-License-Identifier: GPL-3.0-or-later
//! Header/footer and master-page/section resolution on the read path.

#![allow(unused_imports)]
use super::*;

/// The master page LibreOffice gives the first page: the one the first
/// paragraph or table's style names (`style:master-page-name`), else
/// "Standard", else the first one declared.
/// A header's or footer's text as its paragraphs, one a line, each in
/// `style`; the pictures' frames sit in the first.
pub(super) fn hf_paragraphs(text: &str, style: &str, lines: &[(String, Option<String>)], pics: &str) -> String {
    // A floating line's frame goes in the paragraph of the next line that
    // is not (or one of its own after the last), where a reader finds it
    // ahead of that line.
    let mut out = String::new();
    let mut frames = String::new();
    let mut first = true;
    for (k, line) in text.split('\n').enumerate() {
        let (style, frame) = lines.get(k).map_or((style, None), |(s, f)| (s.as_str(), f.as_deref()));
        let content = page_fields(&odf_spaces(&esc(line)));
        if let Some(frame) = frame {
            frames.push_str(&format!("{frame}<text:p{style}>{content}</text:p></draw:text-box></draw:frame>"));
            continue;
        }
        let pics = if std::mem::take(&mut first) { pics } else { "" };
        out.push_str(&format!("<text:p{style}>{pics}{}{content}</text:p>", std::mem::take(&mut frames)));
    }
    if !frames.is_empty() || first {
        out.push_str(&format!("<text:p>{}{frames}</text:p>", if first { pics } else { "" }));
    }
    out
}

/// Text of a header or footer added to its line; a later cell of a table
/// row (`gap`) follows the line's text after a space.
pub(super) fn append_hf(target: &mut Option<String>, txt: &str, gap: bool) {
    let t = target.get_or_insert_with(String::new);
    if gap && !t.is_empty() && !t.ends_with('\n') {
        t.push(' ');
    }
    t.push_str(txt);
}

/// Whether a header's or footer's text so far holds nothing to show.
pub(super) fn blank(text: &Option<String>) -> bool {
    text.as_deref().is_none_or(|t| t.trim().is_empty())
}

/// The line a header's or footer's paragraph of style `style` starts: its
/// alignment and tab stops, and the look of its text, through the styles
/// it is based on (`named`: styles.xml's, its automatic ones among them).
pub(super) fn hf_line(style: Option<&str>, named: &AutoStyles) -> crate::model::HfLine {
    let Some(name) = style else { return Default::default() };
    let mut line = crate::model::HfLine { look: RunStyle { kerned: para_kerned(name, named, named), ..para_text_style(name, named, named) }, ..Default::default() };
    let (mut aligned, mut tabbed) = (false, false);
    let mut cur = Some(name.to_string());
    for _ in 0..8 {
        let Some(n) = cur else { break };
        if let Some(st) = named.para.get(&n) {
            if !aligned && st.alignment != Alignment::Left {
                (line.alignment, aligned) = (st.alignment, true);
            }
            if !tabbed && !st.tab_stops_pt.is_empty() {
                (line.tab_stops, tabbed) = (st.tab_stops_pt.clone(), true);
            }
        }
        cur = named.para_parent.get(&n).cloned();
    }
    let (before, after, _) = inherited_spacing(name, named, named);
    (line.space_before_pt, line.space_after_pt) = (before.unwrap_or(0.0), after.unwrap_or(0.0));
    line.line_height = inherited_para(name, named, named).line_height;
    line
}

/// A text box of the document's own header or footer being read (a
/// classification label): its lines, each with its text, look and whether
/// that look has its first text's yet, and where the box floats. Its lines
/// go ahead of the line of the paragraph that anchors it.
pub(super) struct HfBox {
    pub(super) lines: Vec<(String, crate::model::HfLine, bool)>,
    pub(super) float: Option<crate::model::HfFloat>,
    pub(super) across: Option<Alignment>,
}

/// Text of a header's or footer's text box added to its last line (a
/// line of its own if it has none yet); its first text gives the line its
/// span's look.
pub(super) fn hf_box_text(b: &mut HfBox, txt: &str, spans: &[Option<String>], named: &AutoStyles) {
    if b.lines.is_empty() {
        b.lines.push((String::new(), Default::default(), false));
    }
    let (text, line, styled) = b.lines.last_mut().expect("a line");
    if !*styled && !txt.trim().is_empty() {
        *styled = true;
        if let Some(span) = spans.iter().rev().flatten().next() {
            hf_span_look(&mut line.look, span, named);
        }
    }
    text.push_str(txt);
}

/// A header's or footer's text box read: its lines with text go ahead of
/// the line being read (the paragraph that anchors the box), floating
/// where the box does and across the page as it sits. A box that does not
/// float against the page has its text on that line.
pub(super) fn finish_hf_box(b: HfBox, text: &mut Option<String>, lines: &mut Vec<crate::model::HfLine>) {
    let shown: Vec<(String, crate::model::HfLine)> = b
        .lines
        .into_iter()
        .filter(|(t, ..)| !t.trim().is_empty())
        .map(|(t, mut line, _)| {
            line.float = b.float;
            if let Some(a) = b.across {
                line.alignment = a;
            }
            (t, line)
        })
        .collect();
    if shown.is_empty() {
        return;
    }
    let t = text.get_or_insert_with(String::new);
    // A box placed against its paragraph sits over that paragraph's line:
    // its text is that line's.
    if b.float.is_none() {
        t.extend(shown.iter().map(|(s, _)| s.as_str()));
        return;
    }
    let at = t.rfind('\n').map_or(0, |i| i + 1);
    let joined: String = shown.iter().map(|(s, _)| format!("{s}\n")).collect();
    t.insert_str(at, &joined);
    let k = lines.len().saturating_sub(1);
    lines.splice(k..k, shown.into_iter().map(|(_, l)| l));
}

/// Where a header's or footer's text box of graphic style `style` (at
/// `svg:y` `y`) floats and how it sits across the page: set against the
/// page's top or bottom, its text inset by the style's padding; `None`
/// for a box placed against anything else.
pub(super) fn hf_box_place(style: Option<&str>, y: Option<&str>, named: &AutoStyles) -> (Option<crate::model::HfFloat>, Option<Alignment>) {
    use crate::model::{AnchorAlign, AnchorFrame, HfFloat};
    let Some(place) = style.and_then(|n| named.graphic.get(n)) else { return (None, None) };
    let (top, bottom) = style.and_then(|n| named.graphic_padding.get(n)).copied().unwrap_or((0.0, 0.0));
    let float = match (place.v_from, place.v_align) {
        (AnchorFrame::Page, Some(AnchorAlign::Start)) => Some(HfFloat::Top(top)),
        (AnchorFrame::Page, Some(AnchorAlign::End)) => Some(HfFloat::Bottom(bottom)),
        (AnchorFrame::Page, None) => y.and_then(parse_length_pt).map(|y| HfFloat::Top(y + top)),
        _ => None,
    };
    let across = match (place.h_from, place.h_align) {
        (AnchorFrame::Page | AnchorFrame::Margin, Some(AnchorAlign::Center)) => Some(Alignment::Center),
        (AnchorFrame::Page | AnchorFrame::Margin, Some(AnchorAlign::End)) => Some(Alignment::Right),
        (AnchorFrame::Page | AnchorFrame::Margin, Some(AnchorAlign::Start)) => Some(Alignment::Left),
        _ => None,
    };
    (float, across)
}

/// Span style `name`'s own look over a header's or footer's line's: its
/// first text is drawn so.
pub(super) fn hf_span_look(look: &mut RunStyle, name: &str, named: &AutoStyles) {
    if let Some(face) = named.text_font_name.get(name) {
        look.font_family = Some(face.clone());
    }
    let Some(span) = named.text.get(name) else { return };
    look.bold |= span.bold;
    look.italic |= span.italic;
    if span.font_size_hp.is_some() {
        look.font_size_hp = span.font_size_hp;
    }
    if span.color.is_some() {
        look.color.clone_from(&span.color);
    }
    if span.font_family.is_some() {
        look.font_family.clone_from(&span.font_family);
    }
}

/// A paragraph of a header or footer starts a line of its text: of the
/// master page being read, and of the document's own when it is the
/// first page's (`header` or `footer`). Its first paragraph starts it.
pub(super) fn start_hf_line(master: Option<&mut MasterReading>, doc: &mut Document, any_header: bool, header: bool, footer: bool) {
    let line = |t: &mut Option<String>| match t {
        Some(s) => s.push('\n'),
        None => *t = Some(String::new()),
    };
    if let Some(m) = master {
        line(if any_header { &mut m.header } else { &mut m.footer });
    }
    if header {
        line(&mut doc.header);
    }
    if footer {
        line(&mut doc.footer);
    }
}

pub(super) fn first_master_page(content: &str, styles: &str) -> Option<String> {
    let mut named: std::collections::HashMap<String, String> = Default::default();
    let mut masters: Vec<String> = Vec::new();
    let mut first_style: Option<String> = None;
    for xml in [content, styles] {
        let mut reader = Reader::from_str(xml);
        let mut in_text = false;
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                    "style:style" => {
                        if let (Some(n), Some(m)) = (attr_val(&e, "style:name"), attr_val(&e, "style:master-page-name").filter(|m| !m.is_empty())) {
                            named.insert(n, m);
                        }
                    }
                    "style:master-page" => masters.extend(attr_val(&e, "style:name")),
                    "office:text" => in_text = true,
                    "text:p" | "text:h" | "table:table" if in_text && first_style.is_none() => {
                        first_style = Some(attr_val(&e, if e.name().as_ref() == "table:table" { "table:style-name" } else { "text:style-name" }).unwrap_or_default());
                    }
                    _ => {}
                },
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
    }
    first_style
        .and_then(|s| named.get(&s).cloned())
        .filter(|m| masters.contains(m))
        .or_else(|| masters.iter().find(|m| *m == "Standard").cloned())
        .or_else(|| masters.first().cloned())
}

/// styles.xml with the first page's master (`master`) made to read as
/// Writer lays it out when it names a different `style:next-style-name`:
/// the next master's header and footer run on the pages after the first,
/// and the first master's own become the first page's
/// (`style:header-first`/`style:footer-first`). A first master without a
/// header or footer gives a first page without one.
pub(super) fn follow_next_master(styles: &str, master: &str) -> Option<String> {
    #[derive(Default)]
    struct Master {
        next: Option<String>,
        content: Option<(usize, usize)>,
        // Its own header and footer elements, and whether it has a first
        // page's one already.
        header: Option<(usize, usize)>,
        footer: Option<(usize, usize)>,
        has_first: bool,
    }
    let mut masters: std::collections::HashMap<String, Master> = Default::default();
    let mut reader = Reader::from_str(styles);
    let mut cur: Option<String> = None;
    let mut content_start = 0;
    // The master's child element being read: its name, start and depth.
    let mut child: Option<(String, usize)> = None;
    let mut depth = 0usize;
    loop {
        let before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) if cur.is_none() && e.name().as_ref() == "style:master-page" => {
                let name = attr_val(&e, "style:name").unwrap_or_default();
                masters.insert(name.clone(), Master { next: attr_val(&e, "style:next-style-name"), ..Default::default() });
                cur = Some(name);
                content_start = reader.buffer_position() as usize;
                depth = 0;
            }
            Ok(Event::Empty(e)) if cur.is_none() && e.name().as_ref() == "style:master-page" => {
                let name = attr_val(&e, "style:name").unwrap_or_default();
                masters.insert(name, Master { next: attr_val(&e, "style:next-style-name"), ..Default::default() });
            }
            Ok(Event::End(e)) if depth == 0 && e.name().as_ref() == "style:master-page" => {
                if let Some(m) = cur.take().and_then(|n| masters.get_mut(&n)) {
                    m.content = Some((content_start, before));
                }
            }
            Ok(Event::Start(e)) if cur.is_some() => {
                if depth == 0 {
                    child = Some((e.name().as_ref().to_string(), before));
                }
                depth += 1;
            }
            Ok(Event::End(_)) if cur.is_some() => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some((name, start)) = child.take() {
                        let span = (start, reader.buffer_position() as usize);
                        let m = cur.as_ref().and_then(|n| masters.get_mut(n)).expect("the master being read");
                        match name.as_str() {
                            "style:header" => m.header = Some(span),
                            "style:footer" => m.footer = Some(span),
                            "style:header-first" | "style:footer-first" => m.has_first = true,
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::Empty(e)) if cur.is_some() && depth == 0 => {
                let span = (before, reader.buffer_position() as usize);
                let m = cur.as_ref().and_then(|n| masters.get_mut(n)).expect("the master being read");
                match e.name().as_ref() {
                    "style:header" => m.header = Some(span),
                    "style:footer" => m.footer = Some(span),
                    "style:header-first" | "style:footer-first" => m.has_first = true,
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let first = masters.get(master)?;
    let next = masters.get(first.next.as_deref().filter(|n| *n != master)?)?;
    let (start, end) = first.content?;
    if first.has_first {
        return None;
    }
    let part = |span: Option<(usize, usize)>| span.map_or("", |(s, e)| &styles[s..e]);
    // The first master's own part, renamed to the first page's.
    // One the first master lacks is a first page without, where the
    // pages after have one.
    let as_first = |span: Option<(usize, usize)>, after: Option<(usize, usize)>, tag: &str| -> String {
        match span {
            None if after.is_none() => String::new(),
            None => format!("<style:{tag}-first/>"),
            Some((s, e)) => {
                let text = &styles[s..e];
                let open = format!("<style:{tag}");
                let mut out = format!("<style:{tag}-first{}", &text[open.len()..]);
                let close = format!("</style:{tag}>");
                if out.ends_with(&close) {
                    out.truncate(out.len() - close.len());
                    out.push_str(&format!("</style:{tag}-first>"));
                }
                out
            }
        }
    };
    let mut merged = String::with_capacity(styles.len());
    merged.push_str(&styles[..start]);
    merged.push_str(part(next.header));
    merged.push_str(part(next.footer));
    merged.push_str(&as_first(first.header, next.header, "header"));
    merged.push_str(&as_first(first.footer, next.footer, "footer"));
    merged.push_str(&styles[end..]);
    Some(merged)
}

/// The master page a body block whose style is `name` starts a new page
/// on (LET-005S): the style's `style:master-page-name`, else the one a
/// parent style names. None where no style in the chain names one, or the
/// nearest names "" (no new master).
pub(super) fn block_master(name: &str, auto: &AutoStyles, named: &AutoStyles) -> Option<String> {
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let n = cur?;
        if let Some(m) = auto.master.get(&n).or_else(|| named.master.get(&n)) {
            return Some(m.clone()).filter(|m| !m.is_empty());
        }
        cur = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
    }
    None
}

/// A header's or footer's text, pictures, alignment and lines as the
/// reader has them on the document.
pub(super) type HfParts = (Option<String>, Vec<Run>, Alignment, Vec<HfLine>);

/// Take the header's (`header`) or footer's parts off the document.
pub(super) fn take_hf(doc: &mut Document, header: bool) -> HfParts {
    if header {
        (doc.header.take(), std::mem::take(&mut doc.header_pictures), std::mem::take(&mut doc.header_alignment), std::mem::take(&mut doc.header_lines))
    } else {
        (doc.footer.take(), std::mem::take(&mut doc.footer_pictures), std::mem::take(&mut doc.footer_alignment), std::mem::take(&mut doc.footer_lines))
    }
}

/// Put the header's (`header`) or footer's parts back on the document.
pub(super) fn put_hf(doc: &mut Document, header: bool, (text, pictures, alignment, lines): HfParts) {
    if header {
        (doc.header, doc.header_pictures, doc.header_alignment, doc.header_lines) = (text, pictures, alignment, lines);
    } else {
        (doc.footer, doc.footer_pictures, doc.footer_alignment, doc.footer_lines) = (text, pictures, alignment, lines);
    }
}

/// A master page of styles.xml as sections need it: its page layout's
/// name, its header's and footer's text (`{page}`/`{total}` for the
/// fields) and whether each holds a picture.
#[derive(Default)]
pub(super) struct MasterReading {
    pub(super) layout: Option<String>,
    /// Whether the master page has a header and a footer shown: only then
    /// does its page layout's header or footer area take room.
    pub(super) has_header: bool,
    pub(super) has_footer: bool,
    pub(super) header: Option<String>,
    pub(super) footer: Option<String>,
    pub(super) header_pictures: bool,
    pub(super) footer_pictures: bool,
}

/// The sections a body's master page switches make (LET-005S): `switches`
/// are (first model paragraph, master named) of each top-level block whose
/// style names a master page, in order. The first page's master (`first`)
/// starts section 0, whose page is `doc.page` and whose header and footer
/// are the document's; each switch to another declared master starts a
/// section on that master's page layout. A switch to the master already
/// in use is a page break (`page_break_before`), not a section. A
/// section's header or footer is its master's text ("" for none) where it
/// differs from the section before's, else None (continued). Empty when
/// the body never changes master.
pub(super) fn master_sections(
    doc: &mut Document,
    switches: &[(usize, String)],
    first: Option<&str>,
    masters: &std::collections::HashMap<String, MasterReading>,
    pages: &std::collections::HashMap<String, PageGeometry>,
) -> Vec<DocSection> {
    let Some(first) = first else { return Vec::new() };
    let mut starts: Vec<(usize, &str)> = vec![(0, first)];
    for (at, name) in switches {
        if *at == 0 || !masters.contains_key(name) {
            continue;
        }
        if starts.last().is_some_and(|(_, m)| *m == name.as_str()) {
            if let Some(p) = doc.paragraphs.get_mut(*at) {
                p.style.page_break_before = true;
            }
        } else {
            starts.push((*at, name.as_str()));
        }
    }
    if starts.len() < 2 {
        return Vec::new();
    }
    // What a master's pages show in its header (or footer): its text, and
    // whether it has pictures (which only the first page's master's keep).
    let story_of = |name: &str, header: bool| -> (String, bool) {
        let m = masters.get(name);
        let text = m.and_then(|m| if header { m.header.clone() } else { m.footer.clone() }).unwrap_or_default();
        (text, m.is_some_and(|m| if header { m.header_pictures } else { m.footer_pictures }))
    };
    let story = |k: usize, header: bool| -> Option<String> {
        let own = story_of(starts[k].1, header);
        (k > 0 && own != story_of(starts[k - 1].1, header)).then_some(own.0)
    };
    let first_page = doc.page.unwrap_or_default();
    let total = doc.paragraphs.len();
    starts
        .iter()
        .enumerate()
        .map(|(k, &(start, name))| DocSection {
            id: k as u32,
            range: ParagraphRange::new(start, starts.get(k + 1).map_or(total, |s| s.0)),
            geometry: if k == 0 {
                first_page
            } else {
                pages.get(name).copied().unwrap_or_default()
            },
            header: story(k, true),
            footer: story(k, false),
        })
        .collect()
}

/// Column layout declared on a `text:section` rather than on the page.
///
/// ODF allows either, and the two live in different parts: a page-wide
/// layout carries `style:columns` inside `style:page-layout-properties`
/// in styles.xml, which is what this writer emits. LibreOffice, reading
/// a docx `w:cols`, models it as a section instead and writes
/// `style:section-properties` into content.xml — so a reader that only
/// looks at the page layout sees a two-column document as one column.
/// Nothing is lost in that conversion; it is simply recorded elsewhere.
///
/// Returns the first section's count and gap, the gap only when stated.
pub(super) fn parse_section_columns(xml: &str) -> Option<(u8, Option<f64>)> {
    let mut reader = Reader::from_str(xml);
    let mut in_section_props = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) if e.name().as_ref() == "style:section-properties" => {
                in_section_props = true;
            }
            Ok(Event::End(e)) if e.name().as_ref() == "style:section-properties" => {
                in_section_props = false;
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if in_section_props && e.name().as_ref() == "style:columns" =>
            {
                let count = attr_val(&e, "fo:column-count").and_then(|v| v.parse::<u8>().ok())?;
                if count <= 1 {
                    return None;
                }
                let gap = attr_val(&e, "fo:column-gap").and_then(|v| parse_length_pt(&v));
                return Some((count, gap));
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

/// Each section style's columns, as `parse_section_columns` reads the
/// first: `text:section` style name → count (more than one) and gap, the
/// gap only when stated.
pub(super) fn section_style_columns(xml: &str) -> std::collections::HashMap<String, (u8, Option<f64>)> {
    let mut out = std::collections::HashMap::new();
    let mut reader = Reader::from_str(xml);
    let mut style: Option<String> = None;
    let mut in_section_props = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) if e.name().as_ref() == "style:style" => style = attr_val(&e, "style:name"),
            Ok(Event::End(e)) if e.name().as_ref() == "style:style" => style = None,
            Ok(Event::Start(e)) if e.name().as_ref() == "style:section-properties" => in_section_props = true,
            Ok(Event::End(e)) if e.name().as_ref() == "style:section-properties" => in_section_props = false,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if in_section_props && e.name().as_ref() == "style:columns" => {
                let count = attr_val(&e, "fo:column-count").and_then(|v| v.parse::<u8>().ok()).filter(|c| *c > 1);
                if let (Some(name), Some(count)) = (style.clone(), count) {
                    out.insert(name, (count, attr_val(&e, "fo:column-gap").and_then(|v| parse_length_pt(&v))));
                }
            }
            Ok(Event::Eof) | Err(_) => return out,
            _ => {}
        }
    }
}
