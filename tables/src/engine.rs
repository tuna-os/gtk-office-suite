// engine.rs — Spreadsheet engine: read/write XLSX, ODS, CSV.
use calamine::{open_workbook_auto, Reader};
use std::path::Path;

pub struct Spreadsheet {
    pub cells: Vec<Vec<String>>,
    pub rows: usize,
    pub cols: usize,
}

impl Spreadsheet {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self { cells: vec![vec![String::new(); cols]; rows], rows, cols }
    }
    pub fn set(&mut self, r: usize, c: usize, v: &str) {
        if r < self.rows && c < self.cols { self.cells[r][c] = v.to_string(); }
    }
    pub fn get(&self, r: usize, c: usize) -> &str {
        if r < self.rows && c < self.cols { &self.cells[r][c] } else { "" }
    }
}

pub fn read_spreadsheet(path: &Path) -> Result<Spreadsheet, String> {
    let mut wb = open_workbook_auto(path).map_err(|e| format!("Open: {}", e))?;
    let name = wb.sheet_names().first().cloned().unwrap_or_default();
    let range = wb.worksheet_range(&name).map_err(|e| format!("Read: {}", e))?;
    let rows = range.rows().count().max(1);
    let cols = range.rows().next().map(|r| r.len()).unwrap_or(1).max(1);
    let mut s = Spreadsheet::new(rows, cols);
    for (r, row) in range.rows().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            s.set(r, c, &cell.to_string());
        }
    }
    Ok(s)
}

pub fn write_spreadsheet(path: &Path, s: &Spreadsheet) -> Result<(), String> {
    use rust_xlsxwriter::*;
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    for r in 0..s.rows {
        for c in 0..s.cols {
            if !s.cells[r][c].is_empty() {
                ws.write_string(r as u32, c as u16, &s.cells[r][c])
                    .map_err(|e| format!("Write: {}", e))?;
            }
        }
    }
    wb.save(path).map_err(|e| format!("Save: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.xlsx");
        let mut s = Spreadsheet::new(2, 2);
        s.set(0, 0, "A"); s.set(1, 1, "B");
        write_spreadsheet(&p, &s).unwrap();
        let b = read_spreadsheet(&p).unwrap();
        assert_eq!(b.get(0, 0), "A");
        assert_eq!(b.get(1, 1), "B");
    }
}
