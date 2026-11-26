// SPDX-License-Identifier: GPL-3.0-or-later
//! XML-level helpers for the PPTX reader (#1668).
//!
//! Contract: these take XML events or parsed values and return model-neutral
//! values. They never touch the ZIP archive; only `read_pptx` and
//! `package` own package traversal.

use quick_xml::events::{BytesRef, BytesStart, BytesText};
use suite_common_core::document::Run;

pub(crate) fn unescape_text(t: &BytesText) -> String {
    // quick-xml 0.42 dropped `decode()`: event payloads are already `&str`.
    quick_xml::escape::unescape(t)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| t.to_string())
}

pub(crate) fn resolve_general_ref(r: &BytesRef) -> String {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return c.to_string();
    }
    let name: &str = r;
    quick_xml::escape::resolve_predefined_entity(name)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("&{name};"))
}

/// Close the paragraph a shape has been accumulating, so the next one starts
/// on a new line.
///
/// A text box's paragraphs are `a:p` elements and its runs are the `a:r`
/// inside them, and the two are *not* interchangeable: the runs of one
/// paragraph are consecutive pieces of a single line, while two paragraphs
/// are two lines. Both readers used to record one entry per `a:t` and join
/// the lot with `\n`, which is right only when every paragraph holds exactly
/// one run — so a decoration reading "Plain **Bold**" came back as two
/// lines, and the break was then written into the saved package.
///
/// The break is carried *inside* the runs rather than only in the derived
/// text, because `SlideObject::TextBox` requires concatenated run text to
/// equal `text`. A paragraph that contributed no run of its own cannot
/// carry it: a blank paragraph between runs becomes a second break on the
/// run before, and blank paragraphs ahead of the first run lead its text
/// (`push_run`).
pub(super) fn close_paragraphs(runs: &mut [Run], count: usize) {
    if let Some(last) = runs.last_mut() {
        for _ in 0..count {
            last.text.push('\n');
        }
    }
}

/// The web or mail address an `a:hlinkClick` on a run's properties links
/// to, through the part's external hyperlink relationships `links`
/// (#1998). A click action (`ppaction://` jumps to a slide, a program, a
/// macro) is not an address, and neither is a relationship that is not
/// external: those are not kept, and the save warning says so.
pub(crate) fn run_link(e: &BytesStart, links: &std::collections::HashMap<String, String>) -> Option<String> {
    let mut id = None;
    for a in e.attributes().flatten() {
        let value = a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.into_owned();
        match a.key.as_ref() {
            "r:id" => id = Some(value),
            "action" if !value.is_empty() => return None,
            _ => {}
        }
    }
    links.get(&id?).filter(|t| !t.is_empty()).cloned()
}

/// The `(text, runs)` pair a `TextBox` is built from.
///
/// `text` is *derived* from the runs rather than accumulated beside them,
/// which is what makes the model's invariant structural instead of a thing
/// each walker has to remember.
pub(super) fn text_of(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

/// A shape's `p:cNvPr id`.
pub(super) fn cnvpr_id(e: &BytesStart) -> Option<u32> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == "id")
        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.parse().ok())
}
