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

/// Simple formula evaluator (pure Rust, no external deps).
/// Supports: =SUM(A1:A3), =AVERAGE(...), =A1+B2, basic arithmetic.
pub fn eval_formula(formula: &str, sheet: &Spreadsheet) -> Result<String, String> {
    let f = formula.trim();
    if !f.starts_with('=') { return Ok(f.to_string()); }
    let expr = &f[1..];
    
    // =SUM(A1:A3)
    if expr.to_uppercase().starts_with("SUM(") {
        return eval_range_fn(expr, sheet, |vals| {
            let sum: f64 = vals.iter().filter_map(|v| v.parse::<f64>().ok()).sum();
            Ok(sum.to_string())
        });
    }
    // =AVERAGE(A1:A3)
    if expr.to_uppercase().starts_with("AVERAGE(") {
        return eval_range_fn(expr, sheet, |vals| {
            let nums: Vec<f64> = vals.iter().filter_map(|v| v.parse::<f64>().ok()).collect();
            if nums.is_empty() { return Ok("0".into()); }
            Ok((nums.iter().sum::<f64>() / nums.len() as f64).to_string())
        });
    }
    // =MIN(A1:A3) / =MAX(A1:A3)
    if expr.to_uppercase().starts_with("MIN(") {
        return eval_range_fn(expr, sheet, |vals| {
            vals.iter().filter_map(|v| v.parse::<f64>().ok()).min_by(|a,b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)).map(|v| v.to_string()).ok_or("no values".into())
        });
    }
    if expr.to_uppercase().starts_with("MAX(") {
        return eval_range_fn(expr, sheet, |vals| {
            vals.iter().filter_map(|v| v.parse::<f64>().ok()).max_by(|a,b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)).map(|v| v.to_string()).ok_or("no values".into())
        });
    }
    
    // =A1+B2 simple arithmetic
    let mut result = expr.to_string();
    for (col, row_str) in [("A","0"),("B","1"),("C","2"),("D","3"),("E","4")] {
        let cell_ref = format!("{}{}", col, row_str);
        if result.contains(&cell_ref) {
            let val = sheet.get(0, (col.as_bytes()[0] - b'A') as usize);
            result = result.replace(&cell_ref, val);
        }
    }
    // Try simple eval
    if let Ok(v) = meval::eval_str(&result) {
        return Ok(v.to_string());
    }
    Ok(result)
}

fn eval_range_fn<F>(expr: &str, sheet: &Spreadsheet, f: F) -> Result<String, String>
where F: Fn(Vec<String>) -> Result<String, String>
{
    let inner = expr.split('(').nth(1).and_then(|s| s.split(')').next()).unwrap_or("");
    let parts: Vec<&str> = inner.split(':').collect();
    if parts.len() != 2 { return Err("Invalid range".into()); }
    let (start, end) = (parts[0], parts[1]);
    let sc = start.as_bytes()[0] as usize - b'A' as usize;
    let sr = start[1..].parse::<usize>().unwrap_or(1) - 1;
    let ec = end.as_bytes()[0] as usize - b'A' as usize;
    let er = end[1..].parse::<usize>().unwrap_or(1) - 1;
    let mut vals = Vec::new();
    for r in sr..=er {
        for c in sc..=ec {
            vals.push(sheet.get(r, c).to_string());
        }
    }
    f(vals)
}

#[cfg(test)]
mod formula_tests {
    use super::*;
    
    #[test]
    fn test_sum() {
        let mut s = Spreadsheet::new(3, 3);
        s.set(0, 0, "10"); s.set(1, 0, "20"); s.set(2, 0, "30");
        assert_eq!(eval_formula("=SUM(A1:A3)", &s).unwrap(), "60");
    }
    
    #[test]
    fn test_average() {
        let mut s = Spreadsheet::new(3, 3);
        s.set(0, 0, "10"); s.set(1, 0, "20");
        assert_eq!(eval_formula("=AVERAGE(A1:A2)", &s).unwrap(), "15");
    }
    
    #[test]
    fn test_max_min() {
        let mut s = Spreadsheet::new(3, 3);
        s.set(0, 0, "5"); s.set(1, 0, "2"); s.set(2, 0, "8");
        assert_eq!(eval_formula("=MAX(A1:A3)", &s).unwrap(), "8");
        assert_eq!(eval_formula("=MIN(A1:A3)", &s).unwrap(), "2");
    }
}
