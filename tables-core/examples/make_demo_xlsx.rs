// make_demo_xlsx — generate the walkthrough demo spreadsheet.
// Usage: cargo run -p tables-core --example make_demo_xlsx -- <out.xlsx>

use tables_core::engine::TablesEngine;
use tables_core::sheet::SheetModel;

fn main() -> Result<(), String> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "demo.xlsx".into());

    let mut engine = TablesEngine::new(20, 8)?;
    let cells: &[(usize, usize, &str)] = &[
        (0, 0, "Region"), (0, 1, "Q1"), (0, 2, "Q2"), (0, 3, "Growth"),
        (1, 0, "North"), (1, 1, "1200"), (1, 2, "1380"), (1, 3, "=(C2-B2)/B2"),
        (2, 0, "South"), (2, 1, "950"), (2, 2, "1010"), (2, 3, "=(C3-B3)/B3"),
        (3, 0, "East"), (3, 1, "1430"), (3, 2, "1495"), (3, 3, "=(C4-B4)/B4"),
        (4, 0, "West"), (4, 1, "780"), (4, 2, "905"), (4, 3, "=(C5-B5)/B5"),
        (5, 0, "Total"), (5, 1, "=SUM(B2:B5)"), (5, 2, "=SUM(C2:C5)"), (5, 3, "=(C6-B6)/B6"),
    ];
    for (r, c, v) in cells {
        engine.set_cell_text(*r, *c, v);
    }
    engine.evaluate();

    let mut sheet = SheetModel::new("Revenue", 20, 8, 0);
    sheet.sync_from_engine(&engine);
    tables_core::io::save_sheets_to_xlsx_with_engine(&out, &[sheet], Some(&engine))?;
    println!("wrote {out}");
    Ok(())
}
