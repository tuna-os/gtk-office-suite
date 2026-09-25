// SPDX-License-Identifier: GPL-3.0-or-later
//! The workbook as a Loro document: RFC-0001 Phase 2, behind the `collab`
//! feature (off by default). No transport yet: a [`Replica`] records local
//! op groups into its document, exchanges updates with another replica
//! in-process, and rebuilds the workbook from the merged document.
//!
//! # Encoding
//!
//! The encoding is the one the spike measured (`docs/rfc/0001-spike-results.md`):
//! - `sheets`: a movable list of sheet keys. A deleted sheet stays in the
//!   list with `deleted` set on its map. Deletion is a tombstone that no
//!   concurrent move or edit clears, so delete wins, as the RFC decides.
//!   Undoing the delete clears the tombstone.
//! - `sheet:<key>`: the sheet's `name`, `deleted`, `frozen`, `merges` and
//!   its sheet-wide properties (`p.<name>`). Each is one last-writer-wins
//!   value.
//! - `rows:<key>`, `cols:<key>`: sequences of stable line ids, so a
//!   concurrent insert or delete doesn't shift anyone else's cells.
//! - `cells:<key>`: one register per cell field, keyed
//!   `<row id>|<col id>|<field>`. The fields are `v` (the input, with a
//!   formula's `=`), `nf`, `st`, `bd`, `va` and `lk`. A field at its
//!   default is absent.
//! - `sizes:<key>`: `r|<row id>` and `c|<col id>` to px, absent at the
//!   default.
//! - `names`: the workbook's defined names, name to formula.
//!
//! Only inputs are replicated, never computed values. Every peer computes
//! values from the same inputs, and the determinism test
//! (`tests/determinism.rs`) guards that they agree.
//!
//! # How a local edit gets in
//!
//! Row and column inserts and deletes are read from the ops, because only
//! the op says which line is new. Everything else is a diff of the
//! workbook against the replica's mirror of the document, so no op can be
//! forgotten: values (including the formula rewrites a structural op makes
//! elsewhere), formats, sheets added, deleted, moved or renamed, and names.
//!
//! # Known limits, for Phase 3
//! - A remote change rebuilds the workbook from the document, so the local
//!   undo history no longer applies to it. Undo across a merge needs Loro's
//!   own undo manager or transformed inverses.
//! - Hidden rows, print areas, chart ranges and the like are positions, so
//!   a concurrent row insert can shift what they point at. They still
//!   converge, because both peers read the same value.
//! - A cell's style is one register: bold on one peer and italic on the
//!   other at the same time keeps one of the two.

use std::collections::{BTreeMap, HashMap, HashSet};

use loro::{ExportMode, LoroDoc, LoroList, LoroMap, LoroMovableList, LoroValue, ValueOrContainer};
use serde::{de::DeserializeOwned, Serialize};

use super::ops::{cell_content, resync, Axis, CellContent, Op};
use super::state::WorkbookState;
use crate::sheet::SheetModel;

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("the sheet types serialize")
}

fn unjson<T: DeserializeOwned>(text: &str) -> Option<T> {
    serde_json::from_str(text).ok()
}

fn string_of(value: Option<ValueOrContainer>) -> Option<String> {
    match value {
        Some(ValueOrContainer::Value(LoroValue::String(s))) => Some(s.to_string()),
        _ => None,
    }
}

fn strings(values: Vec<LoroValue>) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|v| match v {
            LoroValue::String(s) => Some(s.to_string()),
            _ => None,
        })
        .collect()
}

/// A cell with nothing set, as a new sheet has it.
fn blank_cell() -> CellContent {
    let s = SheetModel::new("", 1, 1, 0);
    CellContent {
        input: String::new(),
        format: s.formats[0][0].clone(),
        style: s.styles[0][0].clone(),
        border: s.borders[0][0].clone(),
        validation: None,
        lock: s.cell_protections[0][0].clone(),
        note: None,
    }
}

const FIELDS: [&str; 7] = ["v", "nf", "st", "bd", "va", "lk", "no"];

/// A cell's fields as stored: `None` where the field is at its default.
fn encode_cell(c: &CellContent, blank: &CellContent) -> [Option<String>; 7] {
    [
        (!c.input.is_empty()).then(|| c.input.clone()),
        (c.format != blank.format).then(|| json(&c.format)),
        (c.style != blank.style).then(|| json(&c.style)),
        (c.border != blank.border).then(|| json(&c.border)),
        c.validation.as_ref().map(json),
        (c.lock != blank.lock).then(|| json(&c.lock)),
        c.note.clone(),
    ]
}

fn decode_field(cell: &mut CellContent, field: &str, text: &str) {
    match field {
        "v" => cell.input = text.to_string(),
        "nf" => cell.format = unjson(text).unwrap_or_else(|| cell.format.clone()),
        "st" => cell.style = unjson(text).unwrap_or_else(|| cell.style.clone()),
        "bd" => cell.border = unjson(text).unwrap_or_else(|| cell.border.clone()),
        "va" => cell.validation = unjson(text),
        "lk" => cell.lock = unjson(text).unwrap_or_else(|| cell.lock.clone()),
        "no" => cell.note = Some(text.to_string()),
        _ => {}
    }
}

fn sorted<T: Ord + Copy>(set: &HashSet<T>) -> Vec<T> {
    let mut v: Vec<T> = set.iter().copied().collect();
    v.sort_unstable();
    v
}

/// The sheet-wide properties as `(key, encoded value)`, in a fixed order.
fn encode_props(s: &SheetModel) -> Vec<(&'static str, String)> {
    vec![
        ("p.sorted", json(&s.sorted_col)),
        ("p.filtered", json(&sorted(&s.hidden_rows))),
        ("p.hidden_rows", json(&sorted(&s.hidden_rows_manual))),
        ("p.hidden_cols", json(&sorted(&s.hidden_cols))),
        ("p.cond_rules", json(&s.cond_rules)),
        ("p.charts", json(&s.charts)),
        ("p.pivots", json(&s.pivot_tables)),
        ("p.protection", json(&s.protection)),
        ("p.print_area", json(&s.print_area)),
        ("p.page_setup", json(&s.page_setup)),
    ]
}

fn decode_prop(s: &mut SheetModel, key: &str, text: &str) {
    fn set<T: DeserializeOwned>(slot: &mut T, text: &str) {
        if let Some(v) = unjson(text) {
            *slot = v;
        }
    }
    fn set_of(slot: &mut HashSet<usize>, text: &str) {
        if let Some(v) = unjson::<Vec<usize>>(text) {
            *slot = v.into_iter().collect();
        }
    }
    match key {
        "p.sorted" => set(&mut s.sorted_col, text),
        "p.filtered" => set_of(&mut s.hidden_rows, text),
        "p.hidden_rows" => set_of(&mut s.hidden_rows_manual, text),
        "p.hidden_cols" => set_of(&mut s.hidden_cols, text),
        "p.cond_rules" => set(&mut s.cond_rules, text),
        "p.charts" => set(&mut s.charts, text),
        "p.pivots" => set(&mut s.pivot_tables, text),
        "p.protection" => set(&mut s.protection, text),
        "p.print_area" => set(&mut s.print_area, text),
        "p.page_setup" => set(&mut s.page_setup, text),
        _ => {}
    }
}

/// What this replica last read from or wrote to the document for one
/// sheet, so a local edit writes only what changed.
#[derive(Clone, Debug)]
struct SheetMirror {
    key: String,
    rows: Vec<String>,
    cols: Vec<String>,
    /// Cells not at their default, by `(row id, col id)`.
    cells: HashMap<(String, String), CellContent>,
    /// Row heights and column widths not at their default.
    sizes: HashMap<String, f64>,
    /// `name`, `frozen`, `merges` and the `p.*` properties, encoded.
    meta: BTreeMap<String, String>,
    /// The document has this sheet deleted but the workbook shows it (the
    /// last live sheet is never hidden); the next local edit revives it.
    deleted_in_doc: bool,
}

impl SheetMirror {
    fn new(key: String) -> Self {
        SheetMirror {
            key,
            rows: Vec::new(),
            cols: Vec::new(),
            cells: HashMap::new(),
            sizes: HashMap::new(),
            meta: BTreeMap::new(),
            deleted_in_doc: false,
        }
    }

    fn lines(&mut self, axis: Axis) -> &mut Vec<String> {
        match axis {
            Axis::Rows => &mut self.rows,
            Axis::Cols => &mut self.cols,
        }
    }
}

/// One peer's copy of a shared workbook.
pub struct Replica {
    doc: LoroDoc,
    peer: u64,
    counter: u64,
    /// The mirror of each sheet in the workbook, by local sheet id.
    live: HashMap<u32, SheetMirror>,
    /// Sheets deleted here, by the local id undo brings them back with.
    dead: HashMap<u32, SheetMirror>,
    names: BTreeMap<String, String>,
}

impl Replica {
    fn with_doc(doc: LoroDoc, peer: u64) -> Self {
        doc.set_peer_id(peer).expect("a valid peer id");
        Replica { doc, peer, counter: 0, live: HashMap::new(), dead: HashMap::new(), names: BTreeMap::new() }
    }

    /// Start a shared document from `state`, as peer `peer`.
    pub fn new(peer: u64, state: &WorkbookState) -> Self {
        let mut me = Self::with_doc(LoroDoc::new(), peer);
        me.record(state, &[]);
        me
    }

    /// A second peer on a copy of this document, with the workbook it
    /// shows.
    pub fn fork(&self, peer: u64) -> Result<(Replica, WorkbookState), String> {
        self.doc.commit();
        let mut other = Self::with_doc(self.doc.fork(), peer);
        let state = other.rebuild()?;
        Ok((other, state))
    }

    /// A peer that has only the history in `updates` (from
    /// [`Replica::export_all`]): the whole document replayed from its ops.
    pub fn from_updates(peer: u64, updates: &[u8]) -> Result<(Replica, WorkbookState), String> {
        let doc = LoroDoc::new();
        doc.import(updates).map_err(|e| e.to_string())?;
        let mut me = Self::with_doc(doc, peer);
        let state = me.rebuild()?;
        Ok((me, state))
    }

    /// The document's whole history, as updates.
    pub fn export_all(&self) -> Vec<u8> {
        self.doc.commit();
        self.doc.export(ExportMode::all_updates()).expect("export")
    }

    /// Take in what `other` has that this replica doesn't. The workbook
    /// then needs [`Replica::rebuild`].
    pub fn merge_from(&mut self, other: &Replica) -> Result<(), String> {
        other.doc.commit();
        let updates = other.doc.export(ExportMode::updates(&self.doc.oplog_vv())).map_err(|e| e.to_string())?;
        self.doc.import(&updates).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn fresh_id(&mut self) -> String {
        self.counter += 1;
        format!("{:x}.{:x}", self.peer, self.counter)
    }

    fn sheets_list(&self) -> LoroMovableList {
        self.doc.get_movable_list("sheets")
    }

    fn sheet_map(&self, key: &str) -> LoroMap {
        self.doc.get_map(format!("sheet:{key}"))
    }

    fn lines_list(&self, key: &str, axis: Axis) -> LoroList {
        match axis {
            Axis::Rows => self.doc.get_list(format!("rows:{key}")),
            Axis::Cols => self.doc.get_list(format!("cols:{key}")),
        }
    }

    // ---------------------------------------------------------------- local edits

    /// Record into the document an op group that has just been applied to
    /// `state`, as one change.
    pub fn record(&mut self, state: &WorkbookState, ops: &[Op]) {
        for op in ops {
            self.record_lines(op);
        }
        // Undoing a delete brings the sheet back under its old id; a new
        // sheet can get a deleted one's id too, and is still new.
        let revived: HashSet<u32> = ops
            .iter()
            .filter_map(|op| match op {
                Op::AddSheet { sheet_id, .. } => *sheet_id,
                _ => None,
            })
            .collect();
        self.reconcile_sheets(state, &revived);
        // Cell-level ops change only their own sheet. Anything structural
        // can rewrite formulas anywhere, so then every sheet is compared.
        let touched: Option<HashSet<u32>> = if ops.is_empty() {
            None
        } else {
            ops.iter()
            .map(|op| match op {
                Op::SetCell { sheet, .. }
                | Op::SetCells { sheet, .. }
                | Op::SetFormat { sheet, .. }
                | Op::SetStyle { sheet, .. }
                | Op::SetBorder { sheet, .. }
                | Op::SetValidation { sheet, .. }
                | Op::SetLocked { sheet, .. }
                | Op::Merge { sheet, .. }
                | Op::Unmerge { sheet, .. }
                | Op::Resize { sheet, .. }
                | Op::Freeze { sheet, .. }
                | Op::SetProp { sheet, .. } => Some(*sheet),
                _ => None,
            })
            .collect()
        };
        for index in 0..state.sheets.len() {
            let id = state.sheets[index].borrow().sheet_id;
            if touched.as_ref().is_none_or(|t| t.contains(&id)) {
                self.write_sheet(state, index);
            }
        }
        self.write_names(state);
        self.doc.commit();
    }

    /// A row or column insert or delete: new ids for new lines, and the
    /// deleted lines' ids and cells gone.
    fn record_lines(&mut self, op: &Op) {
        let (sheet, axis) = match op {
            Op::Insert { sheet, axis, .. } | Op::Delete { sheet, axis, .. } => (*sheet, *axis),
            _ => return,
        };
        let Some(mut mirror) = self.live.remove(&sheet) else { return };
        let list = self.lines_list(&mirror.key, axis);
        let listed = strings(list.to_vec());
        let position = |id: &String| listed.iter().position(|x| x == id);
        match op {
            Op::Insert { at, lines, .. } => {
                // Before the line now at `at`, or at the end.
                let pos = mirror.lines(axis).get(*at).and_then(position).unwrap_or(listed.len());
                let ids: Vec<String> = (0..lines.len()).map(|_| self.fresh_id()).collect();
                for (i, id) in ids.iter().enumerate() {
                    list.insert(pos + i, id.as_str()).expect("list insert");
                }
                mirror.lines(axis).splice(*at..*at, ids);
            }
            Op::Delete { at, count, .. } => {
                let gone: Vec<String> = mirror.lines(axis).drain(*at..*at + *count).collect();
                let cells = self.doc.get_map(format!("cells:{}", mirror.key));
                let sizes = self.doc.get_map(format!("sizes:{}", mirror.key));
                let prefix = if axis == Axis::Rows { "r" } else { "c" };
                for id in &gone {
                    let listed = strings(list.to_vec());
                    if let Some(pos) = listed.iter().position(|x| x == id) {
                        list.delete(pos, 1).expect("list delete");
                    }
                    let size_key = format!("{prefix}|{id}");
                    if mirror.sizes.remove(&size_key).is_some() {
                        sizes.delete(&size_key).expect("map delete");
                    }
                    let keys: Vec<(String, String)> = mirror
                        .cells
                        .keys()
                        .filter(|(r, c)| if axis == Axis::Rows { r == id } else { c == id })
                        .cloned()
                        .collect();
                    for key in keys {
                        mirror.cells.remove(&key);
                        for field in FIELDS {
                            cells.delete(&format!("{}|{}|{field}", key.0, key.1)).expect("map delete");
                        }
                    }
                }
            }
            _ => {}
        }
        self.live.insert(sheet, mirror);
    }

    /// Sheets added, deleted (a tombstone) or brought back, and the order.
    fn reconcile_sheets(&mut self, state: &WorkbookState, revived: &HashSet<u32>) {
        let local: Vec<u32> = state.sheets.iter().map(|s| s.borrow().sheet_id).collect();
        let gone: Vec<u32> = self.live.keys().filter(|id| !local.contains(id)).copied().collect();
        for id in gone {
            let mirror = self.live.remove(&id).expect("listed");
            self.sheet_map(&mirror.key).insert("deleted", true).expect("map insert");
            self.dead.insert(id, mirror);
        }
        let list = self.sheets_list();
        for (index, id) in local.iter().enumerate() {
            if self.live.contains_key(id) {
                continue;
            }
            if let Some(mirror) = self.dead.remove(id) {
                if revived.contains(id) {
                    self.sheet_map(&mirror.key).insert("deleted", false).expect("map insert");
                    self.live.insert(*id, mirror);
                    continue;
                }
            }
            // A new sheet: its key after the sheet before it (ordered below).
            let key = self.fresh_id();
            let listed = strings(list.to_vec());
            let pos = index
                .checked_sub(1)
                .and_then(|i| self.live.get(&local[i]))
                .and_then(|m| listed.iter().position(|k| *k == m.key))
                .map_or(0, |p| p + 1);
            list.insert(pos, key.as_str()).expect("list insert");
            let (rows, cols) = {
                let s = state.sheets[index].borrow();
                (s.rows, s.cols)
            };
            let mut mirror = SheetMirror::new(key.clone());
            for (axis, n) in [(Axis::Rows, rows), (Axis::Cols, cols)] {
                let lines = self.lines_list(&key, axis);
                for i in 0..n {
                    let line = self.fresh_id();
                    lines.insert(i, line.as_str()).expect("list insert");
                    mirror.lines(axis).push(line);
                }
            }
            self.live.insert(*id, mirror);
        }
        // The order: each sheet after the one before it.
        for pair in local.windows(2) {
            let listed = strings(list.to_vec());
            let at = |id: &u32| listed.iter().position(|k| *k == self.live[id].key).expect("listed sheet");
            let (before, this) = (at(&pair[0]), at(&pair[1]));
            if this < before {
                list.mov(this, before).expect("list move");
            }
        }
    }

    /// Write whatever differs between sheet `index` and its mirror.
    fn write_sheet(&mut self, state: &WorkbookState, index: usize) {
        let id = state.sheets[index].borrow().sheet_id;
        let Some(mut mirror) = self.live.remove(&id) else { return };
        let blank = blank_cell();
        let key = mirror.key.clone();
        let cells = self.doc.get_map(format!("cells:{key}"));
        let sizes = self.doc.get_map(format!("sizes:{key}"));
        let sheet_map = self.sheet_map(&key);
        if mirror.deleted_in_doc {
            sheet_map.insert("deleted", false).expect("map insert");
            mirror.deleted_in_doc = false;
        }
        let s = state.sheets[index].borrow();
        debug_assert_eq!((s.rows, s.cols), (mirror.rows.len(), mirror.cols.len()), "the mirror follows the sheet's lines");
        let default_size = SheetModel::new("", 1, 1, 0);
        for (r, rid) in mirror.rows.iter().enumerate().take(s.rows) {
            for (c, cid) in mirror.cols.iter().enumerate().take(s.cols) {
                let now = cell_content(state, index, r, c);
                let at = (rid.clone(), cid.clone());
                let was = mirror.cells.get(&at).unwrap_or(&blank);
                if now == *was {
                    continue;
                }
                let (old, new) = (encode_cell(was, &blank), encode_cell(&now, &blank));
                for (field, (o, n)) in FIELDS.iter().zip(old.iter().zip(new.iter())) {
                    if o == n {
                        continue;
                    }
                    let k = format!("{rid}|{cid}|{field}");
                    match n {
                        Some(text) => cells.insert(&k, text.as_str()).expect("map insert"),
                        None => cells.delete(&k).expect("map delete"),
                    }
                }
                if now == blank {
                    mirror.cells.remove(&at);
                } else {
                    mirror.cells.insert(at, now);
                }
            }
        }
        let lines = [("r", &mirror.rows, &s.row_heights, default_size.row_heights[0]), ("c", &mirror.cols, &s.col_widths, default_size.col_widths[0])];
        for (prefix, ids, local, default) in lines {
            for (id, size) in ids.iter().zip(local.iter()) {
                let k = format!("{prefix}|{id}");
                let was = mirror.sizes.get(&k).copied().unwrap_or(default);
                if (was - size).abs() > f64::EPSILON {
                    if (size - default).abs() > f64::EPSILON {
                        sizes.insert(&k, *size).expect("map insert");
                        mirror.sizes.insert(k, *size);
                    } else {
                        sizes.delete(&k).expect("map delete");
                        mirror.sizes.remove(&k);
                    }
                }
            }
        }
        let merges: Vec<(String, String, usize, usize)> = s
            .merges
            .iter()
            .filter_map(|&(r, c, rs, cs)| Some((mirror.rows.get(r)?.clone(), mirror.cols.get(c)?.clone(), rs, cs)))
            .collect();
        let mut meta = vec![("name", s.name.clone()), ("frozen", json(&(s.frozen_rows, s.frozen_cols))), ("merges", json(&merges))];
        meta.extend(encode_props(&s));
        for (k, text) in meta {
            if mirror.meta.get(k) != Some(&text) {
                sheet_map.insert(k, text.as_str()).expect("map insert");
                mirror.meta.insert(k.to_string(), text);
            }
        }
        drop(s);
        self.live.insert(id, mirror);
    }

    fn write_names(&mut self, state: &WorkbookState) {
        let now: BTreeMap<String, String> = state
            .engine
            .model
            .workbook
            .defined_names
            .iter()
            .filter(|n| n.sheet_id.is_none())
            .map(|n| (n.name.clone(), n.formula.clone()))
            .collect();
        let map = self.doc.get_map("names");
        for (name, formula) in &now {
            if self.names.get(name) != Some(formula) {
                map.insert(name, formula.as_str()).expect("map insert");
            }
        }
        for name in self.names.keys().filter(|n| !now.contains_key(*n)) {
            map.delete(name).expect("map delete");
        }
        self.names = now;
    }

    // ---------------------------------------------------------------- the document's workbook

    /// The workbook the document holds, built from nothing but the
    /// document. It becomes this replica's workbook: the mirror is reset
    /// to it.
    pub fn rebuild(&mut self) -> Result<WorkbookState, String> {
        self.doc.commit();
        let view = read_doc(&self.doc);
        let state = build(&view)?;
        self.live.clear();
        self.dead.clear();
        for (index, sheet) in view.sheets.into_iter().enumerate() {
            let id = state.sheets[index].borrow().sheet_id;
            self.live.insert(id, sheet.mirror);
        }
        self.names = view.names;
        Ok(state)
    }
}

/// One live sheet as the document has it.
struct SheetView {
    mirror: SheetMirror,
}

struct DocView {
    sheets: Vec<SheetView>,
    names: BTreeMap<String, String>,
}

fn read_doc(doc: &LoroDoc) -> DocView {
    let mut keys: Vec<(String, bool)> = Vec::new();
    for key in strings(doc.get_movable_list("sheets").to_vec()) {
        if keys.iter().any(|(k, _)| *k == key) {
            continue;
        }
        let deleted = matches!(
            doc.get_map(format!("sheet:{key}")).get("deleted"),
            Some(ValueOrContainer::Value(LoroValue::Bool(true)))
        );
        keys.push((key, deleted));
    }
    let mut live: Vec<(String, bool)> = keys.iter().filter(|(_, d)| !*d).cloned().collect();
    // A workbook keeps one sheet: if every sheet is deleted (two peers
    // each deleted a different one), the first stays, on every peer alike.
    if live.is_empty() {
        if let Some((key, _)) = keys.first() {
            live.push((key.clone(), true));
        }
    }
    let blank = blank_cell();
    let sheets = live
        .into_iter()
        .map(|(key, deleted_in_doc)| {
            let mut mirror = SheetMirror::new(key.clone());
            mirror.deleted_in_doc = deleted_in_doc;
            for (axis, prefix) in [(Axis::Rows, "rows"), (Axis::Cols, "cols")] {
                let mut seen = HashSet::new();
                let mut ids: Vec<String> = strings(doc.get_list(format!("{prefix}:{key}")).to_vec())
                    .into_iter()
                    .filter(|id| seen.insert(id.clone()))
                    .collect();
                // A sheet keeps one line: if concurrent deletes took them
                // all, every peer shows the same stand-in line.
                if ids.is_empty() {
                    ids.push(format!("{key}.{prefix}0"));
                }
                *mirror.lines(axis) = ids;
            }
            let (row_set, col_set): (HashSet<&String>, HashSet<&String>) = (mirror.rows.iter().collect(), mirror.cols.iter().collect());
            let mut cells: HashMap<(String, String), CellContent> = HashMap::new();
            doc.get_map(format!("cells:{key}")).for_each(|k, v| {
                let mut parts = k.splitn(3, '|');
                let (Some(r), Some(c), Some(field)) = (parts.next(), parts.next(), parts.next()) else { return };
                let (r, c) = (r.to_string(), c.to_string());
                if !row_set.contains(&r) || !col_set.contains(&c) {
                    return;
                }
                if let ValueOrContainer::Value(LoroValue::String(text)) = v {
                    decode_field(cells.entry((r, c)).or_insert_with(|| blank.clone()), field, text.as_ref());
                }
            });
            cells.retain(|_, c| *c != blank);
            mirror.cells = cells;
            let mut sizes = HashMap::new();
            doc.get_map(format!("sizes:{key}")).for_each(|k, v| {
                if let ValueOrContainer::Value(LoroValue::Double(px)) = v {
                    sizes.insert(k.to_string(), px);
                }
            });
            mirror.sizes = sizes;
            let map = doc.get_map(format!("sheet:{key}"));
            map.for_each(|k, v| {
                if k == "name" || k == "frozen" || k == "merges" || k.starts_with("p.") {
                    if let ValueOrContainer::Value(LoroValue::String(text)) = v {
                        mirror.meta.insert(k.to_string(), text.to_string());
                    }
                }
            });
            SheetView { mirror }
        })
        .collect();
    let mut names = BTreeMap::new();
    doc.get_map("names").for_each(|k, v| {
        if let Some(text) = string_of(Some(v)) {
            names.insert(k.to_string(), text);
        }
    });
    DocView { sheets, names }
}

/// A workbook from a document view, the same on every peer.
fn build(view: &DocView) -> Result<WorkbookState, String> {
    let first = view.sheets.first().ok_or("the document has no sheets")?;
    let mut state = WorkbookState::new(first.mirror.rows.len(), first.mirror.cols.len())?;
    // Names as the document has them, made unique in order (two peers can
    // add a sheet of the same name at once).
    let mut taken: HashSet<String> = HashSet::new();
    for (index, sheet) in view.sheets.iter().enumerate() {
        let wanted = sheet.mirror.meta.get("name").cloned().unwrap_or_else(|| format!("Sheet{}", index + 1));
        let mut name = wanted.clone();
        let mut n = 2;
        while taken.contains(&name.to_lowercase()) {
            name = format!("{wanted} {n}");
            n += 1;
        }
        taken.insert(name.to_lowercase());
        let (rows, cols) = (sheet.mirror.rows.len(), sheet.mirror.cols.len());
        if index == 0 {
            state.rename_sheet(0, &name)?;
        } else {
            state.add_sheet(name, rows, cols)?;
        }
    }
    state.grow_engine();
    for (name, formula) in &view.names {
        state.engine.set_defined_name(name, Some(formula))?;
    }
    let size_defaults = SheetModel::new("", 1, 1, 0);
    for (index, sheet) in view.sheets.iter().enumerate() {
        let m = &sheet.mirror;
        let row_at: HashMap<&String, usize> = m.rows.iter().enumerate().map(|(i, id)| (id, i)).collect();
        let col_at: HashMap<&String, usize> = m.cols.iter().enumerate().map(|(i, id)| (id, i)).collect();
        let mut placed: Vec<(usize, usize, &CellContent)> =
            m.cells.iter().filter_map(|((r, c), cell)| Some((*row_at.get(r)?, *col_at.get(c)?, cell))).collect();
        placed.sort_by_key(|(r, c, _)| (*r, *c));
        state.set_cell_inputs_on_sheet(
            index,
            placed.iter().filter(|(_, _, cell)| !cell.input.is_empty()).map(|(r, c, cell)| (*r, *c, cell.input.as_str())),
        );
        let mut s = state.sheets[index].borrow_mut();
        for (r, c, cell) in &placed {
            s.formats[*r][*c] = cell.format.clone();
            s.styles[*r][*c] = cell.style.clone();
            s.borders[*r][*c] = cell.border.clone();
            s.validations[*r][*c] = cell.validation.clone();
            s.cell_protections[*r][*c] = cell.lock.clone();
            s.notes[*r][*c] = cell.note.clone();
        }
        for (r, id) in m.rows.iter().enumerate() {
            s.row_heights[r] = m.sizes.get(&format!("r|{id}")).copied().unwrap_or(size_defaults.row_heights[0]);
        }
        for (c, id) in m.cols.iter().enumerate() {
            s.col_widths[c] = m.sizes.get(&format!("c|{id}")).copied().unwrap_or(size_defaults.col_widths[0]);
        }
        if let Some((rows, cols)) = m.meta.get("frozen").and_then(|t| unjson::<(usize, usize)>(t)) {
            (s.frozen_rows, s.frozen_cols) = (rows.min(s.rows), cols.min(s.cols));
        }
        if let Some(merges) = m.meta.get("merges").and_then(|t| unjson::<Vec<(String, String, usize, usize)>>(t)) {
            s.merges = merges
                .into_iter()
                .filter_map(|(r, c, rs, cs)| Some((*row_at.get(&r)?, *col_at.get(&c)?, rs, cs)))
                .collect();
        }
        for (k, text) in m.meta.iter().filter(|(k, _)| k.starts_with("p.")) {
            decode_prop(&mut s, k, text);
        }
    }
    state.engine.evaluate();
    for index in 0..state.sheets.len() {
        resync(&mut state, index);
    }
    Ok(state)
}
