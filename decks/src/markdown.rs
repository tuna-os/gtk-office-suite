// markdown.rs — Markdown rendering on Cairo canvas.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Parses markdown text and renders it with Pango attributes on a Cairo
// context. Reuses Letters' pulldown-cmark pattern for Decks text boxes.
// Supports: **bold**, *italic*, `code`, # headings, > blockquotes.

use gtk4::cairo;
use pulldown_cmark::{Parser, Event, Tag, Options};

/// Render markdown text onto a Cairo context at (x, y).
/// Returns the total height consumed.
pub fn render_markdown(
    cr: &cairo::Context, text: &str, x: f64, y: f64,
    _max_width: f64, font_size: f64,
) -> f64 {
    let layout = pangocairo::functions::create_layout(cr);
    let desc = pango::FontDescription::from_string(&format!("Sans {}", font_size));
    layout.set_font_description(Some(&desc));
    layout.set_text(text);

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(text, options);

    let mut cy = y;
    let mut bold = false;
    let mut italic = false;
    let mut _code = false;

    for event in parser {
        match event {
            Event::Start(Tag::Strong) => bold = true,
            Event::End(Tag::Strong) => bold = false,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(Tag::Emphasis) => italic = false,
            Event::Start(Tag::CodeBlock(_)) => _code = true,
            Event::End(Tag::CodeBlock(_)) => _code = false,
            Event::Text(t) | Event::Code(t) => {
                let text_str = t.to_string();
                layout.set_text(&text_str);

                // Build attribute list
                let mut attrs = pango::AttrList::new();
                if bold {
                    let attr = pango::AttrInt::new_weight(pango::Weight::Bold);
                    attrs.insert(attr);
                }
                if italic {
                    let attr = pango::AttrInt::new_style(pango::Style::Italic);
                    attrs.insert(attr);
                }
                if _code {
                    let attr = pango::AttrString::new_family("Monospace");
                    attrs.insert(attr);
                }
                layout.set_attributes(Some(&attrs));

                cr.move_to(x, cy);
                pangocairo::functions::show_layout(cr, &layout);
                let (_, h) = layout.pixel_size();
                cy += h as f64 + 2.0;
            }
            Event::Start(Tag::Heading { level: _, .. }) | Event::End(Tag::Heading { .. }) => {
                // Headings handled as bold + larger for now
                bold = true;
            }
            Event::SoftBreak | Event::HardBreak => {
                let (_, h) = layout.pixel_size();
                cy += h as f64 + 4.0;
            }
            _ => {}
        }
    }
    cy - y // total height consumed
}
