// SPDX-License-Identifier: GPL-3.0-or-later
//! Seeded random op sequences against the workbook (RFC-0001 Phase 0).
//!
//! Two properties, on a fixed list of seeds so a failure names the exact
//! run:
//! - applying an op leaves the workbook exactly as the direct edit it
//!   stands for does;
//! - applying the inverses in reverse undoes a whole sequence, and
//!   applying the ops again (redo) comes back to the same workbook.

use super::ops::{apply, apply_all, blank_lines, Axis, Op, SheetProp, WorkbookImage};
use super::state::WorkbookState;
use crate::sheet::{BorderStyle, CellBorder};
use crate::style::{CellStyle, HAlign, Rgb};
use suite_common_core::format::{NumberFormat, NumberFormatKind};

pub(super) const SEEDS: &[u64] = &[1, 2, 3, 7, 42, 99, 1337, 0x5EED, 20260925, 0xC0FFEE];
const STEPS: usize = 80;

pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0x1234_5678))
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0 | 1;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub(super) fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

pub(super) fn workbook() -> WorkbookState {
    let mut state = WorkbookState::new(8, 5).unwrap();
    state.add_sheet("Data".into(), 8, 5).unwrap();
    for (sheet, r, c, v) in [(0, 0, 0, "1"), (0, 1, 0, "2"), (0, 2, 0, "=A1+A2"), (0, 3, 1, "=SUM(A1:A3)"), (1, 0, 0, "=Sheet1!A3*10"), (1, 1, 1, "text")] {
        state.set_cell_input_on_sheet(sheet, r, c, v);
    }
    state
}

/// A random op that fits `state` as it is now.
pub(super) fn random_op(rng: &mut Rng, state: &WorkbookState) -> Op {
    let pos = rng.below(state.sheets.len());
    let (rows, cols, sheet) = {
        let s = state.sheets[pos].borrow();
        (s.rows, s.cols, s.sheet_id)
    };
    let (row, col) = (rng.below(rows), rng.below(cols));
    let axis = if rng.below(2) == 0 { Axis::Rows } else { Axis::Cols };
    let len = if axis == Axis::Rows { rows } else { cols };
    match rng.below(20) {
        13 => Op::MoveSheet { sheet, to: rng.below(state.sheets.len()) },
        14 => Op::SetCells {
            sheet,
            cells: (0..1 + rng.below(4)).map(|i| (rng.below(rows), rng.below(cols), format!("{}", i * 7 + rng.below(50)))).collect(),
        },
        15 => Op::SetValidation {
            sheet,
            row,
            col,
            rule: [None, Some(crate::sheet::ValidationRule::WholeNumber { min: Some(0), max: Some(9) })][rng.below(2)].clone(),
        },
        16 => Op::SetLocked { sheet, row, col, protection: crate::sheet::CellProtection { locked: rng.below(2) == 0, hidden_formula: rng.below(2) == 0 } },
        17 => Op::SetProp {
            sheet,
            prop: match rng.below(6) {
                0 => SheetProp::Filtered((0..rng.below(3)).map(|_| rng.below(rows)).collect()),
                1 => SheetProp::HiddenCols((0..rng.below(3)).map(|_| rng.below(cols)).collect()),
                2 => SheetProp::PrintArea(Some((0, 0, rng.below(rows), rng.below(cols)))),
                3 => SheetProp::Sorted(Some((rng.below(cols), crate::sheet::SortDirection::Descending))),
                4 => SheetProp::Protection(crate::sheet::SheetProtection { protected: true, ..Default::default() }),
                _ => SheetProp::HiddenRows(Default::default()),
            },
        },
        18 => Op::DefineName { name: format!("Name{}", rng.below(4)), formula: [None, Some("Sheet1!$A$1".to_string())][rng.below(2)].clone() },
        0..=2 => {
            let input = match rng.below(5) {
                0 => String::new(),
                1 => format!("={}{}+1", ["A", "B", "C"][rng.below(3)], 1 + rng.below(6)),
                2 => "=SUM(A1:B4)".into(),
                3 => format!("word{}", rng.below(9)),
                _ => format!("{}", rng.below(1000)),
            };
            Op::SetCell { sheet, row, col, input }
        }
        3 => Op::SetFormat {
            sheet,
            row,
            col,
            format: NumberFormat::new([NumberFormatKind::General, NumberFormatKind::Percent(1), NumberFormatKind::Number(2)][rng.below(3)].clone()),
        },
        4 => Op::SetStyle {
            sheet,
            row,
            col,
            style: CellStyle { bold: rng.below(2) == 0, fill: Some(Rgb(rng.below(255) as u8, 200, 100)), h_align: HAlign::Center, ..CellStyle::default() },
        },
        5 => Op::SetBorder { sheet, row, col, border: CellBorder::outline(BorderStyle::Solid, (0.0, 0.0, 0.0)) },
        6 => Op::Insert { sheet, axis, at: rng.below(len + 1), lines: blank_lines(state, pos, axis, 1 + rng.below(2)), relinks: Vec::new(), merges: None },
        7 => Op::Delete { sheet, axis, at: rng.below(len), count: 1 },
        8 => Op::Merge { sheet, range: (row.min(rows - 2), col.min(cols - 2), 2, 2) },
        9 => Op::Resize { sheet, axis, index: rng.below(len), size: 10.0 + rng.below(90) as f64 },
        10 => Op::Freeze { sheet, rows: rng.below(3), cols: rng.below(3) },
        11 => Op::RenameSheet { sheet, name: format!("S{}", rng.next() % 100_000) },
        _ => match rng.below(2) {
            0 => Op::AddSheet { index: rng.below(state.sheets.len() + 1), name: format!("New{}", rng.next() % 100_000), sheet_id: None, content: None, relinks: Vec::new() },
            _ => Op::DeleteSheet { sheet },
        },
    }
}

/// The same change made through the model's own direct API.
fn direct(state: &mut WorkbookState, op: &Op) -> Result<(), String> {
    let pos = match op.sheet() {
        Some(id) => state.sheet_index_for_id(id).ok_or("no sheet")?,
        None => 0,
    };
    match op {
        Op::SetCell { row, col, input, .. } => state.set_cell_input_on_sheet(pos, *row, *col, input),
        Op::SetFormat { row, col, format, .. } => state.sheets[pos].borrow_mut().formats[*row][*col] = format.clone(),
        Op::SetStyle { row, col, style, .. } => state.sheets[pos].borrow_mut().styles[*row][*col] = style.clone(),
        Op::SetBorder { row, col, border, .. } => state.sheets[pos].borrow_mut().borders[*row][*col] = border.clone(),
        Op::Resize { axis, index, size, .. } => match axis {
            Axis::Rows => state.sheets[pos].borrow_mut().row_heights[*index] = *size,
            Axis::Cols => state.sheets[pos].borrow_mut().col_widths[*index] = *size,
        },
        Op::Freeze { rows, cols, .. } => {
            let mut s = state.sheets[pos].borrow_mut();
            (s.frozen_rows, s.frozen_cols) = (*rows, *cols);
        }
        Op::RenameSheet { name, .. } => state.rename_sheet(pos, name)?,
        Op::Merge { range, .. } => {
            let mut s = state.sheets[pos].borrow_mut();
            if s.merges.contains(range) {
                return Err("merged".into());
            }
            s.merges.push(*range);
        }
        Op::Insert { axis, at, lines, .. } => {
            let n = lines.len();
            match axis {
                Axis::Rows => {
                    state.engine.insert_rows(pos, *at, n)?;
                    state.sheets[pos].borrow_mut().insert_rows(*at, n);
                }
                Axis::Cols => {
                    state.engine.insert_cols(pos, *at, n)?;
                    state.sheets[pos].borrow_mut().insert_cols(*at, n);
                }
            }
            resync_all(state);
        }
        Op::Delete { axis, at, count, .. } => {
            let len = {
                let s = state.sheets[pos].borrow();
                if *axis == Axis::Rows { s.rows } else { s.cols }
            };
            if *count >= len {
                return Err("last line".into());
            }
            match axis {
                Axis::Rows => {
                    state.engine.delete_rows(pos, *at, *count)?;
                    state.sheets[pos].borrow_mut().delete_rows(*at, *count);
                }
                Axis::Cols => {
                    state.engine.delete_cols(pos, *at, *count)?;
                    state.sheets[pos].borrow_mut().delete_cols(*at, *count);
                }
            }
            resync_all(state);
        }
        Op::AddSheet { index, name, .. } => {
            let (rows, cols) = (state.engine.rows, state.engine.cols);
            state.engine.insert_sheet(*index, name, None)?;
            let id = state.engine.sheet_id_at(*index).unwrap();
            state.sheets.insert(*index, std::rc::Rc::new(std::cell::RefCell::new(crate::sheet::SheetModel::new(name, rows, cols, id))));
            state.active_sheet = state.engine.active_sheet();
            resync_all(state);
        }
        Op::DeleteSheet { .. } => {
            state.delete_sheet(pos)?;
            resync_all(state);
        }
        Op::MoveSheet { to, .. } => {
            let mut order: Vec<usize> = (0..state.sheets.len()).collect();
            let moved = order.remove(pos);
            order.insert(*to, moved);
            state.reorder_sheets(&order)?;
        }
        Op::SetCells { cells, .. } => {
            let previous = state.active_sheet;
            state.engine.set_active_sheet(pos)?;
            for (r, c, input) in cells {
                state.engine.set_cell_text(*r, *c, input);
            }
            state.engine.set_active_sheet(previous)?;
            resync_all(state);
        }
        Op::SetValidation { row, col, rule, .. } => state.sheets[pos].borrow_mut().validations[*row][*col] = rule.clone(),
        Op::SetLocked { row, col, protection, .. } => state.sheets[pos].borrow_mut().cell_protections[*row][*col] = protection.clone(),
        Op::SetProp { prop, .. } => {
            let mut s = state.sheets[pos].borrow_mut();
            match prop.clone() {
                SheetProp::Filtered(v) => s.hidden_rows = v,
                SheetProp::HiddenRows(v) => s.hidden_rows_manual = v,
                SheetProp::HiddenCols(v) => s.hidden_cols = v,
                SheetProp::PrintArea(v) => s.print_area = v,
                SheetProp::Sorted(v) => s.sorted_col = v,
                SheetProp::Protection(v) => s.protection = v,
                SheetProp::CondRules(v) => s.cond_rules = v,
                SheetProp::Charts(v) => s.charts = v,
                SheetProp::Pivots(v) => s.pivot_tables = v,
                SheetProp::PageSetup(v) => s.page_setup = v,
            }
        }
        Op::DefineName { name, formula } => {
            state.engine.set_defined_name(name, formula.as_deref())?;
            state.engine.evaluate();
            resync_all(state);
        }
        Op::Unmerge { .. } => unreachable!("not generated"),
    }
    Ok(())
}

fn resync_all(state: &mut WorkbookState) {
    state.grow_engine();
    let previous = state.active_sheet;
    for s in 0..state.sheets.len() {
        state.engine.set_active_sheet(s).unwrap();
        state.active_sheet = s;
        state.sync_active_sheet();
    }
    state.engine.set_active_sheet(previous).unwrap();
    state.active_sheet = previous;
}

#[test]
fn an_op_does_exactly_what_the_direct_edit_does() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed);
        let (mut by_op, mut by_hand) = (workbook(), workbook());
        for step in 0..STEPS {
            let op = random_op(&mut rng, &by_op);
            let a = apply(&mut by_op, &op);
            let b = direct(&mut by_hand, &op);
            assert_eq!(a.is_ok(), b.is_ok(), "seed {seed} step {step}: {op:?} applied {a:?} vs directly {b:?}");
            assert_eq!(WorkbookImage::of(&by_op), WorkbookImage::of(&by_hand), "seed {seed} step {step}: {op:?}");
        }
    }
}

#[test]
fn undoing_every_op_restores_the_workbook_and_redo_returns() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed ^ 0xABCD);
        let mut state = workbook();
        let start = WorkbookImage::of(&state);
        let mut steps: Vec<(Vec<Op>, Vec<Op>)> = Vec::new();
        for _ in 0..STEPS {
            // Groups of one to three ops, as user actions are.
            let group: Vec<Op> = (0..1 + rng.below(3)).map(|_| random_op(&mut rng, &state)).collect();
            if let Ok(inverse) = apply_all(&mut state, &group) {
                steps.push((group, inverse));
            }
        }
        let end = WorkbookImage::of(&state);
        let mut redo = Vec::new();
        for (i, (_, inverse)) in steps.iter().enumerate().rev() {
            let again = apply_all(&mut state, inverse).unwrap_or_else(|e| panic!("seed {seed}: undo of step {i} failed: {e}"));
            redo.push(again);
        }
        assert_eq!(WorkbookImage::of(&state), start, "seed {seed}: undoing everything");
        for (i, ops) in redo.iter().enumerate().rev() {
            apply_all(&mut state, ops).unwrap_or_else(|e| panic!("seed {seed}: redo {i} failed: {e}"));
        }
        assert_eq!(WorkbookImage::of(&state), end, "seed {seed}: redoing everything");
    }
}

#[test]
fn a_group_that_fails_part_way_changes_nothing() {
    let mut state = workbook();
    let before = WorkbookImage::of(&state);
    let id = state.sheets[0].borrow().sheet_id;
    let ops = vec![Op::SetCell { sheet: id, row: 0, col: 0, input: "99".into() }, Op::SetCell { sheet: id, row: 99, col: 0, input: "x".into() }];
    assert!(apply_all(&mut state, &ops).is_err());
    assert_eq!(WorkbookImage::of(&state), before);
}

#[test]
fn deleting_a_row_a_formula_points_at_comes_back_whole_on_undo() {
    let mut state = workbook();
    let before = WorkbookImage::of(&state);
    // A3 is =A1+A2, and Sheet2's A1 points at Sheet1!A3: deleting row 1
    // breaks both references (#REF!).
    let id = state.sheets[0].borrow().sheet_id;
    let inverse = apply(&mut state, &Op::Delete { sheet: id, axis: Axis::Rows, at: 0, count: 1 }).unwrap();
    assert!(state.engine.input_at(0, 1, 0).contains("#REF!"), "{}", state.engine.input_at(0, 1, 0));
    apply_all(&mut state, &inverse).unwrap();
    assert_eq!(WorkbookImage::of(&state), before);
    assert_eq!(state.engine.cell_at(1, 0, 0), "30");
}

#[test]
fn a_deleted_sheet_comes_back_with_its_identity_content_and_references() {
    let mut state = workbook();
    let before = WorkbookImage::of(&state);
    let id = state.sheets[0].borrow().sheet_id;
    let inverse = apply(&mut state, &Op::DeleteSheet { sheet: id }).unwrap();
    assert_eq!(state.sheets.len(), 1);
    apply_all(&mut state, &inverse).unwrap();
    assert_eq!(WorkbookImage::of(&state), before);
}

/// An edit recorded before its sheet moved still edits that sheet; one
/// recorded on a sheet since deleted fails cleanly instead of editing
/// whichever sheet took its place (found by tests/stateful.rs seed 24301).
#[test]
fn ops_follow_their_sheet_not_its_position() {
    let mut state = workbook();
    let data = state.sheets[1].borrow().sheet_id;
    let edit = Op::SetCell { sheet: data, row: 4, col: 4, input: "moved".into() };
    apply(&mut state, &Op::AddSheet { index: 0, name: "First".into(), sheet_id: None, content: None, relinks: Vec::new() }).unwrap();
    apply(&mut state, &edit).unwrap();
    assert_eq!(state.engine.input_at(2, 4, 4), "moved", "the Data sheet is at position 2 now");
    apply(&mut state, &Op::DeleteSheet { sheet: data }).unwrap();
    let before = WorkbookImage::of(&state);
    assert!(apply(&mut state, &edit).is_err());
    assert_eq!(WorkbookImage::of(&state), before);
}

#[test]
fn the_controller_undoes_and_redoes_op_edits() {
    let mut controller = super::WorkbookController::new(6, 4).unwrap();
    controller.edit_cell(0, 0, "5");
    controller.insert_lines(Axis::Rows, 0, 2);
    assert_eq!(controller.state.borrow().sheet().cell(2, 0), "5");
    controller.toggle_merge();
    assert_eq!(controller.state.borrow().sheet().merges.len(), 1);
    assert_eq!(controller.undo_description(), Some("Merge Cells"));
    assert!(controller.undo() && controller.undo());
    assert_eq!(controller.state.borrow().sheet().cell(0, 0), "5");
    assert!(controller.state.borrow().sheet().merges.is_empty());
    assert!(controller.redo());
    assert_eq!(controller.state.borrow().sheet().cell(2, 0), "5");
}
