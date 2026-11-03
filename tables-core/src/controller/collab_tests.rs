// SPDX-License-Identifier: GPL-3.0-or-later
//! Two peers in one process (RFC-0001 Phase 2): seeded random op groups on
//! each side, applied concurrently and merged.

use super::collab::{CollabError, ImportReport, Replica};
use super::ops::{apply_all, blank_lines, Axis, Op, WorkbookImage};
use super::ops_tests::{random_op, workbook, Rng, SEEDS};
use ironcalc_base::cell::CellValue;
use loro::{ExportMode, LoroDoc, LoroValue};
use std::time::Instant;
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

// ---------------------------------------------------------------- deltas, snapshots, import (RFC-0005 PR 1)

/// Every cell's computed value as bytes, the way DCP spec 7 rule 6 compares
/// them: a number by its bit pattern, text and errors as they are.
fn bit_values(state: &WorkbookState) -> Vec<(usize, usize, usize, String)> {
    let mut out = Vec::new();
    for (sheet, model) in state.sheets.iter().enumerate() {
        let (rows, cols) = {
            let s = model.borrow();
            (s.rows, s.cols)
        };
        for r in 0..rows {
            for c in 0..cols {
                let value = match state.engine.model.get_cell_value_by_index(sheet as u32, r as i32 + 1, c as i32 + 1) {
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

fn shuffled<T>(mut items: Vec<T>, seed: u64) -> Vec<T> {
    let mut rng = Rng::new(seed);
    for i in (1..items.len()).rev() {
        items.swap(i, rng.below(i + 1));
    }
    items
}

/// Apply `group` on `state` and record it, returning the delta it made.
fn record_delta(state: &mut WorkbookState, replica: &mut Replica, group: &[Op]) -> Vec<u8> {
    let since = replica.version_bytes();
    apply_all(state, group).unwrap();
    replica.record(state, group);
    replica.export_since(&since).unwrap()
}

#[test]
fn delta_updates_equal_a_replay() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed ^ 0xDE17A);
        let mut state_a = workbook();
        let mut a = Replica::new(1, &state_a);
        // B has only bytes: the genesis, then one delta per edit, in order.
        let (mut b, state_b) = Replica::from_snapshot(2, &a.export_snapshot()).unwrap();
        assert_eq!(image(&state_b), image(&state_a), "seed {seed}: the genesis builds the workbook it was made from");
        let genesis = a.export_snapshot();
        let mut replay = workbook();
        let mut deltas = Vec::new();
        for step in 0..40 {
            let since = a.version_bytes();
            let Some(group) = edit(&mut rng, &mut state_a, &mut a) else { continue };
            apply_all(&mut replay, &group).unwrap();
            let delta = a.export_since(&since).unwrap();
            b.import_updates(&delta).unwrap();
            let seen = b.rebuild().unwrap();
            assert_eq!(image(&seen), image(&replay), "seed {seed}, step {step}: after {group:?}, the deltas differ from a replay");
            assert_eq!(bit_values(&seen), bit_values(&replay), "seed {seed}, step {step}: values differ");
            deltas.push(delta);
        }
        // The same deltas into a peer that builds the workbook once, at the end.
        let (mut c, _) = Replica::from_snapshot(3, &genesis).unwrap();
        for delta in &deltas {
            c.import_updates(delta).unwrap();
        }
        let end = c.rebuild().unwrap();
        assert_eq!(image(&end), image(&replay), "seed {seed}: all deltas at once");
    }
}

#[test]
fn replicas_that_join_through_a_snapshot_converge_under_concurrent_edits() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed ^ 0x5A9);
        let mut state_a = workbook();
        let mut a = Replica::new(1, &state_a);
        // Not a fork: B has never seen A's document except as bytes.
        let (mut b, mut state_b) = Replica::from_snapshot(2, &a.export_snapshot()).unwrap();
        for round in 0..8 {
            let (since_a, since_b) = (a.version_bytes(), b.version_bytes());
            for _ in 0..1 + rng.below(8) {
                edit(&mut rng, &mut state_a, &mut a);
            }
            for _ in 0..1 + rng.below(8) {
                edit(&mut rng, &mut state_b, &mut b);
            }
            // Each side's delta crosses the wire; neither replica is read.
            let (delta_a, delta_b) = (a.export_since(&since_a).unwrap(), b.export_since(&since_b).unwrap());
            a.import_updates(&delta_b).unwrap();
            b.import_updates(&delta_a).unwrap();
            state_a = a.rebuild().unwrap();
            state_b = b.rebuild().unwrap();
            assert_eq!(image(&state_a), image(&state_b), "seed {seed}, round {round}: the peers differ");
            assert_eq!(bit_values(&state_a), bit_values(&state_b), "seed {seed}, round {round}: their values differ");
            let (_, replayed) = Replica::from_updates(3, &a.export_all()).unwrap();
            assert_eq!(image(&replayed), image(&state_a), "seed {seed}, round {round}: a replay differs");
        }
    }
}

#[test]
fn the_same_updates_in_any_order_give_byte_identical_values() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed ^ 0x0DE5);
        let mut state_a = workbook();
        let mut a = Replica::new(1, &state_a);
        let genesis = a.export_snapshot();
        let (mut b, mut state_b) = Replica::from_snapshot(2, &genesis).unwrap();
        // Two authors edit at once; each publishes one update per edit.
        let (mut from_a, mut from_b) = (Vec::new(), Vec::new());
        for _ in 0..6 {
            let since = a.version_bytes();
            if edit(&mut rng, &mut state_a, &mut a).is_some() {
                from_a.push(a.export_since(&since).unwrap());
            }
            let since = b.version_bytes();
            if edit(&mut rng, &mut state_b, &mut b).is_some() {
                from_b.push(b.export_since(&since).unwrap());
            }
        }
        assert!(from_a.len() > 1 && from_b.len() > 1, "seed {seed}: the streams have something to reorder");
        a.merge_from(&b).unwrap();
        let reference = a.rebuild().unwrap();
        let (want_image, want_values) = (image(&reference), bit_values(&reference));

        let interleaved: Vec<&Vec<u8>> = (0..from_a.len().max(from_b.len())).flat_map(|i| from_a.get(i).into_iter().chain(from_b.get(i))).collect();
        let mut orders: Vec<(String, Vec<&Vec<u8>>)> = vec![
            ("A then B".into(), from_a.iter().chain(&from_b).collect()),
            ("B then A".into(), from_b.iter().chain(&from_a).collect()),
            ("interleaved".into(), interleaved),
            ("everything backwards".into(), from_a.iter().chain(&from_b).rev().collect()),
        ];
        for n in 0..8 {
            orders.push((format!("shuffled {n}"), shuffled(from_a.iter().chain(&from_b).collect(), seed ^ n)));
        }
        let mut waited = false;
        for (n, (name, updates)) in orders.iter().enumerate() {
            let (mut c, _) = Replica::from_snapshot(100 + n as u64, &genesis).unwrap();
            for update in updates {
                waited |= c.import_updates(update).unwrap().pending;
            }
            // Every second order also delivers everything again.
            if n % 2 == 1 {
                for update in updates {
                    assert!(!c.import_updates(update).unwrap().changed(), "seed {seed}, {name}: an update already here changed something");
                }
            }
            let got = c.rebuild().unwrap();
            assert_eq!(image(&got), want_image, "seed {seed}, {name}: the workbook differs");
            assert_eq!(bit_values(&got), want_values, "seed {seed}, {name}: the values differ");
        }
        assert!(waited, "seed {seed}: an update that arrived before the ones it follows was never held back");
    }
}

#[test]
fn an_import_that_fails_to_decode_leaves_the_replica_untouched() {
    let mut state_a = workbook();
    let mut a = Replica::new(1, &state_a);
    let (mut b, _) = Replica::from_snapshot(2, &a.export_snapshot()).unwrap();
    let sheet = state_a.sheets[0].borrow().sheet_id;
    let delta = record_delta(&mut state_a, &mut a, &[Op::SetCell { sheet, row: 4, col: 2, input: "42".into() }]);

    let (version, before) = (b.version_bytes(), image(&b.rebuild().unwrap()));
    let mut flipped = delta.clone();
    let last = flipped.len() - 3;
    flipped[last] ^= 0xFF;
    let broken: [(&str, Vec<u8>); 4] = [
        ("garbage", vec![1, 2, 3, 4]),
        ("nothing", Vec::new()),
        ("a byte flipped", flipped),
        ("cut in half", delta[..delta.len() / 2].to_vec()),
    ];
    for (what, bytes) in broken {
        let error = b.import_updates(&bytes).unwrap_err();
        assert!(matches!(error, CollabError::Decode(_)), "{what}: {error}");
        assert_eq!(b.version_bytes(), version, "{what}: the version moved");
        assert_eq!(image(&b.rebuild().unwrap()), before, "{what}: the workbook changed");
    }
    // Nothing is wedged: the real update still applies.
    assert!(b.import_updates(&delta).unwrap().changed());
    assert_eq!(image(&b.rebuild().unwrap()), image(&state_a));
    // A version that is not one, and a snapshot that is not one, are refused too.
    assert!(matches!(a.export_since(&[9, 9, 9, 9, 9, 9]), Err(CollabError::Decode(_))));
    assert!(matches!(Replica::from_snapshot(9, &delta), Err(CollabError::Decode(_))), "an update is not a genesis");
}

/// What a peer that does not keep the rules can send: an update that
/// decodes, written straight into the document.
fn forged(genesis: &[u8], peer: u64, write: impl FnOnce(&LoroDoc, &str)) -> Vec<u8> {
    let doc = LoroDoc::from_snapshot(genesis).unwrap();
    doc.set_peer_id(peer).unwrap();
    let key = match doc.get_movable_list("sheets").to_vec().first() {
        Some(LoroValue::String(key)) => key.to_string(),
        other => panic!("the genesis has no sheet: {other:?}"),
    };
    let from = doc.oplog_vv();
    write(&doc, &key);
    doc.commit();
    doc.export(ExportMode::updates(&from)).unwrap()
}

#[test]
fn an_update_that_decodes_but_would_break_the_workbook_is_refused_by_the_dry_run() {
    let a = Replica::new(1, &workbook());
    let genesis = a.export_snapshot();
    let (mut b, _) = Replica::from_snapshot(2, &genesis).unwrap();
    let (version, before) = (b.version_bytes(), image(&b.rebuild().unwrap()));

    let refused: Vec<(&str, Vec<u8>)> = vec![
        ("a format that is not JSON", forged(&genesis, 0xBAD0, |doc, key| {
            doc.get_map(format!("cells:{key}")).insert("x|y|nf", "{not json").unwrap();
        })),
        ("a cell that is not text", forged(&genesis, 0xBAD1, |doc, key| {
            doc.get_map(format!("cells:{key}")).insert("x|y|v", 3.5).unwrap();
        })),
        ("a cell key without a column", forged(&genesis, 0xBAD2, |doc, key| {
            doc.get_map(format!("cells:{key}")).insert("no-bars", "v").unwrap();
        })),
        ("a negative row height", forged(&genesis, 0xBAD3, |doc, key| {
            doc.get_map(format!("sizes:{key}")).insert("r|x", -5.0).unwrap();
        })),
        ("a sheet name no sheet can have", forged(&genesis, 0xBAD4, |doc, key| {
            doc.get_map(format!("sheet:{key}")).insert("name", "no/slash").unwrap();
        })),
    ];
    for (what, update) in refused {
        let error = b.import_updates(&update).unwrap_err();
        assert!(matches!(error, CollabError::Invalid(_)), "{what}: {error}");
        assert_eq!(b.version_bytes(), version, "{what}: the live document moved");
        assert_eq!(image(&b.rebuild().unwrap()), before, "{what}: the workbook changed");
    }

    // A field this version does not know is a newer peer's, not an attack.
    let newer = forged(&genesis, 0xBAD5, |doc, key| {
        doc.get_map(format!("cells:{key}")).insert("x|y|zz", "from the future").unwrap();
    });
    let report = b.import_updates(&newer).unwrap();
    assert!(report.changed() && !report.structural());
    assert_eq!(image(&b.rebuild().unwrap()), before, "and it adds nothing to the workbook");
}

#[test]
fn a_report_says_a_cell_edit_is_not_structural_and_a_row_insert_a_sheet_delete_are() {
    let mut state_a = workbook();
    let mut a = Replica::new(1, &state_a);
    let (mut b, _) = Replica::from_snapshot(2, &a.export_snapshot()).unwrap();
    let (sheet, data) = (state_a.sheets[0].borrow().sheet_id, state_a.sheets[1].borrow().sheet_id);

    let typed = record_delta(&mut state_a, &mut a, &[Op::SetCell { sheet, row: 4, col: 2, input: "7".into() }]);
    let report = b.import_updates(&typed).unwrap();
    assert!(report.changed() && !report.pending && !report.structural(), "{report:?}");
    let sheets: Vec<_> = report.sheets.values().collect();
    assert_eq!(sheets.len(), 1, "one sheet");
    assert_eq!(sheets[0].cells.len(), 1, "one cell");
    assert!(!sheets[0].lines && !sheets[0].meta);
    assert_eq!(b.import_updates(&typed).unwrap(), ImportReport::default(), "the same update again changes nothing");

    let renamed = record_delta(&mut state_a, &mut a, &[Op::RenameSheet { sheet, name: "Renamed".into() }]);
    let report = b.import_updates(&renamed).unwrap();
    assert!(report.sheets.values().any(|s| s.meta) && !report.structural(), "a rename moves nothing: {report:?}");

    let insert = Op::Insert { sheet, axis: Axis::Rows, at: 0, lines: blank_lines(&state_a, 0, Axis::Rows, 1), relinks: Vec::new(), merges: None };
    let inserted = record_delta(&mut state_a, &mut a, &[insert]);
    let report = b.import_updates(&inserted).unwrap();
    assert!(report.structural() && report.sheets.values().any(|s| s.lines), "a row insert shifts every index below it: {report:?}");

    let deleted = record_delta(&mut state_a, &mut a, &[Op::DeleteSheet { sheet: data }]);
    let report = b.import_updates(&deleted).unwrap();
    assert!(report.structural() && report.sheet_list, "a sheet delete is structural: {report:?}");
    assert_eq!(image(&b.rebuild().unwrap()), image(&state_a), "and B holds what A holds");
}

/// A workbook of `rows` by `cols`: a text column, numbers, and a formula in
/// every fourth column, as RFC-0005 3.7 measured.
fn grid_workbook(rows: usize, cols: usize) -> WorkbookState {
    let mut state = WorkbookState::new(rows, cols).unwrap();
    let inputs: Vec<(usize, usize, String)> = (0..rows)
        .flat_map(|r| {
            (0..cols).map(move |c| {
                let input = if c == 0 {
                    format!("customer {r}")
                } else if c % 4 == 3 {
                    format!("=B{}*2", r + 1)
                } else {
                    format!("{}", (r * c) as f64 * 1.25)
                };
                (r, c, input)
            })
        })
        .collect();
    state.set_cell_inputs_on_sheet(0, inputs.iter().map(|(r, c, input)| (*r, *c, input.as_str()))).unwrap();
    state
}

#[test]
fn a_delta_is_the_size_of_the_edit_not_of_the_workbook() {
    for (rows, cols) in [(10usize, 6usize), (100, 10)] {
        let mut state = grid_workbook(rows, cols);
        let mut a = Replica::new(1, &state);
        let snapshot = a.export_snapshot();
        let sheet = state.sheets[0].borrow().sheet_id;
        let one_cell = record_delta(&mut state, &mut a, &[Op::SetCell { sheet, row: 3, col: 1, input: "42".into() }]);
        let cells: Vec<(usize, usize, String)> = (0..rows).flat_map(|r| (1..cols).map(move |c| (r, c, format!("{}", r * 3 + c)))).take(100).collect();
        let pasted = cells.len();
        let paste = record_delta(&mut state, &mut a, &[Op::SetCells { sheet, cells }]);

        let started = Instant::now();
        let (mut joiner, _) = Replica::from_snapshot(2, &snapshot).unwrap();
        let joined = started.elapsed();
        let started = Instant::now();
        joiner.import_updates(&one_cell).unwrap();
        let imported = started.elapsed();
        joiner.import_updates(&paste).unwrap();
        assert_eq!(image(&joiner.rebuild().unwrap()), image(&state), "{rows}x{cols}: the joiner holds what A holds");

        eprintln!(
            "{rows}x{cols} ({} cells): snapshot {} B, 1-cell delta {} B, {pasted}-cell delta {} B; join {joined:?}, one dry-run import {imported:?}",
            rows * cols,
            snapshot.len(),
            one_cell.len(),
            paste.len(),
        );
        // RFC-0005 3.7 measured about 105 B and 1,734 B; these bounds are loose
        // on purpose: they say "the size of the edit", not an encoding.
        assert!((20..300).contains(&one_cell.len()), "{rows}x{cols}: a one-cell edit is {} B", one_cell.len());
        assert!(paste.len() < 4_000, "{rows}x{cols}: a {pasted}-cell paste is {} B", paste.len());
        if rows * cols >= 1_000 {
            assert!(one_cell.len() * 20 < snapshot.len(), "{rows}x{cols}: a delta must be far smaller than the snapshot");
        }
    }
}
