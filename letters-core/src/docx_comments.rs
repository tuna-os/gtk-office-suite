// SPDX-License-Identifier: GPL-3.0-or-later
//
// docx_comments.rs — comments in .docx: Word's comments part.
//
// A thread's anchor is `w:commentRangeStart` … `w:commentRangeEnd` in the
// text, followed by a run with its `w:commentReference`; its comments are
// `w:comment`s in word/comments.xml, and word/commentsExtended.xml says
// which comment replies to which (`w15:paraIdParent`) and which threads
// are resolved (`w15:done`). A reply has range markers and a reference
// beside its thread's, as Word writes them, since readers only take a
// comment that the text refers to.
//
// rdocx places comments by run index, which the writer does not keep,
// and hands back its range markers only as paragraph items, so, as for
// chips and tracked changes:
// - writing: the writer puts a marker run where a thread's anchor starts
//   and ends, `wrap` turns the markers into the range elements, and
//   `add_parts` adds the two parts with their relationships;
// - reading: `unwrap` turns the range elements into marker runs before
//   rdocx sees the file (and drops the reference runs), and `restore`
//   marks the text between as the thread's (`crate::comments`).
//
// A thread whose text was deleted is written at the start of the document
// with an empty range, which reads back as the same orphan.

use crate::model::{Comment, Document, Run};

const START: char = '\u{E005}';
const END: char = '\u{E006}';
const STOP: char = '\u{E007}';

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The marker run text for the start (`start`) or end of thread `id`.
pub fn marker(id: u32, start: bool) -> String {
    format!("{}{id}{STOP}", if start { START } else { END })
}

/// What to write before a run commented by `want`, as (thread, start):
/// the ends of the open threads it is not in, then the starts of those it
/// opens. `open` is updated. (The .odt writer uses it too.)
pub fn transition(open: &mut Vec<u32>, want: &[u32]) -> Vec<(u32, bool)> {
    let mut out: Vec<(u32, bool)> = open.iter().filter(|id| !want.contains(id)).map(|id| (*id, false)).collect();
    open.retain(|id| want.contains(id));
    for id in want {
        if !open.contains(id) {
            out.push((*id, true));
            open.push(*id);
        }
    }
    out
}

/// A date as the model keeps it: to the second, in UTC ("Z"). LibreOffice
/// writes ODF dates without a zone, and may add fractions of a second.
pub fn normalize_date(date: &str) -> String {
    match date.get(..19) {
        Some(d) if !date[19..].starts_with(['+', '-']) => format!("{d}Z"),
        _ => date.to_string(),
    }
}

/// The thread (root) comments a document's runs should be marked with:
/// only ids that have a first comment.
pub fn roots(doc: &Document) -> Vec<u32> {
    doc.comments.iter().filter(|c| c.parent.is_none()).map(|c| c.id).collect()
}

/// A run's comment ids that are threads.
pub fn threads_of<'a>(roots: &'a [u32], ids: &'a [u32]) -> impl Iterator<Item = u32> + 'a {
    ids.iter().copied().filter(|id| roots.contains(id))
}

/// Turn every marker run into its range element (an end also gets the
/// reference run), with the thread's replies beside it.
pub fn wrap(document_xml: &str, comments: &[Comment]) -> String {
    let mut xml = document_xml.to_string();
    let replies = |id: u32| -> Vec<u32> { comments.iter().filter(|c| c.parent == Some(id)).map(|c| c.id).collect() };
    for c in comments.iter().filter(|c| c.parent.is_none()) {
        let ids: Vec<u32> = std::iter::once(c.id).chain(replies(c.id)).collect();
        for start in [true, false] {
            let text = marker(c.id, start);
            let Some(pos) = xml.find(&text) else { continue };
            let run = [xml[..pos].rfind("<w:r>"), xml[..pos].rfind("<w:r ")].into_iter().flatten().max();
            let Some(run) = run else { continue };
            let Some(end) = xml[pos..].find("</w:r>").map(|e| pos + e + "</w:r>".len()) else { continue };
            let elements: String = ids
                .iter()
                .map(|id| {
                    if start {
                        format!("<w:commentRangeStart w:id=\"{id}\"/>")
                    } else {
                        format!("<w:commentRangeEnd w:id=\"{id}\"/><w:r><w:commentReference w:id=\"{id}\"/></w:r>")
                    }
                })
                .collect();
            xml.replace_range(run..end, &elements);
        }
    }
    xml
}

fn para_id(id: u32) -> String {
    format!("{:08X}", 0x0C00_0000 + id)
}

fn initials(author: &str) -> String {
    author.split_whitespace().filter_map(|w| w.chars().next()).collect()
}

/// word/comments.xml for `comments`.
pub fn comments_xml(comments: &[Comment]) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:comments xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\">",
    );
    for c in comments {
        xml.push_str(&format!(
            "<w:comment w:id=\"{}\" w:author=\"{}\" w:date=\"{}\" w:initials=\"{}\">",
            c.id,
            escape(&c.author),
            escape(&c.date),
            escape(&initials(&c.author))
        ));
        let lines: Vec<&str> = c.text.split('\n').collect();
        for (i, line) in lines.iter().enumerate() {
            // commentsExtended names a comment by its last paragraph.
            let id = if i + 1 == lines.len() { format!(" w14:paraId=\"{}\"", para_id(c.id)) } else { String::new() };
            xml.push_str(&format!("<w:p{id}><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>", escape(line)));
        }
        xml.push_str("</w:comment>");
    }
    xml.push_str("</w:comments>");
    xml
}

/// word/commentsExtended.xml: replies' parents and resolved threads.
pub fn comments_extended_xml(comments: &[Comment]) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w15:commentsEx xmlns:w15=\"http://schemas.microsoft.com/office/word/2012/wordml\">",
    );
    for c in comments {
        let parent = c.parent.map(|p| format!(" w15:paraIdParent=\"{}\"", para_id(p))).unwrap_or_default();
        xml.push_str(&format!("<w15:commentEx w15:paraId=\"{}\"{parent} w15:done=\"{}\"/>", para_id(c.id), u8::from(c.resolved)));
    }
    xml.push_str("</w15:commentsEx>");
    xml
}

const COMMENTS_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml";
const EXTENDED_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml";
const COMMENTS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
const EXTENDED_REL: &str = "http://schemas.microsoft.com/office/2011/relationships/commentsExtended";

/// `package` with the comments parts, their content types and the
/// document's relationships to them.
pub fn add_parts(package: &[u8], comments: &[Comment]) -> Result<Vec<u8>, String> {
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(package)).map_err(|e| e.to_string())?;
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let new_parts = [("word/comments.xml", comments_xml(comments)), ("word/commentsExtended.xml", comments_extended_xml(comments))];
    for i in 0..zin.len() {
        let mut part = zin.by_index(i).map_err(|e| e.to_string())?;
        let name = part.name().to_string();
        if new_parts.iter().any(|(n, _)| *n == name) {
            continue;
        }
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut part, &mut data).map_err(|e| e.to_string())?;
        if name == "[Content_Types].xml" || name == "word/_rels/document.xml.rels" {
            let mut xml = String::from_utf8(data).map_err(|e| e.to_string())?;
            let (close, entries) = if name == "[Content_Types].xml" {
                (
                    "</Types>",
                    format!(
                        "<Override PartName=\"/word/comments.xml\" ContentType=\"{COMMENTS_TYPE}\"/><Override PartName=\"/word/commentsExtended.xml\" ContentType=\"{EXTENDED_TYPE}\"/>"
                    ),
                )
            } else {
                (
                    "</Relationships>",
                    format!(
                        "<Relationship Id=\"rIdLettersComments\" Type=\"{COMMENTS_REL}\" Target=\"comments.xml\"/><Relationship Id=\"rIdLettersCommentsEx\" Type=\"{EXTENDED_REL}\" Target=\"commentsExtended.xml\"/>"
                    ),
                )
            };
            if !xml.contains("/word/comments.xml") && !xml.contains("\"comments.xml\"") {
                if let Some(at) = xml.rfind(close) {
                    xml.insert_str(at, &entries);
                }
            }
            data = xml.into_bytes();
        }
        out.start_file(name, options).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut out, &data).map_err(|e| e.to_string())?;
    }
    for (name, xml) in new_parts {
        out.start_file(name, options).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut out, xml.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(out.finish().map_err(|e| e.to_string())?.into_inner())
}

/// The `w:id` of the tag at `xml[at..]`.
fn id_of(tag: &str) -> Option<u32> {
    let v = tag.find("w:id=\"")? + "w:id=\"".len();
    tag[v..v + tag[v..].find('"')?].parse().ok()
}

/// Turn range elements inside paragraphs into marker runs and drop the
/// reference runs. None when there are none.
pub fn unwrap(document_xml: &str) -> Option<String> {
    if !document_xml.contains("<w:commentRange") && !document_xml.contains("<w:commentReference") {
        return None;
    }
    let xml = document_xml;
    let mut out = String::with_capacity(xml.len());
    let mut depth = 0usize; // inside how many w:p
    let mut i = 0;
    while let Some(off) = xml[i..].find('<') {
        let at = i + off;
        out.push_str(&xml[i..at]);
        let Some(end) = xml[at..].find('>').map(|e| at + e + 1) else {
            out.push_str(&xml[at..]);
            return Some(out);
        };
        let tag = &xml[at..end];
        let name = tag[1..].split([' ', '>', '/']).next().unwrap_or("");
        let closing = tag.starts_with("</");
        match name {
            "w:p" if !tag.ends_with("/>") => depth += 1,
            "" if closing && tag == "</w:p>" => depth = depth.saturating_sub(1),
            "w:commentRangeStart" | "w:commentRangeEnd" => {
                if let (true, Some(id)) = (depth > 0, id_of(tag)) {
                    out.push_str(&format!("<w:r><w:t>{}</w:t></w:r>", marker(id, name == "w:commentRangeStart")));
                }
                i = end;
                continue;
            }
            "w:r" if !closing => {
                // A run holding only a comment reference (and its style).
                if let Some(close) = xml[end..].find("</w:r>").map(|c| end + c) {
                    let body = &xml[end..close];
                    if body.contains("<w:commentReference") && !body.contains("<w:t") {
                        i = close + "</w:r>".len();
                        continue;
                    }
                }
            }
            _ => {}
        }
        out.push_str(tag);
        i = end;
    }
    out.push_str(&xml[i..]);
    Some(out)
}

/// The comments rdocx read from the comments parts, as the model's.
pub fn bodies(doc: &rdocx::Document) -> Vec<Comment> {
    let mut out: Vec<Comment> = doc
        .comments()
        .iter()
        .filter(|c| c.id() >= 0)
        .map(|c| Comment {
            id: c.id() as u32,
            author: c.author().unwrap_or_default().to_string(),
            date: normalize_date(c.date().unwrap_or_default()),
            text: c.text(),
            resolved: c.resolved(),
            parent: c.parent_id().filter(|p| *p >= 0).map(|p| p as u32),
        })
        .collect();
    out.sort_by_key(|c| c.id);
    out.dedup_by_key(|c| c.id);
    // A reply to a reply belongs to the thread; a thread is resolved when
    // its first comment is (as Word shows it).
    let parent_of = |id: u32| out.iter().find(|c| c.id == id).and_then(|c| c.parent);
    let roots: Vec<Option<u32>> = out
        .iter()
        .map(|c| {
            let mut p = c.parent?;
            for _ in 0..out.len() {
                match parent_of(p) {
                    Some(q) if q != p => p = q,
                    _ => break,
                }
            }
            Some(p).filter(|p| out.iter().any(|c| c.id == *p))
        })
        .collect();
    for (c, root) in out.iter_mut().zip(roots) {
        c.parent = root.filter(|r| *r != c.id);
    }
    out
}

/// Strip the markers from `doc`'s text and mark the text between each
/// thread's start and end with it; the document's comments become
/// `comments`.
pub fn restore(doc: &mut Document, comments: Vec<Comment>) {
    let roots: Vec<u32> = comments.iter().filter(|c| c.parent.is_none()).map(|c| c.id).collect();
    doc.comments = comments;
    let mut open: Vec<u32> = Vec::new();
    let with = |style: &crate::model::RunStyle, open: &[u32]| {
        let mut s = style.clone();
        for id in open {
            if let Err(i) = s.comments.binary_search(id) {
                s.comments.insert(i, *id);
            }
        }
        s
    };
    for p in &mut doc.paragraphs {
        let mut runs: Vec<Run> = Vec::new();
        let push = |runs: &mut Vec<Run>, text: &str, style: crate::model::RunStyle| {
            if text.is_empty() {
                return;
            }
            match runs.last_mut() {
                Some(last) if last.style == style && !crate::layout::is_object(last) => last.text.push_str(text),
                _ => runs.push(Run { text: text.to_string(), style }),
            }
        };
        for run in p.runs.drain(..) {
            if crate::layout::is_object(&run) {
                runs.push(Run { style: with(&run.style, &open), text: run.text });
                continue;
            }
            let mut rest = run.text.as_str();
            while let Some(a) = rest.find([START, END]) {
                push(&mut runs, &rest[..a], with(&run.style, &open));
                let start = rest[a..].starts_with(START);
                let body = &rest[a + START.len_utf8()..];
                let Some(s) = body.find(STOP) else { break };
                if let Ok(id) = body[..s].parse::<u32>() {
                    if roots.contains(&id) {
                        open.retain(|x| *x != id);
                        if start {
                            open.push(id);
                        }
                    }
                }
                rest = &body[s + STOP.len_utf8()..];
            }
            push(&mut runs, rest, with(&run.style, &open));
        }
        p.runs = runs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_become_ranges_and_back() {
        let comments = vec![
            Comment { id: 1, author: "Ada Lovelace".into(), date: "2026-09-26T10:00:00Z".into(), text: "Why & how?".into(), resolved: false, parent: None },
            Comment { id: 2, author: "Grace".into(), date: "2026-09-26T10:05:00Z".into(), text: "Because.".into(), resolved: false, parent: Some(1) },
        ];
        let xml = format!(
            "<w:p><w:r><w:t>a</w:t></w:r><w:r><w:t>{}</w:t></w:r><w:r><w:t>b</w:t></w:r><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
            marker(1, true),
            marker(1, false)
        );
        let written = wrap(&xml, &comments);
        assert_eq!(
            written,
            "<w:p><w:r><w:t>a</w:t></w:r><w:commentRangeStart w:id=\"1\"/><w:commentRangeStart w:id=\"2\"/><w:r><w:t>b</w:t></w:r><w:commentRangeEnd w:id=\"1\"/><w:r><w:commentReference w:id=\"1\"/></w:r><w:commentRangeEnd w:id=\"2\"/><w:r><w:commentReference w:id=\"2\"/></w:r></w:p>"
        );
        let read = unwrap(&written).unwrap();
        assert!(!read.contains("comment"), "{read}");
        assert!(comments_xml(&comments).contains("w:author=\"Ada Lovelace\" w:date=\"2026-09-26T10:00:00Z\" w:initials=\"AL\""));
        assert!(comments_extended_xml(&comments).contains("w15:paraIdParent=\"0C000001\""));
        // Reading: the reply's markers are not a thread of their own.
        let mut doc = Document::from_plain_text(&format!("a{}{}b{}{}", marker(1, true), marker(2, true), marker(1, false), marker(2, false)));
        restore(&mut doc, comments.clone());
        let runs = &doc.paragraphs[0].runs;
        assert_eq!(runs.iter().map(|r| (r.text.as_str(), r.style.comments.clone())).collect::<Vec<_>>(), [("a", vec![]), ("b", vec![1])]);
        assert_eq!(doc.comments, comments);
    }

    #[test]
    fn range_markers_outside_paragraphs_are_dropped() {
        let read = unwrap("<w:body><w:commentRangeStart w:id=\"3\"/><w:p><w:r><w:t>x</w:t></w:r><w:commentRangeEnd w:id=\"3\"/></w:p></w:body>").unwrap();
        assert_eq!(read, format!("<w:body><w:p><w:r><w:t>x</w:t></w:r><w:r><w:t>{}</w:t></w:r></w:p></w:body>", marker(3, false)));
    }
}
