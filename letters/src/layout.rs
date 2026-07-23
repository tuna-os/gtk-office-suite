// SPDX-License-Identifier: GPL-3.0-or-later
//
// layout.rs — PangoLayout-based pagination engine for Letters.
// Splits a GtkTextBuffer's text into pages based on GSettings page size and margins.

use gtk4::{self as gtk, prelude::*};

/// A page calculated by the layout engine.
#[derive(Debug, Clone)]
pub struct Page {
    /// 0-based page index. Not yet read anywhere (planned for a "Page N of
    /// M" status indicator); kept as part of the page metadata contract.
    #[allow(dead_code)]
    pub index: usize,
    /// Byte offset at the start of this page's content in the buffer.
    pub start_offset: i32,
    /// Byte offset at the end of this page's content.
    pub end_offset: i32,
    /// Number of lines on this page. Not yet read anywhere; kept as part
    /// of the page metadata contract.
    #[allow(dead_code)]
    pub line_count: usize,
}

/// Layout configuration for pagination.
#[derive(Debug, Clone)]
pub struct LayoutConfig {
    pub page_width_pt: f64,
    pub page_height_pt: f64,
    pub margin_top: f64,
    pub margin_bottom: f64,
    pub margin_left: f64,
    pub margin_right: f64,
    pub column_count: u32,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            page_width_pt: 595.0,
            page_height_pt: 842.0,
            margin_top: 72.0,
            margin_bottom: 72.0,
            margin_left: 72.0,
            margin_right: 72.0,
            column_count: 1,
        }
    }
}

impl LayoutConfig {
    pub fn from_settings(settings: &gtk4::gio::Settings) -> Self {
        Self {
            page_width_pt: settings.double("page-width-pt").max(100.0),
            page_height_pt: settings.double("page-height-pt").max(100.0),
            margin_top: settings.double("page-margin-top").max(0.0),
            margin_bottom: settings.double("page-margin-bottom").max(0.0),
            margin_left: settings.double("page-margin-left").max(0.0),
            margin_right: settings.double("page-margin-right").max(0.0),
            column_count: settings.int("column-count").max(1) as u32,
        }
    }

    pub fn content_height(&self) -> f64 {
        (self.page_height_pt - self.margin_top - self.margin_bottom).max(10.0)
    }

    pub fn content_width(&self) -> f64 {
        let total = (self.page_width_pt - self.margin_left - self.margin_right).max(10.0);
        (total / self.column_count.max(1) as f64).max(10.0)
    }
}

/// Paginate a text buffer into pages using PangoLayout measurement.
/// Uses Pango to measure line count and estimate heights.
pub fn paginate(
    buf: &gtk::TextBuffer,
    config: &LayoutConfig,
    pango_context: &gtk4::pango::Context,
) -> Vec<Page> {
    let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
    if text.is_empty() {
        return vec![Page { index: 0, start_offset: 0, end_offset: 0, line_count: 0 }];
    }

    let content_height_pts = config.content_height();
    let content_width_pts = config.content_width();

    // Create layout and measure
    let layout = pango::Layout::new(pango_context);
    layout.set_text(&text);
    layout.set_width((content_width_pts * pango::SCALE as f64) as i32);

    // Estimate line height from the layout's first line
    let line_count = layout.line_count() as usize;
    let total_height = layout.size().1 as f64 / pango::SCALE as f64;
    let est_line_height = if line_count > 0 {
        total_height / line_count as f64
    } else {
        14.0 // fallback: ~11pt font * 1.27
    };

    let lines_per_page = (content_height_pts / est_line_height).max(1.0) as usize;
    if lines_per_page == 0 {
        return vec![Page { index: 0, start_offset: 0, end_offset: text.len() as i32, line_count }];
    }

    let mut pages = Vec::new();
    let total_chunks = line_count.div_ceil(lines_per_page);

    for page_idx in 0..total_chunks {
        let line_start = page_idx * lines_per_page;
        let line_end = ((page_idx + 1) * lines_per_page).min(line_count);
        let page_lines = line_end - line_start;

        // Approximate byte offsets from line positions
        // Lines in UTF-8: walk the text counting newlines
        let start_offset = line_number_to_byte_offset(&text, line_start);
        let end_offset = if page_idx + 1 >= total_chunks {
            text.len() as i32
        } else {
            line_number_to_byte_offset(&text, line_end)
        };

        pages.push(Page {
            index: page_idx,
            start_offset,
            end_offset,
            line_count: page_lines,
        });
    }

    pages
}

/// Convert a 0-based line number to a byte offset in the text.
/// Lines are separated by '\n'.
fn line_number_to_byte_offset(text: &str, line_num: usize) -> i32 {
    if line_num == 0 { return 0; }
    let mut found = 0usize;
    for (i, &b) in text.as_bytes().iter().enumerate() {
        if b == b'\n' {
            found += 1;
            if found >= line_num {
                return (i + 1) as i32; // position after the newline
            }
        }
    }
    text.len() as i32
}
