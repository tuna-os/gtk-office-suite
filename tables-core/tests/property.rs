// property.rs — generative round-trip testing (#106, first slice).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Hand-authored xlsx tests (io.rs's #[test]s) cover specific scenarios we
// thought to write. proptest generates many small grids of plain (non-
// formula) cell values and asserts the xlsx write/read round trip is
// lossless for each one — catching combinations a human wouldn't think
// to author, at the cost of only covering what the strategy generates
// (plain text/integer cells here; formulas, formatting, and merges are
// out of scope for this first slice).

use proptest::prelude::*;
use tables_core::engine::TablesEngine;
use tables_core::io::{load_xlsx_workbook, save_sheets_to_xlsx_with_engine};
use tables_core::sheet::SheetModel;

const ROWS: usize = 4;
const COLS: usize = 4;

/// Plain cell content: short integers or short lowercase-ASCII words.
/// Deliberately excludes anything that would be parsed as a formula
/// (leading '=') or that xlsx/calamine's own numeric formatting could
/// legitimately reformat (floats, leading zeros, whitespace) — this
/// property is about the round-trip pipeline losing data, not about
/// numeric display formatting, which has its own coverage elsewhere.
fn cell_value_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        (0i64..100000).prop_map(|n| n.to_string()),
        "[a-z]{1,8}",
    ]
}

fn grid_strategy() -> impl Strategy<Value = Vec<Vec<String>>> {
    prop::collection::vec(
        prop::collection::vec(cell_value_strategy(), COLS),
        ROWS,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn xlsx_round_trip_preserves_plain_cell_values(grid in grid_strategy()) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prop.xlsx");

        let mut engine = TablesEngine::new(ROWS, COLS).unwrap();
        for (r, row) in grid.iter().enumerate() {
            for (c, val) in row.iter().enumerate() {
                engine.set_cell_text(r, c, val);
            }
        }
        engine.evaluate();

        let mut sheet = SheetModel::new("Sheet1", ROWS, COLS, 0);
        sheet.sync_from_engine(&engine);

        save_sheets_to_xlsx_with_engine(path.to_str().unwrap(), &[sheet], Some(&engine)).unwrap();
        let (mut loaded, _sheets) = load_xlsx_workbook(path.to_str().unwrap()).unwrap();
        loaded.set_active_sheet(0).unwrap();

        for (r, row) in grid.iter().enumerate() {
            for (c, expected) in row.iter().enumerate() {
                let actual = loaded.cell(r, c);
                prop_assert_eq!(
                    &actual, expected,
                    "cell ({}, {}): wrote {:?}, read back {:?}", r, c, expected, actual
                );
            }
        }
    }
}
