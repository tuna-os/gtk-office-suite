// SPDX-License-Identifier: GPL-3.0-or-later
//! Two peers in one process (RFC-0001 Phase 2): seeded random op groups on
//! each side, applied concurrently and merged.

use super::collab::Replica;
use super::ops::{apply_all, Op, WorkbookImage};
use super::ops_tests::{random_op, workbook, Rng, SEEDS};
use super::state::WorkbookState;

/// The workbook as the document defines it. Sheet ids are local (each
/// peer's engine numbers its own sheets), so they're left out.
fn image(state: &WorkbookState) -> WorkbookImage {
    let mut image = WorkbookImage::of(state);
    for sheet in &mut image.sheets {
        sheet.sheet_id = 0;
    }
    image
}

/// Apply a random group on `state` and record it in `replica`.
fn edit(rng: &mut Rng, state: &mut WorkbookState, replica: &mut Replica) -> Option<Vec<Op>> {
    let group: Vec<Op> = (0..1 + rng.below(3)).map(|_| random_op(rng, state)).collect();
    apply_all(state, &group).ok()?;
    replica.record(state, &group);
    Some(group)
}

#[test]
fn the_document_holds_exactly_the_workbook_after_every_edit() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed ^ 0xD0C);
        let mut state = workbook();
        let mut replica = Replica::new(1, &state);
        for step in 0..60 {
            let Some(group) = edit(&mut rng, &mut state, &mut replica) else { continue };
            let (_, rebuilt) = Replica::from_updates(9, &replica.export_all()).unwrap();
            assert_eq!(image(&rebuilt), image(&state), "seed {seed}, step {step}: after {group:?}");
        }
    }
}

#[test]
fn two_peers_editing_at_once_converge_and_equal_a_replay() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed ^ 0x2BEE5);
        let base = workbook();
        let mut a = Replica::new(1, &base);
        let (mut b, mut state_b) = a.fork(2).unwrap();
        let mut state_a = a.rebuild().unwrap();
        for round in 0..8 {
            // Each side edits without seeing the other's edits.
            for _ in 0..1 + rng.below(8) {
                edit(&mut rng, &mut state_a, &mut a);
            }
            for _ in 0..1 + rng.below(8) {
                edit(&mut rng, &mut state_b, &mut b);
            }
            a.merge_from(&b).unwrap();
            b.merge_from(&a).unwrap();
            state_a = a.rebuild().unwrap();
            state_b = b.rebuild().unwrap();
            assert_eq!(image(&state_a), image(&state_b), "seed {seed}, round {round}: the peers differ");
            // The merged history, replayed into a peer that saw none of it.
            let (_, replayed) = Replica::from_updates(3, &a.export_all()).unwrap();
            assert_eq!(image(&replayed), image(&state_a), "seed {seed}, round {round}: a replay differs");
        }
    }
}

#[test]
fn a_delete_wins_over_a_concurrent_edit_and_move() {
    let base = workbook();
    let mut a = Replica::new(1, &base);
    let (mut b, mut state_b) = a.fork(2).unwrap();
    let mut state_a = a.rebuild().unwrap();
    let data_a = state_a.sheets[1].borrow().sheet_id;
    let data_b = state_b.sheets[1].borrow().sheet_id;
    let delete = vec![Op::DeleteSheet { sheet: data_a }];
    apply_all(&mut state_a, &delete).unwrap();
    a.record(&state_a, &delete);
    let edits = vec![Op::SetCell { sheet: data_b, row: 2, col: 2, input: "kept?".into() }, Op::MoveSheet { sheet: data_b, to: 0 }];
    apply_all(&mut state_b, &edits).unwrap();
    b.record(&state_b, &edits);
    a.merge_from(&b).unwrap();
    b.merge_from(&a).unwrap();
    for replica in [&mut a, &mut b] {
        let state = replica.rebuild().unwrap();
        let names: Vec<String> = state.sheets.iter().map(|s| s.borrow().name.clone()).collect();
        assert_eq!(names, ["Sheet1"], "the deleted sheet stays deleted");
    }
}

#[test]
fn concurrent_row_inserts_keep_every_cell_with_its_row() {
    let base = workbook();
    let mut a = Replica::new(1, &base);
    let (mut b, mut state_b) = a.fork(2).unwrap();
    let mut state_a = a.rebuild().unwrap();
    let sheet_a = state_a.sheets[0].borrow().sheet_id;
    let sheet_b = state_b.sheets[0].borrow().sheet_id;
    // A inserts a row at the top; B, not knowing, types under row 2.
    let insert = vec![Op::Insert {
        sheet: sheet_a,
        axis: super::ops::Axis::Rows,
        at: 0,
        lines: super::ops::blank_lines(&state_a, 0, super::ops::Axis::Rows, 1),
        relinks: Vec::new(),
        merges: None,
    }];
    apply_all(&mut state_a, &insert).unwrap();
    a.record(&state_a, &insert);
    let typed = vec![Op::SetCell { sheet: sheet_b, row: 2, col: 3, input: "under A3".into() }];
    apply_all(&mut state_b, &typed).unwrap();
    b.record(&state_b, &typed);
    a.merge_from(&b).unwrap();
    let merged = a.rebuild().unwrap();
    // Row 2 moved down one; B's cell went with it, next to A3's formula.
    assert_eq!(merged.engine.input_at(0, 3, 3), "under A3");
    assert_eq!(merged.engine.input_at(0, 3, 0), "=A2+A3");
}
