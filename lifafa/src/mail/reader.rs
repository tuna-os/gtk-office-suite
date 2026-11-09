//! Reader view: a message stripped to what it says.
//!
//! The reader pane's Reader View toggle shows every message in the open
//! conversation without the sender's design: no stylesheets, no layout
//! tables, no colors, no hidden preview text, no tracking pixels. What is
//! left — headings, paragraphs, lists, quotes, links, real images, data
//! tables — is set in one uniform sheet that follows the app's theme.
//!
//! The work is done on a parsed DOM, not on the string: the sender's markup
//! is walked once and a fresh, minimal tree is built from it, so nothing of
//! the original attributes, styles or structure survives except what this
//! module chooses to emit. The result is still shown in the same sandboxed
//! frame as any other message (no scripts, the CSP, the remote-content
//! policy), so the reader view narrows what is shown; it never widens what
//! is allowed.

use self::tidy::tidy;
use self::walk::walk;
use crate::encoding::escape_html;

use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::RcDom;

mod tidy;
mod walk;

pub(crate) use walk::is_hidden;

/// The reader document for one message body: the extracted content in a
/// full HTML document carrying the reader sheet. `body` is the message as
/// cached (HTML, or the plain-text wrapper the worker builds).
pub(crate) fn render(body: &str, dark: bool, accent: &str) -> String {
    format!(
        // The content sits directly in the body, no wrapper: the wrapper
        // script's quote fold looks for a quote's topmost ancestor *under
        // the body*, and for content before it; a single wrapper would be
        // that ancestor, with nothing before it, and no quote would fold.
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>{css}</style></head>\
         <body>{content}</body></html>",
        css = stylesheet(dark, accent),
        content = extract(body),
    )
}

/// The message's content as a clean HTML fragment (see the module notes).
pub(crate) fn extract(body: &str) -> String {
    // A body with no markup at all is plain text, cached as it came: shown
    // line for line, as the ordinary view shows it (see `body_html`).
    if !body.contains('<') {
        return format!(
            "<div class=\"hylki-plain\">{}</div>",
            escape_html(body.trim_matches('\n'))
        );
    }
    extract_blocks(body).concat()
}

/// [`extract`]'s fragment as its top-level blocks, in order: what a
/// translation (#327) splits a long message at.
pub(crate) fn extract_blocks(body: &str) -> Vec<String> {
    if !body.contains('<') {
        return vec![format!(
            "<div class=\"hylki-plain\">{}</div>",
            escape_html(body.trim_matches('\n'))
        )];
    }
    let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(body);
    let mut out = Vec::new();
    walk(&dom.document, &mut out, false, false);
    let out = tidy(out, true);
    out.iter()
        .map(|node| {
            let mut html = String::new();
            node.write(&mut html);
            html
        })
        .collect()
}

/// The reader's own sheet: one measure, one type scale, theme colors.
fn stylesheet(dark: bool, accent: &str) -> String {
    let (scheme, fg, muted) = if dark {
        ("dark", "#e6e6e6", "#b0b0b0")
    } else {
        ("light", "#1a1a1a", "#5c5c5c")
    };
    const RULE: &str = "rgba(128,128,128,0.35)";
    const SOFT: &str = "rgba(128,128,128,0.12)";
    format!(
        ":root{{color-scheme:{scheme};}}\
         html{{margin:0;padding:0;background:transparent;}}\
         body{{color:{fg};font:15px/1.6 system-ui,\"Adwaita Sans\",Cantarell,sans-serif;\
           overflow-wrap:anywhere;-webkit-font-smoothing:antialiased;background:transparent;\
           max-width:44em;margin:0 auto;padding:22px 26px 26px;box-sizing:border-box;}}\
         p,ul,ol,dl,blockquote,pre,table,figure,.hylki-plain{{margin:0 0 1em;}}\
         body>:last-child{{margin-bottom:0;}}\
         h1,h2,h3,h4,h5,h6{{margin:1.4em 0 0.5em;line-height:1.25;font-weight:700;}}\
         h1{{font-size:1.5em;}}h2{{font-size:1.3em;}}h3{{font-size:1.15em;}}\
         h4,h5,h6{{font-size:1em;}}\
         body>:first-child{{margin-top:0;}}\
         a{{color:{accent};text-decoration:none;}}a:hover{{text-decoration:underline;}}\
         img{{display:block;max-width:100%;height:auto;margin:1em auto;border-radius:8px;}}\
         a img{{margin:0.5em 0;}}\
         blockquote{{padding-block:0.1em;padding-inline:1em 0;border-inline-start:3px solid {RULE};\
           color:{muted};}}\
         blockquote blockquote{{margin:0.5em 0;}}\
         pre,code,kbd,samp{{font-family:ui-monospace,\"Adwaita Mono\",monospace;font-size:0.9em;}}\
         pre{{background:{SOFT};padding:12px 14px;border-radius:8px;overflow-x:auto;\
           white-space:pre-wrap;}}\
         code,kbd,samp{{background:{SOFT};padding:0.1em 0.3em;border-radius:4px;}}\
         pre code{{background:none;padding:0;}}\
         hr{{border:0;border-top:1px solid {RULE};margin:1.6em 0;}}\
         ul,ol{{padding-inline-start:1.5em;}}li{{margin:0.25em 0;}}\
         dt{{font-weight:700;}}dd{{margin:0 0 0.5em;margin-inline-start:1.5em;}}\
         table{{border-collapse:collapse;max-width:100%;font-size:0.95em;display:block;\
           overflow-x:auto;}}\
         th,td{{padding:6px 10px;border-bottom:1px solid {RULE};text-align:start;\
           vertical-align:top;}}\
         th{{font-weight:700;}}\
         figcaption,small{{font-size:0.85em;color:{muted};}}\
         .hylki-plain{{white-space:pre-wrap;}}\
         mark{{background:rgba(229,165,10,0.35);color:inherit;}}\
         p,li,dt,dd,h1,h2,h3,h4,h5,h6,th,td,figcaption,div,pre{{unicode-bidi:plaintext;}}"
    )
}

// ---------------------------------------------------------------------------
// The output tree
// ---------------------------------------------------------------------------

/// A node of the rebuilt document. Only what this module emits can appear.
enum Out {
    Text(String),
    Elem(Elem),
}

struct Elem {
    tag: &'static str,
    attrs: Vec<(&'static str, String)>,
    kids: Vec<Out>,
}

impl Elem {
    fn new(tag: &'static str) -> Elem {
        Elem {
            tag,
            attrs: Vec::new(),
            kids: Vec::new(),
        }
    }

    fn is_block(&self) -> bool {
        matches!(
            self.tag,
            "p" | "div"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "ul"
                | "ol"
                | "li"
                | "dl"
                | "dt"
                | "dd"
                | "blockquote"
                | "pre"
                | "hr"
                | "table"
                | "tr"
                | "td"
                | "th"
                | "figure"
                | "figcaption"
        )
    }

    /// Whitespace inside is meaningful (shown as written).
    fn keeps_space(&self) -> bool {
        self.tag == "pre"
            || self
                .attrs
                .iter()
                .any(|(k, v)| *k == "class" && v == "hylki-plain")
    }
}

impl Out {
    fn write(&self, into: &mut String) {
        match self {
            Out::Text(t) => into.push_str(&escape_html(t)),
            Out::Elem(e) => {
                into.push('<');
                into.push_str(e.tag);
                for (k, v) in &e.attrs {
                    into.push(' ');
                    into.push_str(k);
                    into.push_str("=\"");
                    into.push_str(&escape_html(v));
                    into.push('"');
                }
                into.push('>');
                if matches!(e.tag, "br" | "hr" | "img") {
                    return;
                }
                for k in &e.kids {
                    k.write(into);
                }
                into.push_str("</");
                into.push_str(e.tag);
                into.push('>');
            }
        }
    }

    /// Anything a reader would see: text that is not blank, or a picture /
    /// rule. Line breaks on their own are not content.
    fn has_content(&self) -> bool {
        match self {
            Out::Text(t) => !is_blank(t),
            Out::Elem(e) => match e.tag {
                "img" | "hr" => true,
                "br" => false,
                _ => e.kids.iter().any(Out::has_content),
            },
        }
    }

    fn is_block(&self) -> bool {
        matches!(self, Out::Elem(e) if e.is_block())
    }

    fn is_br(&self) -> bool {
        matches!(self, Out::Elem(e) if e.tag == "br")
    }

    fn is_blank_text(&self) -> bool {
        matches!(self, Out::Text(t) if is_blank(t))
    }
}

/// Whitespace, no-break spaces and the invisible characters preview text
/// is padded with count as nothing.
fn is_blank(t: &str) -> bool {
    t.chars().all(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '\u{a0}'
                    | '\u{200b}'
                    | '\u{200c}'
                    | '\u{200d}'
                    | '\u{feff}'
                    | '\u{ad}'
                    | '\u{2007}'
                    | '\u{202f}'
            )
    })
}

// ---------------------------------------------------------------------------
// The walk: sender's DOM → output tree
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Tidying: the output tree → paragraphs
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
