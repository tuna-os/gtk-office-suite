//! Release-gate timings for the spreadsheet interaction paths.
//!
//! These are deliberately ordinary integration tests (rather than an
//! opt-in benchmark) so a change which regresses a user-facing path cannot
//! merge without being noticed.  The fixture sizes are large enough to catch
//! accidental full-grid work while keeping the gate usable on CI runners.

use std::hint::black_box;
use std::time::{Duration, Instant};

use tables_core::engine::TablesEngine;
use tables_core::io::{load_workbook, save_sheets_to_xlsx_bytes};
use tables_core::sheet::{col_x, hit_row_divider, row_y, visible_cols, visible_rows, SheetModel, ROW_HEADER_WIDTH};
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
            .set_user_input(0, row + 1, 1, value)
            .expect("seed value");
        engine
            .model
            .set_user_input(0, row + 1, 2, format!("=A{}*2", row + 1))
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
    // Opening what was just saved. Each loader recalculated the workbook
    // after every cell it set, so this open was quadratic in the cell
    // count and did not finish in twenty minutes (#1208).
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("dense.xlsx");
    std::fs::write(&path, save_sheets_to_xlsx_bytes(std::slice::from_ref(&sheet), Some(&engine)).expect("save"))
        .expect("write fixture");
    let reopen = p95("dense xlsx open", || {
        let (_, sheets) = load_workbook(path.to_str().unwrap()).expect("open");
        assert_eq!(sheets[0].cell(511, 127), sheet.cell(511, 127));
        black_box(sheets);
    });
    black_box((open, scroll, edit, recalc, save, reopen));
}

/// performance-accessibility.md row 3 (#1208): a frame's geometry scales
/// with what is on screen, not with how far down the sheet it is. The
/// renderer used to call row_on_screen and row_y for every row from the
/// first, and each re-summed every row above it, so a frame at the bottom
/// of this 100,000-row sheet cost ~5 billion additions. visible_rows walks
/// the rows once.
#[test]
fn a_frame_at_the_bottom_of_a_tall_sheet_stays_within_budget() {
    let mut sheet = SheetModel::new("Tall", 100_000, 4, 1);
    sheet.hidden_rows.insert(50_000);
    sheet.set_row_height(70_000, 60.0);
    let (view_w, view_h) = (1_280.0, 800.0);
    let bottom = tables_core::sheet::max_scroll((view_w, view_h), &sheet).1;
    let frame = p95("tall sheet frame at the bottom", || {
        let rows = visible_rows(bottom, view_h, &sheet);
        let cols = visible_cols(0.0, view_w, &sheet);
        assert_eq!(rows.last().map(|&(r, _)| r), Some(99_999), "the last row is on screen");
        assert!(rows.len() < 60, "only a screenful of rows: {}", rows.len());
        let (r, y) = rows[rows.len() / 2];
        let divider = hit_row_divider(ROW_HEADER_WIDTH / 2.0, y + sheet.row_height(r), bottom, &sheet);
        assert_eq!(divider, Some(r));
        black_box((rows, cols));
    });
    assert!(frame <= Duration::from_millis(300), "a frame's geometry took {frame:?}");
}
