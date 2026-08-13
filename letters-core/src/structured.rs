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
            self.document.delete_range(start, end);
            self.document.insert_text(start, text);
            self.cursor = start + text.chars().count();
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
        assert_eq!(editor.document().style_at(0).bold, true);
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
