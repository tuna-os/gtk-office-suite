// lists.rs — list markers and list geometry, shared by every renderer.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A list item's marker ("•", "3.") and its indent are presentation derived
// from the model (`ParaStyle::list`, `list_level`, `list_start`), never
// document text. The editor buffer, the page layout and exporters all ask
// here, so a numbered item cannot be "3." on screen and "1." in print.

use crate::model::{ListKind, ListLabel, NumberFormat, ParaStyle};

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
/// `list_start` restarts the count at that item. A table cell's list is
/// its own: each cell counts from the start.
pub fn ordinals<'a>(styles: impl IntoIterator<Item = &'a ParaStyle>) -> Vec<u32> {
    let mut counters = [0u32; MAX_LEVEL as usize + 1];
    let mut cell = None;
    styles
        .into_iter()
        .map(|style| {
            if style.table_cell != cell {
                counters = [0; MAX_LEVEL as usize + 1];
                cell = style.table_cell;
            }
            if style.list == ListKind::None {
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

/// The marker of the item styled `style`: as `marker`, but a numbered
/// item shows its own label (`ParaStyle::list_label`) when it has one.
pub fn marker_for(style: &ParaStyle, ordinal: u32) -> Option<String> {
    match (&style.list, &style.list_label) {
        (ListKind::Numbered, Some(label)) => Some(label_text(label, ordinal)),
        (kind, _) => marker(*kind, ordinal),
    }
}

/// `label` around `n`.
pub fn label_text(label: &ListLabel, n: u32) -> String {
    format!("{}{}{}", label.prefix, format_number(label.format, n), label.suffix)
}

/// `n` in `format`: 1, a, A, i, I, or nothing. Letters go on as Word and
/// LibreOffice count them: z, aa, bb.
pub fn format_number(format: NumberFormat, n: u32) -> String {
    match format {
        NumberFormat::Decimal => n.to_string(),
        NumberFormat::LowerLetter | NumberFormat::UpperLetter if n > 0 => {
            let letter = char::from(b'a' + ((n - 1) % 26) as u8);
            let s = letter.to_string().repeat(((n - 1) / 26 + 1) as usize);
            if format == NumberFormat::UpperLetter { s.to_uppercase() } else { s }
        }
        NumberFormat::LowerRoman | NumberFormat::UpperRoman if n > 0 => {
            let s = roman(n);
            if format == NumberFormat::LowerRoman { s.to_lowercase() } else { s }
        }
        NumberFormat::None => String::new(),
        // Letters and roman numerals have no zero.
        _ => n.to_string(),
    }
}

fn roman(mut n: u32) -> String {
    const DIGITS: [(u32, &str); 13] = [
        (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
        (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
    ];
    let mut s = String::new();
    for (value, digits) in DIGITS {
        while n >= value {
            s.push_str(digits);
            n -= value;
        }
    }
    s
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
    fn a_label_wraps_the_number_in_its_format() {
        let label = |prefix: &str, format, suffix: &str| ListLabel { prefix: prefix.into(), format, suffix: suffix.into() };
        assert_eq!(label_text(&label("3.", NumberFormat::Decimal, ""), 2), "3.2");
        assert_eq!(label_text(&label("(", NumberFormat::LowerLetter, ")"), 28), "(bb)");
        assert_eq!(label_text(&label("", NumberFormat::UpperRoman, "."), 14), "XIV.");
        assert_eq!(label_text(&label("", NumberFormat::LowerRoman, ""), 9), "ix");
        assert_eq!(label_text(&label("2.1", NumberFormat::None, ""), 3), "2.1");
        let mut style = ParaStyle { list: ListKind::Numbered, ..Default::default() };
        assert_eq!(marker_for(&style, 4).as_deref(), Some("4."));
        style.list_label = Some(label("", NumberFormat::UpperLetter, ")"));
        assert_eq!(marker_for(&style, 4).as_deref(), Some("D)"));
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
    fn a_cell_counts_its_own_list() {
        let n = ListKind::Numbered;
        let in_cell = |col| {
            let mut p = item(n, 0);
            p.style.table_cell = Some(crate::model::TableCell { table: 0, row: 0, col });
            p
        };
        let paras = [item(n, 0), in_cell(0), in_cell(0), in_cell(1), item(n, 0)];
        assert_eq!(ords(&paras), vec![1, 1, 2, 1, 1]);
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
