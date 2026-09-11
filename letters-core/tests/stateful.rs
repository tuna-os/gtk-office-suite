// stateful.rs — seeded command sequences against StructuredEditor (#442).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The Letters half of the same idea as tables-core/tests/stateful.rs: a
// document is a structure with rules — a cursor inside the text, runs
// that are normalized, a table whose cells form a complete rectangle —
// and those rules are easy to keep while doing one thing at a time and
// hard to keep while doing two hundred things in an order nobody chose.
//
// The generator and the minimizer are deliberately duplicated rather
// than shared: a test-only crate to hold sixty lines of PRNG would tie
// two independent crates' test suites together, and the version each one
// needs is not the same (this one generates text edits, that one
// generates workbook commands). The reasoning behind the hand-rolled
// PRNG is written out there and holds here too: a seed in a failure
// message has to reproduce the run for anyone, forever.

use letters_core::model::{Document, ListKind, StylePatch};
use letters_core::structured::StructuredEditor;

const STEPS: usize = 200;

/// Seeds that run on every pull request. A campaign that finds a failing
/// seed adds it here, so the regression is permanent rather than
/// something the nightly might happen to hit again.
const FIXED_SEEDS: &[u64] = &[
    1, 2, 3, 7, 11, 20260907, 20260911, 0xDEADBEEF,
    0x5EED, 42, 99, 1337, 8675309, 0xC0FFEE, 314159, 271828,
];

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0x1234_5678))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0 | 1;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() % bound as u64) as usize
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Command {
    InsertText(String),
    SetCursor(usize),
    Select { start: usize, end: usize },
    DeleteSelection,
    StyleSelection(&'static str),
    ToggleList(ListKind),
    IndentList,
    OutdentList,
    RestartNumbering,
    TogglePageBreak,
    InsertTable { rows: u32, cols: u32 },
    InsertRow(bool),
    InsertCol(bool),
    DeleteRow,
    DeleteCol,
    MoveTableCell(bool),
}

fn generate(rng: &mut Rng, editor: &StructuredEditor) -> Command {
    let len = editor.document().char_len();
    match rng.below(22) {
        0..=4 => Command::InsertText(match rng.below(5) {
            // Newlines and tabs on purpose: paragraph splitting is where
            // a cursor or a table cell mapping goes wrong.
            0 => "\n".to_string(),
            1 => "\t".to_string(),
            2 => format!("word{} ", rng.below(100)),
            3 => "ünïcödé ".to_string(),
            _ => "a".to_string(),
        }),
        5..=6 => Command::SetCursor(rng.below(len + 1)),
        7..=8 => {
            let a = rng.below(len + 1);
            let b = rng.below(len + 1);
            Command::Select {
                start: a.min(b),
                end: a.max(b),
            }
        }
        9 => Command::DeleteSelection,
        10 => Command::StyleSelection(match rng.below(4) {
            0 => "bold",
            1 => "italic",
            2 => "underline",
            _ => "code",
        }),
        11 => Command::ToggleList(match rng.below(2) {
            0 => ListKind::Bullet,
            _ => ListKind::Numbered,
        }),
        12 => Command::IndentList,
        13 => Command::OutdentList,
        14 => Command::RestartNumbering,
        15 => Command::TogglePageBreak,
        16 => Command::InsertTable {
            rows: 1 + rng.below(4) as u32,
            cols: 1 + rng.below(4) as u32,
        },
        17 => Command::InsertRow(rng.below(2) == 0),
        18 => Command::InsertCol(rng.below(2) == 0),
        19 => Command::DeleteRow,
        20 => Command::DeleteCol,
        _ => Command::MoveTableCell(rng.below(2) == 0),
    }
}

fn apply(editor: &mut StructuredEditor, command: &Command) {
    match command {
        Command::InsertText(text) => editor.insert_text(text),
        Command::SetCursor(offset) => editor.set_cursor(*offset),
        Command::Select { start, end } => editor.select(*start, *end),
        Command::DeleteSelection => {
            editor.delete_selection();
        }
        Command::StyleSelection(which) => {
            let patch = match *which {
                "bold" => StylePatch::set_bold(true),
                "italic" => StylePatch::set_italic(true),
                "underline" => StylePatch::set_underline(true),
                _ => StylePatch::set_code(true),
            };
            editor.apply_selection_style(&patch);
        }
        Command::ToggleList(kind) => {
            editor.toggle_list_at_cursor(*kind);
        }
        Command::IndentList => {
            editor.indent_list_at_cursor();
        }
        Command::OutdentList => {
            editor.outdent_list_at_cursor();
        }
        Command::RestartNumbering => {
            editor.restart_numbering_at_cursor();
        }
        Command::TogglePageBreak => {
            editor.toggle_page_break_at_cursor();
        }
        Command::InsertTable { rows, cols } => {
            editor.insert_table(*rows, *cols);
        }
        Command::InsertRow(below) => {
            editor.insert_row_at_cursor(*below);
        }
        Command::InsertCol(after) => {
            editor.insert_col_at_cursor(*after);
        }
        Command::DeleteRow => {
            editor.delete_row_at_cursor();
        }
        Command::DeleteCol => {
            editor.delete_col_at_cursor();
        }
        Command::MoveTableCell(backwards) => {
            if let Some(cell) = editor.cursor_cell() {
                editor.move_table_cell(cell.table, cell.row, cell.col, *backwards);
            }
        }
    }
}

// ── invariants ───────────────────────────────────────────────────────

fn check_invariants(editor: &StructuredEditor) -> Result<(), String> {
    let document = editor.document();
    let len = document.char_len();

    if document.paragraphs.is_empty() {
        return Err("the document has no paragraphs at all".into());
    }
    if editor.cursor() > len {
        return Err(format!(
            "cursor {} is past the end of {len} character(s)",
            editor.cursor()
        ));
    }
    if let Some((start, end)) = editor.selection() {
        if start > end {
            return Err(format!("selection is inverted: {start}..{end}"));
        }
        if end > len {
            return Err(format!(
                "selection {start}..{end} runs past the end of {len} character(s)"
            ));
        }
    }

    check_runs(document)?;
    check_footnotes(document)?;
    check_tables(document)
}

/// `Paragraph::normalize` promises no empty runs and no two adjacent
/// runs sharing a style. Every mutation is supposed to leave the
/// paragraph normalized; a document that drifts out of that shape
/// renders and exports differently for reasons nothing in the model
/// explains.
fn check_runs(document: &Document) -> Result<(), String> {
    for (index, paragraph) in document.paragraphs.iter().enumerate() {
        for run in &paragraph.runs {
            if run.text.is_empty() && run.style.image.is_none() && run.style.footnote.is_none() {
                return Err(format!("paragraph {index} keeps an empty run"));
            }
        }
        for pair in paragraph.runs.windows(2) {
            if pair[0].style.footnote.is_some() || pair[1].style.footnote.is_some() {
                continue;
            }
            if pair[0].style == pair[1].style {
                return Err(format!(
                    "paragraph {index} has adjacent runs with the same style"
                ));
            }
        }
    }
    Ok(())
}

fn check_footnotes(document: &Document) -> Result<(), String> {
    for (index, paragraph) in document.paragraphs.iter().enumerate() {
        for run in &paragraph.runs {
            if let Some(note) = run.style.footnote {
                if note >= document.footnotes.len() {
                    return Err(format!(
                        "paragraph {index} references footnote {note}, but there are {}",
                        document.footnotes.len()
                    ));
                }
            }
        }
    }
    Ok(())
}

/// A table is a rectangle of cells held in consecutive paragraphs, in
/// row-major order — that is what `Document::reflow_table` maintains and
/// what every renderer, the DOCX writer and the AT-SPI bridge assume. A
/// table with a hole in it, a duplicate cell, or cells interleaved with
/// body text is a corrupt document, not a layout quirk.
fn check_tables(document: &Document) -> Result<(), String> {
    use std::collections::BTreeMap;

    let mut tables: BTreeMap<u32, Vec<(usize, u32, u32)>> = BTreeMap::new();
    for (index, paragraph) in document.paragraphs.iter().enumerate() {
        if let Some(cell) = paragraph.style.table_cell {
            tables
                .entry(cell.table)
                .or_default()
                .push((index, cell.row, cell.col));
        }
    }

    for (table, cells) in tables {
        let rows = cells.iter().map(|c| c.1).max().unwrap_or(0) + 1;
        let cols = cells.iter().map(|c| c.2).max().unwrap_or(0) + 1;

        if cells.len() != (rows * cols) as usize {
            return Err(format!(
                "table {table} spans {rows}x{cols} but has {} cell(s)",
                cells.len()
            ));
        }

        let mut seen = std::collections::BTreeSet::new();
        for &(_, row, col) in &cells {
            if !seen.insert((row, col)) {
                return Err(format!("table {table} has two cells at ({row}, {col})"));
            }
        }

        // Contiguous: the paragraphs holding the cells are consecutive.
        let first = cells[0].0;
        for (offset, &(index, _, _)) in cells.iter().enumerate() {
            if index != first + offset {
                return Err(format!(
                    "table {table}'s cells are not contiguous: expected paragraph {} \
                     at position {offset}, found {index}",
                    first + offset
                ));
            }
        }

        // Row-major: reading the paragraphs in order walks the grid.
        for (offset, &(_, row, col)) in cells.iter().enumerate() {
            let expected_row = offset as u32 / cols;
            let expected_col = offset as u32 % cols;
            if (row, col) != (expected_row, expected_col) {
                return Err(format!(
                    "table {table} is out of row-major order at position {offset}: \
                     expected ({expected_row}, {expected_col}), found ({row}, {col})"
                ));
            }
        }
    }
    Ok(())
}

// ── running and minimizing ───────────────────────────────────────────

fn fresh() -> StructuredEditor {
    StructuredEditor::new(Document::from_plain_text("intro"))
}

fn replay(commands: &[Command]) -> Result<(), (usize, String)> {
    let mut editor = fresh();
    for (index, command) in commands.iter().enumerate() {
        apply(&mut editor, command);
        if let Err(reason) = check_invariants(&editor) {
            return Err((index, reason));
        }
    }
    Ok(())
}

fn minimize(commands: &[Command], failing_at: usize) -> Vec<Command> {
    let mut shortest = commands[..=failing_at].to_vec();
    let mut index = 0;
    while index < shortest.len() {
        let mut candidate = shortest.clone();
        candidate.remove(index);
        if replay(&candidate).is_err() {
            shortest = candidate;
        } else {
            index += 1;
        }
    }
    shortest
}

fn run_seed(seed: u64, steps: usize) -> Result<(), String> {
    let mut rng = Rng::new(seed);
    let mut editor = fresh();
    let mut trace = Vec::with_capacity(steps);

    for step in 0..steps {
        let command = generate(&mut rng, &editor);
        trace.push(command.clone());
        apply(&mut editor, &command);
        if let Err(reason) = check_invariants(&editor) {
            let minimal = minimize(&trace, step);
            // Report the *minimized* trace's own reason. Dropping
            // commands can leave a shorter sequence that fails a
            // different way, and printing the original run's message
            // beside it sends the reader looking for a symptom the
            // listed commands do not produce.
            let reason = match replay(&minimal) {
                Err((_, minimal_reason)) => minimal_reason,
                Ok(()) => reason,
            };
            return Err(format!(
                "seed {seed} failed at step {step}: {reason}\n\
                 minimized to {} command(s):\n{}",
                minimal.len(),
                minimal
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("  {i}: {c:?}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ));
        }
    }
    Ok(())
}

// ── tests ────────────────────────────────────────────────────────────

#[test]
fn fixed_seeds_hold_the_invariants() {
    let mut failures = Vec::new();
    for &seed in FIXED_SEEDS {
        if let Err(report) = run_seed(seed, STEPS) {
            failures.push(report);
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} seed(s) failed:\n\n{}",
        failures.len(),
        FIXED_SEEDS.len(),
        failures.join("\n\n")
    );
}

#[test]
fn a_seed_replays_identically() {
    let commands = |seed: u64| {
        let mut rng = Rng::new(seed);
        let editor = fresh();
        (0..16).map(|_| generate(&mut rng, &editor)).collect::<Vec<_>>()
    };
    assert_eq!(commands(20260911), commands(20260911));
    assert_ne!(commands(1), commands(2));
}

#[test]
fn the_table_invariant_rejects_a_broken_table() {
    // The invariants are the whole value of this file, so at least one
    // has to be shown catching something — otherwise a check that can
    // never fail reads exactly like one that never fires.
    let mut editor = fresh();
    editor.insert_table(2, 2);
    assert!(check_invariants(&editor).is_ok(), "a fresh table is well formed");

    let document = editor.document_mut();
    let broken = document
        .paragraphs
        .iter()
        .position(|p| p.style.table_cell.is_some())
        .expect("the table has cells");
    document.paragraphs.remove(broken);

    let error = check_invariants(&editor).expect_err("a table with a hole must be rejected");
    assert!(
        error.contains("cell(s)") || error.contains("row-major") || error.contains("contiguous"),
        "unexpected rejection: {error}"
    );
}

/// The two-command reproduction of #532, now a regression guard.
#[test]
fn enter_inside_a_table_cell_does_not_duplicate_it() {
    let mut editor = fresh();
    editor.insert_table(2, 3);
    assert!(check_invariants(&editor).is_ok(), "a fresh table is well formed");

    editor.insert_text("\n");

    let cells: Vec<_> = editor
        .document()
        .paragraphs
        .iter()
        .filter_map(|p| p.style.table_cell)
        .collect();
    assert_eq!(
        cells.len(),
        6,
        "a 2x3 table must still have six cells after Enter, found: {cells:?}"
    );
    check_invariants(&editor).expect("the table survives a newline");
}

#[test]
fn enter_in_a_cell_moves_to_the_next_cell_and_stops_at_the_last() {
    let mut editor = fresh();
    let table = editor.insert_table(2, 2);
    let cell = editor.cursor_cell().expect("the caret starts in the first cell");
    assert_eq!((cell.table, cell.row, cell.col), (table, 0, 0));

    for expected in [(0, 1), (1, 0), (1, 1)] {
        editor.insert_text("\n");
        let cell = editor.cursor_cell().expect("still in the table");
        assert_eq!((cell.row, cell.col), expected, "Enter walks the grid row-major");
    }

    // The last cell is the end of the walk, not a place to grow a table
    // from: Enter there leaves the caret alone rather than inventing a row.
    editor.insert_text("\n");
    let cell = editor.cursor_cell().expect("still in the table");
    assert_eq!((cell.row, cell.col), (1, 1));
    check_invariants(&editor).expect("nothing was inserted");
}

#[test]
fn a_multi_line_paste_into_a_cell_becomes_one_line() {
    let mut editor = fresh();
    editor.insert_table(1, 2);
    editor.insert_text("first\nsecond");

    let cells: Vec<String> = editor
        .document()
        .paragraphs
        .iter()
        .filter(|p| p.style.table_cell.is_some())
        .map(|p| p.text())
        .collect();
    assert_eq!(cells, vec!["first second".to_string(), String::new()]);
    check_invariants(&editor).expect("the table survives a pasted newline");
}

#[test]
fn a_newline_outside_a_table_still_splits_the_paragraph() {
    // The guard is scoped to cells; ordinary text keeps working the way
    // every other test in the workspace expects.
    let mut editor = fresh();
    editor.set_cursor(5);
    editor.insert_text("\nsecond");
    assert_eq!(editor.document().paragraphs.len(), 2);
    assert_eq!(editor.document().to_plain_text(), "intro\nsecond");
}

#[test]
fn enter_over_a_selection_that_leaves_the_table_keeps_the_cell_whole() {
    // Selecting forwards out of a table puts the caret past the table
    // while the *insertion* point is still in a cell. Keying the newline
    // guard off the caret let this split the cell paragraph the
    // replacement was written into, which is #532 by another route.
    let mut editor = fresh();
    editor.insert_table(2, 2);
    let cell = editor.cursor_cell().expect("the caret starts in a cell");
    let start = editor.cursor();
    editor.select(start, editor.document().char_len());
    editor.insert_text("\n");

    check_invariants(&editor).expect("the table survives the replacement");
    let caret = editor.cursor_cell().expect("the caret stayed in the table");
    assert_ne!(
        (caret.row, caret.col),
        (cell.row, cell.col),
        "Enter still walks to the next cell"
    );
}

#[test]
fn a_selection_across_a_table_empties_its_cells_instead_of_removing_them() {
    // Removing a cell paragraph leaves the grid with a hole that
    // `reflow_table` cannot close, so a selection dragged over a table
    // clears the text and leaves the structure standing.
    let mut editor = fresh();
    editor.insert_table(2, 2);
    editor.insert_text("a");
    editor.select(0, editor.document().char_len());
    editor.delete_selection();

    check_invariants(&editor).expect("the table is still a table");
    let cells = editor
        .document()
        .paragraphs
        .iter()
        .filter(|p| p.style.table_cell.is_some())
        .count();
    assert_eq!(cells, 4, "every cell is still there, emptied");
}

#[test]
fn deleting_the_last_row_of_a_table_leaves_an_editable_document() {
    // A document that is nothing but one table has no paragraphs left
    // once its rows go, and locating any offset in an empty document
    // used to underflow before anything could clamp the caret.
    let mut editor = fresh();
    editor.insert_table(1, 1);
    // Drop the paragraph `insert_table` left beside the table, so the
    // table really is the whole document.
    let table_only: Vec<_> = editor
        .document()
        .paragraphs
        .iter()
        .filter(|p| p.style.table_cell.is_some())
        .cloned()
        .collect();
    let mut document = Document::from_plain_text("");
    document.paragraphs = table_only;
    let mut editor = StructuredEditor::new(document);
    editor.set_cursor(0);
    assert!(editor.delete_row_at_cursor());

    editor.set_cursor(editor.document().char_len());
    editor.insert_text("after");
    assert_eq!(editor.document().to_plain_text(), "after");
    check_invariants(&editor).expect("the document is still editable");
}

#[test]
fn a_table_inserted_from_inside_a_table_lands_after_it() {
    // There are no nested tables in this model, and splicing the new
    // cells in beside the caret's paragraph dropped them into the middle
    // of the enclosing table's run — two tables interleaved, which
    // `reflow_table` cannot sort out.
    let mut editor = fresh();
    let first = editor.insert_table(2, 2);
    let second = editor.insert_table(2, 2);
    assert_ne!(first, second);

    check_invariants(&editor).expect("both tables are readable");
    let (_, first_last) = editor
        .document()
        .table_paragraph_range(first)
        .expect("the first table is still there");
    let (second_first, _) = editor
        .document()
        .table_paragraph_range(second)
        .expect("the second table exists");
    assert!(
        first_last < second_first,
        "the tables do not interleave: {first_last} < {second_first}"
    );
}

/// The nightly campaign's entry point. Ignored by default so the PR lane
/// stays fast; run with
/// `cargo test -p letters-core --test stateful -- --ignored`.
#[test]
#[ignore = "campaign-scale; run from the nightly stress workflow"]
fn seed_campaign() {
    let base: u64 = std::env::var("STATEFUL_SEED_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000_000);
    let count: u64 = std::env::var("STATEFUL_SEED_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);

    let mut failures = Vec::new();
    for seed in base..base + count {
        if let Err(report) = run_seed(seed, STEPS) {
            failures.push(report);
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {count} seed(s) failed — add each failing seed to FIXED_SEEDS \
         once its bug is fixed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
