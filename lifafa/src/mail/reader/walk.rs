//! Walking the parsed message: which elements and attributes are kept,
//! which are hidden or decoration, and what each becomes in the output.

use super::{Elem, Out};
use markup5ever_rcdom::{Handle, NodeData};

/// The class and id markers kept on an element: what the wrapper's quote
/// fold and the plain-text sheet look for. Nothing else of the sender's
/// classes survives.
const KEPT_CLASSES: [&str; 6] = [
    "gmail_quote",
    "gmail_signature",
    "moz-signature",
    "hylki-quote-attr",
    "hylki-plain",
    "yahoo_quoted",
];

const KEPT_IDS: [&str; 2] = ["divRplyFwdMsg", "Signature"];

fn attr<'a>(attrs: &'a [html5ever::Attribute], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|a| &*a.name.local == name)
        .map(|a| &*a.value)
}

/// Emit `node` and what is under it into `out`. `pre` keeps whitespace as
/// written; `in_link` drops nested links (they are invalid, and a table of
/// links inside a link is a common newsletter mistake).
pub(super) fn walk(node: &Handle, out: &mut Vec<Out>, pre: bool, in_link: bool) {
    match &node.data {
        NodeData::Document => {
            for c in node.children.borrow().iter() {
                walk(c, out, pre, in_link);
            }
        }
        NodeData::Text { contents } => {
            let text = contents.borrow();
            if pre {
                out.push(Out::Text(text.to_string()));
            } else {
                out.push(Out::Text(collapse_space(&text)));
            }
        }
        NodeData::Element { name, attrs, .. } => {
            let tag: String = (*name.local).to_ascii_lowercase();
            let attrs = attrs.borrow();
            if is_hidden(&attrs) {
                return;
            }
            let children = |out: &mut Vec<Out>, pre: bool, in_link: bool| {
                for c in node.children.borrow().iter() {
                    walk(c, out, pre, in_link);
                }
            };
            let push_elem = |out: &mut Vec<Out>, mut e: Elem, pre: bool, in_link: bool| {
                mark(&mut e, &attrs);
                children(&mut e.kids, pre, in_link);
                out.push(Out::Elem(e));
            };
            match tag.as_str() {
                // Dropped with everything inside: code, styling, chrome and
                // the parts of a page a message has no business carrying.
                "script" | "style" | "head" | "title" | "meta" | "link" | "template"
                | "noscript" | "iframe" | "frame" | "frameset" | "object" | "embed" | "applet"
                | "svg" | "math" | "canvas" | "form" | "input" | "button" | "select"
                | "textarea" | "option" | "datalist" | "map" | "area" | "base" | "audio"
                | "video" | "source" | "track" | "picture" | "o:p" | "xml" => {}
                "br" => out.push(Out::Elem(Elem::new("br"))),
                "hr" => out.push(Out::Elem(Elem::new("hr"))),
                "img" => {
                    if let Some(img) = image(&attrs) {
                        out.push(Out::Elem(img));
                    }
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    let tag: &'static str = match tag.as_str() {
                        "h1" => "h1",
                        "h2" => "h2",
                        "h3" => "h3",
                        "h4" => "h4",
                        "h5" => "h5",
                        _ => "h6",
                    };
                    push_elem(out, Elem::new(tag), pre, in_link);
                }
                "p" => push_elem(out, Elem::new("p"), pre, in_link),
                "ul" | "menu" => push_elem(out, Elem::new("ul"), pre, in_link),
                "ol" => push_elem(out, Elem::new("ol"), pre, in_link),
                "li" => push_elem(out, Elem::new("li"), pre, in_link),
                "dl" => push_elem(out, Elem::new("dl"), pre, in_link),
                "dt" => push_elem(out, Elem::new("dt"), pre, in_link),
                "dd" => push_elem(out, Elem::new("dd"), pre, in_link),
                // The Indent of Hylki's composer (and of other WebKit and
                // Blink editors) is a borderless blockquote, not a quote
                // (#358).
                "blockquote"
                    if attr(&attrs, "style").is_some_and(|s| {
                        s.replace(' ', "")
                            .to_ascii_lowercase()
                            .contains("border:none")
                    }) =>
                {
                    push_elem(out, Elem::new("div"), pre, in_link)
                }
                "blockquote" => {
                    let mut e = Elem::new("blockquote");
                    if attr(&attrs, "type").is_some_and(|t| t.eq_ignore_ascii_case("cite")) {
                        e.attrs.push(("type", "cite".into()));
                    }
                    push_elem(out, e, pre, in_link);
                }
                "pre" | "listing" | "xmp" | "plaintext" => {
                    push_elem(out, Elem::new("pre"), true, in_link)
                }
                "figure" => push_elem(out, Elem::new("figure"), pre, in_link),
                "figcaption" | "caption" => push_elem(out, Elem::new("figcaption"), pre, in_link),
                "table" => {
                    if is_data_table(node) {
                        push_elem(out, Elem::new("table"), pre, in_link);
                    } else {
                        // A layout table: its cells become plain blocks, in
                        // reading order, and the grid is gone.
                        push_elem(out, Elem::new("div"), pre, in_link);
                    }
                }
                "tr" => {
                    if in_data_table(node) {
                        push_elem(out, Elem::new("tr"), pre, in_link);
                    } else {
                        children(out, pre, in_link);
                    }
                }
                "td" | "th" => {
                    if in_data_table(node) {
                        let tag: &'static str = if tag == "th" { "th" } else { "td" };
                        push_elem(out, Elem::new(tag), pre, in_link);
                    } else {
                        push_elem(out, Elem::new("div"), pre, in_link);
                    }
                }
                "thead" | "tbody" | "tfoot" | "colgroup" | "col" => {
                    if tag == "colgroup" || tag == "col" {
                        return;
                    }
                    children(out, pre, in_link);
                }
                "div" | "section" | "article" | "main" | "header" | "footer" | "aside" | "nav"
                | "center" | "address" | "details" | "summary" | "fieldset" | "legend" | "body"
                | "html" => {
                    // A plain-text part keeps its line structure (#181): the
                    // worker wraps it in `.hylki-plain`, and so does this.
                    // (`body_html` puts the class on `<body>` itself for a
                    // plain-text message; that body becomes the div.)
                    let plain = has_class(&attrs, "hylki-plain");
                    if (tag == "body" || tag == "html") && !plain {
                        children(out, pre, in_link);
                    } else {
                        push_elem(out, Elem::new("div"), pre || plain, in_link);
                    }
                }
                "a" => match (in_link, link_href(&attrs)) {
                    (false, Some(href)) => {
                        let mut e = Elem::new("a");
                        e.attrs.push(("href", href));
                        if let Some(t) = attr(&attrs, "title").filter(|t| !t.trim().is_empty()) {
                            e.attrs.push(("title", t.trim().to_string()));
                        }
                        push_elem(out, e, pre, true);
                    }
                    _ => children(out, pre, in_link),
                },
                "strong" | "b" => push_elem(out, Elem::new("strong"), pre, in_link),
                "em" | "i" | "cite" | "var" | "dfn" => {
                    push_elem(out, Elem::new("em"), pre, in_link)
                }
                "u" | "ins" => push_elem(out, Elem::new("u"), pre, in_link),
                "s" | "strike" | "del" => push_elem(out, Elem::new("s"), pre, in_link),
                "sub" => push_elem(out, Elem::new("sub"), pre, in_link),
                "sup" => push_elem(out, Elem::new("sup"), pre, in_link),
                "code" | "kbd" | "samp" | "tt" => push_elem(out, Elem::new("code"), pre, in_link),
                "mark" => push_elem(out, Elem::new("mark"), pre, in_link),
                "small" => push_elem(out, Elem::new("small"), pre, in_link),
                "q" => push_elem(out, Elem::new("q"), pre, in_link),
                "abbr" => {
                    let mut e = Elem::new("abbr");
                    if let Some(t) = attr(&attrs, "title").filter(|t| !t.trim().is_empty()) {
                        e.attrs.push(("title", t.trim().to_string()));
                    }
                    push_elem(out, e, pre, in_link);
                }
                // Everything else — span, font, label, o:p, custom tags —
                // is a container the reader sees through.
                _ => children(out, pre, in_link),
            }
        }
        NodeData::Doctype { .. }
        | NodeData::Comment { .. }
        | NodeData::ProcessingInstruction { .. } => {}
    }
}

/// Carry the quote/signature/plain markers over, and nothing else.
fn mark(e: &mut Elem, attrs: &[html5ever::Attribute]) {
    if let Some(class) = attr(attrs, "class") {
        let kept: Vec<&str> = class
            .split_ascii_whitespace()
            .filter(|c| KEPT_CLASSES.contains(c))
            .collect();
        if !kept.is_empty() {
            e.attrs.push(("class", kept.join(" ")));
        }
    }
    if let Some(id) = attr(attrs, "id") {
        if KEPT_IDS.contains(&id.trim()) {
            e.attrs.push(("id", id.trim().to_string()));
        }
    }
}

fn has_class(attrs: &[html5ever::Attribute], class: &str) -> bool {
    attr(attrs, "class").is_some_and(|c| c.split_ascii_whitespace().any(|x| x == class))
}

/// Runs of whitespace become one space, as a browser would show them.
fn collapse_space(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
            if !space {
                out.push(' ');
                space = true;
            }
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

/// The `style` attribute's declarations, lowercased, as (property, value)
/// with `!important` and surrounding space removed.
fn declarations(style: &str) -> Vec<(String, String)> {
    style
        .split(';')
        .filter_map(|d| {
            let (k, v) = d.split_once(':')?;
            let k = k.trim().to_ascii_lowercase();
            let v = v.to_ascii_lowercase().replace("!important", "");
            Some((k, v.trim().to_string()))
        })
        .collect()
}

/// A CSS length that amounts to nothing: `0`, `0px`, `0.0em`…
fn is_zero_length(v: &str) -> bool {
    let n: String = v
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    !n.is_empty() && n.parse::<f64>().is_ok_and(|n| n == 0.0)
}

/// A length in pixels (a bare number counts as pixels, as HTML attributes
/// do); `None` for anything else (percentages, em…).
fn px(v: &str) -> Option<f64> {
    let v = v.trim();
    let n: String = v
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let rest = v[n.len()..].trim();
    if n.is_empty() || !(rest.is_empty() || rest.eq_ignore_ascii_case("px")) {
        return None;
    }
    n.parse().ok()
}

/// Whether the sender hid this element: preview text a client shows in the
/// list but never in the message, Outlook-only blocks, collapsed spacers.
pub(crate) fn is_hidden(attrs: &[html5ever::Attribute]) -> bool {
    if attr(attrs, "hidden").is_some() {
        return true;
    }
    let Some(style) = attr(attrs, "style") else {
        return false;
    };
    let decls = declarations(style);
    let get = |k: &str| {
        decls
            .iter()
            .rev()
            .find(|(p, _)| p == k)
            .map(|(_, v)| v.as_str())
    };
    if get("display").is_some_and(|v| v == "none")
        || get("visibility").is_some_and(|v| v == "hidden")
        || get("mso-hide").is_some_and(|v| v == "all")
        || get("opacity").is_some_and(|v| v.parse::<f64>().is_ok_and(|n| n == 0.0))
    {
        return true;
    }
    // A zero-size box that clips its content is a preheader by another
    // name. Only with the clipping: a zero font size on its own is how
    // MJML and its like build every column and cell (their children set
    // their own), so it says nothing about what is shown.
    let clipped = get("overflow").is_some_and(|v| v == "hidden");
    clipped
        && (get("max-height").is_some_and(is_zero_length)
            || get("height").is_some_and(is_zero_length)
            || get("width").is_some_and(is_zero_length))
}

/// A picture worth showing: a real source, and bigger than a tracking pixel.
fn image(attrs: &[html5ever::Attribute]) -> Option<Elem> {
    let src = attr(attrs, "src")?.trim();
    let lower = src.to_ascii_lowercase();
    let allowed = lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("cid:")
        || lower.starts_with("data:image/");
    if !allowed {
        return None;
    }
    let mut width = attr(attrs, "width").and_then(px);
    let mut height = attr(attrs, "height").and_then(px);
    if let Some(style) = attr(attrs, "style") {
        for (k, v) in declarations(style) {
            match k.as_str() {
                "width" => width = px(&v).or(width),
                "height" => height = px(&v).or(height),
                _ => {}
            }
        }
    }
    if width.is_some_and(|w| w <= 2.0) || height.is_some_and(|h| h <= 2.0) {
        return None;
    }
    let mut e = Elem::new("img");
    e.attrs.push(("src", src.to_string()));
    if let Some(alt) = attr(attrs, "alt").map(str::trim).filter(|a| !a.is_empty()) {
        e.attrs.push(("alt", alt.to_string()));
    }
    Some(e)
}

/// The link's target, or `None` when it is not one the reader will follow
/// (no target at all, or a scheme that means code rather than a place).
fn link_href(attrs: &[html5ever::Attribute]) -> Option<String> {
    let href = attr(attrs, "href")?.trim();
    if href.is_empty() || href.starts_with('#') {
        return None;
    }
    let scheme: String = href
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        .collect::<String>()
        .to_ascii_lowercase();
    let has_scheme = href[scheme.len()..].starts_with(':');
    if !has_scheme {
        // Relative to nothing: a message has no page to resolve it against.
        return None;
    }
    if matches!(
        scheme.as_str(),
        "javascript" | "vbscript" | "data" | "file" | "blob"
    ) {
        return None;
    }
    Some(href.to_string())
}

fn element_name(node: &Handle) -> Option<String> {
    match &node.data {
        NodeData::Element { name, .. } => Some((*name.local).to_ascii_lowercase()),
        _ => None,
    }
}

/// Whether a `tr`/`td`/`th` sits in a table the reader keeps as a grid.
fn in_data_table(node: &Handle) -> bool {
    let mut cur = node.parent.take();
    node.parent.set(cur.clone());
    while let Some(weak) = cur {
        let Some(p) = weak.upgrade() else {
            return false;
        };
        if element_name(&p).as_deref() == Some("table") {
            return is_data_table(&p);
        }
        cur = p.parent.take();
        p.parent.set(cur.clone());
    }
    false
}

/// A table that holds data rather than a layout: headed cells, or a real
/// grid of short cells with nothing block-like inside them. A table with
/// one column, one row, or with paragraphs, pictures or other tables in its
/// cells is a layout, and its cells are read one after another.
fn is_data_table(table: &Handle) -> bool {
    let mut rows = 0usize;
    let mut widest = 0usize;
    let mut headed = false;
    let mut blocky = false;
    fn scan(
        node: &Handle,
        rows: &mut usize,
        widest: &mut usize,
        headed: &mut bool,
        blocky: &mut bool,
    ) {
        for c in node.children.borrow().iter() {
            let Some(name) = element_name(c) else {
                continue;
            };
            match name.as_str() {
                "tr" => {
                    *rows += 1;
                    let mut cells = 0;
                    for cell in c.children.borrow().iter() {
                        match element_name(cell).as_deref() {
                            Some("td") | Some("th") => {
                                cells += 1;
                                if element_name(cell).as_deref() == Some("th") {
                                    *headed = true;
                                }
                                if has_block_inside(cell) {
                                    *blocky = true;
                                }
                            }
                            _ => {}
                        }
                    }
                    *widest = (*widest).max(cells);
                }
                "thead" | "tbody" | "tfoot" => scan(c, rows, widest, headed, blocky),
                _ => {}
            }
        }
    }
    scan(table, &mut rows, &mut widest, &mut headed, &mut blocky);
    if blocky {
        return false;
    }
    headed || (rows >= 2 && widest >= 2)
}

fn has_block_inside(node: &Handle) -> bool {
    for c in node.children.borrow().iter() {
        if let Some(name) = element_name(c) {
            if matches!(
                name.as_str(),
                "table"
                    | "div"
                    | "p"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "ul"
                    | "ol"
                    | "img"
                    | "blockquote"
                    | "pre"
                    | "hr"
                    | "center"
            ) {
                return true;
            }
            if has_block_inside(c) {
                return true;
            }
        }
    }
    false
}
