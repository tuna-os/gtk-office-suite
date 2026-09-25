// SPDX-License-Identifier: GPL-3.0-or-later
//! The engine is a pure function of its inputs (RFC-0001 Phase 2).
//!
//! Collaboration replicates inputs only, and every peer computes values
//! itself. That holds only if the same inputs, arriving in any order, give
//! byte-identical values: the same bits for every number, the same text,
//! the same errors. If they didn't, two peers would see different
//! numbers, which is worse than a slow recalculation. Volatile functions
//! (`RAND`, `NOW`, `TODAY`) are left out: they differ between any two
//! evaluations, so they need to be pinned before collaboration ships.

use ironcalc_base::cell::CellValue;
use tables_core::TablesEngine;

const ROWS: usize = 12;
const COLS: usize = 6;

/// Inputs for two sheets, `(sheet, row, col, input)`. They cover chains
/// written before the cells they read, cross-sheet references, lookups,
/// text, floating-point sums whose result depends on the order of adding,
/// errors and names.
fn inputs() -> Vec<(usize, usize, usize, String)> {
    let mut v: Vec<(usize, usize, usize, String)> = Vec::new();
    for r in 0..10 {
        v.push((0, r, 0, format!("{}", 0.1 * (r as f64 + 1.0))));
        v.push((0, r, 1, format!("=A{}*3+B{}", r + 1, r + 2)));
        v.push((1, r, 0, ["north", "south", "east", "west"][r % 4].to_string()));
        v.push((1, r, 1, format!("{}", (r * 37 % 11) as f64 / 7.0)));
    }
    v.push((0, 10, 1, "1e-17".into()));
    let formulas = [
        (0, 0, 2, "=SUM(A1:A10)"),
        (0, 1, 2, "=A1+A2+A3+A4+A5+A6+A7+A8+A9+A10"),
        (0, 2, 2, "=AVERAGE(B1:B10)/3"),
        (0, 3, 2, "=Data!B1/Data!B2"),
        (0, 4, 2, "=VLOOKUP(\"east\",Data!A1:B10,2,FALSE)"),
        (0, 5, 2, "=INDEX(Data!B1:B10,MATCH(\"west\",Data!A1:A10,0))"),
        (0, 6, 2, "=IF(C1>C2,\"more\",IF(C1<C2,\"less\",\"same\"))"),
        (0, 7, 2, "=CONCATENATE(Data!A1,\"-\",ROUND(C3,4))"),
        (0, 8, 2, "=1/0"),
        (0, 9, 2, "=SUMIF(Data!A1:A10,\"north\",Data!B1:B10)"),
        (0, 0, 3, "=Total*2"),
        (0, 1, 3, "=SQRT(C1)^3-EXP(LN(C2))"),
        (0, 2, 3, "=STDEV(Data!B1:B10)"),
        (0, 3, 3, "=MOD(C4*1000,7)"),
        (0, 4, 3, "=D1+D2+D3+D4"),
        (1, 0, 2, "=Sheet1!C1+Sheet1!D5"),
        (1, 1, 2, "=SUMPRODUCT(B1:B10,Sheet1!A1:A10)"),
        (1, 2, 2, "=MAX(Sheet1!B1:B11)-MIN(Sheet1!B1:B11)"),
    ];
    v.extend(formulas.iter().map(|(s, r, c, f)| (*s, *r, *c, f.to_string())));
    v
}

/// Every cell's value, as bytes: a number by its bit pattern.
fn values(engine: &TablesEngine) -> Vec<(usize, usize, usize, String)> {
    let mut out = Vec::new();
    for sheet in 0..2 {
        for r in 0..ROWS {
            for c in 0..COLS {
                let value = match engine.model.get_cell_value_by_index(sheet as u32, r as i32 + 1, c as i32 + 1) {
                    Ok(CellValue::Number(n)) => format!("n:{:016x}", n.to_bits()),
                    Ok(CellValue::String(s)) => format!("s:{s}"),
                    Ok(CellValue::Boolean(b)) => format!("b:{b}"),
                    Ok(CellValue::None) => continue,
                    Err(e) => format!("e:{e}"),
                };
                out.push((sheet, r, c, value));
            }
        }
    }
    out
}

/// A workbook built from `inputs` in this order, with the name `Total`
/// defined either before or after the cells.
fn build(inputs: &[(usize, usize, usize, String)], name_first: bool) -> TablesEngine {
    let mut engine = TablesEngine::new(ROWS, COLS).unwrap();
    engine.add_sheet("Data").unwrap();
    let define = |engine: &mut TablesEngine| engine.set_defined_name("Total", Some("Sheet1!$C$1")).unwrap();
    if name_first {
        define(&mut engine);
    }
    for (sheet, r, c, input) in inputs {
        engine.set_active_sheet(*sheet).unwrap();
        engine.set_cell_text(*r, *c, input);
    }
    if !name_first {
        define(&mut engine);
    }
    engine.evaluate();
    engine
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

#[test]
fn the_same_inputs_in_any_order_compute_byte_identical_values() {
    let inputs = inputs();
    let reference = values(&build(&inputs, true));
    assert!(reference.iter().any(|(_, _, _, v)| v.starts_with("n:")), "the workbook computes numbers");
    assert!(reference.iter().any(|(_, _, _, v)| v.starts_with("e:") || v.contains('#')), "and an error");

    let mut reversed = inputs.clone();
    reversed.reverse();
    assert_eq!(values(&build(&reversed, false)), reference, "reversed, name last");

    for seed in 1..=20u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut shuffled = inputs.clone();
        for i in (1..shuffled.len()).rev() {
            shuffled.swap(i, rng.below(i + 1));
        }
        assert_eq!(values(&build(&shuffled, seed % 2 == 0)), reference, "seed {seed}");
    }
}

#[test]
fn a_cell_overwritten_ends_where_its_last_input_says_whatever_came_between() {
    // What a merge does: the same final inputs, reached through different
    // intermediate ones.
    let inputs = inputs();
    let reference = values(&build(&inputs, true));
    let mut detour = vec![(0, 0, 2, "=A1".to_string()), (1, 3, 1, "999".to_string()), (0, 4, 3, "text".to_string())];
    detour.extend(inputs.iter().cloned());
    assert_eq!(values(&build(&detour, true)), reference);
}
