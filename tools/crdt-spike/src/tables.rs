//! Tables workload: a recorded editing session over a 1000x26 sheet, the
//! plain replay into `tables_core::SheetModel`, and the merge oracle.
//!
//! Representation (the same in every CRDT), following RFC-0001's "map keyed
//! by cell" but with one change the RFC does not spell out: a cell is keyed
//! by a *stable row id*, not by its row number. Rows live in a sequence CRDT
//! of row ids, so a concurrent row insert on one peer does not silently
//! re-address every cell edit below it on the other. Each cell field is its
//! own last-writer-wins register (`"<row id>.<col>.<field>"`), so a
//! concurrent "make it bold" and "type a value" in the same cell both
//! survive, and there are no per-cell nested containers whose concurrent
//! creation would conflict (see `probe_nested` in main.rs).

use std::collections::{BTreeMap, HashMap, HashSet};

use suite_common_core::format::{NumberFormat, NumberFormatKind};
use tables_core::SheetModel;

use crate::rng::Rng;

pub const COLS: usize = 26;
pub const BASE_ROWS: usize = 1000;

/// Cell fields: value (formulas are text starting with '='), number format,
/// and a small per-cell style map mirroring `CellStyle` (tables-cell-style
/// branch; not on main when this spike was run).
pub const STYLE_FIELDS: [&str; 6] = ["b", "i", "u", "fill", "color", "ha"];

#[derive(Clone, Debug)]
pub enum TOp {
    Set { id: String, col: u8, field: &'static str, value: String },
    Clear { id: String, col: u8, field: &'static str },
    InsertRow { at: usize, id: String },
    /// `keys` are the cell keys this peer knows the row holds, so a backend
    /// can delete them along with the row.
    DeleteRow { at: usize, id: String, keys: Vec<String> },
}

pub fn cell_key(id: &str, col: u8, field: &str) -> String {
    format!("{id}.{col}.{field}")
}

pub fn row_of_key(key: &str) -> &str {
    key.split('.').next().unwrap_or("")
}

/// The replicated state in a library-neutral form: row order plus every
/// cell field of a live row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canon {
    pub rows: Vec<String>,
    pub cells: BTreeMap<String, String>,
}

impl Canon {
    /// Build from a raw read of a CRDT: cells of rows that no longer exist
    /// (a row deleted concurrently with an edit to it) are dropped.
    pub fn from_raw(rows: Vec<String>, raw: impl IntoIterator<Item = (String, String)>) -> Canon {
        let live: HashSet<&str> = rows.iter().map(String::as_str).collect();
        let cells = raw.into_iter().filter(|(k, _)| live.contains(row_of_key(k))).collect();
        Canon { rows, cells }
    }

    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |s: &str| {
            for b in s.bytes().chain(std::iter::once(0xff)) {
                h ^= b as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
            }
        };
        for r in &self.rows {
            eat(r);
        }
        for (k, v) in &self.cells {
            eat(k);
            eat(v);
        }
        h
    }

    /// Bytes of keys and values: a floor for any encoding of this state.
    pub fn payload_bytes(&self) -> usize {
        self.rows.iter().map(String::len).sum::<usize>()
            + self.cells.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>()
    }
}

/// One peer's view while generating: what it would see locally.
#[derive(Clone)]
pub struct View {
    pub rows: Vec<String>,
    pub cells: HashMap<String, BTreeMap<(u8, &'static str), String>>,
}

impl View {
    pub fn base() -> View {
        View { rows: (0..BASE_ROWS).map(|i| format!("r{i:x}")).collect(), cells: HashMap::new() }
    }

    pub fn apply(&mut self, op: &TOp) {
        match op {
            TOp::Set { id, col, field, value } => {
                self.cells.entry(id.clone()).or_default().insert((*col, *field), value.clone());
            }
            TOp::Clear { id, col, field } => {
                if let Some(c) = self.cells.get_mut(id) {
                    c.remove(&(*col, *field));
                }
            }
            TOp::InsertRow { at, id } => self.rows.insert(*at, id.clone()),
            TOp::DeleteRow { at, id, .. } => {
                assert_eq!(&self.rows[*at], id);
                self.rows.remove(*at);
                self.cells.remove(id);
            }
        }
    }

    pub fn canon(&self) -> Canon {
        let mut cells = BTreeMap::new();
        for id in &self.rows {
            if let Some(c) = self.cells.get(id) {
                for ((col, field), v) in c {
                    cells.insert(cell_key(id, *col, field), v.clone());
                }
            }
        }
        Canon { rows: self.rows.clone(), cells }
    }
}

pub struct Session {
    /// Each inner Vec is one user action, applied as one transaction/commit.
    pub actions: Vec<Vec<TOp>>,
    pub final_view: View,
}

impl Session {
    pub fn op_count(&self) -> usize {
        self.actions.iter().map(Vec::len).sum()
    }
}

const WORDS: [&str; 16] = [
    "Revenue", "Costs", "Q1", "Q2", "Q3", "Q4", "Total", "North", "South", "East", "West",
    "Widget", "Gadget", "pending", "done", "Notes on the forecast",
];
const NUMBER_FORMATS: [&str; 8] = ["num:0", "num:2", "cur:2", "pct:0", "pct:1", "date", "sci:2", "text"];
const COLOURS: [&str; 8] = ["C00000", "FFC7CE", "00B050", "FFEB9C", "4472C4", "D9E1F2", "000000", "7F7F7F"];

fn col_letter(c: usize) -> char {
    (b'A' + c as u8) as char
}

fn gen_value(rng: &mut Rng, rows: usize) -> String {
    let r = rng.between(1, rows);
    let c = col_letter(rng.below(COLS));
    match rng.below(20) {
        0..=9 => {
            if rng.chance(0.5) {
                format!("{}", rng.below(100_000))
            } else {
                format!("{}.{:02}", rng.below(10_000), rng.below(100))
            }
        }
        10..=14 => rng.pick(&WORDS).to_string(),
        15 => format!("=SUM({c}{r}:{c}{})", r + rng.between(1, 30)),
        16 => format!("={c}{r}*{}", rng.between(2, 20) as f64 / 10.0),
        17 => format!("=IF({c}{r}>100,\"high\",\"low\")"),
        18 => format!("=AVERAGE({c}{r}:{c}{})", r + rng.between(1, 30)),
        _ => format!("=ROUND({c}{r}*1.2,2)"),
    }
}

fn gen_style_value(rng: &mut Rng, field: &str) -> String {
    match field {
        "b" | "i" | "u" => "1".to_string(),
        "fill" | "color" => rng.pick(&COLOURS).to_string(),
        _ => rng.pick(&["left", "center", "right"]).to_string(),
    }
}

/// Generate a recorded session for one peer, starting from `base`.
/// Row ids the peer creates are prefixed with `peer`, so ids are unique
/// across peers without coordination.
pub fn gen_session(seed: u64, peer: char, base: &View, target_ops: usize) -> Session {
    let mut rng = Rng::new(seed);
    let mut view = base.clone();
    let mut actions = Vec::new();
    let mut ops = 0usize;
    let mut next_row = 0usize;
    let (mut cur_r, mut cur_c) = (0usize, 0usize);
    while ops < target_ops {
        let rows = view.rows.len();
        if rng.chance(0.15) {
            cur_r = rng.below(rows);
            cur_c = rng.below(COLS);
        }
        cur_r = cur_r.min(rows - 1);
        let mut action = Vec::new();
        let roll = rng.below(100);
        match roll {
            // Data entry: type a run of values across or down.
            0..=49 => {
                let len = rng.between(1, 12);
                let down = rng.chance(0.5);
                for k in 0..len {
                    let (r, c) = if down { (cur_r + k, cur_c) } else { (cur_r, cur_c + k) };
                    if r >= rows || c >= COLS {
                        break;
                    }
                    let value = gen_value(&mut rng, rows);
                    action.push(TOp::Set { id: view.rows[r].clone(), col: c as u8, field: "v", value });
                }
                if down {
                    cur_r = (cur_r + len).min(rows - 1);
                } else {
                    cur_r = (cur_r + 1).min(rows - 1);
                }
            }
            // Style a selected range (bold a header, fill a block, ...).
            50..=64 => {
                let field = *rng.pick(&STYLE_FIELDS);
                let (h, w) = (rng.between(1, 6), rng.between(1, 5));
                let clear = rng.chance(0.2);
                let value = gen_style_value(&mut rng, field);
                for r in cur_r..(cur_r + h).min(rows) {
                    for c in cur_c..(cur_c + w).min(COLS) {
                        let id = view.rows[r].clone();
                        let present = view.cells.get(&id).is_some_and(|m| m.contains_key(&(c as u8, field)));
                        if clear {
                            if present {
                                action.push(TOp::Clear { id, col: c as u8, field });
                            }
                        } else {
                            action.push(TOp::Set { id, col: c as u8, field, value: value.clone() });
                        }
                    }
                }
            }
            // Number format down a column segment.
            65..=72 => {
                let nf = rng.pick(&NUMBER_FORMATS).to_string();
                for r in cur_r..(cur_r + rng.between(1, 20)).min(rows) {
                    action.push(TOp::Set { id: view.rows[r].clone(), col: cur_c as u8, field: "nf", value: nf.clone() });
                }
            }
            // Overwrite or clear a random existing value.
            73..=84 => {
                let r = rng.below(rows);
                let id = view.rows[r].clone();
                let cols: Vec<u8> = view
                    .cells
                    .get(&id)
                    .map(|m| m.keys().filter(|(_, f)| *f == "v").map(|(c, _)| *c).collect())
                    .unwrap_or_default();
                if let Some(&c) = cols.get(rng.below(cols.len().max(1))) {
                    if rng.chance(0.7) {
                        let value = gen_value(&mut rng, rows);
                        action.push(TOp::Set { id, col: c, field: "v", value });
                    } else {
                        action.push(TOp::Clear { id, col: c, field: "v" });
                    }
                }
            }
            // Insert 1-3 rows.
            85..=91 => {
                let at = rng.below(rows + 1);
                for k in 0..rng.between(1, 3) {
                    next_row += 1;
                    action.push(TOp::InsertRow { at: at + k, id: format!("{peer}{next_row:x}") });
                }
            }
            // Delete 1-2 rows.
            _ => {
                if rows > BASE_ROWS / 2 {
                    let at = rng.below(rows - 2);
                    for _ in 0..rng.between(1, 2) {
                        let id = view.rows[at].clone();
                        let keys = view
                            .cells
                            .get(&id)
                            .map(|m| m.keys().map(|(c, f)| cell_key(&id, *c, f)).collect())
                            .unwrap_or_default();
                        let op = TOp::DeleteRow { at, id, keys };
                        view.apply(&op);
                        action.push(op);
                    }
                    // already applied to the view above (needed for the second row)
                    ops += action.len();
                    actions.push(action);
                    continue;
                }
            }
        }
        for op in &action {
            view.apply(op);
        }
        ops += action.len();
        if !action.is_empty() {
            actions.push(action);
        }
    }
    Session { actions, final_view: view }
}

pub fn nf_parse(code: &str) -> NumberFormat {
    let arg = |s: &str| s.split(':').nth(1).and_then(|d| d.parse().ok()).unwrap_or(0);
    let kind = match code.split(':').next().unwrap_or("") {
        "num" => NumberFormatKind::Number(arg(code)),
        "cur" => NumberFormatKind::Currency("$".into(), arg(code)),
        "pct" => NumberFormatKind::Percent(arg(code)),
        "date" => NumberFormatKind::Date("%Y-%m-%d".into()),
        "sci" => NumberFormatKind::Scientific(arg(code)),
        "text" => NumberFormatKind::Text,
        _ => NumberFormatKind::General,
    };
    NumberFormat::new(kind)
}

pub fn nf_encode(nf: &NumberFormat) -> Option<String> {
    Some(match &nf.kind {
        NumberFormatKind::General => return None,
        NumberFormatKind::Number(d) => format!("num:{d}"),
        NumberFormatKind::Currency(_, d) => format!("cur:{d}"),
        NumberFormatKind::Percent(d) => format!("pct:{d}"),
        NumberFormatKind::Date(_) => "date".into(),
        NumberFormatKind::Scientific(d) => format!("sci:{d}"),
        NumberFormatKind::Text => "text".into(),
        other => panic!("unexpected format {other:?}"),
    })
}

/// The plain, non-CRDT replay: apply the session to tables-core's real
/// `SheetModel` (values, formula flags, number formats, row insert/delete),
/// with the style map kept in a parallel grid because `CellStyle` is not on
/// main yet. Every CRDT's final state must equal this.
pub fn replay_sheetmodel(base: &View, actions: &[Vec<TOp>]) -> (Canon, SheetModel) {
    let n = base.rows.len();
    let mut sheet = SheetModel::new("Sheet1", n, COLS, 1);
    let mut ids = base.rows.clone();
    let mut style: Vec<Vec<BTreeMap<&'static str, String>>> = vec![vec![BTreeMap::new(); COLS]; n];
    let pos = |ids: &[String], id: &str| ids.iter().position(|x| x == id).expect("row id");
    for op in actions.iter().flatten() {
        match op {
            TOp::Set { id, col, field, value } => {
                let (r, c) = (pos(&ids, id), *col as usize);
                match *field {
                    "v" => {
                        *sheet.cell_mut(r, c) = value.clone();
                        sheet.formulas[r][c] = value.starts_with('=');
                    }
                    "nf" => sheet.formats[r][c] = nf_parse(value),
                    f => {
                        style[r][c].insert(f, value.clone());
                    }
                }
            }
            TOp::Clear { id, col, field } => {
                let (r, c) = (pos(&ids, id), *col as usize);
                match *field {
                    "v" => {
                        sheet.cell_mut(r, c).clear();
                        sheet.formulas[r][c] = false;
                    }
                    "nf" => sheet.formats[r][c] = NumberFormat::default(),
                    f => {
                        style[r][c].remove(f);
                    }
                }
            }
            TOp::InsertRow { at, id } => {
                sheet.insert_rows(*at, 1);
                ids.insert(*at, id.clone());
                style.insert(*at, vec![BTreeMap::new(); COLS]);
            }
            TOp::DeleteRow { at, id, .. } => {
                assert_eq!(&ids[*at], id);
                sheet.delete_rows(*at, 1);
                ids.remove(*at);
                style.remove(*at);
            }
        }
    }
    assert_eq!(sheet.rows, ids.len());
    let mut cells = BTreeMap::new();
    for (r, id) in ids.iter().enumerate() {
        for (c, cell_style) in style[r].iter().enumerate() {
            let v = sheet.cell(r, c);
            if !v.is_empty() {
                assert_eq!(sheet.is_formula(r, c), v.starts_with('='));
                cells.insert(cell_key(id, c as u8, "v"), v.to_string());
            }
            if let Some(code) = nf_encode(&sheet.formats[r][c]) {
                cells.insert(cell_key(id, c as u8, "nf"), code);
            }
            for (f, v) in cell_style {
                cells.insert(cell_key(id, c as u8, f), v.clone());
            }
        }
    }
    (Canon { rows: ids, cells }, sheet)
}

/// What a correct merge of two concurrent sessions must look like, whatever
/// the library's tie-breaking.
#[derive(Default, Debug)]
pub struct MergeCheck {
    pub violations: Vec<String>,
    pub conflicts: usize,
    pub a_wins: usize,
    pub b_wins: usize,
    pub orphaned_edits: usize,
}

fn last_writes(s: &Session) -> (HashMap<String, Option<String>>, HashSet<String>, HashSet<String>) {
    let mut w = HashMap::new();
    let (mut ins, mut del) = (HashSet::new(), HashSet::new());
    for op in s.actions.iter().flatten() {
        match op {
            TOp::Set { id, col, field, value } => {
                w.insert(cell_key(id, *col, field), Some(value.clone()));
            }
            TOp::Clear { id, col, field } => {
                w.insert(cell_key(id, *col, field), None);
            }
            TOp::InsertRow { id, .. } => {
                ins.insert(id.clone());
            }
            TOp::DeleteRow { id, .. } => {
                del.insert(id.clone());
            }
        }
    }
    (w, ins, del)
}

pub fn check_merge(base: &View, a: &Session, b: &Session, merged: &Canon) -> MergeCheck {
    let mut out = MergeCheck::default();
    let (wa, ia, da) = last_writes(a);
    let (wb, ib, db) = last_writes(b);
    // Row set.
    let expected: HashSet<&String> = base
        .rows
        .iter()
        .chain(ia.iter())
        .chain(ib.iter())
        .filter(|r| !da.contains(*r) && !db.contains(*r))
        .collect();
    let got: HashSet<&String> = merged.rows.iter().collect();
    if got.len() != merged.rows.len() {
        out.violations.push(format!("duplicate rows: {}", merged.rows.len() - got.len()));
    }
    if got != expected {
        out.violations.push(format!(
            "row set differs: {} missing, {} unexpected",
            expected.difference(&got).count(),
            got.difference(&expected).count()
        ));
    }
    // Each peer's own row order survives, restricted to rows still alive.
    for (name, s) in [("A", a), ("B", b)] {
        let mine: Vec<&String> = s.final_view.rows.iter().filter(|r| got.contains(r)).collect();
        let theirs: HashSet<&String> = s.final_view.rows.iter().collect();
        let proj: Vec<&String> = merged.rows.iter().filter(|r| theirs.contains(r)).collect();
        if mine != proj {
            out.violations.push(format!("peer {name}'s row order not preserved"));
        }
    }
    // Cells.
    let keys: HashSet<&String> = wa.keys().chain(wb.keys()).chain(merged.cells.keys()).collect();
    for k in keys {
        let row = row_of_key(k);
        if !got.contains(&row.to_string()) {
            // A write by one peer to a row only the *other* peer deleted.
            if (wa.contains_key(k) && db.contains(row) && !da.contains(row))
                || (wb.contains_key(k) && da.contains(row) && !db.contains(row))
            {
                out.orphaned_edits += 1;
            }
            continue;
        }
        let m = merged.cells.get(k);
        match (wa.get(k), wb.get(k)) {
            (None, None) => out.violations.push(format!("{k}: untouched but present")),
            (Some(x), None) | (None, Some(x)) => {
                if m != x.as_ref() {
                    out.violations.push(format!("{k}: lost a one-sided write"));
                }
            }
            (Some(x), Some(y)) if x == y => {
                if m != x.as_ref() {
                    out.violations.push(format!("{k}: lost an agreed write"));
                }
            }
            (Some(x), Some(y)) => {
                out.conflicts += 1;
                if m == x.as_ref() {
                    out.a_wins += 1;
                } else if m == y.as_ref() {
                    out.b_wins += 1;
                } else {
                    out.violations.push(format!("{k}: conflict resolved to neither side"));
                }
            }
        }
    }
    out
}
