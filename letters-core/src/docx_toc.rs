// SPDX-License-Identifier: GPL-3.0-or-later
//
// docx_toc.rs — a table of contents in .docx: Word's TOC field.
//
// Word's table of contents is a complex field, `TOC \o "1-3" \h \z \u`,
// whose result is the entry paragraphs (each with its own PAGEREF field
// for the page number), usually inside a block content control whose
// gallery is "Table of Contents". rdocx writes no fields and reads no
// block content controls, so, as for chips and comments:
// - writing: the writer puts a marker run where the entries start and end,
//   and `wrap` turns them into the field's begin and end;
// - reading: `unwrap` lifts a table-of-contents content control's
//   paragraphs into the body and ends each paragraph in a TOC field's
//   result with a marker naming its level (from its TOC1..TOC9 or
//   Contents1.. style), and `restore` makes those paragraphs entries.

use crate::model::Document;

const BEGIN: char = '\u{E008}';
const END: char = '\u{E009}';

/// The field instruction written: headings 1-3, entries linked (`\h`),
/// no page numbers hidden in web view (`\z`), outline levels used (`\u`).
pub const INSTRUCTION: &str = " TOC \\o \"1-3\" \\h \\z \\u ";

/// The marker run text where a table of contents' entries begin.
pub fn begin_marker() -> String {
    BEGIN.to_string()
}

/// The marker run text where they end.
pub fn end_marker() -> String {
    END.to_string()
}

/// The run holding `marker`, as a byte range of `xml`.
fn run_around(xml: &str, marker: &str) -> Option<(usize, usize)> {
    let pos = xml.find(&format!(">{marker}<"))?;
    let run = [xml[..pos].rfind("<w:r>"), xml[..pos].rfind("<w:r ")].into_iter().flatten().max()?;
    let end = xml[pos..].find("</w:r>").map(|e| pos + e + "</w:r>".len())?;
    Some((run, end))
}

/// Turn the marker runs into the TOC field's begin (instruction, separate)
/// and end.
pub fn wrap(document_xml: &str) -> String {
    let mut xml = document_xml.to_string();
    let begin = format!(
        "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\">{}</w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>",
        INSTRUCTION.replace('"', "&quot;")
    );
    let end = "<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>";
    while let Some((a, b)) = run_around(&xml, &begin_marker()) {
        xml.replace_range(a..b, &begin);
    }
    while let Some((a, b)) = run_around(&xml, &end_marker()) {
        xml.replace_range(a..b, end);
    }
    xml
}

/// The level a TOC entry style names: the number ending "TOC1", "TOC 2",
/// "Contents3" (LibreOffice's), 1 when it names none.
fn level_of(style: &str) -> u8 {
    let digits: String = style.chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse::<u8>().ok().filter(|l| (1..=9).contains(l)).unwrap_or(1)
}

/// The attribute `name` of `tag`.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let v = tag.find(&key)? + key.len();
    Some(&tag[v..v + tag[v..].find('"')?])
}

/// Lift the paragraphs of block content controls holding a table of
/// contents out of the control.
fn lift_controls(xml: &str) -> String {
    let mut out = xml.to_string();
    let mut from = 0;
    while let Some(off) = out[from..].find("<w:sdt>").or_else(|| out[from..].find("<w:sdt ")) {
        let at = from + off;
        // Its matching end.
        let mut depth = 0usize;
        let mut i = at;
        let mut end = None;
        while let Some(o) = out[i..].find("<w:sdt") {
            let j = i + o;
            let close = out[i..].find("</w:sdt>").map(|c| i + c);
            if let Some(c) = close.filter(|c| *c < j) {
                depth -= 1;
                i = c + "</w:sdt>".len();
                if depth == 0 {
                    end = Some(i);
                    break;
                }
                continue;
            }
            // "<w:sdtPr>", "<w:sdtContent>" and "<w:sdtEndPr>" are not controls.
            let name_end = out[j + 1..].find([' ', '>', '/']).map_or(out.len(), |e| j + 1 + e);
            if &out[j + 1..name_end] == "w:sdt" {
                depth += 1;
            }
            i = j + 1;
        }
        if end.is_none() {
            while depth > 0 {
                let Some(c) = out[i..].find("</w:sdt>").map(|c| i + c) else { break };
                depth -= 1;
                i = c + "</w:sdt>".len();
                if depth == 0 {
                    end = Some(i);
                }
            }
        }
        let Some(end) = end else { break };
        let control = &out[at..end];
        let is_toc = control.contains("w:docPartGallery w:val=\"Table of Contents\"") || control.contains("TOC \\");
        let (Some(c0), Some(c1)) = (control.find("<w:sdtContent>"), control.rfind("</w:sdtContent>")) else {
            from = at + 1;
            continue;
        };
        // Only a block control (holding paragraphs) is lifted.
        let inner = &control[c0 + "<w:sdtContent>".len()..c1];
        if is_toc && inner.trim_start().starts_with("<w:p") {
            let inner = inner.to_string();
            out.replace_range(at..end, &inner);
            from = at;
        } else {
            from = at + 1;
        }
    }
    out
}

/// Mark the paragraphs in a TOC field's result with their level (a marker
/// run at each one's end), after lifting table-of-contents controls. None
/// when there is no TOC field.
pub fn unwrap(document_xml: &str) -> Option<String> {
    if !document_xml.contains("TOC \\") && !document_xml.contains("TOC\\") {
        return None;
    }
    let xml = lift_controls(document_xml);
    let mut out = String::with_capacity(xml.len());
    // The fields open at this point: (instruction, in its result).
    let mut fields: Vec<(String, bool)> = Vec::new();
    let in_toc = |fields: &[(String, bool)]| fields.iter().any(|(instr, result)| *result && instr.trim_start().starts_with("TOC"));
    let mut para: Option<(u8, bool)> = None; // (level, marked)
    let mut in_instr = false;
    let mut i = 0;
    let mut found = false;
    while let Some(off) = xml[i..].find('<') {
        let at = i + off;
        let text = &xml[i..at];
        if in_instr {
            if let Some(f) = fields.last_mut() {
                f.0.push_str(text);
            }
        }
        out.push_str(text);
        let Some(end) = xml[at..].find('>').map(|e| at + e + 1) else {
            out.push_str(&xml[at..]);
            break;
        };
        let tag = &xml[at..end];
        let closing = tag.starts_with("</");
        let name = tag[if closing { 2 } else { 1 }..].split([' ', '>', '/']).next().unwrap_or("");
        match (name, closing) {
            ("w:p", false) if !tag.ends_with("/>") => para = Some((1, false)),
            ("w:pStyle", false) => {
                if let (Some(p), Some(v)) = (para.as_mut(), attr(tag, "w:val")) {
                    p.0 = level_of(v);
                }
            }
            ("w:fldChar", false) => match attr(tag, "w:fldCharType") {
                Some("begin") => fields.push((String::new(), false)),
                Some("separate") => {
                    if let Some(f) = fields.last_mut() {
                        f.1 = true;
                    }
                }
                Some("end") => {
                    fields.pop();
                }
                _ => {}
            },
            ("w:instrText", false) if !tag.ends_with("/>") => in_instr = true,
            ("w:instrText", true) => in_instr = false,
            ("w:t", false) if !tag.ends_with("/>") && in_toc(&fields) => {
                if let Some(p) = para.as_mut() {
                    p.1 = true;
                }
            }
            ("w:p", true) => {
                if let Some((level, true)) = para.take() {
                    out.push_str(&format!("<w:r><w:t>{BEGIN}{level}{END}</w:t></w:r>"));
                    found = true;
                }
            }
            _ => {}
        }
        out.push_str(tag);
        i = end;
    }
    out.push_str(&xml[i.min(xml.len())..]);
    found.then_some(out)
}

/// Make the paragraphs ending in a level marker table of contents entries:
/// the marker goes, and so do the entry's links (to its heading's
/// bookmark) and read tab stops (the layout sets its own).
pub fn restore(doc: &mut Document) {
    for p in &mut doc.paragraphs {
        let Some(last) = p.runs.last_mut() else { continue };
        let Some(b) = last.text.rfind(BEGIN) else { continue };
        let Some(level) = last.text[b + BEGIN.len_utf8()..].trim_end_matches(END).parse::<u8>().ok() else { continue };
        last.text.truncate(b);
        if last.text.is_empty() {
            p.runs.pop();
        }
        p.style.toc = Some(level.clamp(1, 9));
        p.style.tab_stops_pt.clear();
        for r in &mut p.runs {
            if r.style.link.as_deref().is_none_or(|l| l.starts_with('#') || !l.contains(':')) {
                r.style.link = None;
            }
        }
        // Equal neighbours merge, as the model keeps them.
        let mut runs: Vec<crate::model::Run> = Vec::new();
        for r in p.runs.drain(..) {
            match runs.last_mut() {
                Some(l) if l.style == r.style && !crate::layout::is_object(l) && !crate::layout::is_object(&r) => l.text.push_str(&r.text),
                _ => runs.push(r),
            }
        }
        p.runs = runs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_table_of_contents_is_found_in_its_control() {
        let xml = concat!(
            "<w:body><w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val=\"Table of Contents\"/></w:docPartObj></w:sdtPr><w:sdtContent>",
            "<w:p><w:pPr><w:pStyle w:val=\"TOCHeading\"/></w:pPr><w:r><w:t>Contents</w:t></w:r></w:p>",
            "<w:p><w:pPr><w:pStyle w:val=\"TOC1\"/></w:pPr><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> TOC \\o \"1-3\" \\h \\z \\u </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>",
            "<w:hyperlink w:anchor=\"_Toc1\"><w:r><w:t>Intro</w:t></w:r><w:r><w:tab/></w:r><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText> PAGEREF _Toc1 \\h </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:hyperlink></w:p>",
            "<w:p><w:pPr><w:pStyle w:val=\"TOC2\"/></w:pPr><w:r><w:t>Detail</w:t></w:r></w:p>",
            "<w:p><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>",
            "</w:sdtContent></w:sdt><w:p><w:r><w:t>Body</w:t></w:r></w:p></w:body>"
        );
        let read = unwrap(xml).unwrap();
        assert!(!read.contains("w:sdt"), "{read}");
        assert!(!read.contains("Contents</w:t></w:r><w:r><w:t>\u{E008}"), "the title is outside the field");
        assert!(read.contains(&format!("<w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:hyperlink><w:r><w:t>{BEGIN}1{END}</w:t></w:r></w:p>")), "{read}");
        assert!(read.contains(&format!("Detail</w:t></w:r><w:r><w:t>{BEGIN}2{END}</w:t></w:r></w:p>")), "{read}");
        assert!(read.ends_with("<w:p><w:r><w:t>Body</w:t></w:r></w:p></w:body>"), "{read}");
    }

    #[test]
    fn markers_become_the_field() {
        let xml = format!("<w:p><w:r><w:t>{}</w:t></w:r><w:r><w:t>A</w:t></w:r></w:p><w:p><w:r><w:t>B</w:t></w:r><w:r><w:t>{}</w:t></w:r></w:p>", begin_marker(), end_marker());
        let written = wrap(&xml);
        assert!(written.starts_with("<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> TOC \\o &quot;1-3&quot;"), "{written}");
        assert!(written.ends_with("<w:t>B</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>"), "{written}");
        assert_eq!(level_of("Contents3"), 3);
        assert_eq!(level_of("TOC 2"), 2);
        assert_eq!(level_of("TOCHeading"), 1);
    }
}
