// SPDX-License-Identifier: GPL-3.0-or-later
//! `text:list-style` parsing and list numbering continuation.

#![allow(unused_imports)]
use super::*;

/// Each `text:list-style` in `xml`: the label of each numbered level that
/// has one other than "N." (`style:num-prefix`, `style:num-format` and
/// `style:num-suffix`), by level, 0-based.
pub(super) fn list_style_labels(xml: &str) -> std::collections::HashMap<String, Vec<Option<ListLabel>>> {
    let mut out: std::collections::HashMap<String, Vec<Option<ListLabel>>> = Default::default();
    let mut reader = Reader::from_str(xml);
    let mut current: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                "text:list-style" => current = attr_val(&e, "style:name"),
                "text:list-level-style-number" => {
                    let (Some(style), Some(level)) = (current.as_ref(), attr_val(&e, "text:level").and_then(|l| l.parse::<usize>().ok())) else { continue };
                    if !(1..=10).contains(&level) {
                        continue;
                    }
                    let format = match attr_val(&e, "style:num-format").as_deref() {
                        Some("a") => NumberFormat::LowerLetter,
                        Some("A") => NumberFormat::UpperLetter,
                        Some("i") => NumberFormat::LowerRoman,
                        Some("I") => NumberFormat::UpperRoman,
                        Some("") | None => NumberFormat::None,
                        _ => NumberFormat::Decimal,
                    };
                    let label = ListLabel {
                        prefix: attr_val(&e, "style:num-prefix").unwrap_or_default(),
                        format,
                        suffix: attr_val(&e, "style:num-suffix").unwrap_or_default(),
                    };
                    let labels = out.entry(style.clone()).or_default();
                    if labels.len() < level {
                        labels.resize(level, None);
                    }
                    labels[level - 1] = (label != ListLabel { suffix: ".".into(), ..Default::default() }).then_some(label);
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.name().as_ref() == "text:list-style" => current = None,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Each `text:list-style` in `xml`: whether each of its levels is numbered,
/// by level (1-based in the file, 0-based here). LibreOffice names the list
/// styles it writes "WWNum1", "L2"…, so the kind can't be read off the name.
pub(super) fn list_style_kinds(xml: &str) -> std::collections::HashMap<String, Vec<ListKind>> {
    let mut out: std::collections::HashMap<String, Vec<ListKind>> = Default::default();
    let mut reader = Reader::from_str(xml);
    let mut current: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                "text:list-style" => current = attr_val(&e, "style:name"),
                name @ ("text:list-level-style-number" | "text:list-level-style-bullet" | "text:list-level-style-image") => {
                    let (Some(style), Some(level)) = (current.as_ref(), attr_val(&e, "text:level").and_then(|l| l.parse::<usize>().ok())) else { continue };
                    let kinds = out.entry(style.clone()).or_default();
                    if (1..=10).contains(&level) {
                        if kinds.len() < level {
                            kinds.resize(level, ListKind::Bullet);
                        }
                        // A number level with no digits is still a label
                        // when it has text: Word's "2.1" with no number.
                        let has_text = |a: &str| attr_val(&e, a).is_some_and(|t| !t.is_empty());
                        let numbered = name == "text:list-level-style-number"
                            && (has_text("style:num-format") || has_text("style:num-prefix") || has_text("style:num-suffix"));
                        kinds[level - 1] = if numbered { ListKind::Numbered } else { ListKind::Bullet };
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.name().as_ref() == "text:list-style" => current = None,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// One list level's position: (where its text starts, how far its marker
/// hangs before that text), see `list_style_indents`.
pub(super) type ListLevelIndent = Option<(f64, Option<f64>)>;

/// `list_style_indents` by `text:list-style` name.
pub(super) type ListIndentMap = std::collections::HashMap<String, Vec<ListLevelIndent>>;

/// Each `text:list-style` in `xml`: where each level puts its text, in
/// points from the paragraph's indent (`fo:margin-left` of its
/// label-alignment properties, Word's 0.5in for a first level), by level,
/// 0-based, with how far its marker hangs before that text (the negated
/// `fo:text-indent`, Word's 0.5in for a "1.1" level) where it says.
/// None where a level declares no position.
pub(super) fn list_style_indents(xml: &str) -> ListIndentMap {
    let mut out: ListIndentMap = Default::default();
    let mut reader = Reader::from_str(xml);
    let mut current: Option<String> = None;
    let mut level: Option<usize> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                "text:list-style" => current = attr_val(&e, "style:name"),
                "text:list-level-style-number" | "text:list-level-style-bullet" | "text:list-level-style-image" => {
                    level = attr_val(&e, "text:level").and_then(|l| l.parse::<usize>().ok()).filter(|l| (1..=10).contains(l));
                }
                "style:list-level-label-alignment" => {
                    let (Some(style), Some(l)) = (current.as_ref(), level) else { continue };
                    if let Some(margin) = attr_val(&e, "fo:margin-left").and_then(|v| parse_length_pt(&v)) {
                        let levels = out.entry(style.clone()).or_default();
                        if levels.len() < l {
                            levels.resize(l, None);
                        }
                        let hanging = attr_val(&e, "fo:text-indent").and_then(|v| parse_length_pt(&v)).map(|t| (-t).max(0.0));
                        levels[l - 1] = Some((margin, hanging));
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.name().as_ref() == "text:list-style" => current = None,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// A list that continues the numbering of the one before it with its
/// style starts where that one stopped: its first numbered item gets the
/// start value, so headings between them (which end a list in the model)
/// do not restart the count at 1. A Word conversion writes every numbered
/// paragraph between headings as such a list.
pub(super) fn continue_numbering(paragraphs: &mut [Paragraph], top_lists: &[(usize, usize, Option<String>, bool)]) {
    let mut counts: std::collections::HashMap<Option<String>, u32> = Default::default();
    for (start, end, style, continues) in top_lists {
        let mut count = if *continues { counts.get(style).copied().unwrap_or(0) } else { 0 };
        let end = (*end).min(paragraphs.len());
        let mut first = true;
        for p in paragraphs.get_mut(*start..end).into_iter().flatten() {
            if p.style.list != ListKind::Numbered || p.style.list_level != 0 {
                continue;
            }
            if first && *continues && count > 0 && p.style.list_start.is_none() {
                p.style.list_start = Some(count + 1);
            }
            first = false;
            count = p.style.list_start.unwrap_or(count + 1);
        }
        counts.insert(style.clone(), count);
    }
}
