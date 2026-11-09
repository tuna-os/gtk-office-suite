//! Translating a message where it stands (#327): the runs of text in its
//! parsed document go out as numbered pieces of HTML and come back into the
//! same places, so the message keeps its design.

use std::collections::HashMap;

use super::CHUNK;

/// Elements that sit inside a line of text. A run of these and text is
/// what goes to the service as one unit, so a sentence with a link or a
/// bold word in it is translated whole.
const INLINE: &[&str] = &[
    "a", "abbr", "b", "bdi", "bdo", "br", "cite", "code", "data", "dfn", "em", "font", "i", "kbd",
    "label", "mark", "q", "s", "samp", "small", "span", "strike", "strong", "sub", "sup", "time",
    "tt", "u", "var", "wbr",
];
/// Elements whose content is never sent.
const UNSENT: &[&str] = &[
    "script", "style", "head", "title", "noscript", "template", "svg", "math", "textarea", "select",
];
/// The attribute that numbers each run in a piece, and the one that stands
/// in for an inline element's own attributes while it is away: an `href`
/// full of tracking, a `style`, cost the user's allowance and say nothing.
pub(super) const RUN_ATTR: &str = "data-hylki-run";
pub(super) const ATTRS_ATTR: &str = "data-hylki-a";

/// A message being translated where it stands (#327): its parsed document,
/// the runs of text in it, and the pieces of HTML they are sent as.
pub(super) struct InPlace {
    dom: markup5ever_rcdom::RcDom,
    /// Each run: its parent and the range of the parent's children.
    pub(super) runs: Vec<(markup5ever_rcdom::Handle, std::ops::Range<usize>)>,
    /// The inline elements' attributes, by the number they were sent with.
    attrs: Vec<Vec<html5ever::Attribute>>,
    pub(super) pieces: Vec<String>,
}

impl InPlace {
    pub(super) fn parse(body: &str) -> InPlace {
        use html5ever::tendril::TendrilSink;
        let dom =
            html5ever::parse_document(markup5ever_rcdom::RcDom::default(), Default::default())
                .one(body);
        let mut runs = Vec::new();
        collect_runs(&dom.document, &mut runs);
        let mut attrs = Vec::new();
        let mut pieces: Vec<String> = Vec::new();
        for (i, (parent, range)) in runs.iter().enumerate() {
            let mut html = format!("<div {RUN_ATTR}=\"{i}\">");
            for node in &parent.children.borrow()[range.clone()] {
                stand_in_attrs(node, &mut attrs);
                html.push_str(&serialize(node));
            }
            html.push_str("</div>");
            match pieces.last_mut() {
                Some(last) if last.len() + html.len() <= CHUNK => last.push_str(&html),
                _ => pieces.push(html),
            }
        }
        InPlace {
            dom,
            runs,
            attrs,
            pieces,
        }
    }

    /// The document with each run replaced by its translation. A run the
    /// service did not send back keeps its own text.
    pub(super) fn fill(&mut self, translated: &[String]) -> String {
        use html5ever::tendril::TendrilSink;
        let mut by_run: HashMap<usize, Vec<markup5ever_rcdom::Handle>> = HashMap::new();
        // Kept until the end: dropping an RcDom empties every node that was
        // in it, the ones moved into the message included.
        let mut answers = Vec::new();
        for piece in translated {
            let dom =
                html5ever::parse_document(markup5ever_rcdom::RcDom::default(), Default::default())
                    .one(format!("<body>{piece}</body>"));
            find_runs(&dom.document, &mut by_run);
            answers.push(dom);
        }
        for (i, (parent, range)) in self.runs.iter().enumerate().rev() {
            let mut kids = parent.children.borrow_mut();
            match by_run.remove(&i) {
                Some(new) => {
                    for n in &new {
                        n.parent.set(Some(std::rc::Rc::downgrade(parent)));
                        restore_attrs(n, &self.attrs);
                    }
                    kids.splice(range.clone(), new);
                }
                None => {
                    for n in &kids[range.clone()] {
                        restore_attrs(n, &self.attrs);
                    }
                }
            }
        }
        let mut out = Vec::new();
        let doc: markup5ever_rcdom::SerializableHandle = self.dom.document.clone().into();
        let _ = html5ever::serialize(&mut out, &doc, Default::default());
        drop(answers);
        String::from_utf8_lossy(&out).into_owned()
    }
}

fn element_name(node: &markup5ever_rcdom::Handle) -> Option<String> {
    match &node.data {
        markup5ever_rcdom::NodeData::Element { name, .. } => {
            Some(name.local.to_string().to_ascii_lowercase())
        }
        _ => None,
    }
}

fn is_hidden_node(node: &markup5ever_rcdom::Handle) -> bool {
    match &node.data {
        markup5ever_rcdom::NodeData::Element { attrs, .. } => {
            crate::mail::reader::is_hidden(&attrs.borrow())
        }
        _ => false,
    }
}

/// Text, or an inline element holding only text and inline elements.
fn is_inline(node: &markup5ever_rcdom::Handle) -> bool {
    use markup5ever_rcdom::NodeData;
    match &node.data {
        NodeData::Text { .. } | NodeData::Comment { .. } => true,
        NodeData::Element { .. } => {
            element_name(node).is_some_and(|n| INLINE.contains(&n.as_str()))
                && !is_hidden_node(node)
                && node.children.borrow().iter().all(is_inline)
        }
        _ => false,
    }
}

fn has_text(node: &markup5ever_rcdom::Handle) -> bool {
    match &node.data {
        markup5ever_rcdom::NodeData::Text { contents } => {
            contents.borrow().chars().any(char::is_alphanumeric)
        }
        _ => node.children.borrow().iter().any(has_text),
    }
}

/// Every run of inline content with words in it, in document order.
fn collect_runs(
    node: &markup5ever_rcdom::Handle,
    runs: &mut Vec<(markup5ever_rcdom::Handle, std::ops::Range<usize>)>,
) {
    let kids = node.children.borrow();
    let mut i = 0;
    while i < kids.len() {
        if is_inline(&kids[i]) {
            let start = i;
            while i < kids.len() && is_inline(&kids[i]) {
                i += 1;
            }
            if kids[start..i].iter().any(has_text) {
                runs.push((node.clone(), start..i));
            }
            continue;
        }
        let child = &kids[i];
        let sent = match element_name(child) {
            Some(name) => !UNSENT.contains(&name.as_str()) && !is_hidden_node(child),
            None => true,
        };
        if sent {
            collect_runs(child, runs);
        }
        i += 1;
    }
}

/// Swap an inline element's attributes, and its descendants', for a number.
fn stand_in_attrs(node: &markup5ever_rcdom::Handle, saved: &mut Vec<Vec<html5ever::Attribute>>) {
    if let markup5ever_rcdom::NodeData::Element { attrs, .. } = &node.data {
        let mut attrs = attrs.borrow_mut();
        if !attrs.is_empty() {
            let n = saved.len();
            saved.push(std::mem::take(&mut *attrs));
            attrs.push(html5ever::Attribute {
                name: html5ever::QualName::new(
                    None,
                    html5ever::ns!(),
                    html5ever::LocalName::from(ATTRS_ATTR),
                ),
                value: n.to_string().into(),
            });
        }
    }
    for c in node.children.borrow().iter() {
        stand_in_attrs(c, saved);
    }
}

/// Put back the attributes [`stand_in_attrs`] took.
fn restore_attrs(node: &markup5ever_rcdom::Handle, saved: &[Vec<html5ever::Attribute>]) {
    if let markup5ever_rcdom::NodeData::Element { attrs, .. } = &node.data {
        let mut attrs = attrs.borrow_mut();
        let n = attrs
            .iter()
            .find(|a| &*a.name.local == ATTRS_ATTR)
            .and_then(|a| a.value.parse::<usize>().ok());
        attrs.retain(|a| &*a.name.local != ATTRS_ATTR);
        if let Some(original) = n.and_then(|n| saved.get(n)) {
            *attrs = original.clone();
        }
    }
    for c in node.children.borrow().iter() {
        restore_attrs(c, saved);
    }
}

/// The numbered runs in a piece that came back: each one's children.
fn find_runs(
    node: &markup5ever_rcdom::Handle,
    out: &mut HashMap<usize, Vec<markup5ever_rcdom::Handle>>,
) {
    if let markup5ever_rcdom::NodeData::Element { attrs, .. } = &node.data {
        let n = attrs
            .borrow()
            .iter()
            .find(|a| &*a.name.local == RUN_ATTR)
            .and_then(|a| a.value.trim().parse::<usize>().ok());
        if let Some(n) = n {
            out.insert(n, node.children.borrow().clone());
            return;
        }
    }
    for c in node.children.borrow().iter() {
        find_runs(c, out);
    }
}

fn serialize(node: &markup5ever_rcdom::Handle) -> String {
    let mut out = Vec::new();
    let handle: markup5ever_rcdom::SerializableHandle = node.clone().into();
    let opts = html5ever::serialize::SerializeOpts {
        traversal_scope: html5ever::serialize::TraversalScope::IncludeNode,
        ..Default::default()
    };
    let _ = html5ever::serialize(&mut out, &handle, opts);
    String::from_utf8_lossy(&out).into_owned()
}
