//! Release-gate timings for the spreadsheet interaction paths.
//!
//! These are deliberately ordinary integration tests (rather than an
//! opt-in benchmark) so a change which regresses a user-facing path cannot
//! merge without being noticed.  The fixture sizes are large enough to catch
//! accidental full-grid work while keeping the gate usable on CI runners.

use std::hint::black_box;
use std::time::{Duration, Instant};

use tables_core::engine::TablesEngine;
use tables_core::io::save_sheets_to_xlsx_bytes;
use tables_core::sheet::{col_x, row_y, SheetModel};
use tables_core::sparse::SparseGrid;

const SAMPLES: usize = 7;
const P95_BUDGET: Duration = Duration::from_millis(2_000);

fn p95<F>(name: &str, mut operation: F) -> Duration
where
    F: FnMut(),
{
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        operation();
        samples.push(start.elapsed());
    }
    samples.sort_unstable();
    let percentile = samples[(SAMPLES * 95).div_ceil(100).saturating_sub(1)];
    eprintln!("{name}: p95={percentile:?} samples={samples:?}");
    assert!(
        percentile <= P95_BUDGET,
        "{name} p95 budget exceeded: {percentile:?} > {P95_BUDGET:?}; samples={samples:?}"
    );
    percentile
}

fn sparse_fixture() -> SparseGrid<String> {
    let mut grid = SparseGrid::new(1_000_000, 16_384);
    for index in 0..10_000 {
        let row = (index * 97) % grid.rows();
        let col = (index * 31) % grid.cols();
        grid.set(row, col, format!("value-{index}"));
    }
    grid
}

fn dense_sheet() -> SheetModel {
    let mut sheet = SheetModel::new("Dense", 512, 128, 1);
    for row in 0..sheet.rows {
        for col in 0..sheet.cols {
            if (row + col) % 3 == 0 {
                *sheet.cell_mut(row, col) = format!("{row}:{col}");
            }
        }
    }
    sheet
}

fn small_save_sheet() -> SheetModel {
    let mut sheet = SheetModel::new("Sparse", 256, 32, 1);
    let (rows, cols) = (sheet.rows, sheet.cols);
    for index in 0..256 {
        *sheet.cell_mut(index * 17 % rows, index * 5 % cols) = index.to_string();
    }
    sheet
}

fn recalc_fixture() -> TablesEngine {
    let mut engine = TablesEngine::new(256, 32).expect("engine fixture");
    for row in 0..256 {
        let value = (row + 1).to_string();
        engine
            .model
            .set_user_input(0, row as i32 + 1, 1, value)
            .expect("seed value");
        engine
            .model
            .set_user_input(0, row as i32 + 1, 2, format!("=A{}*2", row + 1))
            .expect("seed formula");
    }
    engine
}

#[test]
fn sparse_fixture_enforces_open_scroll_edit_budgets() {
    let open = p95("sparse open", || {
        black_box(sparse_fixture());
    });

    let grid = sparse_fixture();
    let mut sheet = SheetModel::new("Sparse viewport", 2_048, 512, 1);
    let scroll = p95("sparse scroll", || {
        let mut checksum = 0.0;
        for offset in 0..1_000 {
            checksum += col_x(offset % sheet.cols, offset as f64, &sheet);
            checksum += row_y(offset % sheet.rows, offset as f64, &sheet);
        }
        black_box(checksum);
    });
    let edit = p95("sparse edit", || {
        let mut grid = grid.clone();
        let (rows, cols) = (grid.rows(), grid.cols());
        for index in 0..1_000 {
            grid.set(index * 997 % rows, index * 17 % cols, "edited".into());
        }
        black_box(grid.len());
    });
    let mut sparse_engine = recalc_fixture();
    let recalc = p95("sparse recalc", || {
        sparse_engine.evaluate();
        black_box(sparse_engine.cell(255, 1));
    });
    let sparse_sheet = small_save_sheet();
    let save = p95("sparse save", || {
        let bytes = save_sheets_to_xlsx_bytes(std::slice::from_ref(&sparse_sheet), Some(&sparse_engine))
            .expect("sparse xlsx fixture save");
        black_box(bytes.len());
    });
    // Keep the geometry fixture live for the whole measurement block; this
    // also guards against the test accidentally measuring a no-op.
    sheet.set_col_width(17, 240.0);
    black_box((open, scroll, edit, recalc, save, sheet.col_width(17)));
}

#[test]
fn dense_fixture_enforces_recalc_and_save_budgets() {
    let open = p95("dense open", || {
        black_box(dense_sheet());
    });
    let mut viewport = dense_sheet();
    let (viewport_rows, viewport_cols) = (viewport.rows, viewport.cols);
    let scroll = p95("dense scroll", || {
        let mut checksum = 0.0;
        for offset in 0..1_000 {
            checksum += col_x(offset % viewport_cols, offset as f64, &viewport);
            checksum += row_y(offset % viewport_rows, offset as f64, &viewport);
        }
        black_box(checksum);
    });
    let edit = p95("dense edit", || {
        for index in 0..1_000 {
            *viewport.cell_mut(index % viewport_rows, (index * 7) % viewport_cols) =
                index.to_string();
        }
        black_box(viewport.cell(511, 127));
    });
    let mut engine = recalc_fixture();
    let recalc = p95("dense recalc", || {
        engine.evaluate();
        black_box(engine.cell(255, 1));
    });

    let sheet = dense_sheet();
    let save = p95("dense save", || {
        let bytes = save_sheets_to_xlsx_bytes(std::slice::from_ref(&sheet), Some(&engine))
            .expect("xlsx fixture save");
        assert!(!bytes.is_empty());
        black_box(bytes.len());
    });
    black_box((open, scroll, edit, recalc, save));
}
