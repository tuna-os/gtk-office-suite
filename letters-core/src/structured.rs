// structured.rs — the GTK-free editing controller for Letters.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Views bind to this small command surface instead of reaching into the
// document's paragraph vectors. It is also the seam used by keyboard journey
// tests and format adapters.

use crate::model::{Document, ListKind, ParagraphLayout, StylePatch, TableCell};

#[derive(Clone, Debug, PartialEq)]
pub struct StructuredEditor {
    document: Document,
    cursor: usize,
    selection: Option<(usize, usize)>,
    table_cell: Option<TableCell>,
}

impl StructuredEditor {
    pub fn new(document: Document) -> Self {
        Self { document, cursor: 0, selection: None, table_cell: None }
    }

    pub fn document(&self) -> &Document { &self.document }
    pub fn document_mut(&mut self) -> &mut Document { &mut self.document }
    pub fn cursor(&self) -> usize { self.cursor }
    pub fn selection(&self) -> Option<(usize, usize)> { self.selection }
    pub fn table_cell(&self) -> Option<TableCell> { self.table_cell }

    pub fn set_cursor(&mut self, offset: usize) {
        self.cursor = offset.min(self.document.char_len());
        self.selection = None;
    }

    pub fn select(&mut self, start: usize, end: usize) {
        let start = start.min(self.document.char_len());
        let end = end.min(self.document.char_len());
        self.selection = Some((start.min(end), start.max(end)));
        self.cursor = end;
    }

    pub fn insert_text(&mut self, text: &str) {
        if let Some((start, end)) = self.selection.take() {
            // Typing over a selection keeps the replaced text's style, as
            // word processors do. The style must be captured before the
            // delete: emptying a paragraph leaves no run to inherit from.
            let style = self.document.style_at(start);
            self.document.delete_range(start, end);
            self.document.insert_text(start, text);
            self.cursor = start + text.chars().count();
            self.document.set_run_style(start, self.cursor, &style);
        } else {
            self.document.insert_text(self.cursor, text);
            self.cursor += text.chars().count();
        }
    }

    pub fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection.take() else { return false };
        self.document.delete_range(start, end);
        self.cursor = start;
        true
    }

    pub fn apply_selection_style(&mut self, patch: &StylePatch) -> bool {
        let Some((start, end)) = self.selection else { return false };
        self.document.apply_run_style(start, end, patch);
        true
    }

    pub fn set_paragraph_layout(&mut self, paragraph: usize, layout: ParagraphLayout) {
        self.document.set_paragraph_layout(paragraph, layout);
    }

    pub fn set_list_item(&mut self, paragraph: usize, kind: ListKind, level: u8, start: Option<u32>) {
        self.document.set_list_item(paragraph, kind, level, start);
    }

    pub fn insert_table_rows(&mut self, table: u32, at: u32, count: u32) -> bool {
        self.document.insert_table_rows(table, at, count)
    }

    pub fn insert_table_cols(&mut self, table: u32, at: u32, count: u32) -> bool {
        self.document.insert_table_cols(table, at, count)
    }

    pub fn delete_table_rows(&mut self, table: u32, at: u32, count: u32) -> bool {
        self.document.delete_table_rows(table, at, count)
    }

    pub fn delete_table_cols(&mut self, table: u32, at: u32, count: u32) -> bool {
        self.document.delete_table_cols(table, at, count)
    }

    /// Insert an empty `rows` × `cols` table at the cursor and return its
    /// id, leaving the cursor in its first cell.
    ///
    /// The cells start empty. An earlier version filled them with "Cell
    /// 1.1"-style placeholders, which a user then had to delete from every
    /// cell of every table they inserted.
    pub fn insert_table(&mut self, rows: u32, cols: u32) -> u32 {
        // The table goes on the line *after* the paragraph the caret is in,
        // so the text you were writing stays above it — except in an empty
        // paragraph, where it takes that line and leaves the empty one
        // below to keep typing in. Splitting a paragraph mid-sentence
        // around a table is a separate behavior and not what any menu item
        // here promises.
        let cursor_para = self.document.paragraph_at(self.cursor);
        let at = if self.document.paragraphs.get(cursor_para).is_some_and(|p| p.text().is_empty()) {
            cursor_para
        } else {
            cursor_para + 1
        };
        let table = self.document.insert_table_at(at, rows, cols);
        if rows > 0 && cols > 0 {
            self.table_cell = Some(TableCell { table, row: 0, col: 0 });
            self.cursor = self.document.paragraph_offset(at);
            self.selection = None;
        }
        table
    }

    /// The cell the cursor is in, if any. GUI commands ask this rather
    /// than assuming which table they are editing.
    pub fn cursor_cell(&self) -> Option<TableCell> {
        self.document.table_cell_at(self.cursor)
    }

    // ── Cursor-relative table commands ──────────────────────────────
    // Each is a no-op returning false when the cursor is not in a table,
    // so a menu item invoked with the caret in ordinary text cannot
    // scramble some unrelated table elsewhere in the document.

    pub fn insert_row_at_cursor(&mut self, below: bool) -> bool {
        let Some(cell) = self.cursor_cell() else { return false };
        self.document.insert_table_rows(cell.table, if below { cell.row + 1 } else { cell.row }, 1)
    }

    pub fn insert_col_at_cursor(&mut self, after: bool) -> bool {
        let Some(cell) = self.cursor_cell() else { return false };
        self.document.insert_table_cols(cell.table, if after { cell.col + 1 } else { cell.col }, 1)
    }

    pub fn delete_row_at_cursor(&mut self) -> bool {
        let Some(cell) = self.cursor_cell() else { return false };
        self.document.delete_table_rows(cell.table, cell.row, 1)
    }

    pub fn delete_col_at_cursor(&mut self) -> bool {
        let Some(cell) = self.cursor_cell() else { return false };
        self.document.delete_table_cols(cell.table, cell.col, 1)
    }

    // ── Cursor-relative list and page-break commands ────────────────
    // The paragraph-indexed versions below stay for callers that know
    // which paragraph they mean (tests, format adapters). Everything
    // driven by a menu item or a shortcut goes through these, because the
    // only paragraph such a command can mean is the one the caret is in —
    // the GUI used to pass a hardcoded 0 and edit the first paragraph of
    // the document instead.

    /// Paragraph the caret is in.
    pub fn cursor_paragraph(&self) -> usize {
        self.document.paragraph_at(self.cursor)
    }

    /// Turn the cursor's paragraph into a list item of `kind`, or back into
    /// body text when it already is one — the toggle a "Bullet List" button
    /// performs. Returns the kind the paragraph ended up with.
    pub fn toggle_list_at_cursor(&mut self, kind: ListKind) -> ListKind {
        let idx = self.cursor_paragraph();
        let current = self.document.paragraphs.get(idx).map(|p| p.style.list).unwrap_or(ListKind::None);
        let next = if current == kind { ListKind::None } else { kind };
        let level = self.document.paragraphs.get(idx).map(|p| p.style.list_level).unwrap_or(0);
        self.document.set_list_item(idx, next, if next == ListKind::None { 0 } else { level }, None);
        next
    }

    pub fn indent_list_at_cursor(&mut self) -> bool {
        let idx = self.cursor_paragraph();
        let before = self.document.paragraphs.get(idx).map(|p| p.style.list_level);
        self.indent_list_item(idx);
        before != self.document.paragraphs.get(idx).map(|p| p.style.list_level)
    }

    pub fn outdent_list_at_cursor(&mut self) -> bool {
        let idx = self.cursor_paragraph();
        let before = self.document.paragraphs.get(idx).map(|p| p.style.list_level);
        self.outdent_list_item(idx);
        before != self.document.paragraphs.get(idx).map(|p| p.style.list_level)
    }

    /// Restart numbering at 1 on the cursor's paragraph. Only meaningful
    /// inside a numbered list, so it reports whether it applied.
    pub fn restart_numbering_at_cursor(&mut self) -> bool {
        let idx = self.cursor_paragraph();
        let Some(para) = self.document.paragraphs.get(idx) else { return false };
        if para.style.list != ListKind::Numbered {
            return false;
        }
        let level = para.style.list_level;
        self.document.set_list_item(idx, ListKind::Numbered, level, Some(1));
        true
    }

    /// Toggle "start this paragraph on a new page" on the cursor's
    /// paragraph, and report the state it ended in.
    pub fn toggle_page_break_at_cursor(&mut self) -> bool {
        let idx = self.cursor_paragraph();
        let Some(para) = self.document.paragraphs.get_mut(idx) else { return false };
        para.style.page_break_before = !para.style.page_break_before;
        para.style.page_break_before
    }

    pub fn indent_list_item(&mut self, paragraph: usize) {
        if let Some(p) = self.document.paragraphs.get_mut(paragraph) {
            if p.style.list != ListKind::None {
                p.style.list_level = (p.style.list_level + 1).min(8);
            }
        }
    }

    pub fn outdent_list_item(&mut self, paragraph: usize) {
        if let Some(p) = self.document.paragraphs.get_mut(paragraph) {
            if p.style.list != ListKind::None {
                p.style.list_level = p.style.list_level.saturating_sub(1);
            }
        }
    }

    pub fn insert_page_break(&mut self, paragraph: usize) {
        if let Some(p) = self.document.paragraphs.get_mut(paragraph) {
            p.style.page_break_before = true;
        }
    }

    /// Move the keyboard focus through a table in row-major order. At either
    /// edge the focus stays put and returns false, matching Tab/Shift-Tab UI
    /// behavior when there is no adjacent cell.
    pub fn move_table_cell(&mut self, table: u32, row: u32, col: u32, backwards: bool) -> bool {
        let Some(cell) = self.document.next_table_cell(table, row, col, backwards) else { return false };
        self.table_cell = Some(cell);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Paragraph, ParaStyle, Run};

    #[test]
    fn replacement_and_selection_style_share_one_cursor_surface() {
        let mut editor = StructuredEditor::new(Document::from_plain_text("hello"));
        editor.select(0, 5);
        assert!(editor.apply_selection_style(&StylePatch::set_bold(true)));
        editor.insert_text("world");
        assert_eq!(editor.document().to_plain_text(), "world");
        assert_eq!(editor.cursor(), 5);
        assert!(editor.document().style_at(0).bold);
    }

    /// A document of three paragraphs with the cursor in the middle one.
    fn three_paragraphs() -> StructuredEditor {
        let doc = Document::from_plain_text("before\nmiddle\nafter");
        StructuredEditor::new(doc)
    }

    #[test]
    fn insert_table_lands_at_the_cursor_not_at_the_end() {
        let mut editor = three_paragraphs();
        editor.set_cursor(editor.document().paragraph_offset(1));
        let table = editor.insert_table(2, 2);

        let kinds: Vec<Option<u32>> = editor.document().paragraphs.iter()
            .map(|p| p.style.table_cell.map(|c| c.table)).collect();
        assert_eq!(kinds, vec![None, None, Some(table), Some(table), Some(table), Some(table), None],
                   "the table belongs on the line after 'middle', not at the end");
        assert_eq!(editor.document().table_dimensions(table), Some((2, 2)));
    }

    #[test]
    fn insert_table_in_an_empty_paragraph_takes_that_line() {
        // A new document is one empty paragraph; a table inserted there
        // should not leave a blank first line above itself.
        let mut editor = StructuredEditor::new(Document::new());
        let table = editor.insert_table(2, 2);
        assert_eq!(editor.document().paragraphs[0].style.table_cell.map(|c| c.table), Some(table));
    }

    #[test]
    fn inserted_cells_start_empty() {
        // Placeholder text ("Cell 1.1") would have to be deleted from every
        // cell of every inserted table before the user could type.
        let mut editor = three_paragraphs();
        let table = editor.insert_table(2, 2);
        assert!(editor.document().paragraphs.iter()
            .filter(|p| p.style.table_cell.is_some_and(|c| c.table == table))
            .all(|p| p.text().is_empty()));
    }

    #[test]
    fn insert_table_leaves_the_cursor_in_the_first_cell() {
        let mut editor = three_paragraphs();
        editor.set_cursor(editor.document().paragraph_offset(1));
        let table = editor.insert_table(2, 2);
        assert_eq!(editor.cursor_cell(), Some(TableCell { table, row: 0, col: 0 }));
    }

    #[test]
    fn each_inserted_table_gets_its_own_id() {
        let mut editor = three_paragraphs();
        let first = editor.insert_table(2, 2);
        editor.set_cursor(editor.document().char_len());
        let second = editor.insert_table(2, 2);
        assert_ne!(first, second);
        assert_eq!(editor.document().table_dimensions(first), Some((2, 2)));
        assert_eq!(editor.document().table_dimensions(second), Some((2, 2)));
    }

    #[test]
    fn row_and_column_commands_follow_the_cursor() {
        let mut editor = three_paragraphs();
        let table = editor.insert_table(2, 2);
        // Cursor in the last cell (row 1, col 1).
        let last = editor.document().paragraphs.iter()
            .position(|p| p.style.table_cell == Some(TableCell { table, row: 1, col: 1 })).unwrap();
        editor.set_cursor(editor.document().paragraph_offset(last));

        assert!(editor.insert_row_at_cursor(true));
        assert_eq!(editor.document().table_dimensions(table), Some((3, 2)));
        assert!(editor.insert_col_at_cursor(false));
        assert_eq!(editor.document().table_dimensions(table), Some((3, 3)));
        assert!(editor.delete_row_at_cursor());
        assert!(editor.delete_col_at_cursor());
        assert_eq!(editor.document().table_dimensions(table), Some((2, 2)));
    }

    #[test]
    fn table_commands_outside_a_table_do_nothing() {
        // Not a silent no-op by accident: the menu items are always
        // sensitive, and the old code answered them by rewriting table 1
        // wherever it happened to be.
        let mut editor = three_paragraphs();
        let table = editor.insert_table(2, 2);
        editor.set_cursor(editor.document().char_len());
        let before = editor.document().clone();

        assert!(!editor.insert_row_at_cursor(true));
        assert!(!editor.insert_col_at_cursor(true));
        assert!(!editor.delete_row_at_cursor());
        assert!(!editor.delete_col_at_cursor());
        assert_eq!(editor.document(), &before);
        assert_eq!(editor.document().table_dimensions(table), Some((2, 2)));
    }

    #[test]
    fn table_cells_stay_contiguous_and_row_major_after_edits() {
        // Adjacency is the table: a reader of the flat paragraph list
        // recovers the grid from consecutive cells sharing an id.
        let mut editor = three_paragraphs();
        editor.set_cursor(0);
        let table = editor.insert_table(2, 2);
        // The cursor is left in the new table's first cell; add a row from
        // there, which is what the menu item does.
        assert!(editor.insert_row_at_cursor(true));

        let cells: Vec<(u32, u32)> = editor.document().paragraphs.iter()
            .filter_map(|p| p.style.table_cell)
            .filter(|c| c.table == table)
            .map(|c| (c.row, c.col)).collect();
        assert_eq!(cells, vec![(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (2, 1)]);

        let first = editor.document().paragraphs.iter()
            .position(|p| p.style.table_cell.is_some()).unwrap();
        assert!(editor.document().paragraphs[first..first + cells.len()]
            .iter().all(|p| p.style.table_cell.is_some()), "cells must be consecutive");
    }

    #[test]
    fn list_commands_act_on_the_cursors_paragraph() {
        // The GUI used to pass a hardcoded 0 here, so every one of these
        // edited the document's first paragraph no matter where the caret
        // was.
        let mut editor = three_paragraphs();
        editor.set_cursor(editor.document().paragraph_offset(1));

        assert_eq!(editor.toggle_list_at_cursor(ListKind::Bullet), ListKind::Bullet);
        assert_eq!(editor.document().paragraphs[1].style.list, ListKind::Bullet);
        assert_eq!(editor.document().paragraphs[0].style.list, ListKind::None);

        assert!(editor.indent_list_at_cursor());
        assert_eq!(editor.document().paragraphs[1].style.list_level, 1);
        assert!(editor.outdent_list_at_cursor());
        assert_eq!(editor.document().paragraphs[1].style.list_level, 0);
    }

    #[test]
    fn toggling_the_same_list_kind_turns_it_off() {
        let mut editor = three_paragraphs();
        editor.set_cursor(editor.document().paragraph_offset(1));
        editor.toggle_list_at_cursor(ListKind::Bullet);
        assert_eq!(editor.toggle_list_at_cursor(ListKind::Bullet), ListKind::None);
        assert_eq!(editor.document().paragraphs[1].style.list, ListKind::None);
        assert_eq!(editor.document().paragraphs[1].style.list_level, 0,
                   "leaving a list also leaves its nesting");
    }

    #[test]
    fn switching_list_kind_keeps_the_nesting_level() {
        let mut editor = three_paragraphs();
        editor.set_cursor(editor.document().paragraph_offset(1));
        editor.toggle_list_at_cursor(ListKind::Bullet);
        editor.indent_list_at_cursor();
        assert_eq!(editor.toggle_list_at_cursor(ListKind::Numbered), ListKind::Numbered);
        assert_eq!(editor.document().paragraphs[1].style.list_level, 1);
    }

    #[test]
    fn indenting_outside_a_list_changes_nothing() {
        let mut editor = three_paragraphs();
        editor.set_cursor(editor.document().paragraph_offset(1));
        assert!(!editor.indent_list_at_cursor());
        assert!(!editor.outdent_list_at_cursor());
        assert!(!editor.restart_numbering_at_cursor(), "nothing to renumber in body text");
    }

    #[test]
    fn restart_numbering_applies_only_to_numbered_items() {
        let mut editor = three_paragraphs();
        editor.set_cursor(editor.document().paragraph_offset(1));
        editor.toggle_list_at_cursor(ListKind::Bullet);
        assert!(!editor.restart_numbering_at_cursor());
        editor.toggle_list_at_cursor(ListKind::Numbered);
        assert!(editor.restart_numbering_at_cursor());
        assert_eq!(editor.document().paragraphs[1].style.list_start, Some(1));
    }

    #[test]
    fn page_break_toggles_on_the_cursors_paragraph() {
        let mut editor = three_paragraphs();
        editor.set_cursor(editor.document().paragraph_offset(2));
        assert!(editor.toggle_page_break_at_cursor());
        assert!(editor.document().paragraphs[2].style.page_break_before);
        assert!(!editor.document().paragraphs[0].style.page_break_before);
        // Inserting one is how a user removes one: the same command.
        assert!(!editor.toggle_page_break_at_cursor());
        assert!(!editor.document().paragraphs[2].style.page_break_before);
    }

    #[test]
    fn table_navigation_stops_at_edges_and_tracks_cell() {
        let mut doc = Document::from_plain_text("");
        for col in 0..2 {
            doc.paragraphs.push(Paragraph {
                style: ParaStyle { table_cell: Some(TableCell { table: 1, row: 0, col }), ..Default::default() },
                runs: vec![Run::plain("")],
            });
        }
        let mut editor = StructuredEditor::new(doc);
        assert!(!editor.move_table_cell(1, 0, 0, true));
        assert!(editor.move_table_cell(1, 0, 0, false));
        assert_eq!(editor.table_cell(), Some(TableCell { table: 1, row: 0, col: 1 }));
        assert!(!editor.move_table_cell(1, 0, 1, false));
    }
}
