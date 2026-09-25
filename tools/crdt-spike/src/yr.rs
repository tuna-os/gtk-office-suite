//! yrs (Yjs) backends.

use std::collections::BTreeMap;
use std::sync::Arc;

use yrs::types::text::Diff;
use yrs::types::Attrs;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Array, ArrayRef, Doc, GetString, Map, MapPrelim, MapRef, Out, ReadTxn, StateVector, Text, TextRef,
    Transact, Update,
};

use crate::backend::{DecksDoc, TablesDoc};
use crate::decks::{read_parent_pointers, DOp, DeckRead, FlatNode, Val};
use crate::tables::{cell_key, Canon, TOp, View};

fn string_of(v: &Out) -> Option<String> {
    match v {
        Out::Any(Any::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn full_update(doc: &Doc) -> Vec<u8> {
    doc.transact().encode_state_as_update_v1(&StateVector::default())
}

fn doc_from(peer: u64, update: &[u8]) -> Doc {
    let doc = Doc::with_client_id(peer);
    doc.transact_mut().apply_update(Update::decode_v1(update).unwrap()).unwrap();
    doc
}

fn encodings(doc: &Doc) -> Vec<(&'static str, Vec<u8>)> {
    let txn = doc.transact();
    vec![
        ("update v1 (state + delete set)", txn.encode_state_as_update_v1(&StateVector::default())),
        ("update v2 (state + delete set)", txn.encode_state_as_update_v2(&StateVector::default())),
    ]
}

fn merge(me: &Doc, other: &Doc) -> usize {
    let sv = me.transact().state_vector();
    let bytes = other.transact().encode_diff_v1(&sv);
    me.transact_mut().apply_update(Update::decode_v1(&bytes).unwrap()).unwrap();
    bytes.len()
}

// ---------------------------------------------------------------- Tables

pub struct YTables {
    doc: Doc,
    rows: ArrayRef,
    cells: MapRef,
}

impl YTables {
    fn wrap(doc: Doc) -> Self {
        let rows = doc.get_or_insert_array("rows");
        let cells = doc.get_or_insert_map("cells");
        YTables { doc, rows, cells }
    }
}

impl TablesDoc for YTables {
    fn base(base: &View) -> Self {
        let me = Self::wrap(Doc::with_client_id(1000));
        {
            let mut txn = me.doc.transact_mut();
            for (i, id) in base.rows.iter().enumerate() {
                me.rows.insert(&mut txn, i as u32, id.as_str());
            }
        }
        me
    }

    fn fork(&mut self, peer: u64) -> Self {
        Self::wrap(doc_from(peer, &full_update(&self.doc)))
    }

    fn apply(&mut self, action: &[TOp]) {
        let mut txn = self.doc.transact_mut();
        for op in action {
            match op {
                TOp::Set { id, col, field, value } => {
                    self.cells.insert(&mut txn, cell_key(id, *col, field), value.as_str());
                }
                TOp::Clear { id, col, field } => {
                    self.cells.remove(&mut txn, &cell_key(id, *col, field));
                }
                TOp::InsertRow { at, id } => {
                    self.rows.insert(&mut txn, *at as u32, id.as_str());
                }
                TOp::DeleteRow { at, keys, .. } => {
                    self.rows.remove(&mut txn, *at as u32);
                    for k in keys {
                        self.cells.remove(&mut txn, k);
                    }
                }
            }
        }
    }

    fn read(&mut self) -> Canon {
        let txn = self.doc.transact();
        let rows = self.rows.iter(&txn).filter_map(|v| string_of(&v)).collect();
        let cells: Vec<(String, String)> =
            self.cells.iter(&txn).filter_map(|(k, v)| string_of(&v).map(|v| (k.to_string(), v))).collect();
        Canon::from_raw(rows, cells)
    }

    fn encodings(&mut self) -> Vec<(&'static str, Vec<u8>)> {
        encodings(&self.doc)
    }

    fn load(bytes: &[u8]) -> Self {
        Self::wrap(doc_from(999, bytes))
    }

    fn merge_from(&mut self, other: &mut Self) -> usize {
        merge(&self.doc, &other.doc)
    }
}

// ---------------------------------------------------------------- Decks, parent pointers

fn y_val(v: &Val) -> Any {
    match v {
        Val::F(f) => Any::from(*f),
        Val::S(s) => Any::from(s.as_str()),
    }
}

fn field_name(f: &str) -> &'static str {
    const F: [&str; 8] = ["kind", "title", "x", "y", "w", "h", "rot", "text"];
    F.iter().find(|x| **x == f).copied().unwrap_or_else(|| panic!("unknown field {f}"))
}

/// yrs has no tree type and its arrays cannot move items between arrays, so
/// the deck uses the same parent-pointer encoding as `am::AmDeckPP`.
pub struct YDeckPP {
    doc: Doc,
    nodes: MapRef,
}

impl DecksDoc for YDeckPP {
    fn base(ops: &[DOp]) -> Self {
        let doc = Doc::with_client_id(1000);
        let nodes = doc.get_or_insert_map("nodes");
        let mut me = YDeckPP { doc, nodes };
        me.apply(ops);
        me
    }

    fn fork(&mut self, peer: u64) -> Self {
        let doc = doc_from(peer, &full_update(&self.doc));
        let nodes = doc.get_or_insert_map("nodes");
        YDeckPP { doc, nodes }
    }

    fn apply(&mut self, action: &[DOp]) {
        let mut txn = self.doc.transact_mut();
        let n = &self.nodes;
        for op in action {
            match op {
                DOp::Add { id, parent, z, fields, .. } => {
                    n.insert(&mut txn, format!("{id}.p"), parent.as_str());
                    n.insert(&mut txn, format!("{id}.z"), *z);
                    for (f, v) in fields {
                        n.insert(&mut txn, format!("{id}.{f}"), y_val(v));
                    }
                }
                DOp::Set { id, field, val } => {
                    n.insert(&mut txn, format!("{id}.{field}"), y_val(val));
                }
                DOp::Move { id, parent, z, .. } => {
                    n.insert(&mut txn, format!("{id}.p"), parent.as_str());
                    n.insert(&mut txn, format!("{id}.z"), *z);
                }
                DOp::Delete { id } => {
                    n.insert(&mut txn, format!("{id}.d"), true);
                }
            }
        }
    }

    fn read(&mut self) -> DeckRead {
        let txn = self.doc.transact();
        let mut flat: BTreeMap<String, FlatNode> = BTreeMap::new();
        for (k, v) in self.nodes.iter(&txn) {
            let (id, f) = k.split_once('.').unwrap();
            let n = flat.entry(id.to_string()).or_insert_with(|| FlatNode {
                id: id.to_string(),
                parent: String::new(),
                order: 0.0,
                deleted: false,
                fields: BTreeMap::new(),
            });
            match (f, v) {
                ("p", v) => n.parent = string_of(&v).unwrap(),
                ("z", Out::Any(Any::Number(z))) => n.order = z,
                ("d", _) => n.deleted = true,
                (f, Out::Any(Any::Number(x))) => {
                    n.fields.insert(field_name(f), Val::F(x));
                }
                (f, v) => {
                    n.fields.insert(field_name(f), Val::S(string_of(&v).unwrap()));
                }
            }
        }
        drop(txn);
        read_parent_pointers(flat.into_values().collect())
    }

    fn encodings(&mut self) -> Vec<(&'static str, Vec<u8>)> {
        encodings(&self.doc)
    }

    fn load(bytes: &[u8]) -> Self {
        let doc = doc_from(999, bytes);
        let nodes = doc.get_or_insert_map("nodes");
        YDeckPP { doc, nodes }
    }

    fn merge_from(&mut self, other: &mut Self) -> usize {
        merge(&self.doc, &other.doc)
    }
}

// ---------------------------------------------------------------- Letters

fn attrs_at(text: &TextRef, doc: &Doc, ch: char) -> Vec<String> {
    let txn = doc.transact();
    let diff: Vec<Diff<()>> = text.diff(&txn, |_| ());
    for d in diff {
        if let Out::Any(Any::String(s)) = &d.insert {
            if s.contains(ch) {
                let mut k: Vec<String> = d.attributes.map(|a| a.keys().map(|k| k.to_string()).collect()).unwrap_or_default();
                k.sort();
                return k;
            }
        }
    }
    vec![]
}

fn attrs(pairs: &[(&str, Any)]) -> Attrs {
    pairs.iter().map(|(k, v)| (Arc::<str>::from(*k), v.clone())).collect()
}

pub fn letters_probe() -> Vec<String> {
    let mut out = Vec::new();
    let doc = Doc::new();
    let text = doc.get_or_insert_text("text");
    {
        let mut txn = doc.transact_mut();
        text.insert(&mut txn, 0, "bold link plain");
        text.format(&mut txn, 0, 4, attrs(&[("bold", Any::Bool(true))]));
        text.format(&mut txn, 5, 4, attrs(&[("link", Any::from("https://example.org"))]));
        text.format(&mut txn, 10, 5, attrs(&[("font_size_hp", Any::from(28.0))]));
        text.insert(&mut txn, 4, "X");
        text.insert(&mut txn, 10, "Y");
        text.insert(&mut txn, 0, "W");
    }
    out.push(format!("text after edits: {:?}", text.get_string(&doc.transact())));
    {
        let txn = doc.transact();
        for d in text.diff(&txn, |_| ()) {
            out.push(format!("  {:?} {:?}", d.insert.to_string(&txn), d.attributes.map(|a| a.into_iter().collect::<BTreeMap<_, _>>())));
        }
    }
    out.push(format!("typed at end of bold is bold (want true): {}", attrs_at(&text, &doc, 'X').contains(&"bold".into())));
    out.push(format!("typed at end of link is linked (want false): {}", attrs_at(&text, &doc, 'Y').contains(&"link".into())));
    out.push(format!("typed before bold is bold (want false): {}", attrs_at(&text, &doc, 'W').contains(&"bold".into())));
    // The application can emulate a non-expanding mark by inserting with
    // explicit attributes: the inherited ones minus the link.
    {
        let at = text.get_string(&doc.transact()).chars().position(|c| c == 'Y').unwrap() as u32 + 1;
        let mut txn = doc.transact_mut();
        text.insert_with_attributes(&mut txn, at, "V", attrs(&[("link", Any::Null)]));
    }
    out.push(format!(
        "insert_with_attributes(link: null) at end of link is linked (want false): {}",
        attrs_at(&text, &doc, 'V').contains(&"link".into())
    ));

    let a = Doc::with_client_id(1);
    let t = a.get_or_insert_text("t");
    t.insert(&mut a.transact_mut(), 0, "word rest");
    let b = doc_from(2, &full_update(&a));
    let tb = b.get_or_insert_text("t");
    t.format(&mut a.transact_mut(), 0, 4, attrs(&[("bold", Any::Bool(true))]));
    tb.insert(&mut b.transact_mut(), 4, "Z");
    merge(&a, &b);
    out.push(format!(
        "concurrent: A bolds 'word', B types Z at its end -> {:?}, Z bold: {}",
        t.get_string(&a.transact()),
        attrs_at(&t, &a, 'Z').contains(&"bold".into())
    ));
    out
}

pub fn nested_probe() -> String {
    let a = Doc::with_client_id(1);
    a.get_or_insert_map("cells");
    let b = doc_from(2, &full_update(&a));
    for (d, k, v) in [(&a, "v", "42"), (&b, "b", "1")] {
        let cells = d.get_or_insert_map("cells");
        let mut txn = d.transact_mut();
        let cell = cells.insert(&mut txn, "r1.0", MapPrelim::default());
        cell.insert(&mut txn, k, v);
    }
    merge(&a, &b);
    let cells = a.get_or_insert_map("cells");
    let txn = a.transact();
    let mut keys: Vec<String> = match cells.get(&txn, "r1.0") {
        Some(Out::YMap(m)) => m.keys(&txn).map(str::to_string).collect(),
        _ => vec![],
    };
    keys.sort();
    format!("fields surviving: {keys:?} (want [\"b\", \"v\"])")
}
