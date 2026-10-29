// SPDX-License-Identifier: GPL-3.0-or-later
//
// docx_text_boxes.rs — text boxes in a .docx body (#1719).
//
// A Word text box is a drawing shape (`wps:wsp`) holding `wps:txbx`, in an
// `mc:AlternateContent` whose fallback is a VML copy (`v:textbox`). rdocx
// reads neither, so the box and its text were lost. As with smart chips
// (docx_chips):
// - **Reading:** `unwrap` turns each text box into a sentinel in its run
//   before rdocx sees the file, and `restore` makes those sentinels text
//   box runs.
// - **Writing:** the writer puts a sentinel run where each box goes, then
//   `wrap` turns each sentinel into the drawing.
//
// A sentinel is private-use characters (not the chips' own) around an index
// into the list of boxes.

use crate::docx_anchors::{attr, frame, placement, position, wrap_element, xml_attr};
use crate::model::{BoxRule, Document, ImageAnchor, Run, RunStyle, TextBox};

const OPEN: char = '\u{E002}';
const CLOSE: char = '\u{E003}';

/// Word's own text box insets: 0.1in left and right, 0.05in top and bottom.
const INSETS: [u32; 4] = [45_720, 91_440, 45_720, 91_440];

/// The sentinel standing for text box `index` in the written text.
pub fn sentinel(index: usize) -> String {
    format!("{OPEN}{index}{CLOSE}")
}

/// Whether `text` is nothing but text box sentinels: the run of a
/// paragraph whose only content floats.
pub fn is_sentinel(text: &str) -> bool {
    text.starts_with(OPEN) && text.ends_with(CLOSE) && text.chars().all(|c| c == OPEN || c == CLOSE || c.is_ascii_digit())
}

/// The end of the element that opens at `xml[start..]` (`<name ` or
/// `<name>`), past its close tag, counting elements of the same name nested
/// in it.
fn element_end(xml: &str, start: usize, name: &str) -> Option<usize> {
    let close = format!("</{name}>");
    let opens = [format!("<{name}>"), format!("<{name} ")];
    let open_tag_end = start + xml[start..].find('>')?;
    if xml[..open_tag_end].ends_with('/') {
        return Some(open_tag_end + 1);
    }
    let mut depth = 0usize;
    let mut at = open_tag_end;
    loop {
        let next_open = opens.iter().filter_map(|o| xml[at..].find(o.as_str()).map(|a| at + a)).min();
        let next_close = at + xml[at..].find(&close)?;
        match next_open {
            Some(o) if o < next_close => {
                // A self-closing one adds nothing to the depth.
                let end = o + xml[o..].find('>')?;
                if !xml[..end].ends_with('/') {
                    depth += 1;
                }
                at = end;
            }
            _ if depth == 0 => return Some(next_close + close.len()),
            _ => {
                depth -= 1;
                at = next_close + close.len();
            }
        }
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The text of a `w:txbxContent`: a line per paragraph, its runs' `w:t`,
/// tabs and breaks.
fn box_text(content: &str) -> String {
    let mut paragraphs = Vec::new();
    let mut rest = content;
    while let Some(at) = ["<w:p>", "<w:p "].iter().filter_map(|t| rest.find(t)).min() {
        let end = element_end(rest, at, "w:p").unwrap_or(rest.len());
        let para = &rest[at..end];
        rest = &rest[end..];
        let mut text = String::new();
        let mut r = para;
        while let Some(lt) = r.find('<') {
            let tag_end = r[lt..].find('>').map_or(r.len(), |e| lt + e + 1);
            let tag = &r[lt..tag_end];
            r = &r[tag_end..];
            if tag.starts_with("<w:t>") || tag.starts_with("<w:t ") {
                let len = r.find("</w:t>").unwrap_or(r.len());
                text.push_str(&unescape(&r[..len]));
                r = &r[len..];
            } else if tag.starts_with("<w:tab/") || tag.starts_with("<w:tab ") {
                text.push('\t');
            } else if tag.starts_with("<w:br/") || tag.starts_with("<w:br ") || tag.starts_with("<w:cr/") {
                text.push('\n');
            }
        }
        paragraphs.push(text);
    }
    paragraphs.join("\n")
}

/// A DrawingML colour, `RRGGBB`, from the first `a:srgbClr` in `xml`.
fn srgb(xml: &str) -> Option<String> {
    let c = attr(xml, "<a:srgbClr ", "val")?.to_uppercase();
    (c.len() == 6 && c.bytes().all(|b| b.is_ascii_hexdigit())).then_some(c)
}

/// The shape properties' fill and outline: a solid fill's colour, and an
/// outline's width and colour unless it is unfilled.
fn shape_look(sp_pr: &str) -> (Option<String>, Option<BoxRule>) {
    let (before_ln, ln) = match sp_pr.find("<a:ln") {
        Some(at) => {
            let end = element_end(sp_pr, at, "a:ln").unwrap_or(sp_pr.len());
            (&sp_pr[..at], Some(&sp_pr[at..end]))
        }
        None => (sp_pr, None),
    };
    let fill = before_ln.find("<a:solidFill>").and_then(|at| srgb(&before_ln[at..]));
    let rule = ln.filter(|ln| !ln.contains("<a:noFill")).map(|ln| BoxRule {
        // A line that does not say its width is Word's default 0.75pt.
        width_emu: attr(ln, "<a:ln", "w").and_then(|w| w.parse().ok()).unwrap_or(9525),
        color: ln.find("<a:solidFill>").and_then(|at| srgb(&ln[at..])).unwrap_or_else(|| "000000".into()),
    });
    (fill, rule)
}

/// A VML length in EMU ("1.5pt", "0.1in", "2mm"; a bare number is EMU).
fn vml_length(v: &str) -> Option<u32> {
    let v = v.trim();
    let (num, per) = [("pt", 12_700.0), ("in", 914_400.0), ("mm", 36_000.0), ("cm", 360_000.0), ("px", 9_525.0)]
        .iter()
        .find_map(|(u, k)| v.strip_suffix(u).map(|n| (n, *k)))
        .unwrap_or((v, 1.0));
    let n: f64 = num.trim().parse().ok()?;
    (n.is_finite() && n >= 0.0).then(|| (n * per).round() as u32)
}

/// The text box in `el` (a `w:drawing`, `w:pict` or `mc:AlternateContent`
/// holding one), with where it is placed: a DrawingML shape's own, or the
/// VML copy's when that is all there is.
fn text_box(el: &str) -> Option<(TextBox, ImageAnchor)> {
    // An alternate's choice, not its fallback (the same box again).
    let el = match el.find("<mc:Fallback") {
        Some(at) if el[..at].contains("<w:txbxContent") => &el[..at],
        _ => el,
    };
    let Some(content_at) = el.find("<w:txbxContent") else { return rule_shape(el) };
    let content_end = element_end(el, content_at, "w:txbxContent")?;
    let text = box_text(&el[content_at..content_end]);
    if let Some(at) = el.find("<wps:wsp") {
        let shape = &el[at..];
        let size = |a| attr(el, "<wp:extent ", a).and_then(|v| v.parse::<u64>().ok());
        let extent_emu = size("cx").zip(size("cy")).unwrap_or((0, 0));
        let (fill, border) = match shape.find("<wps:spPr") {
            Some(sp) => shape_look(&shape[sp..element_end(shape, sp, "wps:spPr").unwrap_or(shape.len())]),
            None => (None, None),
        };
        let inset = |a, d: u32| attr(shape, "<wps:bodyPr", a).and_then(|v| v.parse().ok()).unwrap_or(d);
        let padding_emu = [inset("tIns", INSETS[0]), inset("rIns", INSETS[1]), inset("bIns", INSETS[2]), inset("lIns", INSETS[3])];
        let anchor = match el.find("<wp:anchor") {
            Some(a) => placement(&el[a..element_end(el, a, "wp:anchor").unwrap_or(el.len())]),
            None => ImageAnchor::default(),
        };
        return Some((TextBox { extent_emu, border, fill, padding_emu, text }, anchor));
    }
    // VML only: its size from its style, its look from its attributes; it
    // floats at its paragraph.
    let shape_at = ["<v:shape ", "<v:rect ", "<v:roundrect "].iter().filter_map(|t| el.find(t)).min()?;
    let shape = &el[shape_at..];
    let tag = &shape[..shape.find('>').map_or(shape.len(), |e| e + 1)];
    let get = |name: &str| attr(tag, "<v:", name);
    let style = get("style").unwrap_or("");
    let style_of = |key: &str| style.split(';').find_map(|kv| kv.split_once(':').filter(|(k, _)| k.trim() == key).and_then(|(_, v)| vml_length(v)));
    let extent_emu = (style_of("width").unwrap_or(0) as u64, style_of("height").unwrap_or(0) as u64);
    let off = |v: Option<&str>| v.is_some_and(|v| v == "f" || v == "false");
    let color = |v: Option<&str>| v.map(|c| c.trim_start_matches('#').to_uppercase()).filter(|c| c.len() == 6 && c.bytes().all(|b| b.is_ascii_hexdigit()));
    let fill = (!off(get("filled"))).then(|| color(get("fillcolor"))).flatten();
    let border = (!off(get("stroked"))).then(|| BoxRule {
        width_emu: get("strokeweight").and_then(vml_length).unwrap_or(9525),
        color: color(get("strokecolor")).unwrap_or_else(|| "000000".into()),
    });
    let padding_emu = attr(shape, "<v:textbox", "inset")
        .map(|v| {
            // left, top, right, bottom
            let mut parts = v.split(',').map(|p| vml_length(p).filter(|_| !p.trim().is_empty()));
            let (l, t, r, b) = (parts.next().flatten(), parts.next().flatten(), parts.next().flatten(), parts.next().flatten());
            [t.unwrap_or(INSETS[0]), r.unwrap_or(INSETS[1]), b.unwrap_or(INSETS[2]), l.unwrap_or(INSETS[3])]
        })
        .unwrap_or(INSETS);
    Some((TextBox { extent_emu, border, fill, padding_emu, text }, ImageAnchor::default()))
}

/// A straight line drawn across or down the page (`prst="line"`, or a
/// straight connector), as a box with no text, as long as the line and
/// none deep, ruled with the line's outline: a form's coloured rule under
/// its title. A slanted line, or one in a group, is not one.
fn rule_shape(el: &str) -> Option<(TextBox, ImageAnchor)> {
    let el = el.find("<mc:Fallback").map_or(el, |at| &el[..at]);
    if el.contains("<wpg:") || el.contains("<pic:pic") {
        return None;
    }
    let shape = &el[el.find("<wps:wsp")?..];
    let geometry = attr(shape, "<a:prstGeom ", "prst")?;
    if !matches!(geometry, "line" | "straightConnector1") {
        return None;
    }
    let size = |a| attr(el, "<wp:extent ", a).and_then(|v| v.parse::<u64>().ok());
    let extent_emu = size("cx").zip(size("cy"))?;
    if extent_emu.0 != 0 && extent_emu.1 != 0 {
        return None;
    }
    let sp = shape.find("<wps:spPr")?;
    let (_, border) = shape_look(&shape[sp..element_end(shape, sp, "wps:spPr").unwrap_or(shape.len())]);
    let a = el.find("<wp:anchor")?;
    let anchor = placement(&el[a..element_end(el, a, "wp:anchor").unwrap_or(el.len())]);
    Some((TextBox { extent_emu, border: Some(border?), fill: None, padding_emu: [0; 4], text: String::new() }, anchor))
}

/// Turn every text box in a body's runs into a sentinel in its run, and
/// return the boxes in order. A picture's drawing is left alone.
pub fn unwrap(document_xml: &str) -> (String, Vec<(TextBox, ImageAnchor)>) {
    let mut xml = document_xml.to_string();
    let mut boxes = Vec::new();
    let mut from = 0;
    loop {
        let found = ["mc:AlternateContent", "w:drawing", "w:pict"]
            .iter()
            .filter_map(|name| [format!("<{name}>"), format!("<{name} ")].iter().filter_map(|t| xml[from..].find(t.as_str())).min().map(|a| (from + a, *name)))
            .min_by_key(|(a, _)| *a);
        let Some((start, name)) = found else { break };
        let Some(end) = element_end(&xml, start, name) else { break };
        match text_box(&xml[start..end]) {
            Some(b) => {
                let mark = format!("<w:t>{}</w:t>", sentinel(boxes.len()));
                boxes.push(b);
                xml.replace_range(start..end, &mark);
                from = start + mark.len();
            }
            None => from = end,
        }
    }
    (xml, boxes)
}

/// Make the sentinels `unwrap` left in `doc` text box runs again.
pub fn restore(doc: &mut Document, boxes: &[(TextBox, ImageAnchor)]) {
    if boxes.is_empty() {
        return;
    }
    for p in &mut doc.paragraphs {
        if !p.runs.iter().any(|r| r.text.contains(OPEN)) {
            continue;
        }
        let mut runs = Vec::new();
        for run in p.runs.drain(..) {
            let mut rest = run.text.as_str();
            while let Some(a) = rest.find(OPEN) {
                let Some(b) = rest[a..].find(CLOSE).map(|b| a + b) else { break };
                if a > 0 {
                    runs.push(Run { text: rest[..a].to_string(), style: run.style.clone() });
                }
                match rest[a + OPEN.len_utf8()..b].parse::<usize>().ok().and_then(|i| boxes.get(i)) {
                    Some((tb, anchor)) => runs.push(Run {
                        text: String::new(),
                        // The size and font its run was given: a paragraph
                        // of nothing but floating boxes is a line of them.
                        style: RunStyle {
                            text_box: Some(Box::new(tb.clone())),
                            image_anchor: Some(*anchor),
                            font_size_hp: run.style.font_size_hp,
                            font_family: run.style.font_family.clone(),
                            ..Default::default()
                        },
                    }),
                    None => runs.push(Run { text: rest[a..b + CLOSE.len_utf8()].to_string(), style: run.style.clone() }),
                }
                rest = &rest[b + CLOSE.len_utf8()..];
            }
            if !rest.is_empty() {
                runs.push(Run { text: rest.to_string(), style: run.style.clone() });
            }
        }
        p.runs = runs;
    }
}

const WPS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// Text box `n` (its `docPr` id `id`) as Word writes one: a shape with its
/// fill, outline and insets, holding its text a paragraph a line, placed
/// as its anchor says. The namespaces it needs beyond the document's are
/// declared on it.
fn drawing(tb: &TextBox, anchor: &ImageAnchor, id: u32) -> String {
    let (w, h) = tb.extent_emu;
    let fill = match &tb.fill {
        Some(c) => format!("<a:solidFill><a:srgbClr val=\"{c}\"/></a:solidFill>"),
        None => "<a:noFill/>".into(),
    };
    let ln = match &tb.border {
        Some(b) => format!("<a:ln w=\"{}\"><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill></a:ln>", b.width_emu, b.color),
        None => "<a:ln><a:noFill/></a:ln>".into(),
    };
    let paras: String = tb
        .text
        .split('\n')
        .map(|line| {
            let runs: String = line
                .split('\t')
                .enumerate()
                .map(|(i, part)| {
                    let tab = if i > 0 { "<w:tab/>" } else { "" };
                    let t = if part.is_empty() { String::new() } else { format!("<w:t xml:space=\"preserve\">{}</w:t>", xml_attr(part)) };
                    format!("<w:r>{tab}{t}</w:r>")
                })
                .collect();
            format!("<w:p>{runs}</w:p>")
        })
        .collect();
    let [t, r, b, l] = tb.padding_emu;
    format!(
        "<mc:AlternateContent xmlns:mc=\"{MC}\"><mc:Choice xmlns:wps=\"{WPS}\" Requires=\"wps\"><w:drawing>\
         <wp:anchor distT=\"0\" distB=\"0\" distL=\"114300\" distR=\"114300\" simplePos=\"0\" relativeHeight=\"{id}\" behindDoc=\"{}\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
         <wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"{}\">{}</wp:positionH><wp:positionV relativeFrom=\"{}\">{}</wp:positionV>\
         <wp:extent cx=\"{w}\" cy=\"{h}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>{}<wp:docPr id=\"{id}\" name=\"Text Box {id}\"/><wp:cNvGraphicFramePr/>\
         <a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"{WPS}\"><wps:wsp><wps:cNvSpPr txBox=\"1\"/>\
         <wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>{fill}{ln}</wps:spPr>\
         <wps:txbx><w:txbxContent>{paras}</w:txbxContent></wps:txbx>\
         <wps:bodyPr rot=\"0\" vert=\"horz\" wrap=\"square\" lIns=\"{l}\" tIns=\"{t}\" rIns=\"{r}\" bIns=\"{b}\" anchor=\"t\" anchorCtr=\"0\"><a:noAutofit/></wps:bodyPr>\
         </wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice></mc:AlternateContent>",
        u8::from(anchor.behind),
        frame(anchor.h_from, "column"),
        position(anchor.x_emu, anchor.h_align, ["left", "center", "right"]),
        frame(anchor.v_from, "paragraph"),
        position(anchor.y_emu, anchor.v_align, ["top", "center", "bottom"]),
        wrap_element(anchor.wrap),
    )
}

/// Turn each sentinel run the writer left into its text box.
pub fn wrap(document_xml: &str, boxes: &[(TextBox, ImageAnchor)], first_id: u32) -> String {
    let mut xml = document_xml.to_string();
    for (i, (tb, anchor)) in boxes.iter().enumerate() {
        let mark = sentinel(i);
        let Some(at) = xml.find(&mark) else { continue };
        // The `w:t` around the sentinel, whatever its attributes.
        let Some(t_start) = xml[..at].rfind("<w:t") else { continue };
        let Some(t_end) = xml[at..].find("</w:t>").map(|e| at + e + "</w:t>".len()) else { continue };
        xml.replace_range(t_start..t_end, &drawing(tb, anchor, first_id + i as u32));
    }
    xml
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AnchorAlign, AnchorFrame};

    /// A text box as Word saves one: the shape in the choice, the VML copy
    /// in the fallback.
    const WORD_BOX: &str = r#"<w:p><w:r><w:t xml:space="preserve">Name </w:t></w:r><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="251659264" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="margin"><wp:align>right</wp:align></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>91440</wp:posOffset></wp:positionV><wp:extent cx="1828800" cy="457200"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapSquare wrapText="bothSides"/><wp:docPr id="1" name="Text Box 1"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp><wps:cNvSpPr txBox="1"/><wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1828800" cy="457200"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="deeaf6"/></a:solidFill><a:ln w="12700"><a:solidFill><a:srgbClr val="1F3864"/></a:solidFill></a:ln></wps:spPr><wps:txbx><w:txbxContent><w:p><w:r><w:t>Tom &amp; Ann</w:t></w:r><w:r><w:tab/><w:t>1</w:t></w:r></w:p><w:p><w:r><w:t>second</w:t></w:r></w:p></w:txbxContent></wps:txbx><wps:bodyPr rot="0" vert="horz" wrap="square" lIns="0" tIns="45720" rIns="0" bIns="45720" anchor="t" anchorCtr="0"><a:noAutofit/></wps:bodyPr></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice><mc:Fallback><w:pict><v:shape style="width:2in;height:.5in"><v:textbox><w:txbxContent><w:p><w:r><w:t>Tom &amp; Ann</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></mc:Fallback></mc:AlternateContent></w:r><w:r><w:t>here</w:t></w:r></w:p>"#;

    #[test]
    fn a_word_text_box_becomes_a_sentinel_with_its_look_and_place() {
        let (xml, boxes) = unwrap(WORD_BOX);
        assert_eq!(xml, format!("<w:p><w:r><w:t xml:space=\"preserve\">Name </w:t></w:r><w:r><w:t>{}</w:t></w:r><w:r><w:t>here</w:t></w:r></w:p>", sentinel(0)));
        assert_eq!(boxes.len(), 1, "the fallback is the same box");
        let (tb, a) = &boxes[0];
        assert_eq!(
            tb,
            &TextBox {
                extent_emu: (1_828_800, 457_200),
                border: Some(BoxRule { width_emu: 12_700, color: "1F3864".into() }),
                fill: Some("DEEAF6".into()),
                padding_emu: [45_720, 0, 45_720, 0],
                text: "Tom & Ann\t1\nsecond".into(),
            }
        );
        assert_eq!((a.h_from, a.h_align, a.v_from, a.y_emu, a.wrap), (AnchorFrame::Margin, Some(AnchorAlign::End), AnchorFrame::Text, 91_440, crate::model::Wrap::Parallel));
    }

    /// A picture's drawing is not a text box and is left as it is.
    #[test]
    fn a_picture_is_left_alone() {
        let pic = r#"<w:p><w:r><w:drawing><wp:inline><wp:extent cx="1" cy="1"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rId5"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#;
        assert_eq!(unwrap(pic), (pic.to_string(), Vec::new()));
    }

    /// A straight line shape (a form's coloured rule under its title) is
    /// a box with no text, the line's length and no depth, ruled in the
    /// line's colour and width, where its anchor puts it; its VML fallback
    /// is not a second one. A slanted line is no rule.
    #[test]
    fn a_line_shape_is_a_rule() {
        let line = |cy: u32| {
            format!(
                r##"<w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="1" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="margin"><wp:align>left</wp:align></wp:positionH><wp:positionV relativeFrom="page"><wp:posOffset>2204720</wp:posOffset></wp:positionV><wp:extent cx="5705475" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapNone/><wp:docPr id="2" name="Straight Connector 2"/><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp><wps:cNvCnPr/><wps:spPr><a:xfrm flipV="1"><a:off x="0" y="0"/><a:ext cx="5705475" cy="{cy}"/></a:xfrm><a:prstGeom prst="line"><a:avLst/></a:prstGeom><a:noFill/><a:ln w="22225"><a:solidFill><a:srgbClr val="2CC7DC"/></a:solidFill></a:ln></wps:spPr><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice><mc:Fallback><w:pict><v:line from="0,173.6pt" to="449.25pt,173.6pt" strokecolor="#2cc7dc"/></w:pict></mc:Fallback></mc:AlternateContent></w:r></w:p>"##
            )
        };
        let (xml, boxes) = unwrap(&line(0));
        assert_eq!(xml, format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", sentinel(0)));
        let (tb, a) = &boxes[0];
        assert_eq!(tb, &TextBox { extent_emu: (5_705_475, 0), border: Some(BoxRule { width_emu: 22_225, color: "2CC7DC".into() }), fill: None, padding_emu: [0; 4], text: String::new() });
        assert_eq!((a.h_from, a.h_align, a.v_from, a.y_emu), (AnchorFrame::Margin, Some(AnchorAlign::Start), AnchorFrame::Page, 2_204_720));
        assert!(unwrap(&line(9525)).1.is_empty(), "a slanted line");
    }

    /// A VML-only box (an old Word's) keeps its size, look and insets.
    #[test]
    fn a_vml_text_box_is_read() {
        let xml = r##"<w:p><w:r><w:pict><v:shape id="b" style="position:absolute;width:144pt;height:36pt" fillcolor="#FFFF00" strokecolor="red" strokeweight="1.5pt"><v:textbox inset="0,0.05in,0,0.05in"><w:txbxContent><w:p><w:r><w:t>old</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>"##;
        let (_, boxes) = unwrap(xml);
        let tb = &boxes[0].0;
        assert_eq!((tb.extent_emu, tb.fill.as_deref(), tb.text.as_str()), ((1_828_800, 457_200), Some("FFFF00"), "old"));
        assert_eq!(tb.border, Some(BoxRule { width_emu: 19_050, color: "000000".into() }), "a named colour is not read; the rule is kept");
        assert_eq!(tb.padding_emu, [45_720, 0, 45_720, 0]);
    }

    /// What `wrap` writes, `unwrap` reads back the same.
    #[test]
    fn a_written_text_box_reads_back() {
        let tb = TextBox { extent_emu: (914_400, 457_200), border: None, fill: Some("FFFFFF".into()), padding_emu: [1, 2, 3, 4], text: "a\tb <c>\n\nd".into() };
        let anchor = ImageAnchor { h_from: AnchorFrame::Page, x_emu: 12_700, v_from: AnchorFrame::Margin, v_align: Some(AnchorAlign::Center), behind: true, wrap: crate::model::Wrap::Parallel, ..Default::default() };
        let written = wrap(&format!("<w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>", sentinel(0)), &[(tb.clone(), anchor)], 9500);
        assert!(written.contains("<w:r><mc:AlternateContent") && written.contains("</mc:AlternateContent></w:r>"), "{written}");
        let (_, boxes) = unwrap(&written);
        assert_eq!(boxes, vec![(tb, anchor)]);
    }
}
