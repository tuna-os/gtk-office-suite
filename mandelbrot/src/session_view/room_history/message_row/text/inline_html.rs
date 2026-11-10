//! Helpers for making Pango-compatible strings from inline HTML.

use std::fmt::Write;

use ruma::html::{
    Children, NodeData, NodeRef,
    matrix::{AnchorUri, MatrixElement, SpanData},
};
use tracing::debug;

use crate::{
    components::Pill,
    prelude::*,
    session::Room,
    utils::string::{Linkifier, PangoStrMutExt},
};

/// Helper type to construct a Pango-compatible string from inline HTML nodes.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
pub(super) struct InlineHtmlBuilder<'a> {
    /// Whether this string should be on a single line.
    single_line: bool,
    /// Whether to append an ellipsis at the end of the string.
    ellipsis: bool,
    /// Whether whitespace should be preserved.
    preserve_whitespace: bool,
    /// The mentions detection setting and results.
    mentions: MentionsMode<'a>,
    /// The inner string.
    inner: String,
    /// Whether this string was truncated because at the first newline.
    truncated: bool,
    /// Whether to account for `truncated` when appending children.
    ignore_truncated: bool,
}

impl<'a> InlineHtmlBuilder<'a> {
    /// Constructs a new inline HTML string builder for the given room.
    ///
    /// If `single_line` is set to `true`, the string will be ellipsized at the
    /// first line break.
    ///
    /// If `ellipsis` is set to `true`, an ellipsis will be added at the end of
    /// the string.
    ///
    /// If `preserve_whitespace` is set to `true`, all whitespace will be
    /// preserved, otherwise it will be collapsed according to the HTML spec.
    pub(super) fn new(single_line: bool, ellipsis: bool, preserve_whitespace: bool) -> Self {
        Self {
            single_line,
            ellipsis,
            preserve_whitespace,
            mentions: MentionsMode::default(),
            inner: String::new(),
            truncated: false,
            ignore_truncated: false,
        }
    }

    /// Enable mentions detection in the given room.
    ///
    /// If `detect_at_room` is `true`, it will also try to detect `@room`
    /// mentions.
    pub(super) fn detect_mentions(mut self, room: &'a Room, detect_at_room: bool) -> Self {
        self.mentions = MentionsMode::WithMentions {
            room,
            pills: Vec::new(),
            detect_at_room,
        };
        self
    }

    /// Append and consume the given sender name for an emote, if it is set.
    pub(super) fn append_emote_with_name(mut self, name: &mut Option<&str>) -> Self {
        self.inner.maybe_append_emote_name(name);
        self
    }

    /// Export the Pango-compatible string and the [`Pill`]s that were
    /// constructed, if any.
    pub(super) fn build(self) -> (String, Option<Vec<Pill>>) {
        let mut inner = self.inner;

        // Do not add an ellipsis on an empty inline element, we just want to get rid of
        // it.
        let ellipsis = (self.ellipsis && !inner.is_empty()) | self.truncated;

        if ellipsis {
            inner.append_ellipsis();
        } else if !self.preserve_whitespace {
            inner.truncate_end_whitespaces();
        }

        let pills = if let MentionsMode::WithMentions { pills, .. } = self.mentions {
            (!pills.is_empty()).then_some(pills)
        } else {
            None
        };

        (inner, pills)
    }

    /// Construct the string with the given inline nodes by converting them to
    /// Pango markup.
    ///
    /// Returns the Pango-compatible string and the [`Pill`]s that were
    /// constructed, if any.
    pub(super) fn build_with_nodes(
        mut self,
        nodes: impl IntoIterator<Item = NodeRef>,
    ) -> (String, Option<Vec<Pill>>) {
        self.append_nodes(nodes, true);
        self.build()
    }

    /// Construct the string by traversing the nodes an returning only the text
    /// it contains.
    ///
    /// Node that markup contained in the text is not escaped and newlines are
    /// not removed.
    pub(super) fn build_with_nodes_text(
        mut self,
        nodes: impl IntoIterator<Item = NodeRef>,
    ) -> String {
        self.append_nodes_text(nodes);

        let (inner, _) = self.build();
        inner
    }

    /// Append the given inline node by converting it to Pango markup.
    fn append_node(&mut self, node: &NodeRef, context: NodeContext) {
        match node.data() {
            NodeData::Element(data) => {
                let data = data.to_matrix();
                self.append_element_node(node, data.element, context.should_linkify);
            }
            NodeData::Text(text) => {
                self.append_text_node(text.borrow().as_ref(), context);
            }
            data => {
                debug!("Unexpected HTML node: {data:?}");
            }
        }
    }

    /// Append the given inline element node by converting it to Pango markup.
    fn append_element_node(
        &mut self,
        node: &NodeRef,
        element: MatrixElement,
        should_linkify: bool,
    ) {
        match element {
            MatrixElement::Del | MatrixElement::S => {
                self.append_tags_and_children("s", node.children(), should_linkify);
            }
            MatrixElement::A(anchor) => {
                // First, check if it's a mention, if we detect mentions.
                if let Some(uri) = &anchor.href
                    && let MentionsMode::WithMentions { pills, room, .. } = &mut self.mentions
                    && let Some(pill) = self.inner.maybe_append_mention(uri, room)
                {
                    pills.push(pill);

                    return;
                }

                // It's not a mention, render the link, if it has a URI.
                let mut has_opening_tag = false;

                if let Some(uri) = &anchor.href {
                    has_opening_tag = self.append_link_opening_tag_from_anchor_uri(uri);
                }

                // Always render the children.
                self.ignore_truncated = true;

                // Don't try to linkify text if we render the element, it does not make
                // sense to nest links.
                let should_linkify = !has_opening_tag && should_linkify;

                self.append_nodes(node.children(), should_linkify);

                self.ignore_truncated = false;

                if has_opening_tag {
                    self.inner.push_str("</a>");
                }
            }
            MatrixElement::Sup => {
                self.append_tags_and_children("sup", node.children(), should_linkify);
            }
            MatrixElement::Sub => {
                self.append_tags_and_children("sub", node.children(), should_linkify);
            }
            MatrixElement::B | MatrixElement::Strong => {
                self.append_tags_and_children("b", node.children(), should_linkify);
            }
            MatrixElement::I | MatrixElement::Em => {
                self.append_tags_and_children("i", node.children(), should_linkify);
            }
            MatrixElement::U => {
                self.append_tags_and_children("u", node.children(), should_linkify);
            }
            MatrixElement::Code(_) => {
                // Don't try to linkify text, it does not make sense to detect links inside
                // code.
                self.append_tags_and_children("tt", node.children(), false);
            }
            MatrixElement::Br => {
                if self.single_line {
                    self.truncated = true;
                } else {
                    if !self.preserve_whitespace {
                        // Remove whitespaces before the newline.
                        self.inner.truncate_end_whitespaces();
                    }

                    self.inner.push('\n');
                }
            }
            MatrixElement::Span(span) => {
                self.append_span(&span, node.children(), should_linkify);
            }
            element => {
                debug!("Unexpected HTML inline element: {element:?}");
                self.append_nodes(node.children(), should_linkify);
            }
        }
    }

    /// Append the given text node content.
    fn append_text_node(&mut self, text: &str, context: NodeContext) {
        // Collapse whitespaces and remove them at the beginning and end of an HTML
        // element, and after a newline.
        let text = if self.preserve_whitespace {
            text.to_owned()
        } else {
            text.collapse_whitespaces(
                context.is_first_child || self.inner.ends_with('\n'),
                context.is_last_child,
            )
        };

        if context.should_linkify {
            if let MentionsMode::WithMentions {
                pills,
                room,
                detect_at_room,
            } = &mut self.mentions
            {
                Linkifier::new(&mut self.inner)
                    .detect_mentions(room, pills, *detect_at_room)
                    .linkify(&text);
            } else {
                Linkifier::new(&mut self.inner).linkify(&text);
            }
        } else {
            self.inner.push_str(&text.escape_markup());
        }
    }

    /// Append the given inline nodes, converted to Pango markup.
    fn append_nodes(&mut self, nodes: impl IntoIterator<Item = NodeRef>, should_linkify: bool) {
        let mut is_first_child = true;
        let mut nodes_iter = nodes.into_iter().peekable();

        while let Some(node) = nodes_iter.next() {
            let child_context = NodeContext {
                should_linkify,
                is_first_child,
                is_last_child: nodes_iter.peek().is_none(),
            };

            self.append_node(&node, child_context);

            if self.truncated && !self.ignore_truncated {
                // Stop as soon as the string is truncated.
                break;
            }

            is_first_child = false;
        }
    }

    /// Append the given inline children, converted to Pango markup, surrounded
    /// by tags with the given name.
    fn append_tags_and_children(
        &mut self,
        tag_name: &str,
        children: Children,
        should_linkify: bool,
    ) {
        let _ = write!(self.inner, "<{tag_name}>");

        self.append_nodes(children, should_linkify);

        let _ = write!(self.inner, "</{tag_name}>");
    }

    /// Append the opening Pango markup link tag of the given anchor URI.
    ///
    /// The URI is also used as a title, so users can preview the link on hover.
    ///
    /// Returns `true` if the opening tag was successfully constructed.
    fn append_link_opening_tag_from_anchor_uri(&mut self, uri: &AnchorUri) -> bool {
        match uri {
            AnchorUri::Matrix(uri) => {
                self.inner.append_link_opening_tag(uri.to_string());
                true
            }
            AnchorUri::MatrixTo(uri) => {
                self.inner.append_link_opening_tag(uri.to_string());
                true
            }
            AnchorUri::Other(uri) => {
                self.inner.append_link_opening_tag(uri);
                true
            }
            uri => {
                debug!("Unsupported anchor URI format: {uri:?}");
                false
            }
        }
    }

    /// Append the span with the given data and inline children as Pango Markup.
    ///
    /// Whether we are an inside an anchor or not decides if we try to linkify
    /// the text contained in the children nodes.
    fn append_span(&mut self, span: &SpanData, children: Children, should_linkify: bool) {
        self.inner.push_str("<span");

        if let Some(bg_color) = &span.bg_color {
            let _ = write!(self.inner, r#" bgcolor="{bg_color}""#);
        }
        if let Some(color) = &span.color {
            let _ = write!(self.inner, r#" color="{color}""#);
        }

        self.inner.push('>');

        self.append_nodes(children, should_linkify);

        self.inner.push_str("</span>");
    }

    /// Append the text contained in the nodes to the string.
    ///
    /// Returns `true` if the text was ellipsized.
    fn append_nodes_text(&mut self, nodes: impl IntoIterator<Item = NodeRef>) {
        for node in nodes {
            match node.data() {
                NodeData::Text(t) => {
                    let borrowed_t = t.borrow();
                    let t = borrowed_t.as_ref();

                    if self.single_line
                        && let Some(newline_pos) = t.find('\n')
                    {
                        self.truncated = true;

                        self.inner.push_str(&t[..newline_pos]);
                        self.inner.append_ellipsis();

                        break;
                    }

                    self.inner.push_str(t);
                }
                NodeData::Element(data) => {
                    if data.name.local.as_ref() == "br" {
                        if self.single_line {
                            self.truncated = true;
                            break;
                        }

                        self.inner.push('\n');
                    } else {
                        self.append_nodes_text(node.children());
                    }
                }
                _ => {}
            }

            if self.truncated {
                // Stop as soon as the string is truncated.
                break;
            }
        }
    }
}

/// The mentions mode of the [`InlineHtmlBuilder`].
#[derive(Debug, Default)]
enum MentionsMode<'a> {
    /// The builder will not detect mentions.
    #[default]
    NoMentions,
    /// The builder will detect mentions.
    WithMentions {
        /// The pills for the detected mentions.
        pills: Vec<Pill>,
        /// The room containing the mentions.
        room: &'a Room,
        /// Whether to detect `@room` mentions.
        detect_at_room: bool,
    },
}

/// Context for an HTML node.
#[derive(Debug, Clone, Copy)]
struct NodeContext {
    /// Whether we should try to search for links in the text of the node.
    should_linkify: bool,
    /// Whether this is the first child node of an element.
    is_first_child: bool,
    /// Whether this is the last child node of an element.
    is_last_child: bool,
}
