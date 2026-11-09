//! Quoting a message in a reply or forward: the original's HTML sanitized for the
//! editor, or its plain text escaped, under an attribution line. Pure functions.

/// The quoted block a reply or a forward opens with: the attribution line,
/// then the original body as it was written.
///
/// The body is untrusted mail, so it goes through the same sanitizer a
/// forward has used since #52. A body that is plain text through and through
/// keeps the escaped-text path.
pub(crate) fn quoted_original(attribution: &str, body: &str) -> String {
    if !body.contains('<') {
        return quote_block(attribution, &message_text(body));
    }
    let quoted = sanitized_body(body);
    quote_block_html(attribution, trim_empty_blocks(&quoted))
}

/// An HTML body made safe for the editor: its plain-text regions hard
/// wrapped, then sanitized as a forward is (#52).
fn sanitized_body(body: &str) -> String {
    sanitize_forward_html(&hard_wrap_plain_regions(body))
}

/// Text escaped for HTML, its line ends as `<br>`.
pub(crate) fn escaped_lines(text: &str) -> String {
    gtk::glib::markup_escape_text(text).replace('\n', "<br>")
}

/// Build the HTML quoted block (attribution line + blockquote) for a reply or
/// forward, from plain text so no scripts/remote content leak into the editor.
fn quote_block(attribution: &str, text: &str) -> String {
    let esc = escaped_lines;
    // Under `pre-wrap`, as a plain-text part from the server is quoted
    // (#436): its spaces are part of the text, and a plain-text reply
    // sends them back as they were.
    format!(
        "<p class=\"hylki-quote-attr\">{}</p><blockquote><div style=\"white-space:pre-wrap\">{}</div></blockquote>",
        esc(attribution),
        esc(text)
    )
}

/// Like [`quote_block`], but the quoted body is already-sanitized HTML.
///
/// A body that sets its own colors is marked `hylki-quote-mail`: the editor
/// lays it on the light ground it was designed for, where a dark composer
/// would leave its dark text unreadable.
fn quote_block_html(attribution: &str, inner_html: &str) -> String {
    let esc = escaped_lines;
    let colored = ["color:", "color=\"", "background:"]
        .iter()
        .any(|c| inner_html.contains(c));
    format!(
        "<p class=\"hylki-quote-attr\">{}</p><blockquote{}>{}</blockquote>",
        esc(attribution),
        if colored {
            " class=\"hylki-quote-mail\""
        } else {
            ""
        },
        inner_html
    )
}

/// A message's own content, ready to be edited rather than quoted: the HTML
/// sanitized exactly as a forward's is (#52, it is the same untrusted mail),
/// or the plain text escaped into it.
pub(crate) fn editable_copy_html(body: &str) -> String {
    if body.contains('<') {
        sanitized_body(body)
    } else {
        let text = message_text(body);
        format!("<p>{}</p>", escaped_lines(&text))
    }
}

/// A readable plain-text rendering of a message body, which may be HTML. Used to
/// build safe quoted replies/forwards (no scripts, styles or remote content).
pub(crate) fn message_text(body: &str) -> String {
    if !body.contains('<') {
        return body.trim().to_string();
    }
    crate::mail::markdown::plain_text(body)
}

/// Drop the empty blocks a quoted body opens or closes with. Every message
/// written in the composer starts on a blank line, and quoting one used to
/// carry that blank line in as the first line of the quote.
fn trim_empty_blocks(html: &str) -> &str {
    const EMPTY: [&str; 8] = [
        "<br>",
        "<br/>",
        "<br />",
        "<p></p>",
        "<div></div>",
        "<p><br></p>",
        "<div><br></div>",
        "&nbsp;",
    ];
    let mut out = html.trim();
    'again: loop {
        for empty in EMPTY {
            if let Some(rest) = out.strip_prefix(empty) {
                out = rest.trim_start();
                continue 'again;
            }
            if let Some(rest) = out.strip_suffix(empty) {
                out = rest.trim_end();
                continue 'again;
            }
        }
        return out;
    }
}

/// Line breaks that live in CSS, written out as `<br>`.
///
/// A plain-text part is rendered under `white-space: pre-wrap`, where its own
/// newlines *are* its line breaks. The sanitizer keeps no styling, so those
/// breaks have to be in the markup before it runs or the quote arrives in the
/// composer as one run-on paragraph.
fn hard_wrap_plain_regions(html: &str) -> String {
    // A whole document of plain text (the worker's `wrap_plain`): the rule is
    // on `body`, so every newline in it is a break.
    if body_is_pre_wrap(html) {
        let open = html
            .find("<body")
            .and_then(|i| html[i..].find('>').map(|j| i + j + 1));
        let close = html.rfind("</body>");
        return match (open, close) {
            (Some(open), Some(close)) if open <= close => format!(
                "{}{}{}",
                &html[..open],
                breaks(&html[open..close]),
                &html[close..]
            ),
            _ => breaks(html),
        };
    }
    // A message of mixed parts (`wrap_fragment`): only its text parts are
    // pre-wrap, each one a `.hylki-plain` div the worker writes itself, with
    // nothing but escaped text and links inside it.
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find("class=\"hylki-plain\"") {
        let Some(open) = rest[at..].find('>').map(|i| at + i + 1) else {
            break;
        };
        let Some(close) = rest[open..].find("</div>").map(|i| open + i) else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(&breaks(&rest[open..close]));
        rest = &rest[close..];
    }
    out.push_str(rest);
    out
}

/// A plain-text region's line ends as `<br>`. Mail arrives with CRLF, and a
/// carriage return left in front of the `<br>` is read back as a second
/// line break, under the `pre-wrap` the quote keeps.
fn breaks(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "<br>")
}

/// Whether a body, as the worker renders it, came from mail with no HTML
/// part (#436): text with no markup at all, the worker's `wrap_plain`
/// document, or its `wrap_fragment` of text parts (with any pictures that
/// came between them). A sender's HTML is never only these, and a body that
/// merely uses `<pre>` is still HTML.
pub(crate) fn body_is_plain_text(body: &str) -> bool {
    if !body.contains('<') {
        return !body.trim().is_empty();
    }
    let lower = body.to_ascii_lowercase();
    let Some(open) = lower
        .find("<body")
        .and_then(|i| lower[i..].find('>').map(|j| i + j + 1))
    else {
        return false;
    };
    let close = lower
        .rfind("</body>")
        .filter(|&c| c >= open)
        .unwrap_or(lower.len());
    let whole = body_is_pre_wrap(body);
    let mut text = whole;
    let mut rest = &lower[open..close];
    while let Some(at) = rest.find('<') {
        let Some(end) = rest[at..].find('>').map(|i| at + i) else {
            return false;
        };
        let tag = rest[at + 1..end].trim();
        let ok = match tag.split_whitespace().next().unwrap_or("") {
            // `linkify`'s links, the one markup inside the text.
            "a" | "/a" => true,
            "div" if !whole => {
                let part = tag.contains("class=\"hylki-plain\"");
                text |= part;
                part
            }
            "/div" => !whole,
            "img" => !whole && tag.contains("class=\"hylki-inline\""),
            _ => false,
        };
        if !ok {
            return false;
        }
        rest = &rest[end + 1..];
    }
    text
}

/// Does this document set `white-space: pre-wrap` on `body` itself?
pub(crate) fn body_is_pre_wrap(html: &str) -> bool {
    let mut rest = html;
    while let Some(at) = rest.find("<style") {
        let Some(open) = rest[at..].find('>').map(|i| at + i + 1) else {
            return false;
        };
        let close = rest[open..]
            .find("</style>")
            .map(|i| open + i)
            .unwrap_or(rest.len());
        let css: String = rest[open..close]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        for rule in css.split('}') {
            let Some((selectors, decls)) = rule.split_once('{') else {
                continue;
            };
            if selectors.split(',').any(|s| s == "body") && decls.contains("white-space:pre-wrap") {
                return true;
            }
        }
        rest = &rest[close..];
    }
    false
}

/// Sanitize untrusted message HTML for the editable composer.
///
/// A designed message (an order confirmation, a newsletter) is laid out by
/// its styles, most of them in a `<style>` block. That block cannot come
/// into the composer, where it would restyle everything the user writes, so
/// its rules are first written onto the elements they match, and the body's
/// own look moves onto a wrapper. The quote then looks as the reader shows
/// it (#295).
///
/// Only presentation survives: the tags and attributes mail layout uses,
/// inline styles limited to [`QUOTE_STYLE_PROPERTIES`] with no `url()` or
/// other resource in them, and URLs limited to http/https/mailto plus
/// images already embedded in the message. No scripts or handlers, ever.
fn sanitize_forward_html(html: &str) -> String {
    use std::collections::HashSet;
    let inliner = css_inline::CSSInliner::options()
        .load_remote_stylesheets(false)
        .keep_style_tags(false)
        .keep_link_tags(false)
        .keep_at_rules(false)
        .build();
    let inlined = inliner.inline(html).unwrap_or_else(|e| {
        tracing::debug!("quote: could not inline the message's styles: {e}");
        html.to_string()
    });
    let mut b = ammonia::Builder::default();
    b.add_tags([
        "table", "thead", "tbody", "tfoot", "tr", "td", "th", "font", "center", "u", "span", "img",
    ])
    .add_generic_attributes([
        "style",
        "width",
        "height",
        "align",
        "valign",
        "bgcolor",
        "border",
        "cellpadding",
        "cellspacing",
        "color",
        "face",
        "size",
        "colspan",
        "rowspan",
        "dir",
    ])
    .add_tag_attributes("img", ["src", "alt"])
    // A document's title is not part of its body.
    .add_clean_content_tags(["title"])
    .url_schemes(HashSet::from(["http", "https", "mailto", "data"]))
    .attribute_filter(|element, attribute, value| match (element, attribute) {
        (_, "style") => quote_style(value).map(Into::into),
        // `data:` for the pictures the message carried inside itself,
        // and nowhere else.
        (_, "href" | "src") if starts_with_ignore_case(value.trim_start(), "data:") => {
            (element == "img" && starts_with_ignore_case(value.trim_start(), "data:image/"))
                .then(|| value.into())
        }
        _ => Some(value.into()),
    });
    let inner = b.clean(&inlined).to_string();
    let inner = trim_empty_blocks(&inner);
    // The body's own background, font and spacing, which the sanitizer
    // would otherwise drop with the body tag.
    let Some(body) = body_look(&inlined) else {
        return inner.to_string();
    };
    let wrapper = b
        .clean(&format!("<div style=\"{body}\"></div>"))
        .to_string();
    match wrapper.strip_suffix("</div>") {
        Some(open) if open != "<div>" => format!("{open}{inner}</div>"),
        _ => inner.to_string(),
    }
}

/// The CSS a quoted message may keep: what lays out and colors mail.
/// Positioning is left out, so nothing quoted can cover the composer, and
/// so is anything that takes an image.
const QUOTE_STYLE_PROPERTIES: &[&str] = &[
    "background-color",
    "background",
    "border",
    "border-bottom",
    "border-bottom-color",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "border-bottom-style",
    "border-bottom-width",
    "border-collapse",
    "border-color",
    "border-left",
    "border-left-color",
    "border-left-style",
    "border-left-width",
    "border-radius",
    "border-right",
    "border-right-color",
    "border-right-style",
    "border-right-width",
    "border-spacing",
    "border-style",
    "border-top",
    "border-top-color",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-top-style",
    "border-top-width",
    "border-width",
    "box-sizing",
    "clear",
    "color",
    "direction",
    "display",
    "float",
    "font",
    "font-family",
    "font-size",
    "font-style",
    "font-variant",
    "font-weight",
    "height",
    "letter-spacing",
    "line-height",
    "list-style",
    "list-style-position",
    "list-style-type",
    "margin",
    "margin-bottom",
    "margin-left",
    "margin-right",
    "margin-top",
    "max-height",
    "max-width",
    "min-height",
    "min-width",
    "opacity",
    "overflow",
    "overflow-wrap",
    "padding",
    "padding-bottom",
    "padding-left",
    "padding-right",
    "padding-top",
    "table-layout",
    "text-align",
    "text-decoration",
    "text-indent",
    "text-transform",
    "vertical-align",
    "visibility",
    "white-space",
    "width",
    "word-break",
    "word-spacing",
    "word-wrap",
];

/// A quoted element's `style`, cut down to [`QUOTE_STYLE_PROPERTIES`].
///
/// A value may call color and arithmetic functions and nothing else: that
/// keeps out `url()`, `image-set()` and the rest, whichever property they
/// hide in, so opening a reply fetches nothing from the sender's servers.
/// Escapes are refused outright rather than decoded.
fn quote_style(style: &str) -> Option<String> {
    const FUNCTIONS: [&str; 5] = ["rgb", "rgba", "hsl", "hsla", "calc"];
    let mut kept = String::new();
    for declaration in style.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() || value.contains(['\\', '<', '>']) {
            continue;
        }
        if !QUOTE_STYLE_PROPERTIES.contains(&name.as_str()) {
            continue;
        }
        let lower = value.to_ascii_lowercase();
        let calls_only_allowed = lower.match_indices('(').all(|(at, _)| {
            let head = &lower[..at];
            let start = head
                .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .map_or(0, |i| i + 1);
            FUNCTIONS.contains(&&head[start..])
        });
        if calls_only_allowed {
            kept.push_str(&format!("{name}:{value};"));
        }
    }
    (!kept.is_empty()).then_some(kept)
}

/// The inline style of a document's `<body>`, with its `bgcolor` folded in,
/// still escaped as it was in the markup. `html` is what the inliner wrote,
/// so the attributes are double-quoted.
fn body_look(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<body")? + "<body".len();
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut rest = &html[start..];
    loop {
        rest = rest.trim_start();
        if rest.is_empty() || rest.starts_with('>') || rest.starts_with("/>") {
            break;
        }
        let name_end = rest
            .find(|c: char| c == '=' || c == '>' || c.is_whitespace())
            .unwrap_or(rest.len());
        let name = rest[..name_end].to_ascii_lowercase();
        rest = rest[name_end..].trim_start();
        let Some(after_eq) = rest.strip_prefix('=') else {
            if name.is_empty() {
                break;
            }
            continue;
        };
        let after_eq = after_eq.trim_start();
        let (value, tail) = match after_eq.chars().next() {
            Some(q @ ('"' | '\'')) => {
                let body = &after_eq[1..];
                let end = body.find(q)?;
                (&body[..end], &body[end + 1..])
            }
            _ => {
                let end = after_eq
                    .find(|c: char| c == '>' || c.is_whitespace())
                    .unwrap_or(after_eq.len());
                (&after_eq[..end], &after_eq[end..])
            }
        };
        attrs.push((name, value.to_string()));
        rest = tail;
    }
    let get = |n: &str| {
        attrs
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.trim().to_string())
    };
    let mut look = get("style").unwrap_or_default();
    if let Some(bg) = get("bgcolor").filter(|v| !v.is_empty()) {
        if !look.is_empty() && !look.ends_with(';') {
            look.push(';');
        }
        // Ahead of the style, so a background the style sets still wins,
        // as it does over `bgcolor` in the reader.
        look = format!("background-color:{bg};{look}");
    }
    (!look.trim().is_empty()).then_some(look)
}

fn starts_with_ignore_case(s: &str, prefix: &str) -> bool {
    s.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_reply_keeps_the_levels_of_quoting() {
        // An answer to an answer: the inner quote has to stay one level in
        // (#248), not be flattened into the outer one.
        let body = "<!doctype html><html><head></head><body><div>yada yada</div>\
            <p class=\"hylki-quote-attr\">On Monday, Ada wrote:</p>\
            <blockquote><div>tagada</div></blockquote></body></html>";
        let quoted = super::quoted_original("On Tuesday, Bo wrote:", body);
        let inner = quoted.split_once("</p>").expect("attribution line").1;
        assert!(
            inner.contains("<blockquote><div>tagada</div></blockquote>"),
            "{quoted}"
        );
        assert_eq!(inner.matches("<blockquote>").count(), 2, "{quoted}");
    }

    #[test]
    fn quoted_plain_text_keeps_its_line_breaks() {
        // A plain-text part's breaks live in `white-space: pre-wrap`, which
        // the sanitizer strips: they have to become <br> first.
        let doc = "<!doctype html><html><head><meta charset=\"utf-8\"><style>\
            body{margin:0;font:14px/1.5 system-ui,sans-serif;\
            white-space:pre-wrap;word-wrap:break-word}\
            </style></head><body>Hi,\n\nTwo lines.\n</body></html>";
        let quoted = super::quoted_original("hdr", doc);
        assert!(quoted.contains("Hi,<br><br>Two lines."), "{quoted}");
        assert!(!quoted.contains('\n'), "{quoted}");

        // The same in a message of several parts, where only the text part
        // is pre-wrap and the HTML part's source newlines are not breaks.
        let mixed = "<!doctype html><html><head><style>.hylki-plain{white-space:pre-wrap}\
            </style></head><body><div class=\"hylki-plain\">One\nTwo</div>\
            <div>\n<b>Three</b>\n</div></body></html>";
        let quoted = super::quoted_original("hdr", mixed);
        assert!(quoted.contains("One<br>Two"), "{quoted}");
        assert!(quoted.contains("<b>Three</b>"), "{quoted}");
        assert!(!quoted.contains("<br>\n<b>"), "{quoted}");
    }

    #[test]
    fn plain_text_mail_is_told_from_html_mail() {
        use crate::worker::extract_body;
        let plain = extract_body(
            b"Content-Type: text/plain\r\n\r\nHi,\r\n\r\n  indented, see http://example.com/x <here>\r\n",
        );
        assert!(super::body_is_plain_text(&plain), "{plain}");
        // Text and a picture between two text parts, as Apple Mail sends.
        let mixed = extract_body(
            b"Content-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nOne\r\n\
              --b\r\nContent-Type: image/png\r\nContent-Disposition: inline\r\nContent-Transfer-Encoding: base64\r\n\r\n\
              iVBORw0KGgo=\r\n--b\r\nContent-Type: text/plain\r\n\r\nTwo\r\n--b--\r\n",
        );
        assert!(super::body_is_plain_text(&mixed), "{mixed}");
        assert!(super::body_is_plain_text("Raw text the demo keeps"));

        let alternative = extract_body(
            b"Content-Type: multipart/alternative; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nHi\r\n\
              --b\r\nContent-Type: text/html\r\n\r\n<p>Hi</p>\r\n--b--\r\n",
        );
        assert!(!super::body_is_plain_text(&alternative), "{alternative}");
        // HTML set in <pre> is still HTML, and so is a pre-wrap body with
        // markup of its own.
        assert!(!super::body_is_plain_text(
            "<html><body><pre>text</pre></body></html>"
        ));
        assert!(!super::body_is_plain_text(
            "<html><head><style>body{white-space:pre-wrap}</style></head><body><b>Hi</b></body></html>"
        ));
        // A text part and an HTML part side by side.
        let both = "<!doctype html><html><head></head><body><div class=\"hylki-plain\">One</div>\
            <div><b>Two</b></div></body></html>";
        assert!(!super::body_is_plain_text(both));
        assert!(!super::body_is_plain_text(""));
    }

    #[test]
    fn quoted_plain_text_keeps_its_spaces_and_one_break_per_line() {
        // CRLF mail: a carriage return left before the <br> read back as a
        // second break under the quote's pre-wrap. The indent stays, and so
        // does the escaped-text path's.
        let body =
            crate::worker::extract_body(b"Content-Type: text/plain\r\n\r\nHi,\r\n  indented\r\n");
        let quoted = super::quoted_original("hdr", &body);
        assert!(quoted.contains("Hi,<br>  indented"), "{quoted}");
        assert!(!quoted.contains(['\r', '\n']), "{quoted}");
        let raw = super::quoted_original("hdr", "Hi,\n  indented");
        assert!(
            raw.contains("<div style=\"white-space:pre-wrap\">Hi,<br>  indented</div>"),
            "{raw}"
        );
    }

    #[test]
    fn quoted_body_drops_the_blank_line_it_starts_on() {
        // Every message written in the composer opens on a blank line; the
        // quote should not begin with it.
        let body = "<html><body><div><br></div><div>yada yada</div><br></body></html>";
        let quoted = super::quoted_original("hdr", body);
        let inner = quoted.split_once("</p>").expect("attribution line").1;
        assert!(
            inner.starts_with("<blockquote><div>yada yada</div>"),
            "{quoted}"
        );
        assert!(!inner.contains("<br></blockquote>"), "{quoted}");
    }

    #[test]
    fn forward_sanitizer_strips_active_content() {
        use super::sanitize_forward_html;
        let dirty = r#"<table><tr><td onclick="evil()">Total: <b>$40</b></td></tr></table>
            <script>steal()</script>
            <img src="javascript:alert(1)">
            <a href="https://example.com" onmouseover="x()">pay</a>
            <div style="background:url(https://tracker.example/p.gif)">hi</div>"#;
        let clean = sanitize_forward_html(dirty);
        assert!(!clean.contains("script"), "{clean}");
        assert!(
            !clean.contains("onclick") && !clean.contains("onmouseover"),
            "{clean}"
        );
        assert!(!clean.contains("javascript:"), "{clean}");
        assert!(!clean.contains("style"), "{clean}");
        // The structure and safe pieces survive.
        assert!(
            clean.contains("<table>") && clean.contains("<b>$40</b>"),
            "{clean}"
        );
        assert!(
            clean.contains(r#"<a href="https://example.com""#),
            "{clean}"
        );
    }

    #[test]
    fn forward_sanitizer_keeps_presentation_attributes() {
        use super::sanitize_forward_html;
        let html = r##"<table width="600" border="0"><tr><td align="right" bgcolor="#eeeeee">
            <font color="#333333" size="2">Invoice</font></td></tr></table>
            <img src="https://example.com/logo.png" width="120" alt="logo">"##;
        let clean = sanitize_forward_html(html);
        for keep in [
            "width=\"600\"",
            "align=\"right\"",
            "bgcolor=\"#eeeeee\"",
            "color=\"#333333\"",
            "src=\"https://example.com/logo.png\"",
        ] {
            assert!(clean.contains(keep), "missing {keep} in {clean}");
        }
    }

    #[test]
    fn quoted_message_keeps_its_stylesheet_look() {
        // A designed message styles itself from a <style> block the
        // composer cannot take; the rules come in on the elements (#295).
        let html = "<html><head><title>Your order</title><style>\
            .bar{background-color:#34495e;color:#ffffff;padding:20px}\
            @media (max-width:600px){.bar{padding:4px}}</style></head>\
            <body bgcolor=\"#eeeeee\" style=\"margin:0\"><div class=\"bar\">Order placed</div></body></html>";
        let clean = super::sanitize_forward_html(html);
        assert!(
            !clean.contains("<style") && !clean.contains("class="),
            "{clean}"
        );
        assert!(!clean.contains("Your order"), "{clean}");
        assert!(clean.contains("background-color:#34495e;"), "{clean}");
        assert!(clean.contains("color:#ffffff;"), "{clean}");
        // The body's ground moves onto a wrapper.
        assert!(
            clean.starts_with("<div style=\"background-color:#eeeeee;margin:0;\">"),
            "{clean}"
        );
    }

    #[test]
    fn quoted_styles_fetch_nothing_and_cover_nothing() {
        use super::quote_style;
        assert_eq!(
            quote_style(
                "position:fixed; top:0; color:red; background:#fff url(https://t.example/p.gif)"
            ),
            Some("color:red;".into())
        );
        assert_eq!(quote_style("background-image:url(x)"), None);
        assert_eq!(quote_style("background:u\\72l(x)"), None);
        assert_eq!(quote_style("width:-webkit-image-set(x 1x)"), None);
        assert_eq!(
            quote_style("color: rgb(1, 2, 3); width: calc(100% - 20px)"),
            Some("color:rgb(1, 2, 3);width:calc(100% - 20px);".into())
        );
    }

    #[test]
    fn quoted_message_keeps_its_embedded_pictures() {
        // The reader's copy has its cid: parts as data: URIs; the quote keeps
        // them, and the send path turns them back into parts.
        let html = r#"<img src="data:image/png;base64,iVBORw0KGgo=" alt="logo">
            <img src="data:text/html;base64,PHNjcmlwdD4=">
            <a href="data:text/html,hi">x</a>"#;
        let clean = super::sanitize_forward_html(html);
        assert!(
            clean.contains(r#"src="data:image/png;base64,iVBORw0KGgo=""#),
            "{clean}"
        );
        assert!(!clean.contains("data:text"), "{clean}");
    }

    /// A copy to edit carries the message's own content, not a quote of it:
    /// the HTML survives its sanitizing, and plain text becomes a paragraph
    /// with its line breaks kept.
    #[test]
    fn a_copy_keeps_the_body_unquoted() {
        let html = super::editable_copy_html("<p>Hi <b>there</b></p><script>steal()</script>");
        assert!(html.contains("<b>there</b>"), "{html}");
        assert!(!html.contains("script"), "{html}");
        assert!(!html.contains("blockquote"), "{html}");

        let html = super::editable_copy_html("Line one\nLine two");
        assert_eq!(html, "<p>Line one<br>Line two</p>");
    }

    #[test]
    fn body_look_folds_bgcolor_in_front_of_the_style() {
        assert_eq!(
            body_look("<html><BODY bgcolor=\"#fff\" style=\"color: red\">x</body>").as_deref(),
            Some("background-color:#fff;color: red;")
        );
        assert_eq!(
            body_look("<body bgcolor=red>").as_deref(),
            Some("background-color:red;")
        );
        assert_eq!(body_look("<body style=' '>"), None);
        assert_eq!(body_look("<p>no body</p>"), None);
        assert_eq!(body_look("<body style=\"unclosed>"), None);
    }

    #[test]
    fn breaks_turn_every_line_end_into_one_br() {
        assert_eq!(breaks("a\r\nb\rc\nd"), "a<br>b<br>c<br>d");
    }

    #[test]
    fn empty_blocks_are_trimmed_from_both_ends() {
        assert_eq!(
            trim_empty_blocks(" <br> <p></p><p>x</p><div><br></div>&nbsp; "),
            "<p>x</p>"
        );
        assert_eq!(trim_empty_blocks("<br/><br />"), "");
    }

    #[test]
    fn quote_blocks_escape_the_attribution() {
        assert_eq!(
            quote_block("On <d>,\nA wrote:", "a & b\nc"),
            "<p class=\"hylki-quote-attr\">On &lt;d&gt;,<br>A wrote:</p><blockquote><div style=\"white-space:pre-wrap\">a &amp; b<br>c</div></blockquote>"
        );
        assert_eq!(
            quote_block_html("A", "<p>x</p>"),
            "<p class=\"hylki-quote-attr\">A</p><blockquote><p>x</p></blockquote>"
        );
        assert_eq!(
            quote_block_html("A", "<p style=\"background: red\">x</p>"),
            "<p class=\"hylki-quote-attr\">A</p><blockquote class=\"hylki-quote-mail\"><p style=\"background: red\">x</p></blockquote>"
        );
    }

    #[test]
    fn message_text_trims_plain_text() {
        assert_eq!(message_text("  hello\n "), "hello");
    }

    #[test]
    fn prefix_check_ignores_ascii_case_and_char_boundaries() {
        assert!(starts_with_ignore_case("Content-Type", "content-"));
        assert!(!starts_with_ignore_case("Co", "content"));
        assert!(!starts_with_ignore_case("é1", "e"));
    }
}
