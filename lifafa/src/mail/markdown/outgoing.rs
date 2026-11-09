//! The HTML that goes on the wire.

/// The sanitizer every user-authored body goes through on its way out: the
/// HTML a message is written in by hand, and the HTML a Markdown source
/// renders to (Markdown may carry raw HTML of its own).
///
/// It is the user's own material, so this is not a defence against them —
/// it is a defence for them. A `<script>` that reaches a recipient will at
/// best be stripped by their client and at worst put the message in the
/// spam folder, and a `<style>` block restyles whatever it lands in. The
/// structure mail actually needs — tables, inline styles, images, the tags
/// Markdown emits — all survives.
pub(crate) fn sanitize_outgoing(html: &str) -> String {
    use std::collections::HashSet;
    let mut b = ammonia::Builder::default();
    b.add_tags([
        "table",
        "thead",
        "tbody",
        "tfoot",
        "tr",
        "td",
        "th",
        "font",
        "center",
        "u",
        "span",
        "img",
        "mark",
        "sup",
        "sub",
        "del",
        "ins",
        "kbd",
        "samp",
        "var",
        "dl",
        "dt",
        "dd",
        "figure",
        "figcaption",
        "input",
        "section",
        "article",
        "details",
        "summary",
        "small",
        "big",
    ])
    .add_generic_attributes([
        "style",
        "class",
        "id",
        "align",
        "valign",
        "width",
        "height",
        "bgcolor",
        "border",
        "cellpadding",
        "cellspacing",
        "color",
        "face",
        "size",
        "colspan",
        "rowspan",
        "start",
        "dir",
        "lang",
    ])
    .add_tag_attributes("img", ["src", "alt", "width", "height", "title"])
    // Task lists: the box is an <input>, and a disabled one carries no
    // behaviour of any kind.
    .add_tag_attributes("input", ["type", "checked", "disabled"])
    .url_schemes(HashSet::from([
        "http", "https", "mailto", "tel", "data", "cid",
    ]))
    .link_rel(None);
    b.clean(html).to_string()
}

/// Break already-written HTML onto one line per block, so switching a
/// message into the HTML view shows something a person can edit rather than
/// one unbroken line. Nothing inside a `<pre>` is touched, where a line
/// break would change the message.
pub(crate) fn pretty_html(html: &str) -> String {
    const BLOCKS: &[&str] = &[
        "p",
        "div",
        "table",
        "thead",
        "tbody",
        "tfoot",
        "tr",
        "ul",
        "ol",
        "li",
        "blockquote",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hr",
        "pre",
        "dl",
        "dt",
        "dd",
        "section",
        "article",
    ];
    let mut out = String::with_capacity(html.len() + 64);
    let mut rest = html;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if tail.starts_with("<pre") {
            // Straight through to the end of the block.
            let end = tail.find("</pre>").map(|p| p + 6).unwrap_or(tail.len());
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&tail[..end]);
            out.push('\n');
            rest = &tail[end..];
            continue;
        }
        let name: String = tail
            .trim_start_matches('<')
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        let is_block_tag = BLOCKS.contains(&name.as_str());
        let closing = tail.starts_with("</");
        let end = tail.find('>').map(|p| p + 1).unwrap_or(tail.len());
        if is_block_tag && !closing && !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&tail[..end]);
        if is_block_tag && closing {
            out.push('\n');
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    // Collapse the runs of blank lines the breaks above can leave behind.
    let mut cleaned = String::with_capacity(out.len());
    for line in out.lines() {
        let line = line.trim_end();
        if line.is_empty() && cleaned.ends_with("\n\n") {
            continue;
        }
        cleaned.push_str(line);
        cleaned.push('\n');
    }
    cleaned.trim_matches('\n').to_string()
}
