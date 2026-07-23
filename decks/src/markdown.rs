// markdown.rs — Markdown rendering on Cairo canvas using pulldown-cmark + Pango.
// Parses **bold**, *italic*, `code` and renders via Pango attributes on Cairo.
//
// Not yet called from slide rendering (see canvas.rs, which currently
// renders plain text) — kept rather than deleted since it's a working,
// self-contained rendering path planned for slide body text.
#![allow(dead_code)]

use gtk4::cairo;
use pulldown_cmark::{Parser, Event, Tag, TagEnd, Options};

pub fn render_markdown(
    cr: &cairo::Context, text: &str, x: f64, y: f64,
    _max_width: f64, _font_size: f64,
) -> f64 {
    let layout = pangocairo::functions::create_layout(cr);
    let desc = pango::FontDescription::from_string("Sans 14");
    layout.set_font_description(Some(&desc));

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(text, options);

    let mut cy = y;
    let mut bold = false;
    let mut italic = false;
    let mut is_code = false;

    for event in parser {
        match event {
            Event::Start(Tag::Strong) => bold = true,
            Event::End(TagEnd::Strong) => bold = false,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(TagEnd::Emphasis) => italic = false,
            Event::Start(Tag::CodeBlock(_)) => is_code = true,
            Event::End(TagEnd::CodeBlock) => is_code = false,
            Event::Text(t) => {
                render_text(cr, &layout, t.as_ref(), x, &mut cy, bold, italic, is_code);
            }
            Event::Code(t) => {
                render_text(cr, &layout, t.as_ref(), x, &mut cy, bold, italic, true);
            }
            Event::SoftBreak => {
                let (_, h) = layout.pixel_size();
                cy += h as f64 + 4.0;
            }
            Event::HardBreak => {
                let (_, h) = layout.pixel_size();
                cy += h as f64 + 4.0;
            }
            _ => {}
        }
    }
    cy - y
}

#[allow(clippy::too_many_arguments)]
fn render_text(
    cr: &cairo::Context, layout: &pango::Layout, text_str: &str,
    x: f64, cy: &mut f64, bold: bool, italic: bool, is_code: bool,
) {
    layout.set_text(text_str);
    let attrs = pango::AttrList::new();
    if bold { attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold)); }
    if italic { attrs.insert(pango::AttrInt::new_style(pango::Style::Italic)); }
    if is_code { attrs.insert(pango::AttrString::new_family("Monospace")); }
    layout.set_attributes(Some(&attrs));
    cr.move_to(x, *cy);
    pangocairo::functions::show_layout(cr, layout);
    let (_, h) = layout.pixel_size();
    *cy += h as f64 + 2.0;
}
