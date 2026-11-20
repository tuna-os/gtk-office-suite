// diagram.rs — SmartArt as the shapes PowerPoint drew for it.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A SmartArt graphic is a p:graphicFrame naming its data, layout, style
// and colour parts (`dgm:relIds`). Laying those out is PowerPoint's own
// algorithm; but PowerPoint also saves what it drew, in a drawing part
// (`ppt/diagrams/drawingN.xml`, Microsoft's `dsp:` schema) the slide
// relates as `diagramDrawing`. Its shapes are slide shapes in all but
// their prefix. The reader took the frame for nothing and the diagram's
// boxes and words were lost (the CNSC roundtable's six themes).
//
// So before the slide's passes run, each SmartArt frame is replaced by a
// group of the drawing's shapes at the frame's place: every pass then
// reads them as it reads any shape. A save writes them back as shapes.

/// `slide_xml` with each SmartArt frame replaced by its drawing's shapes,
/// grouped at the frame's place. `rels` is the slide's relationships part;
/// `read` reads a package part by name, empty if it is missing. A frame
/// whose drawing can't be found is left as it was.
pub(super) fn inline(slide_xml: &str, rels: &str, slide_dir: &str, mut read: impl FnMut(&str) -> String) -> String {
    if !slide_xml.contains("relIds") {
        return slide_xml.to_string();
    }
    let rels = relationships(rels);
    let drawings: Vec<&Rel> = rels.iter().filter(|r| r.kind.ends_with("/diagramDrawing")).collect();
    let mut out = String::with_capacity(slide_xml.len());
    let mut rest = slide_xml;
    let mut nth = 0;
    while let Some(start) = rest.find("<p:graphicFrame") {
        let Some(len) = rest[start..].find("</p:graphicFrame>") else { break };
        let end = start + len + "</p:graphicFrame>".len();
        let frame = &rest[start..end];
        out.push_str(&rest[..start]);
        rest = &rest[end..];
        if !frame.contains(":relIds") {
            out.push_str(frame);
            continue;
        }
        // The drawing the frame's data part names (dsp:dataModelExt),
        // else the slide's nth diagram drawing.
        let data_id = attr_in(frame, ":relIds", "r:dm");
        let named = data_id
            .and_then(|id| rels.iter().find(|r| r.id == id))
            .map(|r| read(&part_path(slide_dir, &r.target)))
            .and_then(|data| attr_in(&data, "dataModelExt", "relId").map(str::to_string))
            .and_then(|id| rels.iter().find(|r| r.id == id));
        let drawing = named.or(drawings.get(nth).copied());
        nth += 1;
        let shapes = drawing.map(|d| read(&part_path(slide_dir, &d.target))).and_then(|xml| shapes_of(&xml));
        match (shapes, frame_box(frame)) {
            (Some(shapes), Some((x, y, w, h))) => out.push_str(&format!(
                "<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"0\" name=\"Diagram\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
                 <p:grpSpPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/>\
                 <a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"{w}\" cy=\"{h}\"/></a:xfrm></p:grpSpPr>{shapes}</p:grpSp>"
            )),
            _ => out.push_str(frame),
        }
    }
    out.push_str(rest);
    out
}

struct Rel {
    id: String,
    kind: String,
    target: String,
}

fn relationships(rels: &str) -> Vec<Rel> {
    rels.split("<Relationship ")
        .skip(1)
        .filter_map(|r| {
            // The split took the space before the first attribute.
            let r = format!(" {}", &r[..r.find('>').unwrap_or(r.len())]);
            let get = |k: &str| attr(&r, k).map(str::to_string);
            Some(Rel { id: get("Id")?, kind: get("Type")?, target: get("Target")? })
        })
        .collect()
}

/// The value of attribute `name` in the tag text `tag`.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    Some(&tag[at..at + tag[at..].find('"')?])
}

/// Attribute `name` of the first element whose name ends in `element`.
fn attr_in<'a>(xml: &'a str, element: &str, name: &str) -> Option<&'a str> {
    let at = xml.find(element)?;
    let tag = &xml[at..at + xml[at..].find('>')?];
    attr(tag, name)
}

/// A relationship target, relative to the slide's folder, as a part name.
fn part_path(slide_dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if target.starts_with('/') { Vec::new() } else { slide_dir.split('/').filter(|s| !s.is_empty()).collect() };
    for seg in target.split('/').filter(|s| !s.is_empty()) {
        match seg {
            ".." => {
                parts.pop();
            }
            "." => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// The frame's offset and size from its p:xfrm, in EMU.
fn frame_box(frame: &str) -> Option<(i64, i64, i64, i64)> {
    let x = frame.find("<p:xfrm")?;
    let xfrm = &frame[x..x + frame[x..].find("</p:xfrm>")?];
    let num = |el: &str, k: &str| attr_in(xfrm, el, k).and_then(|v| v.parse().ok());
    Some((num("a:off", "x")?, num("a:off", "y")?, num("a:ext", "cx")?, num("a:ext", "cy")?))
}

/// The drawing's shapes as slide shapes: its spTree's children with the
/// `dsp:` prefix made `p:`, and their text rectangles (`dsp:txXfrm`)
/// dropped, whose offset and size would otherwise read as the shape's.
fn shapes_of(drawing: &str) -> Option<String> {
    let open = drawing.find("<dsp:spTree")?;
    let body_start = open + drawing[open..].find('>')? + 1;
    let body_end = drawing.rfind("</dsp:spTree>")?;
    let mut body = drawing[body_start..body_end].to_string();
    while let Some(s) = body.find("<dsp:txXfrm") {
        let e = match body[s..].find("</dsp:txXfrm>") {
            Some(n) => s + n + "</dsp:txXfrm>".len(),
            None => s + body[s..].find("/>")? + 2,
        };
        body.replace_range(s..e, "");
    }
    // The tree's own group properties are not a shape.
    for el in ["nvGrpSpPr", "grpSpPr"] {
        strip_leading(&mut body, el);
    }
    Some(body.replace("<dsp:", "<p:").replace("</dsp:", "</p:"))
}

/// Remove element `dsp:<el>` (empty or with content) if `body` starts
/// with it.
fn strip_leading(body: &mut String, el: &str) {
    let trimmed = body.trim_start();
    let lead = body.len() - trimmed.len();
    let open = format!("<dsp:{el}");
    // `<dsp:grpSpPr` must not match `<dsp:grpSpPrX`.
    if !trimmed.starts_with(&open) || !matches!(trimmed[open.len()..].chars().next(), Some('>' | '/' | ' ')) {
        return;
    }
    let Some(tag_end) = trimmed.find('>') else { return };
    let end = if trimmed[..tag_end].ends_with('/') {
        tag_end + 1
    } else {
        let close = format!("</dsp:{el}>");
        match trimmed.find(&close) {
            Some(c) => c + close.len(),
            None => return,
        }
    };
    body.replace_range(lead..lead + end, "");
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELS: &str = r#"<Relationships><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/diagramData" Target="../diagrams/data1.xml"/><Relationship Id="rId7" Type="http://schemas.microsoft.com/office/2007/relationships/diagramDrawing" Target="../diagrams/drawing1.xml"/></Relationships>"#;
    const FRAME: &str = r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="Diagram 3"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="100" y="200"/><a:ext cx="3000" cy="2000"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/diagram"><dgm:relIds r:dm="rId3" r:lo="rId4" r:qs="rId5" r:cs="rId6"/></a:graphicData></a:graphic></p:graphicFrame>"#;
    const DATA: &str = r#"<dgm:dataModel><dgm:extLst><a:ext uri="x"><dsp:dataModelExt relId="rId7" minVer="x"/></a:ext></dgm:extLst></dgm:dataModel>"#;
    const DRAWING: &str = r#"<dsp:drawing><dsp:spTree><dsp:nvGrpSpPr><dsp:cNvPr id="0" name=""/><dsp:cNvGrpSpPr/></dsp:nvGrpSpPr><dsp:grpSpPr/><dsp:sp modelId="a"><dsp:nvSpPr><dsp:cNvPr id="0" name=""/><dsp:cNvSpPr/></dsp:nvSpPr><dsp:spPr><a:xfrm><a:off x="0" y="10"/><a:ext cx="1000" cy="900"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></dsp:spPr><dsp:txBody><a:bodyPr/><a:p><a:r><a:t>Communication</a:t></a:r></a:p></dsp:txBody><dsp:txXfrm><a:off x="5" y="15"/><a:ext cx="990" cy="890"/></dsp:txXfrm></dsp:sp></dsp:spTree></dsp:drawing>"#;

    fn read(name: &str) -> String {
        match name {
            "ppt/diagrams/data1.xml" => DATA.into(),
            "ppt/diagrams/drawing1.xml" => DRAWING.into(),
            _ => String::new(),
        }
    }

    #[test]
    fn a_smartart_frame_becomes_its_drawings_shapes_at_its_place() {
        let slide = format!("<p:spTree>{FRAME}</p:spTree>");
        let out = inline(&slide, RELS, "ppt/slides", read);
        assert!(!out.contains("graphicFrame"), "{out}");
        assert!(out.contains(r#"<a:off x="100" y="200"/><a:ext cx="3000" cy="2000"/><a:chOff x="0" y="0"/><a:chExt cx="3000" cy="2000"/>"#), "{out}");
        assert!(out.contains(r#"<p:sp modelId="a"><p:nvSpPr>"#), "{out}");
        assert!(out.contains("<a:t>Communication</a:t></a:r></a:p></p:txBody></p:sp></p:grpSp>"), "the text rectangle is dropped: {out}");
        assert!(!out.contains("dsp:") && !out.contains("cNvGrpSpPr/></p:nvGrpSpPr><p:grpSpPr/>"), "{out}");
    }

    #[test]
    fn a_frame_without_its_drawing_and_other_frames_stay_as_they_were() {
        let slide = format!("<p:spTree>{FRAME}</p:spTree>");
        assert_eq!(inline(&slide, RELS, "ppt/slides", |_| String::new()), slide);
        let table = r#"<p:spTree><p:graphicFrame><a:tbl/></p:graphicFrame></p:spTree>"#;
        assert_eq!(inline(table, RELS, "ppt/slides", read), table);
    }

    #[test]
    fn targets_resolve_against_the_slides_folder() {
        assert_eq!(part_path("ppt/slides", "../diagrams/drawing1.xml"), "ppt/diagrams/drawing1.xml");
        assert_eq!(part_path("ppt/slides", "/ppt/diagrams/d.xml"), "ppt/diagrams/d.xml");
    }
}
