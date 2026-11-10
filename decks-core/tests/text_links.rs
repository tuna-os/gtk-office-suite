// text_links.rs — #1998: a slide's text linked to a web or mail address
// keeps its link through PPTX and ODP, in text boxes and table cells,
// and across the two formats.
// The table cell is checked in PPTX alone: the ODP reader does not read
// tables back yet.
// SPDX-License-Identifier: GPL-3.0-or-later

use decks_core::engine::table::{TableCell, TableData};
use decks_core::engine::{Run, RunStyle, SlideObject, TextBody};
use decks_core::Deck;

const SITE: &str = "https://example.org/a?b=1&c=2";
const MAIL: &str = "mailto:team@example.org";

fn link(url: &str) -> RunStyle {
    RunStyle {
        link: Some(url.into()),
        underline: true,
        ..Default::default()
    }
}

fn deck_with_links() -> Deck {
    let mut d = Deck::new();
    let runs = vec![
        Run {
            text: "Read ".into(),
            style: RunStyle::default(),
        },
        Run {
            text: "the site".into(),
            style: link(SITE),
        },
        Run {
            text: " or ".into(),
            style: RunStyle::default(),
        },
        Run {
            text: "write".into(),
            style: link(MAIL),
        },
    ];
    let cell = TableCell {
        runs: vec![Run {
            text: "docs".into(),
            style: link(SITE),
        }],
        ..Default::default()
    };
    d.slides[0].objects = vec![
        SlideObject::TextBox {
            text: "Read the site or write".into(),
            x: 60.0,
            y: 60.0,
            w: 600.0,
            h: 100.0,
            rotation: 0.0,
            runs,
            body: TextBody::default(),
        },
        SlideObject::Table {
            x: 60.0,
            y: 200.0,
            w: 320.0,
            h: 40.0,
            rotation: 0.0,
            table: TableData {
                col_widths: vec![160.0, 160.0],
                row_heights: vec![40.0],
                rows: vec![vec![cell, TableCell::default()]],
                first_row: false,
                band_row: false,
                accent: None,
                cell_margins: None,
                style_id: None,
            },
        },
    ];
    d
}

/// Each linked piece of text with its link, neighbours with the same link
/// merged, in reading order.
fn links_in(runs: &[Run]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for r in runs {
        let Some(url) = &r.style.link else { continue };
        match out.last_mut() {
            Some(last) if &last.1 == url => last.0.push_str(&r.text),
            _ => out.push((r.text.clone(), url.clone())),
        }
    }
    out
}

/// `with_table`: ODP does not read tables back yet, so a deck that went
/// through it has only its text box to check.
fn check(d: &Deck, what: &str, with_table: bool) {
    let objects = &d.slides[0].objects;
    let text_box = objects.iter().find_map(|o| match o {
        SlideObject::TextBox { runs, .. } => Some(runs),
        _ => None,
    });
    assert_eq!(
        links_in(text_box.unwrap_or_else(|| panic!("{what}: no text box"))),
        [
            ("the site".to_string(), SITE.to_string()),
            ("write".to_string(), MAIL.to_string())
        ],
        "{what}: text box links"
    );
    if !with_table {
        return;
    }
    let table = objects.iter().find_map(|o| match o {
        SlideObject::Table { table, .. } => Some(table),
        _ => None,
    });
    let table = table.unwrap_or_else(|| panic!("{what}: no table"));
    assert_eq!(
        links_in(&table.rows[0][0].runs),
        [("docs".to_string(), SITE.to_string())],
        "{what}: cell link"
    );
    assert!(
        table.rows[0][1].runs.iter().all(|r| r.style.link.is_none()),
        "{what}: a plain cell gains no link"
    );
}

#[test]
fn text_links_save_and_reopen_in_both_formats_and_across_them() {
    let dir = tempfile::tempdir().unwrap();
    let deck = deck_with_links();
    for ext in ["pptx", "odp"] {
        let path = dir.path().join(format!("links.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck).unwrap();
        let back = decks_core::read_deck(path.to_str().unwrap()).unwrap();
        check(&back, ext, ext == "pptx");
        let other = if ext == "pptx" { "odp" } else { "pptx" };
        let cross = dir.path().join(format!("from-{ext}.{other}"));
        decks_core::write_deck(cross.to_str().unwrap(), &back).unwrap();
        check(
            &decks_core::read_deck(cross.to_str().unwrap()).unwrap(),
            &format!("{ext} -> {other}"),
            false,
        );
    }
}

#[test]
fn a_saved_deck_with_links_reports_no_loss() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["pptx", "odp"] {
        let path = dir.path().join(format!("links.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck_with_links()).unwrap();
        let report = decks_core::loss::content_a_save_drops(path.to_str().unwrap());
        assert!(
            !report.features.iter().any(|f| f.id == "text-hyperlinks"),
            "{ext}: our own links must not warn on reopen"
        );
    }
}
