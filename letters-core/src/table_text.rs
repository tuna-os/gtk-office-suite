// table_text.rs — the textual shape of a table in an editing buffer.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Letters' document model stores a table as a flat run of paragraphs
// tagged with (table, row, col). A GtkTextBuffer has no table widget, so
// the editor shows one as a pipe grid — the same shape CommonMark uses:
//
//     | Header 1 | Header 2 |
//     | --- | --- |
//     | Cell 1.1 | Cell 1.2 |
//
// Render and capture must agree on that shape exactly, or a table stops
// surviving a round trip through the buffer. Both sides therefore share
// this module instead of each spelling the separators out: it decides
// where the cell text sits within a line, and nothing else.
//
// GTK-free on purpose (the architecture rule): the buffer bridge maps
// these ranges onto tags, this module owns the text.

use std::ops::Range;

use crate::model::Run;

/// What surrounds cell text in a rendered row: `"| "`, `" | "`, `" |"`.
const OPEN: &str = "| ";
const SEP: &str = " | ";
const CLOSE: &str = " |";
const DELIMITER_CELL: &str = "---";

/// One rendered row: the exact line text, and where each cell's text
/// lives inside it, as character (not byte) ranges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowLayout {
    pub line: String,
    pub cells: Vec<Range<usize>>,
}

/// Lay out one row of cell texts as a pipe line.
pub fn layout_row(cells: &[String]) -> RowLayout {
    let mut line = String::new();
    let mut ranges = Vec::with_capacity(cells.len());
    for (i, cell) in cells.iter().enumerate() {
        line.push_str(if i == 0 { OPEN } else { SEP });
        let start = line.chars().count();
        line.push_str(cell);
        ranges.push(start..line.chars().count());
    }
    if cells.is_empty() {
        // A zero-column table cannot be addressed by the grid model; render
        // an empty line rather than a stray "| |" that would capture back
        // as a one-column table.
        return RowLayout { line, cells: ranges };
    }
    line.push_str(CLOSE);
    RowLayout { line, cells: ranges }
}

/// Build the runs for one rendered row, keeping each cell's own styles
/// and putting the separators in plain text.
///
/// Callers render runs, not strings, so this is the render-side twin of
/// [`parse_row`] + [`slice_runs`]: laying the separators out here (rather
/// than at each call site) is what keeps the two sides in step.
pub fn layout_row_runs(cells: &[Vec<Run>]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    // Appending through one merging push keeps the run list normalized
    // (invariant 2 in model.rs) without a separate pass.
    let push = |runs: &mut Vec<Run>, run: Run| match runs.last_mut() {
        Some(last) if last.style == run.style => last.text.push_str(&run.text),
        _ => {
            if !run.text.is_empty() {
                runs.push(run);
            }
        }
    };
    for (i, cell) in cells.iter().enumerate() {
        push(&mut runs, Run::plain(if i == 0 { OPEN } else { SEP }));
        for run in cell {
            push(&mut runs, run.clone());
        }
    }
    if !cells.is_empty() {
        push(&mut runs, Run::plain(CLOSE));
    }
    runs
}

/// The `| --- | --- |` line that separates a header row from the body.
pub fn delimiter_line(cols: usize) -> String {
    layout_row(&vec![DELIMITER_CELL.to_string(); cols]).line
}

/// Does this line separate a table's header from its body? Accepts the
/// CommonMark variants (`:---`, `---:`, `:---:`, any dash count) so a
/// table typed by hand or pasted from a file is recognized, not only one
/// this module rendered.
pub fn is_delimiter_line(line: &str) -> bool {
    let Some(cells) = split_row(line) else { return false };
    !cells.is_empty()
        && cells.iter().all(|(_, c)| {
            let c = c.trim();
            let core = c.trim_start_matches(':').trim_end_matches(':');
            !core.is_empty() && core.chars().all(|ch| ch == '-')
        })
}

/// Character ranges of each cell's *trimmed* text within a pipe row, or
/// None when the line is not a table row. Ranges address the text itself,
/// so the caller can slice styled runs out of the same line.
pub fn parse_row(line: &str) -> Option<Vec<Range<usize>>> {
    let cells = split_row(line)?;
    let mut ranges = Vec::with_capacity(cells.len());
    for (start, text) in cells {
        let leading = text.len() - text.trim_start().len();
        let trimmed_len = text.trim().chars().count();
        let lead_chars = text[..leading].chars().count();
        let cell_start = start + lead_chars;
        ranges.push(cell_start..cell_start + trimmed_len);
    }
    Some(ranges)
}

/// Split a pipe row into (char offset of the cell's raw text, raw text).
/// Returns None unless the line is delimited by leading and trailing
/// pipes, which is what this editor renders and what keeps an ordinary
/// paragraph that merely contains a "|" from being read as a table.
fn split_row(line: &str) -> Option<Vec<(usize, &str)>> {
    let trimmed = line.trim();
    if trimmed.len() != line.len() {
        // Leading/trailing whitespace would shift every range; the editor
        // never renders it, and accepting it here would silently mis-map
        // the styled runs that ride along with the text.
        return None;
    }
    if !trimmed.starts_with('|') || !trimmed.ends_with('|') || trimmed.chars().count() < 2 {
        return None;
    }
    let inner = &line[1..line.len() - 1];
    if inner.is_empty() {
        return None;
    }
    let mut cells = Vec::new();
    let mut cell_start_byte = 1; // past the opening pipe
    for part in inner.split('|') {
        let char_offset = line[..cell_start_byte].chars().count();
        cells.push((char_offset, part));
        cell_start_byte += part.len() + 1; // + the separating pipe
    }
    Some(cells)
}

/// Copy the part of `runs` covering `range`, preserving run styles.
/// Used to carry bold/link/code inside a cell back into the model when a
/// rendered row is captured from the buffer.
pub fn slice_runs(runs: &[Run], range: &Range<usize>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    let mut pos = 0usize;
    for run in runs {
        let len = run.char_len();
        let run_range = pos..pos + len;
        pos += len;
        let start = run_range.start.max(range.start);
        let end = run_range.end.min(range.end);
        if start >= end {
            continue;
        }
        let local = (start - run_range.start)..(end - run_range.start);
        let text: String = run.text.chars().skip(local.start).take(local.len()).collect();
        if text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.style == run.style => last.text.push_str(&text),
            _ => out.push(Run { text, style: run.style.clone() }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RunStyle;

    fn bold(text: &str) -> Run {
        Run { text: text.into(), style: RunStyle { bold: true, ..Default::default() } }
    }

    #[test]
    fn layout_places_cells_where_parse_finds_them() {
        // The round-trip property this module exists for: what render
        // wrote is exactly what capture reads back, ranges included.
        let cells = vec!["Header 1".to_string(), "Header 2".to_string()];
        let layout = layout_row(&cells);
        assert_eq!(layout.line, "| Header 1 | Header 2 |");
        assert_eq!(parse_row(&layout.line).unwrap(), layout.cells);
    }

    #[test]
    fn layout_round_trips_unicode_cells() {
        // Char ranges, not byte ranges: a CJK or emoji cell must still
        // address the same text after a parse.
        let cells = vec!["日本語".to_string(), "e👍".to_string()];
        let layout = layout_row(&cells);
        let parsed = parse_row(&layout.line).unwrap();
        assert_eq!(parsed, layout.cells);
        let chars: Vec<char> = layout.line.chars().collect();
        for (range, expected) in parsed.iter().zip(cells.iter()) {
            let got: String = chars[range.clone()].iter().collect();
            assert_eq!(&got, expected);
        }
    }

    #[test]
    fn empty_cells_keep_their_positions() {
        let layout = layout_row(&["".to_string(), "b".to_string()]);
        assert_eq!(layout.line, "|  | b |");
        assert_eq!(parse_row(&layout.line).unwrap().len(), 2);
    }

    #[test]
    fn delimiter_line_matches_the_column_count() {
        assert_eq!(delimiter_line(3), "| --- | --- | --- |");
        assert!(is_delimiter_line(&delimiter_line(3)));
        assert_eq!(parse_row(&delimiter_line(3)).unwrap().len(), 3);
    }

    #[test]
    fn commonmark_delimiter_variants_are_recognized() {
        assert!(is_delimiter_line("| :--- | ---: | :---: |"));
        assert!(is_delimiter_line("| - | ---------- |"));
        assert!(!is_delimiter_line("| --- | text |"));
        assert!(!is_delimiter_line("| | |"));
    }

    #[test]
    fn prose_containing_a_pipe_is_not_a_row() {
        // Otherwise an ordinary sentence would be swallowed into a table
        // the user never created.
        assert!(parse_row("a | b").is_none());
        assert!(parse_row("| unterminated").is_none());
        assert!(parse_row("  | indented |").is_none());
        assert!(parse_row("|").is_none());
        assert!(parse_row("||").is_none());
    }

    #[test]
    fn slice_runs_keeps_styles_within_a_cell() {
        // "| a | B |" with the second cell bold.
        let runs = vec![Run::plain("| a | "), bold("B"), Run::plain(" |")];
        let ranges = parse_row("| a | B |").unwrap();
        assert_eq!(slice_runs(&runs, &ranges[0]), vec![Run::plain("a")]);
        assert_eq!(slice_runs(&runs, &ranges[1]), vec![bold("B")]);
    }

    #[test]
    fn styled_cells_survive_a_render_capture_round_trip() {
        // The property the editor depends on: what layout_row_runs writes
        // into the buffer, parse_row + slice_runs read back as the same
        // cells, styles included.
        let cells = vec![
            vec![Run::plain("plain")],
            vec![Run::plain("a"), bold("B")],
            vec![],
        ];
        let runs = layout_row_runs(&cells);
        let line: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(line, "| plain | aB |  |");
        let ranges = parse_row(&line).unwrap();
        assert_eq!(ranges.len(), 3);
        for (range, expected) in ranges.iter().zip(cells.iter()) {
            assert_eq!(&slice_runs(&runs, range), expected);
        }
    }

    #[test]
    fn layout_row_runs_keeps_the_run_list_normalized() {
        let cells = vec![vec![Run::plain("a"), Run::plain("b")], vec![]];
        let runs = layout_row_runs(&cells);
        assert!(runs.iter().all(|r| !r.text.is_empty()), "no empty runs: {runs:?}");
        assert!(
            runs.windows(2).all(|w| w[0].style != w[1].style),
            "adjacent equal styles must be merged: {runs:?}"
        );
    }

    #[test]
    fn slice_runs_merges_adjacent_equal_styles_and_drops_empties() {
        let runs = vec![Run::plain("ab"), Run::plain("cd")];
        assert_eq!(slice_runs(&runs, &(1..3)), vec![Run::plain("bc")]);
        assert!(slice_runs(&runs, &(2..2)).is_empty());
    }

    #[test]
    fn slice_runs_spanning_a_style_boundary_keeps_both_parts() {
        let runs = vec![Run::plain("ab"), bold("cd")];
        assert_eq!(slice_runs(&runs, &(1..3)), vec![Run::plain("b"), bold("c")]);
    }
}
