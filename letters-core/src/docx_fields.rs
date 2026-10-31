// SPDX-License-Identifier: GPL-3.0-or-later
//
// docx_fields.rs — page-number fields in .docx headers and footers.
//
// The model's header and footer are text templates in which "{page}" and
// "{total}" stand for the page number and page count; the layout engine
// fills them per page (`layout::page_field_text`). In .docx they are Word
// fields: PAGE and NUMPAGES, simple (`w:fldSimple`) or complex
// (`w:fldChar` begin / `w:instrText` / separate / result / end). rdocx
// reads a header's text with each field's cached result ("1"), which put
// the same number on every page, and wrote "{page}" as literal text.

/// "{page}" / "{total}" for a field instruction, if it is one of those.
fn placeholder(instr: &str) -> Option<&'static str> {
    match instr.split_whitespace().next()?.to_ascii_uppercase().as_str() {
        "PAGE" => Some("{page}"),
        "NUMPAGES" | "SECTIONPAGES" => Some("{total}"),
        _ => None,
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The attribute `attr` of the tag starting at `xml[at..]`.
fn attr(xml: &str, at: usize, attr: &str) -> Option<String> {
    let tag = &xml[at..at + xml[at..].find('>')?];
    let key = format!("{attr}=\"");
    let v = tag.find(&key)? + key.len();
    Some(unescape(&tag[v..v + tag[v..].find('"')?]))
}

/// A text box's text: its paragraphs' runs, joined by '\n'.
fn box_text(xml: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut i = 0;
    while let Some(off) = xml[i..].find('<') {
        let at = i + off;
        let rest = &xml[at..];
        let tag_end = at + rest.find('>').map_or(rest.len(), |e| e + 1);
        if rest.starts_with("<w:p>") || rest.starts_with("<w:p ") {
            lines.push(String::new());
        } else if (rest.starts_with("<w:t>") || rest.starts_with("<w:t ")) && !xml[at..tag_end].ends_with("/>") {
            let close = xml[tag_end..].find("</w:t>").map_or(tag_end, |c| tag_end + c);
            if lines.is_empty() {
                lines.push(String::new());
            }
            lines.last_mut().unwrap().push_str(&unescape(&xml[tag_end..close]));
            i = close;
            continue;
        }
        i = tag_end;
    }
    lines.retain(|l| !l.trim().is_empty());
    lines.join("\n")
}

/// A header or footer part's text as a template: paragraphs joined by
/// '\n', PAGE and NUMPAGES fields as "{page}" and "{total}", their cached
/// results dropped. `None` when the part holds no such field (rdocx's own
/// reading is then used as it is).
pub fn template(part_xml: &str) -> Option<String> {
    let (text, found, _) = walk(part_xml);
    found.then_some(text)
}

/// A header or footer part's text as `template` reads it, whether or not
/// it holds a page field: a section's own header or footer, which rdocx
/// reads only for the document's last section.
pub(crate) fn story_text(part_xml: &str) -> String {
    walk(part_xml).0
}

/// A header or footer part's text as `story_text` reads it, with each of
/// its lines' alignment, look and tab stops (`styles_xml` for what their
/// paragraph styles give them).
pub(crate) fn story(part_xml: &str, styles_xml: &str) -> (String, Vec<crate::model::HfLine>) {
    let (text, _, starts) = walk(part_xml);
    let lines = starts
        .into_iter()
        .map(|at| {
            let mut line = line_of(part_xml, at, styles_xml);
            // A text box's line sits across the page as the box does, and
            // floats where the box is on the page.
            if let Some(a) = box_alignment(part_xml, at) {
                line.alignment = a;
            }
            line.float = box_float(part_xml, at);
            line
        })
        .collect();
    (text, lines)
}

/// The `wp:anchor` element the text box whose paragraph starts at
/// `part_xml[at..]` is in; `None` for a paragraph not in a box.
fn box_anchor(part_xml: &str, at: usize) -> Option<&str> {
    let before = &part_xml[..at];
    let anchor = before.match_indices("<wp:anchor").map(|(a, _)| a).filter(|&a| matches!(before.as_bytes().get(a + 10), Some(b' ' | b'>'))).last()?;
    if before[anchor..].contains("</wp:anchor>") || !before[anchor..].contains("<w:txbxContent>") {
        return None;
    }
    let end = part_xml[at..].find("</wp:anchor>").map_or(part_xml.len(), |e| at + e);
    Some(&part_xml[anchor..end])
}

/// Where the text box whose paragraph starts at `part_xml[at..]` floats:
/// set against the page's top or bottom edge (`wp:positionV` from the
/// page, by alignment or offset), its text inset by the box's
/// `tIns`/`bIns`. `None` for a box placed against anything else.
fn box_float(part_xml: &str, at: usize) -> Option<crate::model::HfFloat> {
    use crate::model::HfFloat;
    let anchor = box_anchor(part_xml, at)?;
    let v_at = anchor.find("<wp:positionV")?;
    let v = &anchor[v_at..v_at + anchor[v_at..].find("</wp:positionV>")?];
    if attr(v, 0, "relativeFrom").as_deref() != Some("page") {
        return None;
    }
    let inner = |tag: &str| {
        let open = format!("<{tag}>");
        let start = v.find(&open)? + open.len();
        Some(v[start..start + v[start..].find('<')?].trim().to_string())
    };
    // The box's insets, Word's 0.05in where it gives none.
    let body = anchor.find("<wps:bodyPr").map_or("", |b| &anchor[b..]);
    let inset = |name: &str| attr(body, 0, name).and_then(|v| v.parse::<f64>().ok()).unwrap_or(45_720.0) / 12_700.0;
    match inner("wp:align").as_deref() {
        Some("top") => Some(HfFloat::Top(inset("tIns"))),
        Some("bottom") => Some(HfFloat::Bottom(inset("bIns"))),
        Some(_) => None,
        None => inner("wp:posOffset").and_then(|o| o.parse::<f64>().ok()).map(|o| HfFloat::Top(o / 12_700.0 + inset("tIns"))),
    }
}

/// How the text box whose paragraph starts at `part_xml[at..]` is
/// aligned across the page or margins (`wp:positionH`'s `wp:align`);
/// `None` for a paragraph not in a box, or a box placed by offset.
fn box_alignment(part_xml: &str, at: usize) -> Option<crate::model::Alignment> {
    use crate::model::Alignment;
    let h = box_anchor(part_xml, at)?;
    let h = &h[h.find("<wp:positionH")?..];
    let h = &h[..h.find("</wp:positionH>")?];
    let v = &h[h.find("<wp:align>")? + "<wp:align>".len()..];
    match &v[..v.find('<')?] {
        "center" => Some(Alignment::Center),
        "right" | "outside" => Some(Alignment::Right),
        "left" | "inside" => Some(Alignment::Left),
        _ => None,
    }
}

/// The walk behind `template`, `story_text` and `story`: the part's text,
/// whether it held a PAGE or NUMPAGES field, and where in `part_xml` the
/// paragraph each line of the text is starts.
fn walk(part_xml: &str) -> (String, bool, Vec<usize>) {
    let mut out = String::new();
    let mut found = false;
    let mut starts: Vec<usize> = Vec::new();
    let mut paragraphs = 0;
    // Where the current paragraph's text starts in `out`.
    let mut para_start = 0;
    // Inside a complex field: collecting its instruction until `separate`,
    // then skipping its result until `end`.
    let mut complex: Option<(String, bool)> = None;
    let mut i = 0;
    while let Some(off) = part_xml[i..].find('<') {
        let at = i + off;
        let rest = &part_xml[at..];
        let name_end = rest.find([' ', '>', '/']).unwrap_or(rest.len());
        let name = &rest[1..name_end];
        let tag_end = at + rest.find('>').map_or(rest.len(), |e| e + 1);
        match name {
            // A text box floats over the header or footer. The part cannot
            // float it, so its text becomes a line of its own ahead of the
            // paragraph that anchors it, rather than running into that
            // paragraph's text; a fallback is a second copy of it.
            "w:txbxContent" | "mc:Fallback" if !rest[..rest.find('>').unwrap_or(0)].ends_with('/') => {
                let close = format!("</{name}>");
                let inner_end = part_xml[tag_end..].find(&close).map_or(part_xml.len(), |c| tag_end + c);
                if name == "w:txbxContent" {
                    let text = box_text(&part_xml[tag_end..inner_end]);
                    if !text.is_empty() {
                        out.insert_str(para_start, &format!("{text}\n"));
                        para_start += text.len() + 1;
                        // Its lines ahead of the paragraph that anchors it.
                        let own = paragraph_spans(&part_xml[tag_end..inner_end])
                            .into_iter()
                            .filter(|r| !box_text(&part_xml[tag_end + r.start..tag_end + r.end]).is_empty())
                            .map(|r| tag_end + r.start);
                        let at = starts.len().saturating_sub(1);
                        starts.splice(at..at, own);
                    }
                }
                i = (inner_end + close.len()).min(part_xml.len());
                continue;
            }
            "w:p" => {
                if paragraphs > 0 {
                    out.push('\n');
                }
                paragraphs += 1;
                para_start = out.len();
                starts.push(at);
            }
            "w:fldSimple" => {
                let instr = attr(part_xml, at, "w:instr").unwrap_or_default();
                if let Some(p) = placeholder(&instr) {
                    found = true;
                    out.push_str(p);
                    // Skip the cached result.
                    if !rest[..rest.find('>').unwrap_or(0)].ends_with('/') {
                        let close = part_xml[tag_end..].find("</w:fldSimple>").map_or(part_xml.len(), |c| tag_end + c + "</w:fldSimple>".len());
                        i = close;
                        continue;
                    }
                }
            }
            "w:fldChar" => match attr(part_xml, at, "w:fldCharType").as_deref() {
                Some("begin") => complex = Some((String::new(), false)),
                Some("separate") => {
                    if let Some((instr, in_result)) = complex.as_mut() {
                        if let Some(p) = placeholder(instr) {
                            found = true;
                            out.push_str(p);
                            *in_result = true;
                        } else {
                            // Not ours: show its result as text.
                            complex = None;
                        }
                    }
                }
                Some("end") => complex = None,
                _ => {}
            },
            "w:instrText" => {
                let close = part_xml[tag_end..].find("</w:instrText>").map_or(tag_end, |c| tag_end + c);
                if let Some((instr, _)) = complex.as_mut() {
                    instr.push_str(&unescape(&part_xml[tag_end..close]));
                }
                i = close;
                continue;
            }
            "w:t" if !rest[..rest.find('>').unwrap_or(0)].ends_with('/') => {
                let close = part_xml[tag_end..].find("</w:t>").map_or(tag_end, |c| tag_end + c);
                if !complex.as_ref().is_some_and(|c| c.1) {
                    out.push_str(&unescape(&part_xml[tag_end..close]));
                }
                i = close;
                continue;
            }
            // A run's tab; a tab stop's definition has a position.
            "w:tab" if attr(part_xml, at, "w:pos").is_none() => out.push('\t'),
            _ => {}
        }
        i = tag_end;
    }
    (out, found, starts)
}

/// Turn "{page}" and "{total}" in a written header or footer part's text
/// into PAGE and NUMPAGES fields.
pub fn fields(part_xml: &str) -> String {
    let field = |instr: &str| {
        format!(
            "</w:t></w:r><w:fldSimple w:instr=\" {instr} \"><w:r><w:t>1</w:t></w:r></w:fldSimple><w:r><w:t xml:space=\"preserve\">"
        )
    };
    part_xml
        .replace("<w:t>", "<w:t xml:space=\"preserve\">")
        .replace("{page}", &field("PAGE"))
        .replace("{total}", &field("NUMPAGES"))
}

/// The package paths of the default header and footer parts
/// ("word/header1.xml"): the first section's references, resolved
/// through the document's relationships. A document of one section has
/// one set of section properties, so this is its header and footer; in a
/// document of several the first section's are the document's own
/// (`Document::header`, its pictures and alignment).
pub fn default_parts(document_xml: &str, rels_xml: &str) -> (Option<String>, Option<String>) {
    let Some(first) = section_properties(document_xml).into_iter().next() else { return (None, None) };
    let sect = &document_xml[first];
    let part = |element: &str| default_reference(sect, element).and_then(|id| relationship_target(rels_xml, &id));
    (part("w:headerReference"), part("w:footerReference"))
}

/// The package paths of the first page's own header and footer parts,
/// when the first section gives its first page its own (`w:titlePg`):
/// `None` for a document whose first page has the others' header and
/// footer; a part of `None` within is a first page without one.
pub fn first_page_parts(document_xml: &str, rels_xml: &str) -> Option<(Option<String>, Option<String>)> {
    let first = section_properties(document_xml).into_iter().next()?;
    let sect = &document_xml[first];
    let at = sect.find("<w:titlePg").filter(|&at| matches!(sect.as_bytes().get(at + 10), Some(b' ' | b'/' | b'>')))?;
    if matches!(attr(sect, at, "w:val").as_deref(), Some("0" | "false" | "off")) {
        return None;
    }
    let part = |element: &str| typed_reference(sect, element, "first").and_then(|id| relationship_target(rels_xml, &id));
    Some((part("w:headerReference"), part("w:footerReference")))
}

/// Where each `w:sectPr` element of `document_xml` is, in document order
/// (the sections' own: paragraph-level ones, then the body's last). The
/// old value a `w:sectPrChange` holds is inside its section's element and
/// is not one of them.
pub(crate) fn section_properties(document_xml: &str) -> Vec<std::ops::Range<usize>> {
    const CLOSE: &str = "</w:sectPr>";
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(off) = document_xml[from..].find("<w:sectPr") {
        let at = from + off;
        // `<w:sectPrChange` is not a section's element.
        if !matches!(document_xml.as_bytes().get(at + 9), Some(b' ' | b'>' | b'/')) {
            from = at + 9;
            continue;
        }
        let Some(open_end) = document_xml[at..].find('>').map(|e| at + e + 1) else { break };
        if document_xml[..open_end].ends_with("/>") {
            found.push(at..open_end);
            from = open_end;
            continue;
        }
        // A change's own `w:sectPr` closes first: the section's element
        // closes after the change does.
        let first_close = document_xml[open_end..].find(CLOSE).map(|c| open_end + c);
        let change = document_xml[open_end..]
            .find("<w:sectPrChange")
            .map(|c| open_end + c)
            .filter(|&c| first_close.is_some_and(|close| c < close));
        let search = change.and_then(|c| document_xml[c..].find("</w:sectPrChange>").map(|e| c + e)).unwrap_or(open_end);
        let Some(close) = document_xml[search..].find(CLOSE).map(|c| search + c + CLOSE.len()) else { break };
        found.push(at..close);
        from = close;
    }
    found
}

/// The relationship id of the `default` reference `element`
/// (`w:headerReference`, `w:footerReference`) in a section's properties.
pub(crate) fn default_reference(sect_xml: &str, element: &str) -> Option<String> {
    typed_reference(sect_xml, element, "default")
}

/// The relationship id of the reference `element` of type `kind`
/// ("default", "first", "even") in a section's properties.
fn typed_reference(sect_xml: &str, element: &str, kind: &str) -> Option<String> {
    let mut from = 0;
    while let Some(off) = sect_xml[from..].find(&format!("<{element} ")) {
        let at = from + off;
        if attr(sect_xml, at, "w:type").as_deref() == Some(kind) {
            return attr(sect_xml, at, "r:id");
        }
        from = at + 1;
    }
    None
}

/// The package path of the part relationship `id` of
/// `word/document.xml` names ("word/header1.xml").
pub(crate) fn relationship_target(rels_xml: &str, id: &str) -> Option<String> {
    let mut from = 0;
    while let Some(off) = rels_xml[from..].find("<Relationship ") {
        let at = from + off;
        if attr(rels_xml, at, "Id").as_deref() == Some(id) {
            let target = attr(rels_xml, at, "Target")?;
            return Some(match target.strip_prefix('/') {
                Some(abs) => abs.to_string(),
                None => format!("word/{target}"),
            });
        }
        from = at + 1;
    }
    None
}

/// How a header or footer part's text is aligned: its first paragraph
/// with text (a text box's is not the part's), by its own `w:jc` or its
/// paragraph style's (`styles_xml`, through the styles it is based on).
pub fn alignment(part_xml: &str, styles_xml: &str) -> crate::model::Alignment {
    use crate::model::Alignment;
    let jc = |v: &str| match v {
        "center" => Some(Alignment::Center),
        "right" | "end" => Some(Alignment::Right),
        "both" | "distribute" => Some(Alignment::Justify),
        "left" | "start" => Some(Alignment::Left),
        _ => None,
    };
    // The text boxes and fallbacks out, as `template` leaves them out.
    let xml = without_boxes(part_xml);
    let mut from = 0;
    while let Some(off) = xml[from..].find("<w:p") {
        let at = from + off;
        from = at + 4;
        if !matches!(xml.as_bytes().get(at + 4), Some(b' ' | b'>')) {
            continue;
        }
        let end = xml[at..].find("</w:p>").map_or(xml.len(), |e| at + e);
        let para = &xml[at..end];
        let has_text = para.match_indices("<w:t").any(|(t, _)| {
            let tag_end = para[t..].find('>').map_or(para.len(), |e| t + e + 1);
            matches!(para.as_bytes().get(t + 4), Some(b' ' | b'>')) && para[tag_end..].find('<').is_some_and(|c| !para[tag_end..tag_end + c].trim().is_empty())
        });
        if !has_text {
            continue;
        }
        let ppr = para.find("<w:pPr>").map_or("", |p| &para[p..p + para[p..].find("</w:pPr>").unwrap_or(0)]);
        if let Some(a) = ppr.find("<w:jc ").and_then(|j| attr(ppr, j, "w:val")).and_then(|v| jc(&v)) {
            return a;
        }
        let style = ppr.find("<w:pStyle ").and_then(|p| attr(ppr, p, "w:val"));
        return style_chain(styles_xml, style).iter().find_map(|def| def.find("<w:jc ").and_then(|j| attr(def, j, "w:val")).and_then(|v| jc(&v))).unwrap_or(Alignment::Left);
    }
    Alignment::Left
}

/// A part with its text boxes' contents and its fallbacks (a second copy
/// of a text box) taken out.
fn without_boxes(part_xml: &str) -> String {
    let mut xml = String::with_capacity(part_xml.len());
    let mut rest = part_xml;
    while let Some((at, open)) = ["<w:txbxContent>", "<mc:Fallback>"].iter().filter_map(|t| rest.find(t).map(|a| (a, *t))).min() {
        xml.push_str(&rest[..at]);
        let close = format!("</{}", &open[1..]);
        rest = rest[at..].find(&close).map_or("", |c| &rest[at + c..]);
    }
    xml.push_str(rest);
    xml
}

/// The definitions of paragraph style `id` and the styles it is based on,
/// its own first.
fn style_chain(styles_xml: &str, id: Option<String>) -> Vec<&str> {
    let mut chain = Vec::new();
    let mut style = id;
    // A loop of styles based on each other ends.
    for _ in 0..8 {
        let Some(id) = style.take() else { break };
        let Some(s_at) = styles_xml.find(&format!("w:styleId=\"{id}\"")) else { break };
        let s_end = styles_xml[s_at..].find("</w:style>").map_or(styles_xml.len(), |e| s_at + e);
        let def = &styles_xml[s_at..s_end];
        chain.push(def);
        style = def.find("<w:basedOn ").and_then(|b| attr(def, b, "w:val"));
    }
    chain
}

/// Where each paragraph of `xml` is, its own and not those of a text box
/// inside it, in order.
fn paragraph_spans(xml: &str) -> Vec<std::ops::Range<usize>> {
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(off) = xml[from..].find("<w:p") {
        let at = from + off;
        if !matches!(xml.as_bytes().get(at + 4), Some(b' ' | b'>' | b'/')) {
            from = at + 4;
            continue;
        }
        let end = paragraph_end(xml, at);
        spans.push(at..end);
        from = end;
    }
    spans
}

/// Where the paragraph starting at `xml[at..]` ends, past the paragraphs
/// of any text box in it.
fn paragraph_end(xml: &str, at: usize) -> usize {
    let open_end = xml[at..].find('>').map_or(xml.len(), |e| at + e + 1);
    if xml[..open_end].ends_with("/>") {
        return open_end;
    }
    let mut depth = 1;
    let mut i = open_end;
    while let Some(off) = xml[i..].find("<w:p") .into_iter().chain(xml[i..].find("</w:p>")).min() {
        let t = i + off;
        if xml[t..].starts_with("</w:p>") {
            depth -= 1;
            i = t + 6;
            if depth == 0 {
                return i;
            }
        } else {
            let tag_end = xml[t..].find('>').map_or(xml.len(), |e| t + e + 1);
            if matches!(xml.as_bytes().get(t + 4), Some(b' ' | b'>')) && !xml[..tag_end].ends_with("/>") {
                depth += 1;
            }
            i = tag_end;
        }
    }
    xml.len()
}

/// The line the paragraph starting at `part_xml[at..]` makes: its
/// alignment and tab stops, by its own properties or its style's, and the
/// look of its first text, by the run's own properties over its style's.
fn line_of(part_xml: &str, at: usize, styles_xml: &str) -> crate::model::HfLine {
    use crate::model::{Alignment, HfLine, RunStyle, TabAlign, TabLeader, TabStop};
    let para = without_boxes(&part_xml[at..paragraph_end(part_xml, at)]);
    let ppr = para.find("<w:pPr>").map_or("", |p| &para[p..p + para[p..].find("</w:pPr>").unwrap_or(0)]);
    // The paragraph mark's own run properties are not its text's.
    let ppr_own = ppr.find("<w:rPr>").map_or(ppr, |r| &ppr[..r]);
    let chain = style_chain(styles_xml, ppr.find("<w:pStyle ").and_then(|p| attr(ppr, p, "w:val")));
    let alignment = std::iter::once(ppr_own)
        .chain(chain.iter().copied())
        .find_map(|x| {
            x.find("<w:jc ").and_then(|j| attr(x, j, "w:val")).and_then(|v| match v.as_str() {
                "center" => Some(Alignment::Center),
                "right" | "end" => Some(Alignment::Right),
                "both" | "distribute" => Some(Alignment::Justify),
                "left" | "start" => Some(Alignment::Left),
                _ => None,
            })
        })
        .unwrap_or_default();
    // A paragraph's stops are its style's, the most basic first, with
    // its own added and those it clears taken away, as Word sets them.
    let mut tab_stops: Vec<TabStop> = Vec::new();
    for x in chain.iter().rev().copied().chain(std::iter::once(ppr_own)) {
        let Some(open) = x.find("<w:tabs>") else { continue };
        let tabs = &x[open..open + x[open..].find("</w:tabs>").unwrap_or(0)];
        for (t, _) in tabs.match_indices("<w:tab ") {
            let Some(pos_pt) = attr(tabs, t, "w:pos").and_then(|p| p.parse::<f64>().ok()).map(|p| p / 20.0) else { continue };
            tab_stops.retain(|s| (s.pos_pt - pos_pt).abs() > 0.5);
            let align = match attr(tabs, t, "w:val").as_deref() {
                Some("right" | "end") => TabAlign::Right,
                Some("center") => TabAlign::Center,
                Some("decimal") => TabAlign::Decimal,
                Some("left" | "start") => TabAlign::Left,
                _ => continue,
            };
            let leader = match attr(tabs, t, "w:leader").as_deref() {
                Some("dot") => TabLeader::Dot,
                Some("hyphen") => TabLeader::Hyphen,
                Some("underscore") => TabLeader::Underscore,
                _ => TabLeader::None,
            };
            tab_stops.push(TabStop { pos_pt, align, leader });
        }
    }
    tab_stops.sort_by(|a, b| a.pos_pt.total_cmp(&b.pos_pt));
    let mut look = RunStyle::default();
    // The style's run properties, the most basic first.
    for def in chain.iter().rev() {
        if let Some(r) = def.find("<w:rPr>") {
            apply_run_properties(&mut look, &def[r..r + def[r..].find("</w:rPr>").unwrap_or(0)]);
        }
    }
    let first_text = para.match_indices("<w:r>").chain(para.match_indices("<w:r ")).map(|(r, _)| r).filter(|&r| {
        let run = &para[r..r + para[r..].find("</w:r>").unwrap_or(0)];
        run.find("<w:t").is_some_and(|t| matches!(run.as_bytes().get(t + 4), Some(b' ' | b'>')))
    });
    if let Some(r) = first_text.min() {
        let run = &para[r..r + para[r..].find("</w:r>").unwrap_or(0)];
        if let Some(p) = run.find("<w:rPr>") {
            apply_run_properties(&mut look, &run[p..p + run[p..].find("</w:rPr>").unwrap_or(0)]);
        }
    }
    let chain = if ppr.contains("<w:pStyle ") { chain } else { style_chain(styles_xml, default_paragraph_style(styles_xml)) };
    let (gap_above, gap_below) = (border_gap(ppr_own, &chain, "top"), border_gap(ppr_own, &chain, "bottom"));
    let (space_before_pt, space_after_pt, line_height) = line_spacing(ppr_own, chain, styles_xml);
    HfLine { alignment, look, tab_stops, space_before_pt, space_after_pt, pad_above_pt: gap_above, pad_below_pt: gap_below, line_height, ..Default::default() }
}

/// How far a header or footer paragraph's `side` border (`w:pBdr`) stands
/// off its text, in points, by its own properties or its style's: the
/// border's gap and, when it is drawn, its width. Writer keeps the gap of
/// an invisible (`none`) border as padding: the questionnaire's Normal
/// style pads every side 31pt that way, and its empty footer stands 62pt
/// taller than its one line, pushing the page's text up with it.
fn border_gap(ppr_own: &str, chain: &[&str], side: &str) -> f64 {
    for x in std::iter::once(ppr_own).chain(chain.iter().copied()) {
        let Some(open) = x.find("<w:pBdr>") else { continue };
        let pbdr = &x[open..open + x[open..].find("</w:pBdr>").unwrap_or(0)];
        let Some(e) = pbdr.find(&format!("<w:{side} ")) else { continue };
        let val = attr(pbdr, e, "w:val").unwrap_or_default();
        if val == "nil" {
            return 0.0;
        }
        let space = attr(pbdr, e, "w:space").and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0).clamp(0.0, 31.0);
        let width = if val == "none" { 0.0 } else { attr(pbdr, e, "w:sz").and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(4.0).max(0.0) / 8.0 };
        return space + width;
    }
    0.0
}

/// A header or footer paragraph's space before and after, in points, and
/// its line height when it is exact or a minimum: each by the nearest of
/// its own properties (`ppr_own`), its style's (`chain`, the default
/// paragraph style's for a paragraph that names none) and the document's
/// defaults that sets it, as Word resolves them.
fn line_spacing(ppr_own: &str, chain: Vec<&str>, styles_xml: &str) -> (f64, f64, Option<crate::model::LineHeight>) {
    use crate::model::LineHeight;
    let defaults = styles_xml.find("<w:pPrDefault>").map_or("", |d| &styles_xml[d..d + styles_xml[d..].find("</w:pPrDefault>").unwrap_or(0)]);
    let sources: Vec<&str> = std::iter::once(ppr_own).chain(chain).chain(std::iter::once(defaults)).collect();
    let set = |name: &str| sources.iter().find_map(|x| x.find("<w:spacing ").and_then(|at| Some((attr(x, at, name)?, attr(x, at, "w:lineRule")))));
    let points = |name: &str| set(name).and_then(|(v, _)| v.parse::<f64>().ok()).map_or(0.0, |tw| tw / 20.0);
    let line_height = set("w:line").and_then(|(v, rule)| {
        let pt = v.parse::<f64>().ok()? / 20.0;
        match rule.as_deref() {
            Some("exact") => Some(LineHeight::Exact(pt)),
            Some("atLeast") => Some(LineHeight::Minimum(pt)),
            _ => None,
        }
    });
    (points("w:before"), points("w:after"), line_height)
}

/// The id of the document's default paragraph style ("Normal").
fn default_paragraph_style(styles_xml: &str) -> Option<String> {
    styles_xml.match_indices("<w:style ").map(|(at, _)| at).find_map(|at| {
        (attr(styles_xml, at, "w:type").as_deref() == Some("paragraph") && matches!(attr(styles_xml, at, "w:default").as_deref(), Some("1" | "true"))).then(|| attr(styles_xml, at, "w:styleId")).flatten()
    })
}

/// Run properties (`w:rPr`'s contents) over `look`: weight, slant, size,
/// colour and font.
fn apply_run_properties(look: &mut crate::model::RunStyle, rpr: &str) {
    let on = |tag: &str| {
        let open = format!("<{tag}");
        rpr.match_indices(&open).find(|(t, _)| matches!(rpr.as_bytes().get(t + open.len()), Some(b' ' | b'/' | b'>'))).map(|(t, _)| {
            !matches!(attr(rpr, t, "w:val").as_deref(), Some("0" | "false" | "off" | "none"))
        })
    };
    if let Some(b) = on("w:b") {
        look.bold = b;
    }
    if let Some(i) = on("w:i") {
        look.italic = i;
    }
    if let Some(sz) = rpr.find("<w:sz ").and_then(|t| attr(rpr, t, "w:val")).and_then(|v| v.parse().ok()) {
        look.font_size_hp = Some(sz);
    }
    if let Some(c) = rpr.find("<w:color ").and_then(|t| attr(rpr, t, "w:val")).filter(|c| c.len() == 6 && c.chars().all(|ch| ch.is_ascii_hexdigit())) {
        look.color = Some(c.to_lowercase());
    }
    if let Some(f) = rpr.find("<w:rFonts ").and_then(|t| attr(rpr, t, "w:ascii")) {
        look.font_family = Some(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_and_complex_page_fields_read_as_placeholders() {
        let simple = "<w:hdr><w:p><w:r><w:t xml:space=\"preserve\">Page </w:t></w:r>\
                      <w:fldSimple w:instr=\" PAGE \"><w:r><w:t>1</w:t></w:r></w:fldSimple>\
                      <w:r><w:t xml:space=\"preserve\"> of </w:t></w:r>\
                      <w:fldSimple w:instr=\"NUMPAGES \\* MERGEFORMAT\"><w:r><w:t>3</w:t></w:r></w:fldSimple></w:p></w:hdr>";
        assert_eq!(template(simple).as_deref(), Some("Page {page} of {total}"));
        let complex = "<w:ftr><w:p><w:r><w:t xml:space=\"preserve\">Draft, page </w:t></w:r>\
                       <w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> PAGE </w:instrText></w:r>\
                       <w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>7</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p></w:ftr>";
        assert_eq!(template(complex).as_deref(), Some("Draft, page {page}"));
        // A date field is not ours: its result is kept as text, and a part
        // without page fields is left to rdocx.
        let other = "<w:hdr><w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText>DATE</w:instrText></w:r>\
                     <w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>today</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p></w:hdr>";
        assert_eq!(template(other), None);
    }

    /// Each line of a part has its own alignment, look and tab stops: a
    /// text box's line sits across the page as its box does, a line's look
    /// is its run's over its style's, and its stops its style's with its
    /// own added and cleared; a tab stop's definition is not a tab.
    #[test]
    fn a_parts_lines_each_have_their_own_alignment_look_and_stops() {
        use crate::model::{Alignment, TabAlign};
        let styles = r#"<w:styles><w:style w:type="paragraph" w:styleId="Footer"><w:basedOn w:val="Base"/>
            <w:pPr><w:tabs><w:tab w:val="center" w:pos="4513"/><w:tab w:val="right" w:pos="9026"/></w:tabs></w:pPr></w:style>
            <w:style w:type="paragraph" w:styleId="Base"><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial"/><w:sz w:val="18"/></w:rPr></w:style></w:styles>"#;
        let part = r#"<w:ftr><w:p><w:pPr><w:jc w:val="right"/></w:pPr><w:r><mc:AlternateContent><mc:Choice><w:drawing><wp:anchor><wp:positionH relativeFrom="page"><wp:align>center</wp:align></wp:positionH>
            <wps:txbx><w:txbxContent><w:p><w:r><w:rPr><w:sz w:val="24"/></w:rPr><w:t>OFFICIAL</w:t></w:r></w:p></w:txbxContent></wps:txbx></wp:anchor></w:drawing></mc:Choice></mc:AlternateContent></w:r>
            <w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p>
            <w:p><w:pPr><w:pStyle w:val="Footer"/><w:tabs><w:tab w:val="clear" w:pos="4513"/><w:tab w:val="right" w:pos="9638"/></w:tabs><w:rPr><w:sz w:val="40"/></w:rPr></w:pPr>
            <w:r><w:rPr><w:b/><w:color w:val="1F4E79"/></w:rPr><w:t>Ship</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>ID</w:t></w:r></w:p></w:ftr>"#;
        let (text, lines) = story(part, styles);
        assert_eq!(text, "OFFICIAL\n{page}\nShip\tID");
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0].alignment, lines[0].look.font_size_hp), (Alignment::Center, Some(24)), "the box's line, centred as its box");
        assert_eq!(lines[1].alignment, Alignment::Right);
        let ship = &lines[2];
        assert_eq!(ship.alignment, Alignment::Left);
        assert_eq!(
            (ship.look.font_family.as_deref(), ship.look.font_size_hp, ship.look.bold, ship.look.color.as_deref()),
            (Some("Arial"), Some(18), true, Some("1f4e79")),
            "its run over its style, not its paragraph mark"
        );
        let stops: Vec<(f64, TabAlign)> = ship.tab_stops.iter().map(|t| (t.pos_pt, t.align)).collect();
        assert_eq!(stops, [(451.3, TabAlign::Right), (481.9, TabAlign::Right)]);
    }

    /// A text box set against the page's top or foot floats there, its
    /// text inset by the box's own insets (Word's 0.05in where it gives
    /// none) or placed by its offset; a box against the paragraph does not.
    #[test]
    fn a_text_box_against_the_page_floats() {
        use crate::model::HfFloat;
        let part = |v: &str, body: &str| {
            format!(
                r#"<w:hdr><w:p><w:r><mc:AlternateContent><mc:Choice><w:drawing><wp:anchor distT="0"><wp:positionH relativeFrom="page"><wp:align>center</wp:align></wp:positionH>{v}
                <wps:txbx><w:txbxContent><w:p><w:r><w:t>OFFICIAL</w:t></w:r></w:p></w:txbxContent></wps:txbx>{body}</wp:anchor></w:drawing></mc:Choice></mc:AlternateContent></w:r><w:r><w:t>Title</w:t></w:r></w:p></w:hdr>"#
            )
        };
        let float = |v: &str, body: &str| story(&part(v, body), "").1.first().and_then(|l| l.float);
        let page = |inner: &str| format!(r#"<wp:positionV relativeFrom="page">{inner}</wp:positionV>"#);
        assert_eq!(float(&page("<wp:align>top</wp:align>"), r#"<wps:bodyPr tIns="190500" bIns="0"/>"#), Some(HfFloat::Top(15.0)));
        assert_eq!(float(&page("<wp:align>bottom</wp:align>"), r#"<wps:bodyPr tIns="0" bIns="190500"/>"#), Some(HfFloat::Bottom(15.0)));
        assert_eq!(float(&page("<wp:posOffset>127000</wp:posOffset>"), "<wps:bodyPr/>"), Some(HfFloat::Top(10.0 + 3.6)));
        assert_eq!(float(r#"<wp:positionV relativeFrom="paragraph"><wp:align>top</wp:align></wp:positionV>"#, "<wps:bodyPr/>"), None);
        assert_eq!(story(&part(&page("<wp:align>top</wp:align>"), "<wps:bodyPr/>"), "").1[1].float, None, "the anchoring line does not");
    }

    /// A header's alignment is its first text paragraph's, by its own
    /// `w:jc` or its style's; an empty paragraph or a text box's text
    /// before it is not that paragraph.
    #[test]
    fn a_parts_alignment_is_its_first_text_paragraphs() {
        use crate::model::Alignment;
        let styles = r#"<w:styles><w:style w:type="paragraph" w:styleId="Footer"><w:basedOn w:val="Base"/></w:style>
            <w:style w:type="paragraph" w:styleId="Base"><w:pPr><w:jc w:val="center"/></w:pPr></w:style></w:styles>"#;
        let own = r#"<w:hdr><w:p/><w:p><w:r><mc:AlternateContent><mc:Choice><w:drawing><w:txbxContent><w:p><w:pPr><w:jc w:val="left"/></w:pPr><w:r><w:t>OFFICIAL</w:t></w:r></w:p></w:txbxContent></w:drawing></mc:Choice></mc:AlternateContent></w:r></w:p>
            <w:p><w:pPr><w:jc w:val="right"/></w:pPr><w:r><w:t>FINANCE BILL</w:t></w:r></w:p></w:hdr>"#;
        assert_eq!(alignment(own, styles), Alignment::Right);
        let styled = r#"<w:ftr><w:p><w:pPr><w:pStyle w:val="Footer"/></w:pPr><w:r><w:t>OFFICIAL</w:t></w:r></w:p></w:ftr>"#;
        assert_eq!(alignment(styled, styles), Alignment::Center, "through the style it is based on");
        assert_eq!(alignment("<w:hdr><w:p><w:r><w:t>Report</w:t></w:r></w:p></w:hdr>", styles), Alignment::Left);
    }

    /// The crime-supervisor form's footer: an "OFFICIAL" text box (and its
    /// VML fallback, a second copy) in the paragraph with the page number.
    /// Its text is a line of its own, once, ahead of the page number: it
    /// used to come out as "OFFICIAL\nOFFICIAL{page}", and then not at all.
    #[test]
    fn a_text_box_is_a_line_of_its_own_and_its_fallback_is_dropped() {
        let footer = "<w:ftr><w:p><w:r><mc:AlternateContent><mc:Choice Requires=\"wps\"><w:drawing><wp:anchor><wps:txbx>\
                      <w:txbxContent><w:p><w:r><w:t>OFFICIAL</w:t></w:r></w:p></w:txbxContent></wps:txbx></wp:anchor></w:drawing></mc:Choice>\
                      <mc:Fallback><w:pict><v:textbox><w:txbxContent><w:p><w:r><w:t>OFFICIAL</w:t></w:r></w:p></w:txbxContent></v:textbox></w:pict></mc:Fallback>\
                      </mc:AlternateContent></w:r><w:fldSimple w:instr=\" PAGE \"><w:r><w:t>2</w:t></w:r></w:fldSimple></w:p>\
                      <w:p><w:r><w:t>October 2025</w:t></w:r></w:p></w:ftr>";
        assert_eq!(template(footer).as_deref(), Some("OFFICIAL\n{page}\nOctober 2025"));
    }

    #[test]
    fn placeholders_are_written_as_fields_and_read_back() {
        let written = fields("<w:hdr><w:p><w:r><w:t>Page {page} of {total}</w:t></w:r></w:p></w:hdr>");
        assert!(written.contains("<w:fldSimple w:instr=\" PAGE \">"), "{written}");
        assert!(!written.contains("{page}"));
        assert_eq!(template(&written).as_deref(), Some("Page {page} of {total}"));
    }

    /// A part without page fields is no template, but still a section's
    /// text: an empty paragraph is an explicitly empty header.
    #[test]
    fn story_text_reads_a_part_with_or_without_fields() {
        assert_eq!(story_text("<w:hdr><w:p><w:r><w:t>Appendix</w:t></w:r></w:p><w:p><w:r><w:t>Draft</w:t></w:r></w:p></w:hdr>"), "Appendix\nDraft");
        assert_eq!(story_text("<w:ftr><w:p></w:p></w:ftr>"), "");
        let fielded = fields("<w:hdr><w:p><w:r><w:t>Appendix {page}</w:t></w:r></w:p></w:hdr>");
        assert_eq!(story_text(&fielded), "Appendix {page}");
    }

    /// Each section's properties, in order, a change's old value inside
    /// its own; the default parts are the first section's.
    #[test]
    fn default_parts_are_the_first_sections() {
        let document = r#"<w:body><w:p><w:pPr><w:sectPr><w:headerReference w:type="even" r:id="rId9"/><w:headerReference w:type="default" r:id="rId1"/><w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:pPr></w:p>
            <w:p><w:pPr><w:sectPr/></w:pPr></w:p>
            <w:p/><w:sectPr><w:headerReference w:type="default" r:id="rId2"/><w:footerReference w:type="default" r:id="rId3"/><w:sectPrChange w:id="1"><w:sectPr><w:pgSz w:w="1"/></w:sectPr></w:sectPrChange></w:sectPr></w:body>"#;
        let spans = section_properties(document);
        assert_eq!(spans.len(), 3);
        assert!(document[spans[1].clone()].starts_with("<w:sectPr/>"));
        assert!(document[spans[2].clone()].ends_with("</w:sectPrChange></w:sectPr>"));
        assert_eq!(default_reference(&document[spans[0].clone()], "w:headerReference").as_deref(), Some("rId1"));
        let rels = r#"<Relationships><Relationship Id="rId1" Type="h" Target="header1.xml"/><Relationship Id="rId2" Type="h" Target="/word/header2.xml"/><Relationship Id="rId3" Type="f" Target="footer1.xml"/></Relationships>"#;
        assert_eq!(default_parts(document, rels), (Some("word/header1.xml".to_string()), None));
        assert_eq!(relationship_target(rels, "rId2").as_deref(), Some("word/header2.xml"));
    }
}
