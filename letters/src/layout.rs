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
/// Uses Pango to measure line heights and positions accurately across styled runs.
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

    let line_count = layout.line_count() as usize;
    if line_count == 0 {
        return vec![Page { index: 0, start_offset: 0, end_offset: snap_to_char_boundary(&text, text.len()) as i32, line_count: 0 }];
    }

    let mut pages = Vec::new();
    let mut page_start_line = 0usize;
    let mut current_page_height = 0.0f64;
    let mut page_idx = 0usize;

    for i in 0..line_count {
        let line_height = if let Some(line) = layout.line(i as i32) {
            let extents = line.extents().1;
            (extents.height() as f64 / pango::SCALE as f64).max(1.0)
        } else {
            14.0
        };

        if current_page_height + line_height > content_height_pts && i > page_start_line {
            let start_offset = line_number_to_byte_offset(&text, page_start_line);
            let end_offset = line_number_to_byte_offset(&text, i);
            pages.push(Page {
                index: page_idx,
                start_offset,
                end_offset,
                line_count: i - page_start_line,
            });
            page_idx += 1;
            page_start_line = i;
            current_page_height = line_height;
        } else {
            current_page_height += line_height;
        }
    }

    if page_start_line < line_count {
        let start_offset = line_number_to_byte_offset(&text, page_start_line);
        let end_offset = snap_to_char_boundary(&text, text.len()) as i32;
        pages.push(Page {
            index: page_idx,
            start_offset,
            end_offset,
            line_count: line_count - page_start_line,
        });
    }

    pages
}

/// Snap any raw byte offset to a valid UTF-8 character boundary.
pub fn snap_to_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Convert a 0-based line number to a byte offset in the text, snapped to char boundary.
fn line_number_to_byte_offset(text: &str, line_num: usize) -> i32 {
    if line_num == 0 { return 0; }
    let mut found = 0usize;
    for (i, &b) in text.as_bytes().iter().enumerate() {
        if b == b'\n' {
            found += 1;
            if found >= line_num {
                return snap_to_char_boundary(text, i + 1) as i32;
            }
        }
    }
    snap_to_char_boundary(text, text.len()) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a LayoutConfig with fixed margins; only page size/columns vary.
    fn config(page_width_pt: f64, page_height_pt: f64, column_count: u32) -> LayoutConfig {
        LayoutConfig {
            page_width_pt,
            page_height_pt,
            margin_top: 72.0,
            margin_bottom: 72.0,
            margin_left: 72.0,
            margin_right: 72.0,
            column_count,
        }
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn default_config_content_area_is_a4_minus_margins() {
        let cfg = LayoutConfig::default();
        assert_close(cfg.content_height(), 842.0 - 72.0 - 72.0);
        assert_close(cfg.content_width(), 595.0 - 72.0 - 72.0);
    }

    #[test]
    fn content_width_splits_across_columns() {
        let cfg = config(595.0, 842.0, 2);
        assert_close(cfg.content_width(), (595.0 - 144.0) / 2.0);
    }

    #[test]
    fn zero_column_count_clamps_to_single_column() {
        let cfg = config(595.0, 842.0, 0);
        assert_close(cfg.content_width(), 595.0 - 144.0);
    }

    #[test]
    fn content_area_never_drops_below_ten_points() {
        // Margins larger than the page → clamp to the 10pt floor.
        let cfg = config(100.0, 100.0, 1);
        assert_close(cfg.content_height(), 10.0);
        assert_close(cfg.content_width(), 10.0);
    }

    #[test]
    fn line_zero_always_maps_to_zero() {
        assert_eq!(line_number_to_byte_offset("", 0), 0);
        assert_eq!(line_number_to_byte_offset("a\nb", 0), 0);
    }

    #[test]
    fn byte_offset_lands_after_each_newline() {
        let text = "a\nb\nc";
        assert_eq!(line_number_to_byte_offset(text, 1), 2); // points at 'b'
        assert_eq!(line_number_to_byte_offset(text, 2), 4); // points at 'c'
    }

    #[test]
    fn byte_offset_past_last_line_returns_len() {
        let text = "a\nb\nc";
        assert_eq!(line_number_to_byte_offset(text, 3), text.len() as i32);
        assert_eq!(line_number_to_byte_offset(text, 99), text.len() as i32);
    }

    #[test]
    fn byte_offset_is_byte_based_not_char_based_for_utf8() {
        // 'é' and 'ö' are two bytes each; offsets must skip whole bytes.
        let text = "héllo\nwörld";
        assert_eq!(text.len(), 13);
        assert_eq!(line_number_to_byte_offset(text, 1), 7); // after the first \n
        assert_eq!(line_number_to_byte_offset(text, 2), text.len() as i32);
    }

    #[test]
    fn trailing_newline_counts_as_line_break() {
        let text = "x\n";
        assert_eq!(line_number_to_byte_offset(text, 1), text.len() as i32);
    }

    #[test]
    fn empty_text_maps_any_line_to_zero() {
        assert_eq!(line_number_to_byte_offset("", 5), 0);
    }

    #[test]
    fn consecutive_newlines_are_empty_lines() {
        let text = "a\n\nb";
        assert_eq!(line_number_to_byte_offset(text, 1), 2);
        assert_eq!(line_number_to_byte_offset(text, 2), 3);
        assert_eq!(line_number_to_byte_offset(text, 3), text.len() as i32);
    }

    #[test]
    fn snap_to_char_boundary_handles_multibyte_utf8() {
        let text = "🦀emoji test 🌸";
        // '🦀' is 4 bytes (indices 0..4)
        assert_eq!(snap_to_char_boundary(text, 0), 0);
        assert_eq!(snap_to_char_boundary(text, 1), 0);
        assert_eq!(snap_to_char_boundary(text, 2), 0);
        assert_eq!(snap_to_char_boundary(text, 3), 0);
        assert_eq!(snap_to_char_boundary(text, 4), 4);
    }
}
