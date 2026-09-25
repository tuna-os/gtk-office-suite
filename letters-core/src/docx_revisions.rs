// SPDX-License-Identifier: GPL-3.0-or-later
//
// docx_revisions.rs — tracked changes in .docx: Word's `w:ins` and `w:del`.
//
// A run with a revision (`RunStyle::revision`) is written inside `w:ins`
// or `w:del` (its text as `w:delText`), with the author, date and an id; a
// deletion of someone else's pending insertion is `w:del` inside `w:ins`.
// rdocx neither writes these nor reads the runs inside them, so, as for
// smart chips (docx_chips.rs):
// - writing: the writer puts sentinels around each such run's text and
//   `wrap` turns every sentinel-carrying run into the revision;
// - reading: `unwrap` turns `w:ins`/`w:del` into plain runs whose text is
//   bracketed by sentinels naming the revision, before rdocx sees the file,
//   and `restore` makes those stretches revisions again.
//
// Limits: only text runs carry revisions in .docx (an image or footnote
// reference is written as it is); paragraph-mark and formatting revisions
// are not read or written; moves (`w:moveFrom`/`w:moveTo`) read as plain
// text.

use crate::model::{Document, Revision, RevisionKind, Run};

const OPEN: char = '\u{E002}';
const MID: char = '\u{E003}';
const CLOSE: char = '\u{E004}';

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// `text` bracketed as revision `index`'s.
pub fn bracket(index: usize, text: &str) -> String {
    format!("{OPEN}{index}{MID}{text}{CLOSE}")
}

/// The attribute `name` of the tag starting at `xml[at..]`.
fn attr(xml: &str, at: usize, name: &str) -> Option<String> {
    let tag = &xml[at..at + xml[at..].find('>')?];
    let key = format!(" {name}=\"");
    let v = tag.find(&key)? + key.len();
    Some(unescape(&tag[v..v + tag[v..].find('"')?]))
}

fn open_tag(kind: RevisionKind, id: usize, rev: &Revision) -> String {
    let name = if kind == RevisionKind::Insert { "w:ins" } else { "w:del" };
    format!("<{name} w:id=\"{}\" w:author=\"{}\" w:date=\"{}\">", 9000 + id, escape(&rev.author), escape(&rev.date))
}

/// Wrap each run carrying a bracket in its revision's `w:ins`/`w:del`, and
/// strip the brackets. `revisions[i]` is bracket `i`'s.
pub fn wrap(document_xml: &str, revisions: &[Revision]) -> String {
    let mut xml = document_xml.to_string();
    let mut ids = 0;
    for (i, rev) in revisions.iter().enumerate() {
        let open = format!("{OPEN}{i}{MID}");
        let Some(pos) = xml.find(&open) else { continue };
        let run = [xml[..pos].rfind("<w:r>"), xml[..pos].rfind("<w:r ")].into_iter().flatten().max();
        let Some(run) = run else { continue };
        let Some(end) = xml[pos..].find("</w:r>").map(|e| pos + e + "</w:r>".len()) else { continue };
        // The text may begin or end with a space once the brackets are
        // gone; without xml:space="preserve" readers strip it.
        let mut inner = xml[run..end]
            .replacen(&open, "", 1)
            .replacen(CLOSE, "", 1)
            .replace("<w:t>", "<w:t xml:space=\"preserve\">");
        let deleted = rev.kind == RevisionKind::Delete;
        if deleted {
            inner = inner.replace("<w:t>", "<w:delText>").replace("<w:t ", "<w:delText ").replace("</w:t>", "</w:delText>");
        }
        let mut wrapped = format!("{}{inner}{}", open_tag(rev.kind, ids, rev), if deleted { "</w:del>" } else { "</w:ins>" });
        ids += 1;
        if let (true, Some(under)) = (deleted, rev.under.as_deref()) {
            wrapped = format!("{}{wrapped}</w:ins>", open_tag(RevisionKind::Insert, ids, under));
            ids += 1;
        }
        xml.replace_range(run..end, &wrapped);
    }
    xml
}

/// Turn `w:ins`/`w:del` into plain runs whose text is bracketed as the
/// revision it was in (`w:delText` becomes `w:t`); returns the revisions,
/// indexed by bracket. Self-closing `w:ins`/`w:del` (paragraph-mark
/// revisions) are dropped.
pub fn unwrap(document_xml: &str) -> (String, Vec<Revision>) {
    let xml = document_xml;
    let mut out = String::with_capacity(xml.len());
    let mut revisions: Vec<Revision> = Vec::new();
    // The open revisions: (tag, index into `revisions` or None if unread).
    let mut stack: Vec<(&'static str, Option<usize>)> = Vec::new();
    let mut i = 0;
    while let Some(off) = xml[i..].find('<') {
        let at = i + off;
        out.push_str(&xml[i..at]);
        let Some(end) = xml[at..].find('>').map(|e| at + e + 1) else {
            out.push_str(&xml[at..]);
            return (out, revisions);
        };
        let tag = &xml[at..end];
        // "w:t", "/w:ins" (a closing tag keeps its slash), "w:ins" of "<w:ins/>".
        let name = tag[1..].split([' ', '>']).next().unwrap_or("").trim_end_matches('/');
        let self_closing = tag.ends_with("/>");
        let current = || stack.iter().rev().find_map(|(_, r)| *r);
        match name {
            "w:ins" | "w:del" if !self_closing => {
                let kind = if name == "w:ins" { RevisionKind::Insert } else { RevisionKind::Delete };
                let under = stack.iter().rev().find_map(|(t, r)| (*t == "w:ins").then_some(*r).flatten()).map(|u| Box::new(revisions[u].clone()));
                revisions.push(Revision {
                    kind,
                    author: attr(xml, at, "w:author").unwrap_or_default(),
                    date: attr(xml, at, "w:date").unwrap_or_default(),
                    under: if kind == RevisionKind::Delete { under } else { None },
                });
                stack.push((if kind == RevisionKind::Insert { "w:ins" } else { "w:del" }, Some(revisions.len() - 1)));
            }
            "w:ins" | "w:del" => {}
            "/w:ins" | "/w:del" => {
                stack.pop();
            }
            "w:t" | "w:delText" if !self_closing => {
                let Some(close) = xml[end..].find(if name == "w:t" { "</w:t>" } else { "</w:delText>" }).map(|c| end + c) else {
                    out.push_str(tag);
                    i = end;
                    continue;
                };
                let text = &xml[end..close];
                let open = format!("<w:t{}", &tag[name.len() + 1..]);
                match current() {
                    Some(r) if !text.is_empty() => out.push_str(&format!("{open}{}</w:t>", bracket(r, text))),
                    _ => out.push_str(&format!("{open}{text}</w:t>")),
                }
                i = close + if name == "w:t" { "</w:t>".len() } else { "</w:delText>".len() };
                continue;
            }
            _ => out.push_str(tag),
        }
        i = end;
    }
    out.push_str(&xml[i..]);
    (out, revisions)
}

/// Split the runs of `doc` at brackets and give each bracketed stretch its
/// revision.
pub fn restore(doc: &mut Document, revisions: &[Revision]) {
    if revisions.is_empty() {
        return;
    }
    for p in &mut doc.paragraphs {
        if !p.runs.iter().any(|r| r.text.contains(OPEN)) {
            continue;
        }
        let mut runs: Vec<Run> = Vec::new();
        let push = |runs: &mut Vec<Run>, text: &str, style: &crate::model::RunStyle| {
            if text.is_empty() {
                return;
            }
            match runs.last_mut() {
                Some(last) if &last.style == style && !crate::layout::is_object(last) => last.text.push_str(text),
                _ => runs.push(Run { text: text.to_string(), style: style.clone() }),
            }
        };
        for run in p.runs.drain(..) {
            if crate::layout::is_object(&run) {
                runs.push(run);
                continue;
            }
            let mut rest = run.text.as_str();
            while let Some(a) = rest.find(OPEN) {
                push(&mut runs, &rest[..a], &run.style);
                let body = &rest[a + OPEN.len_utf8()..];
                let (Some(m), Some(c)) = (body.find(MID), body.find(CLOSE)) else { break };
                let mut style = run.style.clone();
                style.revision = body[..m].parse::<usize>().ok().and_then(|i| revisions.get(i)).cloned();
                push(&mut runs, &body[m + MID.len_utf8()..c], &style);
                rest = &body[c + CLOSE.len_utf8()..];
            }
            push(&mut runs, rest, &run.style);
        }
        p.runs = runs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rev(kind: RevisionKind, author: &str) -> Revision {
        Revision { kind, author: author.into(), date: "2026-09-25T20:30:00Z".into(), under: None }
    }

    #[test]
    fn bracketed_runs_become_ins_and_del_and_back() {
        let ins = rev(RevisionKind::Insert, "Ada & co");
        let del = Revision { under: Some(Box::new(ins.clone())), ..rev(RevisionKind::Delete, "Grace") };
        let xml = format!(
            "<w:p><w:r><w:t>keep </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r><w:r><w:t>{}</w:t></w:r></w:p>",
            bracket(0, "added "),
            bracket(1, "gone")
        );
        let revisions = vec![ins.clone(), del.clone()];
        let written = wrap(&xml, &revisions);
        assert!(written.contains("<w:ins w:id=\"9000\" w:author=\"Ada &amp; co\""), "{written}");
        assert!(written.contains("<w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">added </w:t></w:r></w:ins>"), "{written}");
        assert!(written.contains("<w:del w:id=\"9001\" w:author=\"Grace\""), "{written}");
        assert!(written.contains("<w:delText xml:space=\"preserve\">gone</w:delText></w:r></w:del></w:ins>"), "a deleted insertion nests: {written}");
        assert!(!written.contains(OPEN));
        let (read, found) = unwrap(&written);
        assert!(!read.contains("w:ins") && !read.contains("w:del") && !read.contains("delText"), "{read}");
        assert_eq!(found[0], ins);
        // The second found is the insertion the deletion sits in; the third
        // the deletion, which remembers it.
        assert_eq!(found[2], del);
        assert!(read.contains(&bracket(2, "gone")), "{read}");
    }

    #[test]
    fn restore_splits_runs_and_ignores_paragraph_mark_revisions() {
        let (read, found) = unwrap("<w:p><w:pPr><w:rPr><w:ins w:id=\"1\" w:author=\"A\" w:date=\"d\"/></w:rPr></w:pPr><w:r><w:t>a</w:t></w:r><w:ins w:id=\"2\" w:author=\"B\" w:date=\"d\"><w:r><w:t>b</w:t></w:r></w:ins></w:p>");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].author, "B");
        let mut doc = Document::from_plain_text(&format!("a{}", bracket(0, "b")));
        restore(&mut doc, &found);
        let runs = &doc.paragraphs[0].runs;
        assert_eq!((runs.len(), runs[1].text.as_str(), runs[1].style.revision.as_ref().map(|r| r.author.as_str())), (2, "b", Some("B")));
        assert!(read.contains(&bracket(0, "b")));
    }
}
