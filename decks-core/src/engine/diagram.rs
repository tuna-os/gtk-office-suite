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
// A drawing's own relationships (a picture fill's `r:embed`) name parts
// from the drawing's folder; they join the slide's under fresh ids.

/// `slide_xml` with each SmartArt frame replaced by its drawing's shapes,
/// grouped at the frame's place, and the `Relationship` elements the
/// slide's relationships part needs added for them. `rels` is that part;
/// `read` reads a package part by name, empty if it is missing. A frame
/// whose drawing can't be found is left as it was.
pub(super) fn inline(slide_xml: &str, rels: &str, slide_dir: &str, mut read: impl FnMut(&str) -> String) -> (String, String) {
    if !slide_xml.contains("relIds") {
        return (slide_xml.to_string(), String::new());
    }
    let mut extra_rels = String::new();
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
        let shapes = drawing.and_then(|d| {
            let path = part_path(slide_dir, &d.target);
            let mut shapes = shapes_of(&read(&path))?;
            // The drawing's relationships, renamed so they can't collide
            // with the slide's, their targets made relative to the slide.
            let (dir, file) = path.rsplit_once('/').unwrap_or(("", &path));
            for r in relationships(&read(&format!("{dir}/_rels/{file}.rels"))) {
                let id = format!("dgm{nth}_{}", r.id);
                if !shapes.contains(&format!("=\"{}\"", r.id)) {
                    continue;
                }
                for attr in ["r:embed", "r:link", "r:id"] {
                    shapes = shapes.replace(&format!("{attr}=\"{}\"", r.id), &format!("{attr}=\"{id}\""));
                }
                // An external target (a hyperlink's URL) is kept as written.
                let rel = if r.external {
                    format!(r#"<Relationship Id="{id}" Type="{}" Target="{}" TargetMode="External"/>"#, r.kind, r.target)
                } else {
                    format!(r#"<Relationship Id="{id}" Type="{}" Target="{}"/>"#, r.kind, relative_to(slide_dir, &part_path(dir, &r.target)))
                };
                extra_rels.push_str(&rel);
            }
            Some(shapes)
        });
        match (shapes, frame_box(frame)) {
            (Some(shapes), Some((x, y, w, h))) => out.push_str(&format!(
                "<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"0\" name=\"Diagram\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
                 <p:grpSpPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/>\
                 <a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"{w}\" cy=\"{h}\"/></a:xfrm></p:grpSpPr>{shapes}</p:grpSp>"
            )),
            _ => out.push_str(frame),
        }
        nth += 1;
    }
    out.push_str(rest);
    (out, extra_rels)
}

/// Part `part`'s path relative to folder `dir`, as a relationship target.
fn relative_to(dir: &str, part: &str) -> String {
    let from: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = part.split('/').filter(|s| !s.is_empty()).collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut out: Vec<&str> = vec![".."; from.len() - common];
    out.extend(&to[common..]);
    out.join("/")
}

struct Rel {
    id: String,
    kind: String,
    /// As written: escaped as it was in the XML, so it can be written back.
    target: String,
    /// `TargetMode="External"`: a URL, not a part.
    external: bool,
}

fn relationships(rels: &str) -> Vec<Rel> {
    rels.split("<Relationship ")
        .skip(1)
        .filter_map(|r| {
            // The split took the space before the first attribute.
            let r = format!(" {}", &r[..r.find('>').unwrap_or(r.len())]);
            let get = |k: &str| attr(&r, k).map(str::to_string);
            Some(Rel { id: get("Id")?, kind: get("Type")?, target: get("Target")?, external: get("TargetMode").as_deref() == Some("External") })
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
    let mut body = String::new();
    let mut rest = &drawing[body_start..body_end];
    // Each shape's text rectangle (dsp:txXfrm) as insets on its text, then
    // dropped: its offset and size would otherwise read as the shape's.
    while let Some(s) = rest.find("<dsp:sp ").or_else(|| rest.find("<dsp:sp>")) {
        let e = s + rest[s..].find("</dsp:sp>")? + "</dsp:sp>".len();
        body.push_str(&rest[..s]);
        body.push_str(&text_rect_as_insets(&rest[s..e]));
        rest = &rest[e..];
    }
    body.push_str(rest);
    // The tree's own group properties are not a shape.
    for el in ["nvGrpSpPr", "grpSpPr"] {
        strip_leading(&mut body, el);
    }
    Some(body.replace("<dsp:", "<p:").replace("</dsp:", "</p:"))
}

/// A drawing shape `sp` with its text rectangle (`dsp:txXfrm`) turned into
/// its body's insets: PowerPoint lays a diagram shape's text out in that
/// rectangle, which may sit well inside the shape (a label beside an icon,
/// an arc's caption off to one side). The rectangle is then removed.
fn text_rect_as_insets(sp: &str) -> String {
    let Some(t) = sp.find("<dsp:txXfrm") else { return sp.to_string() };
    let end = match (sp[t..].find("</dsp:txXfrm>"), sp[t..].find("/>")) {
        (Some(n), _) => t + n + "</dsp:txXfrm>".len(),
        (None, Some(n)) => t + n + 2,
        (None, None) => return sp.to_string(),
    };
    let rect = |xml: &str| -> Option<(f64, f64, f64, f64)> {
        let n = |el: &str, k: &str| attr_in(xml, el, k).and_then(|v| v.parse::<f64>().ok());
        Some((n("a:off", "x")?, n("a:off", "y")?, n("a:ext", "cx")?, n("a:ext", "cy")?))
    };
    let tx_xml = &sp[t..end];
    let shape = sp.find("<dsp:spPr").and_then(|p| sp[p..].find("</dsp:spPr>").map(|q| &sp[p..p + q])).and_then(rect);
    let mut out = format!("{}{}", &sp[..t], &sp[end..]);
    // A turned text rectangle is not a plain inset; keep the shape's box.
    let turned = attr_in(tx_xml, "dsp:txXfrm", "rot").is_some_and(|r| r != "0");
    let (Some((x, y, w, h)), Some((tx, ty, tw, th)), false) = (shape, rect(tx_xml), turned) else { return out };
    let delta = [(tx - x), (ty - y), (x + w - tx - tw), (y + h - ty - th)].map(|d| d.max(0.0));
    let Some(b) = out.find("<a:bodyPr") else { return out };
    let tag_end = b + out[b..].find('>').unwrap_or(0);
    let tag = &out[b..tag_end];
    let own = |k: &str, d: f64| attr(tag, k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(d);
    let insets = [("lIns", own("lIns", 91440.0) + delta[0]), ("tIns", own("tIns", 45720.0) + delta[1]), ("rIns", own("rIns", 91440.0) + delta[2]), ("bIns", own("bIns", 45720.0) + delta[3])];
    // The tag without its own insets, then the combined ones.
    let mut new_tag = tag.to_string();
    for (k, _) in insets {
        if let Some(v) = attr(&new_tag, k) {
            new_tag = new_tag.replacen(&format!(" {k}=\"{v}\""), "", 1);
        }
    }
    let self_closing = new_tag.ends_with('/');
    let mut new_tag = new_tag.trim_end_matches('/').to_string();
    for (k, v) in insets {
        new_tag.push_str(&format!(" {k}=\"{}\"", v.round() as i64));
    }
    if self_closing {
        new_tag.push('/');
    }
    out.replace_range(b..tag_end, &new_tag);
    out
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
        let (out, extra) = inline(&slide, RELS, "ppt/slides", read);
        assert_eq!(extra, "", "no pictures, no relationships");
        assert!(!out.contains("graphicFrame"), "{out}");
        assert!(out.contains(r#"<a:off x="100" y="200"/><a:ext cx="3000" cy="2000"/><a:chOff x="0" y="0"/><a:chExt cx="3000" cy="2000"/>"#), "{out}");
        assert!(out.contains(r#"<p:sp modelId="a"><p:nvSpPr>"#), "{out}");
        assert!(out.contains("<a:t>Communication</a:t></a:r></a:p></p:txBody></p:sp></p:grpSp>"), "the text rectangle is dropped: {out}");
        assert!(!out.contains("dsp:") && !out.contains("cNvGrpSpPr/></p:nvGrpSpPr><p:grpSpPr/>"), "{out}");
    }

    #[test]
    fn a_frame_without_its_drawing_and_other_frames_stay_as_they_were() {
        let slide = format!("<p:spTree>{FRAME}</p:spTree>");
        assert_eq!(inline(&slide, RELS, "ppt/slides", |_| String::new()).0, slide);
        let table = r#"<p:spTree><p:graphicFrame><a:tbl/></p:graphicFrame></p:spTree>"#;
        assert_eq!(inline(table, RELS, "ppt/slides", read).0, table);
    }

    /// A diagram shape's text rectangle becomes its text's insets: here
    /// 5 EMU in from the left and top, 5 from the right and bottom, over
    /// the body's own defaults.
    #[test]
    fn a_text_rectangle_becomes_the_texts_insets() {
        let (out, _) = inline(&format!("<p:spTree>{FRAME}</p:spTree>"), RELS, "ppt/slides", read);
        assert!(out.contains(r#"<a:bodyPr lIns="91445" tIns="45725" rIns="91445" bIns="45725"/>"#), "{out}");
        let turned = DRAWING.replace("<dsp:txXfrm>", r#"<dsp:txXfrm rot="5400000">"#);
        let read_turned = |name: &str| if name == "ppt/diagrams/drawing1.xml" { turned.clone() } else { read(name) };
        let (out, _) = inline(&format!("<p:spTree>{FRAME}</p:spTree>"), RELS, "ppt/slides", read_turned);
        assert!(out.contains("<a:bodyPr/>") && !out.contains("txXfrm"), "a turned rectangle is dropped, not inset: {out}");
    }

    #[test]
    fn targets_resolve_against_the_slides_folder() {
        assert_eq!(part_path("ppt/slides", "../diagrams/drawing1.xml"), "ppt/diagrams/drawing1.xml");
        assert_eq!(part_path("ppt/slides", "/ppt/diagrams/d.xml"), "ppt/diagrams/d.xml");
        assert_eq!(relative_to("ppt/slides", "ppt/media/image3.png"), "../media/image3.png");
        assert_eq!(relative_to("ppt/slides", "ppt/slides/x.png"), "x.png");
    }

    /// A picture in a diagram names it in the drawing's own relationships:
    /// it joins the slide's under a fresh id, its target from the slide.
    #[test]
    fn a_pictures_relationship_follows_it_into_the_slide() {
        let drawing = DRAWING.replace(
            "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>",
            "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:blipFill><a:blip r:embed=\"rId1\"/><a:stretch><a:fillRect/></a:stretch></a:blipFill>",
        );
        let drawing_rels = r#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image3.png"/></Relationships>"#;
        let read = |name: &str| match name {
            "ppt/diagrams/data1.xml" => DATA.to_string(),
            "ppt/diagrams/drawing1.xml" => drawing.clone(),
            "ppt/diagrams/_rels/drawing1.xml.rels" => drawing_rels.to_string(),
            _ => String::new(),
        };
        let (out, extra) = inline(&format!("<p:spTree>{FRAME}</p:spTree>"), RELS, "ppt/slides", read);
        assert!(out.contains(r#"<a:blip r:embed="dgm0_rId1"/>"#), "{out}");
        assert_eq!(extra, r#"<Relationship Id="dgm0_rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image3.png"/>"#);
    }

    /// A hyperlink on a diagram's text names its URL in the drawing's
    /// relationships: its r:id is renamed with it, the URL kept as written.
    #[test]
    fn a_hyperlinks_relationship_follows_it_into_the_slide() {
        let drawing = DRAWING.replace("<a:r><a:t>Communication", r#"<a:r><a:rPr><a:hlinkClick r:id="rId2"/></a:rPr><a:t>Communication"#);
        let drawing_rels = r#"<Relationships><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.org/a?b=1&amp;c=2" TargetMode="External"/></Relationships>"#;
        let read = |name: &str| match name {
            "ppt/diagrams/data1.xml" => DATA.to_string(),
            "ppt/diagrams/drawing1.xml" => drawing.clone(),
            "ppt/diagrams/_rels/drawing1.xml.rels" => drawing_rels.to_string(),
            _ => String::new(),
        };
        let (out, extra) = inline(&format!("<p:spTree>{FRAME}</p:spTree>"), RELS, "ppt/slides", read);
        assert!(out.contains(r#"<a:hlinkClick r:id="dgm0_rId2"/>"#), "{out}");
        assert_eq!(extra, r#"<Relationship Id="dgm0_rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.org/a?b=1&amp;c=2" TargetMode="External"/>"#);
    }
}
