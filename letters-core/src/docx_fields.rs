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

/// A header or footer part's text as a template: paragraphs joined by
/// '\n', PAGE and NUMPAGES fields as "{page}" and "{total}", their cached
/// results dropped. `None` when the part holds no such field (rdocx's own
/// reading is then used as it is).
pub fn template(part_xml: &str) -> Option<String> {
    let mut out = String::new();
    let mut found = false;
    let mut paragraphs = 0;
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
            "w:p" => {
                if paragraphs > 0 {
                    out.push('\n');
                }
                paragraphs += 1;
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
            "w:tab" => out.push('\t'),
            _ => {}
        }
        i = tag_end;
    }
    found.then_some(out)
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
/// ("word/header1.xml"): the last section's references, resolved through
/// the document's relationships.
pub fn default_parts(document_xml: &str, rels_xml: &str) -> (Option<String>, Option<String>) {
    let reference = |element: &str| -> Option<String> {
        let mut id = None;
        let mut from = 0;
        while let Some(off) = document_xml[from..].find(&format!("<{element} ")) {
            let at = from + off;
            if attr(document_xml, at, "w:type").as_deref() == Some("default") {
                id = attr(document_xml, at, "r:id");
            }
            from = at + 1;
        }
        let id = id?;
        let mut from = 0;
        while let Some(off) = rels_xml[from..].find("<Relationship ") {
            let at = from + off;
            if attr(rels_xml, at, "Id").as_deref() == Some(id.as_str()) {
                let target = attr(rels_xml, at, "Target")?;
                return Some(match target.strip_prefix('/') {
                    Some(abs) => abs.to_string(),
                    None => format!("word/{target}"),
                });
            }
            from = at + 1;
        }
        None
    };
    (reference("w:headerReference"), reference("w:footerReference"))
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

    #[test]
    fn placeholders_are_written_as_fields_and_read_back() {
        let written = fields("<w:hdr><w:p><w:r><w:t>Page {page} of {total}</w:t></w:r></w:p></w:hdr>");
        assert!(written.contains("<w:fldSimple w:instr=\" PAGE \">"), "{written}");
        assert!(!written.contains("{page}"));
        assert_eq!(template(&written).as_deref(), Some("Page {page} of {total}"));
    }
}
