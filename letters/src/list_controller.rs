// SPDX-License-Identifier: GPL-3.0-or-later
//
// list_controller.rs — List continuation and markdown macro expanders for Letters.

use gtk4::{self as gtk, glib, prelude::*};
use libadwaita as adw;
use crate::formatting::active_buffer;

pub fn line_text(buf: &gtk::TextBuffer, iter: &gtk::TextIter) -> String {
    let mut start = *iter;
    start.backward_line();
    let mut end = *iter;
    end.forward_line();
    buf.text(&start, &end, false).to_string()
}

pub fn toggle_list(tv: &adw::TabView, kind: &str) {
    if let Some(buf) = active_buffer(tv) {
        let bounds = buf.selection_bounds();
        let (ins, _) = bounds.unwrap_or((buf.start_iter(), buf.start_iter()));
        let text = line_text(&buf, &ins);
        // Check if already a list item
        let has_bullet = text.trim_start().starts_with('\u{2022}')
            || text.trim_start().starts_with("- ");
        let has_number = text.trim_start().starts_with(|c: char| c.is_ascii_digit())
            && text.trim_start().contains(". ");

        buf.begin_user_action();
        let mut start = ins; start.backward_line();
        let mut end = ins; end.forward_line();

        if (kind == "bullet" && has_bullet) || (kind == "numbered" && has_number) {
            // Remove list prefix - delete from line start to after prefix
            let line = line_text(&buf, &ins);
            let trimmed = line.trim_start();
            let prefix_end = if kind == "bullet" {
                trimmed.find(|c| c != '\u{2022}' && c != ' ').unwrap_or(0)
            } else {
                trimmed.find(". ").map(|i| i + 2).unwrap_or(0)
            };
            let indent = line.len() - trimmed.len();
            let del_len = indent + prefix_end;
            if del_len > 0 {
                let mut del_end = start;
                del_end.forward_chars(del_len as i32);
                if del_end > start { buf.delete(&mut start, &mut del_end); }
            }
        } else {
            // Insert list prefix
            let prefix = if kind == "bullet" { "\u{2022} " } else { "1. " };
            buf.insert(&mut start, prefix);
        }
        buf.end_user_action();
    }
}

/// Connect list auto-continuation on Enter for a new buffer.
/// Uses EventControllerKey on the TextView to detect Enter.
pub fn connect_list_continuation(editor: &gtk::TextView, buf: &gtk::TextBuffer) {
    let buf = buf.clone();
    let ctrl = gtk::EventControllerKey::new();
    ctrl.connect_key_pressed(move |_, key, _code, _state| {
        if key == gtk::gdk::Key::Return || key == gtk::gdk::Key::KP_Enter {
            let bounds = buf.selection_bounds();
            let (ins, _) = bounds.unwrap_or((buf.start_iter(), buf.start_iter()));
            let mut line_start = ins;
            line_start.backward_line();
            let mut line_end = ins;
            line_end.forward_line();
            let line = buf.text(&line_start, &line_end, false);
            let trimmed = line.trim_start();

            // Bullet list continuation
            if trimmed.starts_with("\u{2022}") || trimmed.starts_with("- ") {
                let indent = line.len() - trimmed.len();
                let marker = "\u{2022} ";
                let after_marker = trimmed
                    .strip_prefix("\u{2022}").or_else(|| trimmed.strip_prefix("- "))
                    .unwrap_or("").trim_start();
                if after_marker.is_empty() {
                    return glib::Propagation::Proceed;
                }
                let prefix = format!("{}{}", " ".repeat(indent), marker);
                buf.insert(&mut line_end, &prefix);
                return glib::Propagation::Stop;
            }

            // Numbered list continuation
            if trimmed.starts_with(|c: char| c.is_ascii_digit()) && trimmed.contains(". ") {
                let num_str: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
                let after_num = &trimmed[num_str.len()..];
                let rest = after_num.strip_prefix(". ").unwrap_or("");
                if let Ok(n) = num_str.parse::<usize>() {
                    if rest.is_empty() {
                        return glib::Propagation::Proceed;
                    }
                    let indent = line.len() - trimmed.len();
                    let new_prefix = format!("{}{}. ", " ".repeat(indent), n + 1);
                    buf.insert(&mut line_end, &new_prefix);
                    return glib::Propagation::Stop;
                }
            }
        }
        glib::Propagation::Proceed
    });
    editor.add_controller(ctrl);
}

// ── Markdown macros ──────────────────────────────────────────────────
// Auto-formatting on Space/Enter: converts markdown syntax to rich text.

pub fn connect_markdown_macros(buf: &gtk::TextBuffer) {
    let buf = buf.clone();
    buf.connect_insert_text(move |buf, pos, text| {
        // Only trigger on Space (inline patterns) and Enter (block patterns)
        if text != " " && text != "\n" && text != "\r\n" { return; }

        let insert_pos = pos.offset();

        // ── Inline patterns (on Space) ──────────────────────────────
        if text == " " {
            // Check 2-10 chars before cursor for markdown patterns
            let start = if insert_pos >= 10 { insert_pos - 10 } else { 0 };
            let mut iter = buf.start_iter();
            iter.set_offset(start);
            let mut end = buf.start_iter();
            end.set_offset(insert_pos);
            let before = buf.text(&iter, &end, false).to_string();

            // Bold: **text** 
            if let Some(inner) = extract_md_pattern(&before, "**", "**") {
                apply_md_pattern(buf, &before, "**", inner, "bold");
                return;
            }
            // Italic: *text*
            if let Some(inner) = extract_md_pattern(&before, "*", "*") {
                apply_md_pattern(buf, &before, "*", inner, "italic");
                return;
            }
            // Strikethrough: ~~text~~
            if let Some(inner) = extract_md_pattern(&before, "~~", "~~") {
                apply_md_pattern(buf, &before, "~~", inner, "strikethrough");
                return;
            }
            // Inline code: `text`
            if let Some(inner) = extract_md_pattern(&before, "`", "`") {
                apply_md_pattern(buf, &before, "`", inner, "code");
                return;
            }
        }

        // ── Block patterns (on Enter) ──────────────────────────────
        if text == "\n" || text == "\r\n" {
            let mut line_iter = buf.start_iter();
            line_iter.set_offset(insert_pos);
            let mut line_start = line_iter;
            line_start.backward_line();
            let mut line_end = line_iter;
            line_end.forward_line();
            let line = buf.text(&line_start, &line_end, false);
            let trimmed = line.trim_start();

            // Heading: # ## ###
            for level in 1..=6 {
                let prefix = format!("{} ", "#".repeat(level));
                if trimmed.starts_with(&prefix) {
                    let tag_name = format!("h{}", level);
                    let _content = trimmed[prefix.len()..].to_string();
                    let indent = line.len() - trimmed.len();
                    buf.begin_user_action();
                    // Delete the markdown prefix
                    let mut del_start = line_start;
                    del_start.forward_chars(indent as i32 + prefix.len() as i32);
                    buf.delete(&mut line_start, &mut del_start);
                    // Apply heading tag
                    if let Some(tag) = buf.tag_table().lookup(&tag_name) {
                        let start = line_start; // now at content start
                        let mut end = line_end;
                        end.backward_char(); // exclude trailing newline
                        buf.apply_tag(&tag, &start, &end);
                    }
                    buf.end_user_action();
                    return;
                }
            }

            // Blockquote: >
            if trimmed.starts_with("> ") {
                let indent = line.len() - trimmed.len();
                buf.begin_user_action();
                let mut del_start = line_start;
                del_start.forward_chars(indent as i32 + 2);
                buf.delete(&mut line_start, &mut del_start);
                if let Some(tag) = buf.tag_table().lookup("blockquote") {
                    let start = line_start;
                    let mut end = line_end;
                    end.backward_char();
                    buf.apply_tag(&tag, &start, &end);
                }
                buf.end_user_action();
            }
        }
    });
}

/// Extract content between two delimiters in the text before cursor.
/// Returns the inner text if the pattern is found at the end of the string.
pub fn extract_md_pattern<'a>(before: &'a str, open: &str, close: &str) -> Option<&'a str> {
    // The pattern should be at the end: "something **text** "
    let trimmed = before.trim_end();
    // Check for space before pattern (word boundary)
    if !trimmed.ends_with(close) { return None; }
    let close_pos = trimmed.len() - close.len();
    if close_pos < open.len() { return None; }
    let before_close = &trimmed[..close_pos];
    if !before_close.ends_with(open) { return None; }
    let open_pos = before_close.len() - open.len();
    if open_pos == 0 || before_close.as_bytes()[open_pos - 1] == b' ' {
        let inner = &before_close[open_pos + open.len()..];
        if !inner.is_empty() {
            return Some(inner);
        }
    }
    None
}

/// Apply a markdown pattern: delete the markers, insert clean text, apply tag.
pub fn apply_md_pattern(buf: &gtk::TextBuffer, before: &str, delimiter: &str, inner: &str, tag_name: &str) {
    let offset = before.len() as i32;
    let del_len = (delimiter.len() * 2 + inner.len()) as i32;
    let start_off = offset - del_len;

    buf.begin_user_action();
    // Delete the markdown syntax (delimiters + inner text)
    let mut start = buf.start_iter();
    start.set_offset(start_off);
    let mut end = buf.start_iter();
    end.set_offset(offset);
    buf.delete(&mut start, &mut end);
    // Insert clean text
    let mut pos = buf.start_iter();
    pos.set_offset(start_off);
    buf.insert(&mut pos, inner);
    // Apply the formatting tag
    if let Some(tag) = buf.tag_table().lookup(tag_name) {
        let mut tag_start = buf.start_iter();
        tag_start.set_offset(start_off);
        let mut tag_end = buf.start_iter();
        tag_end.set_offset(start_off + inner.len() as i32);
        buf.apply_tag(&tag, &tag_start, &tag_end);
    }
    // Insert trailing space
    let mut space_pos = buf.start_iter();
    space_pos.set_offset(start_off + inner.len() as i32);
    buf.insert(&mut space_pos, " ");
    buf.end_user_action();
}
