// lists.rs — list markers and list geometry, shared by every renderer.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A list item's marker ("•", "3.") and its indent are presentation derived
// from the model (`ParaStyle::list`, `list_level`, `list_start`), never
// document text. The editor buffer, the page layout and exporters all ask
// here, so a numbered item cannot be "3." on screen and "1." in print.

use crate::model::{ListKind, ParaStyle};

/// Indent per nesting level, in points. Word's built-in "List Bullet N"
/// styles and LibreOffice's rendering of them step 0.25in per level.
pub const LEVEL_INDENT_PT: f64 = 18.0;

/// Hanging indent of a list item, in points: the marker sits this far left
/// of the item's text, and wrapped lines align with the text, not the marker.
pub const HANGING_PT: f64 = 18.0;

/// Deepest nesting level a renderer distinguishes (Word and ODF allow 9).
pub const MAX_LEVEL: u8 = 8;

/// Left edge of a list item's text, in points from the paragraph's own
/// left indent. The marker starts `HANGING_PT` before it.
pub fn text_indent_pt(level: u8) -> f64 {
    f64::from(level.min(MAX_LEVEL) + 1) * LEVEL_INDENT_PT
}

/// The ordinal each numbered paragraph displays; 0 for every other one.
///
/// Numbering runs per nesting level. A deeper item does not interrupt the
/// count of the level above it (1. / • / 2.), a shallower item restarts
/// every deeper level, and anything that is not a list item ends the list.
/// `list_start` restarts the count at that item.
pub fn ordinals<'a>(styles: impl IntoIterator<Item = &'a ParaStyle>) -> Vec<u32> {
    let mut counters = [0u32; MAX_LEVEL as usize + 1];
    styles
        .into_iter()
        .map(|style| {
            if style.list == ListKind::None || style.table_cell.is_some() {
                counters = [0; MAX_LEVEL as usize + 1];
                return 0;
            }
            let level = usize::from(style.list_level.min(MAX_LEVEL));
            for deeper in &mut counters[level + 1..] {
                *deeper = 0;
            }
            if style.list != ListKind::Numbered {
                counters[level] = 0;
                return 0;
            }
            counters[level] = match style.list_start {
                Some(start) => start,
                None => counters[level] + 1,
            };
            counters[level]
        })
        .collect()
}

/// The marker drawn before a list item's text: a bullet glyph, or the
/// ordinal and a full stop. `None` for a paragraph that is not a list item.
pub fn marker(kind: ListKind, ordinal: u32) -> Option<String> {
    match kind {
        ListKind::None => None,
        ListKind::Bullet => Some(BULLET.to_string()),
        ListKind::Numbered => Some(format!("{ordinal}.")),
    }
}

/// The bullet glyph. U+2022, as Word's and LibreOffice's default bullets.
pub const BULLET: char = '\u{2022}';

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Paragraph;

    fn ords(paras: &[Paragraph]) -> Vec<u32> {
        ordinals(paras.iter().map(|p| &p.style))
    }

    fn item(kind: ListKind, level: u8) -> Paragraph {
        Paragraph {
            style: ParaStyle { list: kind, list_level: level, ..Default::default() },
            runs: vec![crate::model::Run::plain("x")],
        }
    }

    #[test]
    fn numbering_counts_within_a_list() {
        let n = ListKind::Numbered;
        let paras = [item(n, 0), item(n, 0), item(n, 0)];
        assert_eq!(ords(&paras), vec![1, 2, 3]);
    }

    #[test]
    fn nested_items_do_not_interrupt_the_outer_count() {
        let (n, b) = (ListKind::Numbered, ListKind::Bullet);
        let paras = [item(n, 0), item(b, 1), item(n, 1), item(n, 1), item(n, 0), item(n, 1)];
        // The level-1 count restarts under each new level-0 item.
        assert_eq!(ords(&paras), vec![1, 0, 1, 2, 2, 1]);
    }

    #[test]
    fn a_plain_paragraph_ends_the_list() {
        let n = ListKind::Numbered;
        let paras = [item(n, 0), item(n, 0), item(ListKind::None, 0), item(n, 0)];
        assert_eq!(ords(&paras), vec![1, 2, 0, 1]);
    }

    #[test]
    fn list_start_restarts_at_that_item() {
        let n = ListKind::Numbered;
        let mut paras = vec![item(n, 0), item(n, 0), item(n, 0)];
        paras[1].style.list_start = Some(5);
        assert_eq!(ords(&paras), vec![1, 5, 6]);
    }

    #[test]
    fn markers_are_glyphs_not_markdown() {
        assert_eq!(marker(ListKind::Bullet, 0).as_deref(), Some("\u{2022}"));
        assert_eq!(marker(ListKind::Numbered, 12).as_deref(), Some("12."));
        assert_eq!(marker(ListKind::None, 0), None);
    }

    #[test]
    fn each_level_indents_one_step_further() {
        assert_eq!(text_indent_pt(0), 18.0);
        assert_eq!(text_indent_pt(2), 54.0);
        assert_eq!(text_indent_pt(200), text_indent_pt(MAX_LEVEL));
    }
}
