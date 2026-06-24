// markdown.rs — Markdown rendering on Cairo canvas via Pango attributes.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Interprets markdown conventions (**bold**, *italic*, `code`, # headings)
// and renders them with Pango attributes on the Cairo context.
// Used by Decks TextBox rendering — matches Letters' markdown macro pattern.

use gtk4::cairo;
use pulldown_cmark::{Parser, Event, Tag, Options};

/// Render markdown text onto a Cairo context via Pango layout.
/// Returns the total vertical height consumed.
pub fn render_markdown(
    cr: &cairo::Context, text: &str,
    x: f64, mut y: f64, _max_width: f64, font_size: f64,
) -> f64 {
    let layout = pangocairo::functions::create_layout(cr);
    let desc = pango::FontDescription::from_string(&format!("Sans {}", font_size));
    layout.set_font_description(Some(&desc));

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(text, options);

    let mut bold = false;
    let mut italic = false;
    let mut is_code = false;
    let start_y = y;

    for event in parser {
        match event {
            Event::Start(Tag::Strong) => bold = true,
            Event::End(Tag::Strong)   => bold = false,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(Tag::Emphasis)   => italic = false,
            Event::Start(Tag::CodeBlock(_)) => is_code = true,
            Event::End(Tag::CodeBlock(_))   => is_code = false,
            Event::Text(t) | Event::Code(t) => {
                let text_str = t.into_string();
                layout.set_text(&text_str);
                let mut attrs = pango::AttrList::new();
                if bold   { attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold)); }
                if italic { attrs.insert(pango::AttrInt::new_style(pango::Style::Italic)); }
                if is_code { attrs.insert(pango::AttrString::new_family("Monospace")); }
                layout.set_attributes(Some(&attrs));
                cr.move_to(x, y);
                pangocairo::functions::show_layout(cr, &layout);
                let (_, h) = layout.pixel_size();
                y += h as f64 + 2.0;
            }
            Event::Start(Tag::Heading { .. }) => bold = true,
            Event::End(Tag::Heading { .. }) => { bold = false; y += 6.0; }
            Event::SoftBreak | Event::HardBreak => {
                let (_, h) = layout.pixel_size();
                y += h as f64 + 4.0;
            }
            _ => {}
        }
    }
    y - start_y
}
