//! The HTML tree the Markdown writer and `plain_text` walk.

use super::writer::is_block;

/// A parsed element.
#[derive(Debug)]
pub(super) struct Elem {
    pub(super) name: String,
    pub(super) attrs: Vec<(String, String)>,
    pub(super) children: Vec<Node>,
}

#[derive(Debug)]
pub(super) enum Node {
    Text(String),
    Elem(Elem),
}

impl Elem {
    pub(super) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Parse HTML into a forest the way a browser would (html5ever): unknown
/// tags, unclosed tags and stray closers are all survivable, because this
/// runs over mail bodies and hand-written source, not validated documents.
/// Only the body's content is kept; `<script>`, `<style>` and comments never
/// become text.
pub(super) fn parse(html: &str) -> Vec<Node> {
    use html5ever::tendril::TendrilSink;
    use markup5ever_rcdom::{Handle, NodeData, RcDom};

    fn element_named(parent: &Handle, tag: &str) -> Option<Handle> {
        parent
            .children
            .borrow()
            .iter()
            .find(
                |c| matches!(&c.data, NodeData::Element { name, .. } if name.local.as_ref() == tag),
            )
            .cloned()
    }
    fn children(parent: &Handle) -> Vec<Node> {
        parent
            .children
            .borrow()
            .iter()
            .filter_map(convert)
            .collect()
    }
    fn convert(handle: &Handle) -> Option<Node> {
        match &handle.data {
            NodeData::Text { contents } => Some(Node::Text(contents.borrow().to_string())),
            NodeData::Element { name, attrs, .. } => {
                let tag = name.local.to_string();
                if tag == "script" || tag == "style" {
                    return None;
                }
                let attrs = attrs
                    .borrow()
                    .iter()
                    .map(|a| (a.name.local.to_string(), a.value.to_string()))
                    .collect();
                Some(Node::Elem(Elem {
                    name: tag,
                    attrs,
                    children: children(handle),
                }))
            }
            _ => None,
        }
    }

    let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
    element_named(&dom.document, "html")
        .and_then(|root| element_named(&root, "body"))
        .map(|body| children(&body))
        .unwrap_or_default()
}

/// An HTML body as plain text, for list previews: its words, a line break
/// for each block and `<br>`, whitespace collapsed as a browser does
/// outside `<pre>`, each line trimmed, and at most one blank line in a row.
pub(crate) fn plain_text(html: &str) -> String {
    fn walk(nodes: &[Node], pre: bool, out: &mut String) {
        for node in nodes {
            match node {
                Node::Text(t) if pre => out.push_str(t),
                Node::Text(t) => {
                    let mut last_space = out.ends_with([' ', '\n']);
                    for c in t.chars() {
                        let space = c.is_whitespace();
                        if !(space && last_space) {
                            out.push(if space { ' ' } else { c });
                        }
                        last_space = space;
                    }
                }
                Node::Elem(e) if e.name == "br" => out.push('\n'),
                Node::Elem(e) => {
                    let block = is_block(&e.name);
                    if block && !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    walk(&e.children, pre || e.name == "pre", out);
                    if block {
                        out.push_str(if e.name == "p" { "\n\n" } else { "\n" });
                    }
                }
            }
        }
    }
    let mut text = String::new();
    walk(&parse(html), false, &mut text);
    let mut out = String::new();
    let mut blank = false;
    for line in text
        .lines()
        .map(|l| l.trim_matches(|c: char| c == ' ' || c == '\u{a0}'))
    {
        if line.is_empty() {
            if !blank && !out.is_empty() {
                out.push('\n');
            }
            blank = true;
        } else {
            out.push_str(line);
            out.push('\n');
            blank = false;
        }
    }
    out.trim().to_string()
}
