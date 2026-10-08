// Floating pictures in a .docx: where each `wp:anchor` places its image.
//
// rdocx hands back an anchored drawing's image and size but not its
// position, so the positions are read from the document part here, in
// document order, per image relationship: the n-th anchored drawing of
// `rId7` that the reader meets is the n-th `wp:anchor` embedding `rId7`.

use std::collections::{HashMap, VecDeque};

use crate::model::{AnchorAlign, AnchorFrame, ImageAnchor};

/// Each image relationship's anchored placements, in document order.
pub type Placements = HashMap<String, VecDeque<ImageAnchor>>;

/// The placements of the pictures anchored in `document_xml`.
pub fn placements(document_xml: &str) -> Placements {
    let mut out = Placements::new();
    let mut rest = document_xml;
    while let Some(at) = rest.find("<wp:anchor") {
        let Some(len) = rest[at..].find("</wp:anchor>") else { break };
        let anchor = &rest[at..at + len];
        rest = &rest[at + len..];
        let Some(embed) = attr(anchor, "<a:blip ", "r:embed") else { continue };
        out.entry(embed.to_string()).or_default().push_back(placement(anchor));
    }
    out
}

/// A picture found in raw markup: its image relationship, alt text,
/// displayed size in EMU, and whether it floats.
pub type RawPicture = (String, Option<String>, Option<(u64, u64)>, bool);

/// A picture rdocx keeps as raw markup (LibreOffice writes a floating
/// picture inside `mc:AlternateContent`): its image relationship, alt
/// text, displayed size in EMU, and whether it floats.
pub fn raw_picture(raw: &str) -> Option<RawPicture> {
    let embed = attr(raw, "<a:blip ", "r:embed")?.to_string();
    let anchored = raw.contains("<wp:anchor");
    if !anchored && !raw.contains("<wp:inline") {
        return None;
    }
    let size = |a| attr(raw, "<wp:extent ", a).and_then(|v| v.parse::<u64>().ok());
    let extent = size("cx").zip(size("cy")).filter(|(w, h)| *w > 0 && *h > 0);
    let alt = attr(raw, "<wp:docPr ", "descr").filter(|d| !d.is_empty()).or_else(|| attr(raw, "<wp:docPr ", "title")).map(str::to_string);
    Some((embed, alt, extent, anchored))
}

/// A picture in a part's markup: its image relationship, alt text,
/// displayed size in EMU, and its placement when it floats.
pub struct PartPicture {
    pub embed: String,
    pub alt: String,
    pub extent: Option<(u64, u64)>,
    pub anchor: Option<ImageAnchor>,
}

/// The pictures of a header or footer part, in order: each `w:drawing`
/// holding an image (a text box, which holds none, is not one).
pub fn pictures_in(part_xml: &str) -> Vec<PartPicture> {
    let mut out = Vec::new();
    let mut rest = part_xml;
    while let Some(at) = rest.find("<w:drawing>") {
        let Some(len) = rest[at..].find("</w:drawing>") else { break };
        let drawing = &rest[at..at + len];
        rest = &rest[at + len..];
        let Some((embed, alt, extent, anchored)) = raw_picture(drawing) else { continue };
        out.push(PartPicture { embed, alt: alt.unwrap_or_default(), extent, anchor: anchored.then(|| placement(drawing)) });
    }
    out
}

/// The target of relationship `id` in a part's relationships, resolved
/// against the part's folder (`word/`): `media/image1.png` is
/// `word/media/image1.png`.
pub fn relationship_target(rels_xml: &str, id: &str) -> Option<String> {
    let mut rest = rels_xml;
    while let Some(at) = rest.find("<Relationship ") {
        let tag = &rest[at..at + rest[at..].find('>')? + 1];
        rest = &rest[at + tag.len()..];
        if attr(tag, "<Relationship", "Id") != Some(id) {
            continue;
        }
        let target = attr(tag, "<Relationship", "Target")?;
        return Some(match target.strip_prefix('/') {
            Some(abs) => abs.to_string(),
            None => format!("word/{target}"),
        });
    }
    None
}

/// One `wp:anchor` element's placement.
fn placement(anchor: &str) -> ImageAnchor {
    let (h_from, x_emu, h_align) = axis(anchor, "wp:positionH");
    let (v_from, y_emu, v_align) = axis(anchor, "wp:positionV");
    ImageAnchor {
        h_from,
        x_emu,
        h_align,
        v_from,
        y_emu,
        v_align,
        behind: attr(anchor, "<wp:anchor", "behindDoc").is_some_and(|v| v == "1" || v == "true"),
    }
}

/// An axis's frame and its offset or alignment.
fn axis(anchor: &str, element: &str) -> (AnchorFrame, i64, Option<AnchorAlign>) {
    let Some(at) = anchor.find(&format!("<{element} ")) else { return (AnchorFrame::Text, 0, None) };
    let close = format!("</{element}>");
    let body = &anchor[at..at + anchor[at..].find(&close).unwrap_or(anchor.len() - at)];
    let frame = match attr(body, &format!("<{element}"), "relativeFrom").unwrap_or("") {
        "page" | "leftMargin" | "rightMargin" | "topMargin" | "insideMargin" | "outsideMargin" => AnchorFrame::Page,
        "margin" | "bottomMargin" => AnchorFrame::Margin,
        // column, character, paragraph, line
        _ => AnchorFrame::Text,
    };
    let offset = text_of(body, "wp:posOffset").and_then(|t| t.trim().parse().ok()).unwrap_or(0);
    let align = text_of(body, "wp:align").and_then(|t| match t.trim() {
        "left" | "top" | "inside" => Some(AnchorAlign::Start),
        "center" => Some(AnchorAlign::Center),
        "right" | "bottom" | "outside" => Some(AnchorAlign::End),
        _ => None,
    });
    (frame, offset, align)
}

/// The value of `name` on the first `tag` element (`tag` includes its `<`).
fn attr<'a>(xml: &'a str, tag: &str, name: &str) -> Option<&'a str> {
    let at = xml.find(tag)?;
    let open = &xml[at..at + xml[at..].find('>')?];
    let key = format!(" {name}=\"");
    let v = open.find(&key)? + key.len();
    Some(&open[v..v + open[v..].find('"')?])
}

/// The text of the first `<element>…</element>`.
fn text_of<'a>(xml: &'a str, element: &str) -> Option<&'a str> {
    let open = format!("<{element}>");
    let at = xml.find(&open)? + open.len();
    Some(&xml[at..at + xml[at..].find('<')?])
}

/// `document_xml` with each picture in `floating` (its image relationship,
/// alt text and placement) turned from the inline drawing the writer made
/// into a `wp:anchor` placed as the model says, in front of or behind the
/// text and without wrapping it. Each gets a `wp:docPr` id of its own,
/// counting up from `first_id`.
pub fn float(document_xml: &str, floating: &[(String, String, ImageAnchor)], first_id: u32) -> String {
    let mut out = String::with_capacity(document_xml.len() + floating.len() * 300);
    let mut rest = document_xml;
    let mut next_id = first_id - 1;
    while let Some(at) = rest.find("<wp:inline") {
        let Some(len) = rest[at..].find("</wp:inline>") else { break };
        let inline = &rest[at..at + len];
        out.push_str(&rest[..at]);
        rest = &rest[at + len + "</wp:inline>".len()..];
        let embed = attr(inline, "<a:blip ", "r:embed");
        let Some((_, alt, anchor)) = floating.iter().find(|(rel, _, _)| Some(rel.as_str()) == embed) else {
            out.push_str(inline);
            out.push_str("</wp:inline>");
            continue;
        };
        let body = &inline[inline.find('>').map_or(inline.len(), |e| e + 1)..];
        let (extent, tail) = body.split_at(body.find("<wp:docPr").unwrap_or(body.len()));
        let tail = tail.strip_prefix("<wp:docPr").unwrap_or(tail);
        // The writer's own docPr attributes go; its id, name and the alt
        // text are written afresh.
        let tail = &tail[tail.find("/>").or_else(|| tail.find('>')).unwrap_or(0)..];
        next_id += 1;
        out.push_str(&format!(
            "<wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" relativeHeight=\"{next_id}\" \
             behindDoc=\"{}\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\"><wp:simplePos x=\"0\" y=\"0\"/>\
             <wp:positionH relativeFrom=\"{}\">{}</wp:positionH><wp:positionV relativeFrom=\"{}\">{}</wp:positionV>\
             {extent}<wp:wrapNone/><wp:docPr id=\"{next_id}\" name=\"Picture {next_id}\" descr=\"{}\"{tail}</wp:anchor>",
            u8::from(anchor.behind),
            frame(anchor.h_from, "column"),
            position(anchor.x_emu, anchor.h_align, ["left", "center", "right"]),
            frame(anchor.v_from, "paragraph"),
            position(anchor.y_emu, anchor.v_align, ["top", "center", "bottom"]),
            xml_attr(alt),
        ));
    }
    out.push_str(rest);
    out
}

/// A header or footer part (`w:hdr` or `w:ftr`) holding `pictures` and
/// then `text`, a paragraph a line, each aligned as `alignment` says (left,
/// Word's default, unsaid): the pictures go in the first line's
/// paragraph, where the reader finds them. Returns the part and its images
/// as (relationship id, bytes, file name); a picture whose file cannot be
/// read is left out.
pub fn header_footer_part(is_header: bool, text: &str, pictures: &[crate::model::Run], alignment: crate::model::Alignment) -> (String, Vec<(String, Vec<u8>, String)>) {
    let mut images = Vec::new();
    let mut floating = Vec::new();
    let mut drawings = String::new();
    for run in pictures {
        let Some(src) = run.style.image.as_deref() else { continue };
        let Ok(bytes) = std::fs::read(src) else { continue };
        let rel = format!("rIdLettersPic{}", images.len() + 1);
        let n = 9800 + images.len();
        let (w, h) = run.style.image_extent_emu.unwrap_or((914_400, 914_400));
        drawings.push_str(&format!(
            "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{w}\" cy=\"{h}\"/>\
             <wp:docPr id=\"{n}\" name=\"Picture {n}\" descr=\"{alt}\"/><wp:cNvGraphicFramePr/>\
             <a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic>\
             <pic:nvPicPr><pic:cNvPr id=\"0\" name=\"Picture {n}\"/><pic:cNvPicPr/></pic:nvPicPr>\
             <pic:blipFill><a:blip r:embed=\"{rel}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
             <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
             </pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>",
            alt = xml_attr(&run.text),
        ));
        if let Some(anchor) = run.style.image_anchor {
            floating.push((rel.clone(), run.text.clone(), anchor));
        }
        let name = std::path::Path::new(src).file_name().map_or_else(|| "image.png".to_string(), |n| n.to_string_lossy().into_owned());
        images.push((rel, bytes, name));
    }
    let mut body = String::new();
    let jc = match alignment {
        crate::model::Alignment::Left => String::new(),
        crate::model::Alignment::Center => "<w:pPr><w:jc w:val=\"center\"/></w:pPr>".to_string(),
        crate::model::Alignment::Right => "<w:pPr><w:jc w:val=\"right\"/></w:pPr>".to_string(),
        crate::model::Alignment::Justify => "<w:pPr><w:jc w:val=\"both\"/></w:pPr>".to_string(),
    };
    for (k, line) in text.split('\n').enumerate() {
        body.push_str("<w:p>");
        body.push_str(&jc);
        if k == 0 {
            body.push_str(&drawings);
        }
        if !line.is_empty() {
            body.push_str(&format!("<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>", xml_attr(line)));
        }
        body.push_str("</w:p>");
    }
    let tag = if is_header { "w:hdr" } else { "w:ftr" };
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <{tag} xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
         xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
         xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
         xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">{body}</{tag}>"
    );
    let xml = if floating.is_empty() { xml } else { float(&xml, &floating, if is_header { 9700 } else { 9750 }) };
    (xml, images)
}

/// A frame's `relativeFrom`, `text` naming the anchoring paragraph's.
fn frame(f: AnchorFrame, text: &str) -> &str {
    match f {
        AnchorFrame::Page => "page",
        AnchorFrame::Margin => "margin",
        AnchorFrame::Text => text,
    }
}

/// An axis's `wp:align` or `wp:posOffset`.
fn position(offset: i64, align: Option<AnchorAlign>, names: [&str; 3]) -> String {
    match align {
        Some(AnchorAlign::Start) => format!("<wp:align>{}</wp:align>", names[0]),
        Some(AnchorAlign::Center) => format!("<wp:align>{}</wp:align>", names[1]),
        Some(AnchorAlign::End) => format!("<wp:align>{}</wp:align>", names[2]),
        None => format!("<wp:posOffset>{offset}</wp:posOffset>"),
    }
}

fn xml_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Ofsted logo in the audit-committee minutes: right-aligned in the
    /// margins, 52pt above its paragraph.
    #[test]
    fn an_anchor_reads_its_frames_offsets_and_alignments() {
        let xml = r#"<w:p><w:r><w:drawing><wp:anchor distT="0" behindDoc="0" relativeHeight="1">
            <wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="margin"><wp:align>right</wp:align></wp:positionH>
            <wp:positionV relativeFrom="paragraph"><wp:posOffset>-660400</wp:posOffset></wp:positionV>
            <wp:extent cx="1296035" cy="1097915"/><a:graphic><pic:pic><pic:blipFill><a:blip r:embed="rId7"/></pic:blipFill></pic:pic></a:graphic>
            </wp:anchor></w:drawing></w:r></w:p>
            <w:p><w:r><w:drawing><wp:anchor behindDoc="1"><wp:positionH relativeFrom="page"><wp:posOffset>914400</wp:posOffset></wp:positionH>
            <wp:positionV relativeFrom="page"><wp:posOffset>12700</wp:posOffset></wp:positionV><a:blip r:embed="rId7"/></wp:anchor></w:drawing></w:r></w:p>
            <w:p><w:r><w:drawing><wp:anchor><wp:positionH relativeFrom="column"><wp:posOffset>0</wp:posOffset></wp:positionH>
            <wp:wsp/></wp:anchor></w:drawing></w:r></w:p>"#;
        let mut found = placements(xml);
        let logos = found.get_mut("rId7").expect("rId7 placements");
        assert_eq!(
            logos.pop_front(),
            Some(ImageAnchor {
                h_from: AnchorFrame::Margin,
                x_emu: 0,
                h_align: Some(AnchorAlign::End),
                v_from: AnchorFrame::Text,
                y_emu: -660_400,
                v_align: None,
                behind: false,
            })
        );
        let second = logos.pop_front().expect("the second use of rId7");
        assert_eq!((second.h_from, second.x_emu, second.v_from, second.y_emu, second.behind), (AnchorFrame::Page, 914_400, AnchorFrame::Page, 12_700, true));
        assert_eq!(found.len(), 1, "a shape without a picture is not a placement");
    }

    /// The tier 2/3 notification's header: the OFSI logo in line, and an
    /// "OFFICIAL" text box, which is no picture.
    #[test]
    fn a_parts_pictures_and_their_relationships() {
        let header = r#"<w:hdr><w:p><w:r><w:drawing><wp:inline><wp:extent cx="1304925" cy="542925"/><wp:docPr id="20" name="Picture 20" descr="Office of Financial Sanctions Implementation"/><a:graphic><a:blip r:embed="rId1"/></a:graphic></wp:inline></w:drawing></w:r></w:p>
            <w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wp:anchor><wp:extent cx="1" cy="1"/><wps:txbx><w:txbxContent><w:p><w:r><w:t>OFFICIAL</w:t></w:r></w:p></w:txbxContent></wps:txbx></wp:anchor></w:drawing></mc:Choice><mc:Fallback><w:pict/></mc:Fallback></mc:AlternateContent></w:r></w:p></w:hdr>"#;
        let pics = pictures_in(header);
        assert_eq!(pics.len(), 1);
        assert_eq!((pics[0].embed.as_str(), pics[0].alt.as_str(), pics[0].extent, pics[0].anchor), ("rId1", "Office of Financial Sanctions Implementation", Some((1_304_925, 542_925)), None));
        let rels = r#"<Relationships><Relationship Id="rId2" Type="x" Target="media/image9.png"/><Relationship Id="rId1" Type="x" Target="media/image1.png"/></Relationships>"#;
        assert_eq!(relationship_target(rels, "rId1").as_deref(), Some("word/media/image1.png"));
        assert_eq!(relationship_target(rels, "rId3"), None);
    }

    /// LibreOffice's floating picture, kept raw by rdocx, is still found.
    #[test]
    fn a_picture_in_alternate_content_is_read_from_its_markup() {
        let raw = r#"<mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wp:anchor behindDoc="0"><wp:extent cx="1296035" cy="1097915"/><wp:docPr id="1" name="Picture 1" title="Ofsted logo"></wp:docPr><a:graphic><a:blip r:embed="rId2"></a:blip></a:graphic></wp:anchor></w:drawing></mc:Choice><mc:Fallback><w:pict/></mc:Fallback></mc:AlternateContent>"#;
        assert_eq!(raw_picture(raw), Some(("rId2".into(), Some("Ofsted logo".into()), Some((1_296_035, 1_097_915)), true)));
        assert_eq!(raw_picture("<mc:AlternateContent><wps:wsp/></mc:AlternateContent>"), None);
    }

    /// A floating picture written inline becomes an anchor that reads back
    /// to the same placement; other inline pictures are left alone.
    #[test]
    fn a_floating_picture_is_written_as_an_anchor() {
        let xml = r#"<w:p><w:r><w:drawing><wp:inline><wp:extent cx="100" cy="50"/><wp:docPr id="1" name="Picture 1"/><a:graphic><a:blip r:embed="rId9"/></a:graphic></wp:inline></w:drawing></w:r><w:r><w:drawing><wp:inline><wp:extent cx="1" cy="1"/><wp:docPr id="1" name="x"/><a:graphic><a:blip r:embed="rId4"/></a:graphic></wp:inline></w:drawing></w:r></w:p>"#;
        let anchor = ImageAnchor { h_from: AnchorFrame::Margin, h_align: Some(AnchorAlign::End), v_from: AnchorFrame::Text, y_emu: -660_400, ..Default::default() };
        let out = float(xml, &[("rId9".into(), "Ofsted \"logo\"".into(), anchor)], 9001);
        assert!(out.contains(r#"<wp:positionH relativeFrom="margin"><wp:align>right</wp:align></wp:positionH>"#), "{out}");
        assert!(out.contains(r#"<wp:extent cx="100" cy="50"/><wp:wrapNone/><wp:docPr id="9001" name="Picture 9001" descr="Ofsted &quot;logo&quot;"/>"#), "{out}");
        assert_eq!(out.matches("<wp:inline").count(), 1, "the other picture stays inline: {out}");
        assert_eq!(placements(&out).get_mut("rId9").and_then(|q| q.pop_front()), Some(anchor));
    }
}
