//! Build HTML messages.

use gettextrs::gettext;
use gtk::{pango, prelude::*};
use ruma::html::{
    Children, NodeRef,
    matrix::{MatrixElement, OrderedListData},
};
use sourceview::prelude::*;
use tracing::debug;

use super::{SUPPORTED_BLOCK_ELEMENTS, inline_html::InlineHtmlBuilder};
use crate::{
    components::{AtRoom, LabelWithWidgets},
    prelude::*,
    session::Room,
};

/// The immutable config fields to build a HTML widget tree.
#[derive(Debug, Clone, Copy)]
pub(super) struct HtmlWidgetConfig<'a> {
    /// The room where the message constructed by this widget was sent.
    ///
    /// Used for generating mentions.
    pub(super) room: &'a Room,
    /// Whether we should try to detect an `@room` mention in the HTML to
    /// render.
    pub(super) detect_at_room: bool,
    /// Whether to ellipsize the message.
    pub(super) ellipsize: bool,
    /// Whether this is preformatted text.
    ///
    /// Whitespaces are untouched in preformatted text.
    pub(super) is_preformatted: bool,
}

/// Construct a new label for displaying a message's content.
pub(super) fn new_message_label() -> gtk::Label {
    gtk::Label::builder()
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .xalign(0.0)
        .valign(gtk::Align::Start)
        .use_markup(true)
        .css_classes(["document"])
        .build()
}

/// Create a widget for the given HTML nodes in the given room.
///
/// If `detect_at_room` is `true`, we will try to detect `@room` in the text.
///
/// If `ellipsize` is true, we will only render the first block.
///
/// If the sender name is set, it will be added as soon as possible.
///
/// Returns `None` if the widget would have been empty.
pub(super) fn widget_for_html_nodes(
    nodes: impl IntoIterator<Item = NodeRef>,
    config: HtmlWidgetConfig<'_>,
    add_ellipsis: bool,
    sender_name: &mut Option<&str>,
) -> Option<gtk::Widget> {
    let nodes = nodes.into_iter().collect::<Vec<_>>();

    if nodes.is_empty() {
        return None;
    }

    let groups = group_inline_nodes(nodes);
    let len = groups.len();

    let mut children = Vec::new();
    for (i, group) in groups.into_iter().enumerate() {
        let is_last = i == (len - 1);
        let add_ellipsis = add_ellipsis || (config.ellipsize && !is_last);

        let widget = match group {
            NodeGroup::Inline(inline_nodes) => {
                let Some(widget) =
                    label_for_inline_html(inline_nodes, config, add_ellipsis, sender_name)
                else {
                    continue;
                };

                widget
            }
            NodeGroup::Block(block_node) => {
                let Some(widget) =
                    widget_for_html_block(&block_node, config, add_ellipsis, sender_name)
                else {
                    continue;
                };

                // Include sender name before, if the child widget did not handle it.
                if let Some(sender_name) = sender_name.take() {
                    let label = new_message_label();
                    let (text, _) = InlineHtmlBuilder::new(false, false, config.is_preformatted)
                        .append_emote_with_name(&mut Some(sender_name))
                        .build();
                    label.set_label(&text);

                    children.push(label.upcast());
                }

                widget
            }
        };

        children.push(widget);

        if config.ellipsize {
            // Stop at the first constructed child.
            break;
        }
    }

    if children.is_empty() {
        return None;
    }
    if children.len() == 1 {
        return children.into_iter().next();
    }

    let grid = gtk::Grid::builder()
        .row_spacing(6)
        .accessible_role(gtk::AccessibleRole::Group)
        .build();

    for (row, child) in children.into_iter().enumerate() {
        let row = row.try_into().unwrap_or(i32::MAX);
        grid.attach(&child, 0, row, 1, 1);
    }

    Some(grid.upcast())
}

/// A group of nodes, representing the nodes contained in a single widget.
#[derive(Debug)]
enum NodeGroup {
    /// A group of inline nodes.
    Inline(Vec<NodeRef>),
    /// A block node.
    Block(NodeRef),
}

/// Group subsequent nodes that are inline.
///
/// Allows to group nodes by widget that will need to be constructed.
fn group_inline_nodes(nodes: Vec<NodeRef>) -> Vec<NodeGroup> {
    let mut result = Vec::new();
    let mut inline_group = None;

    for node in nodes {
        let is_block = node
            .as_element()
            .is_some_and(|element| SUPPORTED_BLOCK_ELEMENTS.contains(&element.name.local.as_ref()));

        if is_block {
            if let Some(inline) = inline_group.take() {
                result.push(NodeGroup::Inline(inline));
            }

            result.push(NodeGroup::Block(node));
        } else {
            let inline = inline_group.get_or_insert_with(Vec::default);
            inline.push(node);
        }
    }

    if let Some(inline) = inline_group.take() {
        result.push(NodeGroup::Inline(inline));
    }

    result
}

/// Construct a `GtkLabel` for the given inline nodes.
///
/// Returns `None` if the label would have been empty.
fn label_for_inline_html(
    nodes: impl IntoIterator<Item = NodeRef>,
    config: HtmlWidgetConfig<'_>,
    add_ellipsis: bool,
    sender_name: &mut Option<&str>,
) -> Option<gtk::Widget> {
    let (text, widgets) =
        InlineHtmlBuilder::new(config.ellipsize, add_ellipsis, config.is_preformatted)
            .detect_mentions(config.room, config.detect_at_room)
            .append_emote_with_name(sender_name)
            .build_with_nodes(nodes);

    if text.is_empty() {
        return None;
    }

    if let Some(widgets) = widgets {
        for pill in &widgets {
            if !pill.source().is_some_and(|s| s.is::<AtRoom>()) {
                // Show the profile on click.
                pill.set_activatable(true);
            }
        }
        let w = LabelWithWidgets::new();
        w.add_css_class("document");
        w.set_use_markup(true);
        w.set_ellipsize(config.ellipsize);
        w.set_label_and_widgets(text, widgets);
        Some(w.upcast())
    } else {
        let w = new_message_label();
        w.set_markup(&text);
        w.set_ellipsize(if config.ellipsize {
            pango::EllipsizeMode::End
        } else {
            pango::EllipsizeMode::None
        });
        Some(w.upcast())
    }
}

/// Create a widget for the given HTML block node.
fn widget_for_html_block(
    node: &NodeRef,
    config: HtmlWidgetConfig<'_>,
    add_ellipsis: bool,
    sender_name: &mut Option<&str>,
) -> Option<gtk::Widget> {
    let widget = match node.as_element()?.to_matrix().element {
        MatrixElement::H(heading) => {
            // Heading should only have inline elements as children.
            let w = label_for_inline_html(node.children(), config, add_ellipsis, sender_name)
                .unwrap_or_else(|| {
                    // We should show an empty title.
                    new_message_label().upcast()
                });
            w.add_css_class(&format!("h{}", heading.level.value()));
            w
        }
        MatrixElement::Blockquote => {
            let w = widget_for_html_nodes(node.children(), config, add_ellipsis, &mut None)?;
            w.add_css_class("quote");
            w
        }
        MatrixElement::P | MatrixElement::Div(_) | MatrixElement::Li | MatrixElement::Summary => {
            widget_for_html_nodes(node.children(), config, add_ellipsis, sender_name)?
        }
        MatrixElement::Ul => {
            widget_for_list(ListType::Unordered, node.children(), config, add_ellipsis)?
        }
        MatrixElement::Ol(list) => {
            widget_for_list(list.into(), node.children(), config, add_ellipsis)?
        }
        MatrixElement::Hr => gtk::Separator::new(gtk::Orientation::Horizontal).upcast(),
        MatrixElement::Pre => {
            widget_for_preformatted_text(node.children(), config, add_ellipsis, sender_name)?
        }
        MatrixElement::Details => widget_for_details(node.children(), config, add_ellipsis)?,
        element => {
            debug!("Unexpected HTML block element: {element:?}");
            return None;
        }
    };

    Some(widget)
}

/// Create a widget for a list.
fn widget_for_list(
    list_type: ListType,
    list_items: Children,
    config: HtmlWidgetConfig<'_>,
    add_ellipsis: bool,
) -> Option<gtk::Widget> {
    let list_items = list_items
        // Lists are supposed to only have list items as children.
        .filter(|node| {
            node.as_element()
                .is_some_and(|element| element.name.local.as_ref() == "li")
        })
        .collect::<Vec<_>>();

    if list_items.is_empty() {
        return None;
    }

    let grid = gtk::Grid::builder()
        .row_spacing(6)
        .column_spacing(6)
        .margin_end(6)
        .margin_start(6)
        .build();

    let len = list_items.len();

    for (pos, li) in list_items.into_iter().enumerate() {
        let is_last = pos == (len - 1);
        let add_ellipsis = add_ellipsis || (config.ellipsize && !is_last);

        let w = widget_for_html_nodes(li.children(), config, add_ellipsis, &mut None)
            // We should show an empty list item.
            .unwrap_or_else(|| new_message_label().upcast());

        let bullet = list_type.bullet(pos);

        let row = pos.try_into().unwrap_or(i32::MAX);
        grid.attach(&bullet, 0, row, 1, 1);
        grid.attach(&w, 1, row, 1, 1);

        if config.ellipsize {
            break;
        }
    }

    Some(grid.upcast())
}

/// The type of bullet for a list.
#[derive(Debug, Clone, Copy)]
enum ListType {
    /// An unordered list.
    Unordered,
    /// An ordered list.
    Ordered {
        /// The number to start counting from.
        start: i64,
    },
}

impl ListType {
    /// Construct the widget for the bullet of the current type at the given
    /// position.
    fn bullet(&self, position: usize) -> gtk::Label {
        let bullet = gtk::Label::builder()
            .css_classes(["document"])
            .valign(gtk::Align::Baseline)
            .build();

        match self {
            ListType::Unordered => bullet.set_label("•"),
            ListType::Ordered { start } => {
                bullet.set_label(&format!(
                    "{}.",
                    *start + i64::try_from(position).unwrap_or(i64::MAX)
                ));
            }
        }

        bullet
    }
}

impl From<OrderedListData> for ListType {
    fn from(value: OrderedListData) -> Self {
        Self::Ordered {
            start: value.start.unwrap_or(1),
        }
    }
}

/// Create a widget for preformatted text.
fn widget_for_preformatted_text(
    children: Children,
    config: HtmlWidgetConfig<'_>,
    add_ellipsis: bool,
    sender_name: &mut Option<&str>,
) -> Option<gtk::Widget> {
    let children = children.collect::<Vec<_>>();

    if children.is_empty() {
        return None;
    }

    let unique_code_child = (children.len() == 1)
        .then_some(&children[0])
        .and_then(|child| {
            match child
                .as_element()
                .map(|element| element.to_matrix().element)
            {
                Some(MatrixElement::Code(code)) => Some((child, code)),
                _ => None,
            }
        });

    let Some((child, code)) = unique_code_child else {
        // This is just preformatted text, we need to construct the children hierarchy.
        let config = HtmlWidgetConfig {
            is_preformatted: true,
            ..config
        };
        let widget = widget_for_html_nodes(children, config, add_ellipsis, sender_name)?;

        // We use the monospace font for preformatted text.
        widget.add_css_class("monospace");

        return Some(widget);
    };

    let children = child.children().collect::<Vec<_>>();

    if children.is_empty() {
        return None;
    }

    let text = InlineHtmlBuilder::new(config.ellipsize, add_ellipsis, config.is_preformatted)
        .build_with_nodes_text(children);

    if config.ellipsize {
        // Present text as inline code.
        let label = new_message_label();
        label.set_ellipsize(pango::EllipsizeMode::End);
        label.add_css_class("monospace");
        label.set_label(&text.escape_markup());

        return Some(label.upcast());
    }

    let buffer = sourceview::Buffer::builder()
        .highlight_matching_brackets(false)
        .text(text)
        .build();
    crate::utils::sourceview::setup_style_scheme(&buffer);

    let language = code
        .language
        .and_then(|lang| sourceview::LanguageManager::default().language(lang.as_ref()));
    buffer.set_language(language.as_ref());

    let view = sourceview::View::builder()
        .buffer(&buffer)
        .editable(false)
        .css_classes(["codeview", "frame", "monospace"])
        .hexpand(true)
        .build();

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
    scrolled.set_child(Some(&view));
    Some(scrolled.upcast())
}

/// Create a widget for a details disclosure element.
fn widget_for_details(
    children: Children,
    config: HtmlWidgetConfig<'_>,
    add_ellipsis: bool,
) -> Option<gtk::Widget> {
    let (summary, other_children) = children.partition::<Vec<_>, _>(|node| {
        node.as_element()
            .is_some_and(|element| element.name.local.as_ref() == "summary")
    });

    let content = widget_for_html_nodes(other_children, config, add_ellipsis, &mut None);

    let summary = summary
        .into_iter()
        .next()
        .and_then(|node| widget_for_details_summary(node.children(), config, add_ellipsis));

    if let Some(content) = content {
        let summary = summary.unwrap_or_else(|| {
            let label = new_message_label();
            // Translators: this is the fallback title for an expander.
            label.set_label(&gettext("Details"));
            label.upcast()
        });

        let expander = gtk::Expander::builder()
            .label_widget(&summary)
            .child(&content)
            .build();
        Some(expander.upcast())
    } else {
        summary
    }
}

/// Create a widget for a details disclosure element's summary.
fn widget_for_details_summary(
    children: Children,
    config: HtmlWidgetConfig<'_>,
    add_ellipsis: bool,
) -> Option<gtk::Widget> {
    let children = children.collect::<Vec<_>>();

    if children.is_empty() {
        return None;
    }

    // Only inline elements or a single header element are allowed in summary.
    if children.len() == 1
        && let Some(node) = children.first().filter(|node| {
            node.as_element().is_some_and(|element| {
                matches!(
                    element.name.local.as_ref(),
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                )
            })
        })
        && let Some(widget) = widget_for_html_block(node, config, add_ellipsis, &mut None)
    {
        return Some(widget);
    }

    label_for_inline_html(children, config, add_ellipsis, &mut None)
}
