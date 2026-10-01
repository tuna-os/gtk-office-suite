//! Peak memory budgets for the spreadsheet paths (#1208).
//!
//! performance-accessibility.md row 2 asks for peak memory beside latency.
//! A sparse million-row grid holding 10,000 values, and a dense 512×128
//! sheet saved to XLSX and opened again: each operation's peak heap above
//! what was live before it started is printed and held to a budget. The
//! sparse grid's budget is the point of the sparse model: it must scale
//! with the values held, not with the 16 billion cells it spans. One test,
//! so the harness's parallel threads cannot add a neighbour's allocations
//! to a measurement.
//!
//! Budgets are about three times what the development container measured
//! (debug build) when they were set: the sparse grid 0.9 MB; the dense
//! sheet's build 13.5 MB, XLSX save 6 MB and open 31 MB.
//!
//! Writing this found opening an xlsx quadratic in its cells: every loader
//! recalculated the whole workbook after each cell it set, so this dense
//! sheet did not open in twenty minutes. The loaders, paste and PDF export
//! now set cells with `put_cell_text` and recalculate once; it opens in
//! 0.8 s, and performance_budgets.rs times that open too.

use std::hint::black_box;

use suite_common_core::peak_heap::{peak_during, CountingAlloc};
use tables_core::io::{load_workbook, save_sheets_to_xlsx_bytes};
use tables_core::sheet::SheetModel;
use tables_core::sparse::SparseGrid;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const MB: usize = 1 << 20;

fn within(name: &str, budget_mb: usize, peak: usize) {
    eprintln!("{name}: peak heap {:.1} MB, budget {budget_mb} MB", peak as f64 / MB as f64);
    assert!(peak <= budget_mb * MB, "{name}: peak heap {peak} bytes is over its {budget_mb} MB budget");
}

#[test]
fn sparse_and_dense_sheets_stay_within_their_memory_budgets() {
    let (grid, peak) = peak_during(|| {
        let mut grid = SparseGrid::new(1_000_000, 16_384);
        for index in 0..10_000 {
            let row = (index * 97) % grid.rows();
            let col = (index * 31) % grid.cols();
            grid.set(row, col, format!("value-{index}"));
        }
        grid
    });
    within("sparse 10,000 values over 1M×16K", 8, peak);
    black_box(grid);

    let (sheet, peak) = peak_during(|| {
        let mut sheet = SheetModel::new("Dense", 512, 128, 1);
        for row in 0..sheet.rows {
            for col in 0..sheet.cols {
                if (row + col) % 3 == 0 {
                    *sheet.cell_mut(row, col) = format!("{row}:{col}");
                }
            }
        }
        sheet
    });
    within("dense 512×128 build", 48, peak);
    let (bytes, peak) = peak_during(|| save_sheets_to_xlsx_bytes(std::slice::from_ref(&sheet), None).expect("save"));
    within("dense xlsx save", 32, peak);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dense.xlsx");
    std::fs::write(&path, &bytes).unwrap();
    let (back, peak) = peak_during(|| load_workbook(path.to_str().unwrap()).expect("open"));
    within("dense xlsx open", 96, peak);
    assert_eq!(back.1[0].cell(511, 127), sheet.cell(511, 127), "the reopened sheet differs");
    black_box(back);
}
