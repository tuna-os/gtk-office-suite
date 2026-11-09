//! Markdown for the composer: the source a message is written in,
//! turned into the HTML that goes on the wire, and back again.
//!
//! The dialect is the one <https://www.markdownguide.org/> documents —
//! CommonMark for the basic syntax, plus every extended item that guide
//! lists: tables, fenced code blocks, footnotes, heading IDs, definition
//! lists, strikethrough, task lists, emoji shortcodes, highlighting,
//! subscript and superscript, and bare URLs turned into links. The pieces
//! pulldown-cmark does not carry (highlight, emoji, bare URLs) are done over
//! its event stream here, so they compose with everything else instead of
//! being string surgery on the finished HTML.
//!
//! The HTML is written for *mail*, not for a browser: every rule that
//! matters rides as a `style` attribute, because a `<style>` block is the
//! first thing a webmail client throws away.

mod dom;
mod emoji;
mod linkify;
mod outgoing;
mod render;
#[cfg(test)]
mod tests;
mod writer;

pub(crate) use dom::plain_text;
pub(crate) use outgoing::{pretty_html, sanitize_outgoing};
pub(crate) use render::to_html;
pub(crate) use writer::{from_html, html_to_text};
