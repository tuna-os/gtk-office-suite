//! Escaping for the documents the reader builds, and the wrapper that
//! makes a tag-less body readable.

use crate::encoding::escape_html;

/// Escape a string for use inside a double-quoted HTML **attribute** value
/// (e.g. `srcdoc`), and nothing else.
///
/// This deliberately leaves `<` and `>` alone — inside a quoted attribute they
/// are ordinary characters, and the `srcdoc` payload is a whole HTML document
/// that must survive intact. It is therefore **not** safe for element text
/// content: use [`crate::encoding::escape_html`] there.
pub(super) fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;")
}

/// The worker stores ready-to-render HTML, but cached bodies from older versions
/// (or odd messages) may be tag-less plain text — wrap those so they read well.
pub(super) fn body_html(body: &str) -> String {
    if body.contains('<') {
        body.to_string()
    } else {
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><style>\
             body{{margin:0;padding:20px;font:14px/1.5 system-ui,sans-serif;\
             white-space:pre-wrap;word-wrap:break-word}}\
             </style></head><body class=\"hylki-plain\">{}</body></html>",
            escape_html(body)
        )
    }
}
