//! Sparse, coordinate-addressed cell storage for large worksheets.
//!
//! The grid keeps dimensions as metadata and stores only non-default cells.
//! This makes an empty million-row worksheet cheap while retaining predictable
//! coordinates for structural edits.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct SparseGrid<T> {
    rows: usize,
    cols: usize,
    cells: BTreeMap<(usize, usize), T>,
}

impl<T> SparseGrid<T> {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self { rows, cols, cells: BTreeMap::new() }
    }

    pub fn rows(&self) -> usize { self.rows }
    pub fn cols(&self) -> usize { self.cols }
    pub fn len(&self) -> usize { self.cells.len() }
    pub fn is_empty(&self) -> bool { self.cells.is_empty() }

    pub fn get(&self, row: usize, col: usize) -> Option<&T> {
        self.cells.get(&(row, col))
    }

    pub fn iter(&self) -> impl Iterator<Item = ((usize, usize), &T)> {
        self.cells.iter().map(|(&(row, col), value)| ((row, col), value))
    }
}

impl<T: Default + PartialEq> SparseGrid<T> {
    pub fn set(&mut self, row: usize, col: usize, value: T) {
        assert!(row < self.rows && col < self.cols, "cell outside sparse grid");
        if value == T::default() { self.cells.remove(&(row, col)); }
        else { self.cells.insert((row, col), value); }
    }

    pub fn insert_rows(&mut self, at: usize, count: usize) {
        assert!(at <= self.rows);
        if count == 0 { return; }
        self.cells = std::mem::take(&mut self.cells).into_iter().map(|((r, c), v)| {
            ((if r >= at { r + count } else { r }, c), v)
        }).collect();
        self.rows += count;
    }

    pub fn delete_rows(&mut self, at: usize, count: usize) {
        assert!(at <= self.rows && count <= self.rows - at);
        if count == 0 { return; }
        let end = at + count;
        self.cells = std::mem::take(&mut self.cells).into_iter().filter_map(|((r, c), v)| {
            if (at..end).contains(&r) { None }
            else { Some(((if r >= end { r - count } else { r }, c), v)) }
        }).collect();
        self.rows -= count;
    }

    pub fn insert_cols(&mut self, at: usize, count: usize) {
        assert!(at <= self.cols);
        if count == 0 { return; }
        self.cells = std::mem::take(&mut self.cells).into_iter().map(|((r, c), v)| {
            ((r, if c >= at { c + count } else { c }), v)
        }).collect();
        self.cols += count;
    }

    pub fn delete_cols(&mut self, at: usize, count: usize) {
        assert!(at <= self.cols && count <= self.cols - at);
        if count == 0 { return; }
        let end = at + count;
        self.cells = std::mem::take(&mut self.cells).into_iter().filter_map(|((r, c), v)| {
            if (at..end).contains(&c) { None }
            else { Some(((r, if c >= end { c - count } else { c }), v)) }
        }).collect();
        self.cols -= count;
    }
}

#[cfg(test)]
mod tests {
    use super::SparseGrid;

    #[test]
    fn empty_large_grid_has_no_cell_allocation() {
        let grid = SparseGrid::<String>::new(1_000_000, 16_384);
        assert_eq!(grid.len(), 0);
        assert_eq!((grid.rows(), grid.cols()), (1_000_000, 16_384));
    }

    #[test]
    fn structural_edits_shift_only_stored_cells() {
        let mut grid = SparseGrid::new(4, 4);
        grid.set(1, 1, "value".to_string());
        grid.insert_rows(1, 2);
        grid.insert_cols(2, 1);
        assert_eq!(grid.get(3, 1).map(String::as_str), Some("value"));
        grid.delete_rows(2, 1);
        grid.delete_cols(0, 1);
        assert_eq!(grid.get(2, 0).map(String::as_str), Some("value"));
        // Deleting the column the cell lives in drops the cell entirely.
        grid.delete_cols(0, 1);
        assert_eq!(grid.get(2, 0), None);
    }
}
