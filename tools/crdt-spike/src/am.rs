//! Automerge backends.

use std::collections::{BTreeMap, HashMap};

use automerge::marks::{ExpandMark, Mark};
use automerge::transaction::Transactable;
use automerge::{ActorId, AutoCommit, ChangeHash, ObjId, ObjType, ReadDoc, ScalarValueRef, Value, ValueRef, ROOT};

use crate::backend::{DecksDoc, TablesDoc};
use crate::decks::{read_parent_pointers, DOp, DeckRead, FlatNode, Tree, Val, ROOT as DECK_ROOT};
use crate::tables::{cell_key, Canon, TOp, View};

fn actor(peer: u64) -> ActorId {
    ActorId::from(peer.to_be_bytes())
}

/// The heads to ask a peer for changes after: ours, plus the shared base
/// (heads the peer has never seen are ignored by `save_after`, so without the
/// base it would resend the whole history). This is what a sync would send.
fn known(doc: &mut AutoCommit, since: &[ChangeHash]) -> Vec<ChangeHash> {
    let mut h = doc.get_heads();
    h.extend_from_slice(since);
    h
}

fn scalar_string(v: &ValueRef<'_>) -> Option<String> {
    match v {
        ValueRef::Scalar(ScalarValueRef::Str(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn get_obj(doc: &AutoCommit, parent: &ObjId, key: &str) -> ObjId {
    match doc.get(parent, key).unwrap() {
        Some((Value::Object(_), id)) => id,
        other => panic!("{key}: expected object, got {other:?}"),
    }
}

// ---------------------------------------------------------------- Tables

pub struct AmTables {
    doc: AutoCommit,
    rows: ObjId,
    cells: ObjId,
    since: Vec<ChangeHash>,
}

impl TablesDoc for AmTables {
    fn base(base: &View) -> Self {
        let mut doc = AutoCommit::new().with_actor(actor(1000));
        let rows = doc.put_object(ROOT, "rows", ObjType::List).unwrap();
        let cells = doc.put_object(ROOT, "cells", ObjType::Map).unwrap();
        for (i, id) in base.rows.iter().enumerate() {
            doc.insert(&rows, i, id.as_str()).unwrap();
        }
        doc.commit();
        let since = doc.get_heads();
        AmTables { doc, rows, cells, since }
    }

    fn fork(&mut self, peer: u64) -> Self {
        let doc = self.doc.fork().with_actor(actor(peer));
        AmTables { doc, rows: self.rows.clone(), cells: self.cells.clone(), since: self.doc.get_heads() }
    }

    fn apply(&mut self, action: &[TOp]) {
        let d = &mut self.doc;
        for op in action {
            match op {
                TOp::Set { id, col, field, value } => {
                    d.put(&self.cells, cell_key(id, *col, field), value.as_str()).unwrap()
                }
                TOp::Clear { id, col, field } => d.delete(&self.cells, cell_key(id, *col, field)).unwrap(),
                TOp::InsertRow { at, id } => d.insert(&self.rows, *at, id.as_str()).unwrap(),
                TOp::DeleteRow { at, keys, .. } => {
                    d.delete(&self.rows, *at).unwrap();
                    for k in keys {
                        d.delete(&self.cells, k.as_str()).unwrap();
                    }
                }
            }
        }
        d.commit();
    }

    fn read(&mut self) -> Canon {
        let rows = self.doc.list_range(&self.rows, ..).filter_map(|it| scalar_string(&it.value)).collect();
        let cells: Vec<(String, String)> = self
            .doc
            .map_range(&self.cells, ..)
            .filter_map(|it| scalar_string(&it.value).map(|v| (it.key.to_string(), v)))
            .collect();
        Canon::from_raw(rows, cells)
    }

    fn encodings(&mut self) -> Vec<(&'static str, Vec<u8>)> {
        vec![("full history (save)", self.doc.save()), ("full history, uncompressed", self.doc.save_nocompress())]
    }

    fn load(bytes: &[u8]) -> Self {
        let doc = AutoCommit::load(bytes).unwrap();
        let rows = get_obj(&doc, &ROOT, "rows");
        let cells = get_obj(&doc, &ROOT, "cells");
        AmTables { doc, rows, cells, since: vec![] }
    }

    fn merge_from(&mut self, other: &mut Self) -> usize {
        let bytes = other.doc.save_after(&known(&mut self.doc, &self.since));
        self.doc.load_incremental(&bytes).unwrap();
        bytes.len()
    }
}

// ---------------------------------------------------------------- Decks, parent pointers

fn am_scalar(v: &Val) -> automerge::ScalarValue {
    match v {
        Val::F(f) => automerge::ScalarValue::F64(*f),
        Val::S(s) => automerge::ScalarValue::Str(s.as_str().into()),
    }
}

fn field_name(f: &str) -> &'static str {
    const F: [&str; 9] = ["kind", "title", "x", "y", "w", "h", "rot", "text", "id"];
    F.iter().find(|x| **x == f).copied().unwrap_or_else(|| panic!("unknown field {f}"))
}

fn val_of(v: &ValueRef<'_>) -> Option<Val> {
    match v {
        ValueRef::Scalar(ScalarValueRef::Str(s)) => Some(Val::S(s.to_string())),
        ValueRef::Scalar(ScalarValueRef::F64(f)) => Some(Val::F(*f)),
        _ => None,
    }
}

/// Automerge has no tree or move operation. This encoding keeps one flat map
/// of LWW registers per node: `<id>.p` (parent), `<id>.z` (fractional
/// position among siblings), `<id>.d` (tombstone), `<id>.<field>`. A move is
/// a single register write, so it can never duplicate a node, but two
/// concurrent moves can form a cycle that the application must repair.
pub struct AmDeckPP {
    doc: AutoCommit,
    nodes: ObjId,
    since: Vec<ChangeHash>,
}

impl AmDeckPP {
    fn put(&mut self, id: &str, f: &str, v: automerge::ScalarValue) {
        self.doc.put(&self.nodes, format!("{id}.{f}"), v).unwrap();
    }
}

impl DecksDoc for AmDeckPP {
    fn base(ops: &[DOp]) -> Self {
        let mut doc = AutoCommit::new().with_actor(actor(1000));
        let nodes = doc.put_object(ROOT, "nodes", ObjType::Map).unwrap();
        let mut me = AmDeckPP { doc, nodes, since: vec![] };
        me.apply(ops);
        me.since = me.doc.get_heads();
        me
    }

    fn fork(&mut self, peer: u64) -> Self {
        let doc = self.doc.fork().with_actor(actor(peer));
        AmDeckPP { doc, nodes: self.nodes.clone(), since: self.doc.get_heads() }
    }

    fn apply(&mut self, action: &[DOp]) {
        for op in action {
            match op {
                DOp::Add { id, parent, z, fields, .. } => {
                    self.put(id, "p", parent.as_str().into());
                    self.put(id, "z", (*z).into());
                    for (f, v) in fields {
                        self.put(id, f, am_scalar(v));
                    }
                }
                DOp::Set { id, field, val } => self.put(id, field, am_scalar(val)),
                DOp::Move { id, parent, z, .. } => {
                    self.put(id, "p", parent.as_str().into());
                    self.put(id, "z", (*z).into());
                }
                DOp::Delete { id } => self.put(id, "d", true.into()),
            }
        }
        self.doc.commit();
    }

    fn read(&mut self) -> DeckRead {
        let mut flat: BTreeMap<String, FlatNode> = BTreeMap::new();
        for it in self.doc.map_range(&self.nodes, ..) {
            let (id, f) = it.key.split_once('.').unwrap();
            let n = flat.entry(id.to_string()).or_insert_with(|| FlatNode {
                id: id.to_string(),
                parent: String::new(),
                order: 0.0,
                deleted: false,
                fields: BTreeMap::new(),
            });
            match (f, &it.value) {
                ("p", v) => n.parent = scalar_string(v).unwrap(),
                ("z", ValueRef::Scalar(ScalarValueRef::F64(z))) => n.order = *z,
                ("d", _) => n.deleted = true,
                (f, v) => {
                    n.fields.insert(field_name(f), val_of(v).unwrap());
                }
            }
        }
        read_parent_pointers(flat.into_values().collect())
    }

    fn encodings(&mut self) -> Vec<(&'static str, Vec<u8>)> {
        vec![("full history (save)", self.doc.save()), ("full history, uncompressed", self.doc.save_nocompress())]
    }

    fn load(bytes: &[u8]) -> Self {
        let doc = AutoCommit::load(bytes).unwrap();
        let nodes = get_obj(&doc, &ROOT, "nodes");
        AmDeckPP { doc, nodes, since: vec![] }
    }

    fn merge_from(&mut self, other: &mut Self) -> usize {
        let bytes = other.doc.save_after(&known(&mut self.doc, &self.since));
        self.doc.load_incremental(&bytes).unwrap();
        bytes.len()
    }
}

// ---------------------------------------------------------------- Decks, naive nested lists

/// The encoding a JSON-shaped document invites: slides and groups hold a
/// list `c` of child maps, and a move is delete-here + insert-a-copy-there
/// (Automerge lists have no move). Included to show what goes wrong.
pub struct AmDeckNaive {
    doc: AutoCommit,
    root: ObjId,
    /// This replica's view: node id -> (its map object, its parent id).
    at: HashMap<String, (ObjId, String)>,
    since: Vec<ChangeHash>,
}

impl AmDeckNaive {
    fn children(&self, id: &str) -> ObjId {
        get_obj(&self.doc, &self.at[id].0, "c")
    }

    fn index_in_parent(&self, id: &str) -> (ObjId, usize) {
        let list = self.children(&self.at[id].1);
        let target = &self.at[id].0;
        let pos = self.doc.list_range(&list, ..).position(|it| &it.id() == target).expect("in parent");
        (list, pos)
    }

    fn register(&mut self, obj: ObjId, parent: &str) {
        let id = match self.doc.get(&obj, "id").unwrap() {
            Some((v, _)) => v.as_str().unwrap().to_string(),
            None => panic!("node without id"),
        };
        if let Some((Value::Object(_), c)) = self.doc.get(&obj, "c").unwrap() {
            let kids: Vec<ObjId> = self.doc.list_range(&c, ..).map(|it| it.id()).collect();
            for k in kids {
                self.register(k, &id);
            }
        }
        self.at.insert(id, (obj, parent.to_string()));
    }
}

impl DecksDoc for AmDeckNaive {
    fn base(ops: &[DOp]) -> Self {
        let mut doc = AutoCommit::new().with_actor(actor(1000));
        let root = doc.put_object(ROOT, "deck", ObjType::Map).unwrap();
        doc.put(&root, "id", DECK_ROOT).unwrap();
        doc.put_object(&root, "c", ObjType::List).unwrap();
        let mut at = HashMap::new();
        at.insert(DECK_ROOT.to_string(), (root.clone(), String::new()));
        let mut me = AmDeckNaive { doc, root, at, since: vec![] };
        me.apply(ops);
        me.since = me.doc.get_heads();
        me
    }

    fn fork(&mut self, peer: u64) -> Self {
        let doc = self.doc.fork().with_actor(actor(peer));
        AmDeckNaive { doc, root: self.root.clone(), at: self.at.clone(), since: self.doc.get_heads() }
    }

    fn apply(&mut self, action: &[DOp]) {
        for op in action {
            match op {
                DOp::Add { id, parent, index, fields, .. } => {
                    let list = self.children(parent);
                    let obj = self.doc.insert_object(&list, *index, ObjType::Map).unwrap();
                    self.doc.put(&obj, "id", id.as_str()).unwrap();
                    for (f, v) in fields {
                        self.doc.put(&obj, *f, am_scalar(v)).unwrap();
                    }
                    if fields.iter().any(|(f, v)| *f == "kind" && matches!(v, Val::S(s) if s == "slide" || s == "group")) {
                        self.doc.put_object(&obj, "c", ObjType::List).unwrap();
                    }
                    self.at.insert(id.clone(), (obj, parent.clone()));
                }
                DOp::Set { id, field, val } => {
                    let obj = self.at[id].0.clone();
                    self.doc.put(&obj, *field, am_scalar(val)).unwrap();
                }
                DOp::Move { id, parent, index, .. } => {
                    let copy = self.doc.hydrate(&self.at[id].0, None).unwrap();
                    let (list, pos) = self.index_in_parent(id);
                    self.doc.delete(&list, pos).unwrap();
                    let target = self.children(parent);
                    self.doc.splice(&target, *index, 0, [copy]).unwrap();
                    let obj = self.doc.get(&target, *index).unwrap().unwrap().1;
                    self.register(obj, parent);
                }
                DOp::Delete { id } => {
                    let (list, pos) = self.index_in_parent(id);
                    self.doc.delete(&list, pos).unwrap();
                    self.at.remove(id);
                }
            }
        }
        self.doc.commit();
    }

    fn read(&mut self) -> DeckRead {
        fn walk(doc: &AutoCommit, obj: &ObjId, t: &mut Tree, parent: &str, visible: &mut Vec<String>) {
            let c = match doc.get(obj, "c").unwrap() {
                Some((Value::Object(_), c)) => c,
                _ => return,
            };
            for it in doc.list_range(&c, ..) {
                let child = it.id();
                let mut fields = BTreeMap::new();
                let mut id = String::new();
                for m in doc.map_range(&child, ..) {
                    match m.key.as_ref() {
                        "id" => id = scalar_string(&m.value).unwrap(),
                        "c" => {}
                        k => {
                            fields.insert(field_name(k), val_of(&m.value).unwrap());
                        }
                    }
                }
                visible.push(id.clone());
                // A duplicated id appears once in the dump's node table but
                // twice in `visible`; suffix it so the dump shows both.
                let key = if t.nodes.contains_key(&id) { format!("{id}#dup{}", visible.len()) } else { id.clone() };
                t.nodes.insert(
                    key.clone(),
                    crate::decks::Node { parent: parent.into(), children: vec![], z: 0.0, fields, deleted: false },
                );
                t.nodes.get_mut(parent).unwrap().children.push(key.clone());
                walk(doc, &child, t, &key, visible);
            }
        }
        let mut t = Tree::new();
        let mut visible = Vec::new();
        walk(&self.doc, &self.root, &mut t, DECK_ROOT, &mut visible);
        DeckRead::from_tree(&t, visible, 0, 0)
    }

    fn encodings(&mut self) -> Vec<(&'static str, Vec<u8>)> {
        vec![("full history (save)", self.doc.save()), ("full history, uncompressed", self.doc.save_nocompress())]
    }

    fn load(bytes: &[u8]) -> Self {
        let doc = AutoCommit::load(bytes).unwrap();
        let root = get_obj(&doc, &ROOT, "deck");
        let mut me = AmDeckNaive { doc, root: root.clone(), at: HashMap::new(), since: vec![] };
        me.register(root, "");
        me
    }

    fn merge_from(&mut self, other: &mut Self) -> usize {
        let bytes = other.doc.save_after(&known(&mut self.doc, &self.since));
        self.doc.load_incremental(&bytes).unwrap();
        bytes.len()
    }
}

// ---------------------------------------------------------------- Letters

/// Can Automerge marks express RunStyle's per-mark expand behaviour?
pub fn letters_probe() -> Vec<String> {
    let mut out = Vec::new();
    let mut doc = AutoCommit::new();
    let text = doc.put_object(ROOT, "text", ObjType::Text).unwrap();
    doc.splice_text(&text, 0, 0, "bold link plain").unwrap();
    // bold [0,4) expands after; link [5,9) never expands.
    doc.mark(&text, Mark::new("bold".into(), true, 0, 4), ExpandMark::After).unwrap();
    doc.mark(&text, Mark::new("link".into(), "https://example.org", 5, 9), ExpandMark::None).unwrap();
    doc.mark(&text, Mark::new("font_size_hp".into(), 28i64, 10, 15), ExpandMark::Both).unwrap();
    doc.splice_text(&text, 4, 0, "X").unwrap(); // at bold's end
    doc.splice_text(&text, 10, 0, "Y").unwrap(); // at link's end (now [6, 10))
    doc.splice_text(&text, 0, 0, "W").unwrap(); // before bold's start
    let s = doc.text(&text).unwrap();
    let marks = doc.marks(&text).unwrap();
    let has = |name: &str, ch: char| {
        let i = s.chars().position(|c| c == ch).unwrap();
        marks.iter().any(|m| m.name == name && m.start <= i && i < m.end)
    };
    out.push(format!("text after edits: {s:?}"));
    for m in &marks {
        out.push(format!("  mark {} = {} on [{}, {})", m.name, m.value, m.start, m.end));
    }
    out.push(format!("typed at end of bold is bold (want true): {}", has("bold", 'X')));
    out.push(format!("typed at end of link is linked (want false): {}", has("link", 'Y')));
    out.push(format!("typed before bold is bold (want false): {}", has("bold", 'W')));

    // Concurrent: A bolds a word while B types at its end.
    let mut a = AutoCommit::new().with_actor(actor(1));
    let t = a.put_object(ROOT, "t", ObjType::Text).unwrap();
    a.splice_text(&t, 0, 0, "word rest").unwrap();
    a.commit();
    let mut b = a.fork().with_actor(actor(2));
    a.mark(&t, Mark::new("bold".into(), true, 0, 4), ExpandMark::After).unwrap();
    b.splice_text(&t, 4, 0, "Z").unwrap();
    a.merge(&mut b).unwrap();
    let s = a.text(&t).unwrap();
    let z = s.chars().position(|c| c == 'Z').unwrap();
    let bold_z = a.marks(&t).unwrap().iter().any(|m| m.name == "bold" && m.start <= z && z < m.end);
    out.push(format!("concurrent: A bolds 'word', B types Z at its end -> {s:?}, Z bold: {bold_z}"));
    out
}

/// Two peers concurrently create the same nested map key and write
/// different fields into it. Does either write get lost?
pub fn nested_probe() -> String {
    let mut a = AutoCommit::new().with_actor(actor(1));
    let cells = a.put_object(ROOT, "cells", ObjType::Map).unwrap();
    a.commit();
    let mut b = a.fork().with_actor(actor(2));
    let ca = a.put_object(&cells, "r1.0", ObjType::Map).unwrap();
    a.put(&ca, "v", "42").unwrap();
    let cb = b.put_object(&cells, "r1.0", ObjType::Map).unwrap();
    b.put(&cb, "b", "1").unwrap();
    a.merge(&mut b).unwrap();
    let cell = get_obj(&a, &cells, "r1.0");
    let keys: Vec<String> = a.keys(&cell).collect();
    format!("fields surviving: {keys:?} (want [\"b\", \"v\"])")
}
