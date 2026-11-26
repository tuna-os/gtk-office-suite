// SPDX-License-Identifier: GPL-3.0-or-later
//! ODF XML primitives: text unescaping, escaping and attribute/length helpers.

#![allow(unused_imports)]
use super::*;

pub(super) fn unescape_text(t: &BytesText) -> String {
    // quick-xml 0.42 dropped `decode()`: event payloads are already `&str`.
    quick_xml::escape::unescape(t)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| t.to_string())
}

// quick-xml 0.41 stopped inlining `&entity;`/`&#NN;` references into
// Event::Text — they now arrive as a separate Event::GeneralRef that must
// be resolved and appended alongside surrounding text, or escaped
// characters silently vanish from read documents.
pub(super) fn resolve_general_ref(r: &BytesRef) -> String {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return c.to_string();
    }
    let name: &str = r;
    quick_xml::escape::resolve_predefined_entity(name)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("&{name};"))
}

pub(super) fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(super) fn attr_val(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    e.attributes().filter_map(|a| a.ok()).find_map(|a| {
        if a.key.into_inner() == name {
            // Attribute values are escaped XML too: a link to "?a=1&b=2"
            // is written "&amp;" and read back as "&amp;" without this.
            let raw = a.value.to_string();
            Some(quick_xml::escape::unescape(&raw).map(|s| s.into_owned()).unwrap_or(raw))
        } else {
            None
        }
    })
}

/// Text with its tabs and its runs of spaces as ODF's elements: raw, they
/// are whitespace a reader collapses to one space.
pub(super) fn odf_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut spaces = 0;
    // A run of spaces is one element: a space left as text beside an
    // element (a page field) is whitespace a reader drops.
    let flush = |out: &mut String, n: usize, at_start: bool| match n {
        0 => {}
        1 if !at_start => out.push(' '),
        n => out.push_str(&format!("<text:s text:c=\"{n}\"/>")),
    };
    for c in text.chars() {
        if c == ' ' {
            spaces += 1;
            continue;
        }
        let at_start = out.is_empty();
        flush(&mut out, spaces, at_start);
        spaces = 0;
        if c == '\t' {
            out.push_str("<text:tab/>");
        } else {
            out.push(c);
        }
    }
    let at_start = out.is_empty();
    flush(&mut out, spaces, at_start);
    out
}

/// Parse an ODF length ("595.3pt", "21cm", "210mm", "8.5in") to points.
pub(super) fn parse_length_pt(v: &str) -> Option<f64> {
    let v = v.trim();
    let (num, unit) = v.split_at(v.find(|c: char| c.is_ascii_alphabetic())?);
    let n: f64 = num.parse().ok()?;
    Some(match unit {
        "pt" => n,
        "cm" => n * 72.0 / 2.54,
        "mm" => n * 72.0 / 25.4,
        "in" => n * 72.0,
        _ => return None,
    })
}
