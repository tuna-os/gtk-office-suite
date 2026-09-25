//! Loro backends.

use std::collections::{BTreeMap, HashMap};

use loro::{
    ExpandType, ExportMode, LoroDoc, LoroList, LoroMap, LoroText, LoroTree, LoroValue, StyleConfig, StyleConfigMap,
    TextDelta, TreeID, TreeParentId, ValueOrContainer,
};

use crate::backend::{DecksDoc, TablesDoc};
use crate::decks::{DOp, DeckRead, Node, Tree, Val, ROOT as DECK_ROOT};
use crate::tables::{cell_key, Canon, TOp, View};

fn string_of(v: &ValueOrContainer) -> Option<String> {
    match v {
        ValueOrContainer::Value(LoroValue::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn merge_updates(me: &LoroDoc, other: &LoroDoc) -> usize {
    let bytes = other.export(ExportMode::updates(&me.oplog_vv())).unwrap();
    me.import(&bytes).unwrap();
    bytes.len()
}

fn encodings(doc: &LoroDoc) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("snapshot (history + state)", doc.export(ExportMode::Snapshot).unwrap()),
        ("updates only (history)", doc.export(ExportMode::all_updates()).unwrap()),
        ("state only (shallow)", doc.export(ExportMode::StateOnly(None)).unwrap()),
    ]
}

// ---------------------------------------------------------------- Tables

pub struct LoTables {
    doc: LoroDoc,
    rows: LoroList,
    cells: LoroMap,
}

impl LoTables {
    fn wrap(doc: LoroDoc) -> Self {
        let rows = doc.get_list("rows");
        let cells = doc.get_map("cells");
        LoTables { doc, rows, cells }
    }
}

impl TablesDoc for LoTables {
    fn base(base: &View) -> Self {
        let doc = LoroDoc::new();
        doc.set_peer_id(1000).unwrap();
        let me = Self::wrap(doc);
        for (i, id) in base.rows.iter().enumerate() {
            me.rows.insert(i, id.as_str()).unwrap();
        }
        me.doc.commit();
        me
    }

    fn fork(&mut self, peer: u64) -> Self {
        let doc = self.doc.fork();
        doc.set_peer_id(peer).unwrap();
        Self::wrap(doc)
    }

    fn apply(&mut self, action: &[TOp]) {
        for op in action {
            match op {
                TOp::Set { id, col, field, value } => {
                    self.cells.insert(&cell_key(id, *col, field), value.as_str()).unwrap()
                }
                TOp::Clear { id, col, field } => self.cells.delete(&cell_key(id, *col, field)).unwrap(),
                TOp::InsertRow { at, id } => self.rows.insert(*at, id.as_str()).unwrap(),
                TOp::DeleteRow { at, keys, .. } => {
                    self.rows.delete(*at, 1).unwrap();
                    for k in keys {
                        self.cells.delete(k).unwrap();
                    }
                }
            }
        }
        self.doc.commit();
    }

    fn read(&mut self) -> Canon {
        let mut rows = Vec::new();
        self.rows.for_each(|v| rows.extend(string_of(&v)));
        let mut cells = Vec::new();
        self.cells.for_each(|k, v| cells.extend(string_of(&v).map(|v| (k.to_string(), v))));
        Canon::from_raw(rows, cells)
    }

    fn encodings(&mut self) -> Vec<(&'static str, Vec<u8>)> {
        encodings(&self.doc)
    }

    fn load(bytes: &[u8]) -> Self {
        Self::wrap(LoroDoc::from_snapshot(bytes).unwrap())
    }

    fn merge_from(&mut self, other: &mut Self) -> usize {
        merge_updates(&self.doc, &other.doc)
    }
}

// ---------------------------------------------------------------- Decks, MovableTree

fn lo_val(v: &Val) -> LoroValue {
    match v {
        Val::F(f) => LoroValue::Double(*f),
        Val::S(s) => LoroValue::String(s.as_str().into()),
    }
}

fn field_name(f: &str) -> &'static str {
    const F: [&str; 8] = ["kind", "title", "x", "y", "w", "h", "rot", "text"];
    F.iter().find(|x| **x == f).copied().unwrap_or_else(|| panic!("unknown field {f}"))
}

/// The deck as a Loro `MovableTree` (fractional-index sibling order), with
/// each node's fields in its metadata map.
pub struct LoDeck {
    doc: LoroDoc,
    tree: LoroTree,
    ids: HashMap<String, TreeID>,
}

impl LoDeck {
    fn wrap(doc: LoroDoc) -> Self {
        let tree = doc.get_tree("deck");
        tree.enable_fractional_index(0);
        LoDeck { doc, tree, ids: HashMap::new() }
    }

    fn reindex(&mut self) {
        self.ids.clear();
        for n in self.tree.nodes() {
            if let Ok(meta) = self.tree.get_meta(n) {
                if let Some(id) = meta.get("id").as_ref().and_then(string_of) {
                    self.ids.insert(id, n);
                }
            }
        }
    }
}

impl DecksDoc for LoDeck {
    fn base(ops: &[DOp]) -> Self {
        let doc = LoroDoc::new();
        doc.set_peer_id(1000).unwrap();
        let mut me = Self::wrap(doc);
        let root = me.tree.create(None).unwrap();
        me.tree.get_meta(root).unwrap().insert("id", DECK_ROOT).unwrap();
        me.ids.insert(DECK_ROOT.to_string(), root);
        me.apply(ops);
        me
    }

    fn fork(&mut self, peer: u64) -> Self {
        let doc = self.doc.fork();
        doc.set_peer_id(peer).unwrap();
        let mut me = Self::wrap(doc);
        me.ids = self.ids.clone();
        me
    }

    fn apply(&mut self, action: &[DOp]) {
        for op in action {
            match op {
                DOp::Add { id, parent, index, fields, .. } => {
                    let n = self.tree.create_at(self.ids[parent], *index).unwrap();
                    let meta = self.tree.get_meta(n).unwrap();
                    meta.insert("id", id.as_str()).unwrap();
                    for (f, v) in fields {
                        meta.insert(f, lo_val(v)).unwrap();
                    }
                    self.ids.insert(id.clone(), n);
                }
                DOp::Set { id, field, val } => {
                    self.tree.get_meta(self.ids[id]).unwrap().insert(field, lo_val(val)).unwrap();
                }
                DOp::Move { id, parent, index, .. } => {
                    self.tree.mov_to(self.ids[id], self.ids[parent], *index).unwrap();
                }
                DOp::Delete { id } => self.tree.delete(self.ids[id]).unwrap(),
            }
        }
        self.doc.commit();
    }

    fn read(&mut self) -> DeckRead {
        let tree = &self.tree;
        let mut t = Tree::new();
        let mut visible = Vec::new();
        let roots = tree.roots();
        let root = roots[0];
        let mut stack = vec![(root, DECK_ROOT.to_string())];
        while let Some((n, name)) = stack.pop() {
            for c in tree.children(TreeParentId::from(n)).unwrap_or_default() {
                let meta = tree.get_meta(c).unwrap();
                let mut fields = BTreeMap::new();
                let mut id = String::new();
                meta.for_each(|k, v| match (k, v) {
                    ("id", v) => id = string_of(&v).unwrap(),
                    (k, ValueOrContainer::Value(LoroValue::Double(f))) => {
                        fields.insert(field_name(k), Val::F(f));
                    }
                    (k, v) => {
                        fields.insert(field_name(k), Val::S(string_of(&v).unwrap()));
                    }
                });
                visible.push(id.clone());
                t.nodes.insert(id.clone(), Node { parent: name.clone(), children: vec![], z: 0.0, fields, deleted: false });
                t.nodes.get_mut(&name).unwrap().children.push(id.clone());
                stack.push((c, id));
            }
        }
        DeckRead::from_tree(&t, visible, 0, 0)
    }

    fn encodings(&mut self) -> Vec<(&'static str, Vec<u8>)> {
        encodings(&self.doc)
    }

    fn load(bytes: &[u8]) -> Self {
        let mut me = Self::wrap(LoroDoc::from_snapshot(bytes).unwrap());
        me.reindex();
        me
    }

    fn merge_from(&mut self, other: &mut Self) -> usize {
        let n = merge_updates(&self.doc, &other.doc);
        self.reindex();
        n
    }
}

// ---------------------------------------------------------------- Letters

fn style_config() -> StyleConfigMap {
    let mut m = StyleConfigMap::new();
    m.insert("bold".into(), StyleConfig { expand: ExpandType::After });
    m.insert("link".into(), StyleConfig { expand: ExpandType::None });
    m.insert("font_size_hp".into(), StyleConfig { expand: ExpandType::After });
    m
}

fn attrs_at(text: &LoroText, ch: char) -> Vec<String> {
    let mut out = Vec::new();
    for d in text.to_delta() {
        if let TextDelta::Insert { insert, attributes } = d {
            if insert.contains(ch) {
                if let Some(a) = attributes {
                    let mut k: Vec<String> = a.keys().cloned().collect();
                    k.sort();
                    out = k;
                }
            }
        }
    }
    out
}

pub fn letters_probe() -> Vec<String> {
    let mut out = Vec::new();
    let doc = LoroDoc::new();
    doc.config_text_style(style_config());
    let text = doc.get_text("text");
    text.insert(0, "bold link plain").unwrap();
    text.mark(0..4, "bold", true).unwrap();
    text.mark(5..9, "link", "https://example.org").unwrap();
    text.mark(10..15, "font_size_hp", 28).unwrap();
    text.insert(4, "X").unwrap();
    text.insert(10, "Y").unwrap();
    text.insert(0, "W").unwrap();
    doc.commit();
    out.push(format!("text after edits: {:?}", text.to_string()));
    for d in text.to_delta() {
        if let TextDelta::Insert { insert, attributes } = d {
            out.push(format!("  {insert:?} {:?}", attributes.map(|a| a.into_iter().collect::<BTreeMap<_, _>>())));
        }
    }
    out.push(format!("typed at end of bold is bold (want true): {}", attrs_at(&text, 'X').contains(&"bold".into())));
    out.push(format!("typed at end of link is linked (want false): {}", attrs_at(&text, 'Y').contains(&"link".into())));
    out.push(format!("typed before bold is bold (want false): {}", attrs_at(&text, 'W').contains(&"bold".into())));

    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    a.config_text_style(style_config());
    let t = a.get_text("t");
    t.insert(0, "word rest").unwrap();
    a.commit();
    let b = a.fork();
    b.set_peer_id(2).unwrap();
    b.config_text_style(style_config());
    t.mark(0..4, "bold", true).unwrap();
    a.commit();
    b.get_text("t").insert(4, "Z").unwrap();
    b.commit();
    merge_updates(&a, &b);
    out.push(format!(
        "concurrent: A bolds 'word', B types Z at its end -> {:?}, Z bold: {}",
        t.to_string(),
        attrs_at(&t, 'Z').contains(&"bold".into())
    ));
    out
}

pub fn nested_probe() -> String {
    let run = |mergeable: bool| {
        let a = LoroDoc::new();
        a.set_peer_id(1).unwrap();
        a.get_map("cells");
        a.commit();
        let b = a.fork();
        b.set_peer_id(2).unwrap();
        let make = |d: &LoroDoc| {
            let cells = d.get_map("cells");
            if mergeable {
                cells.ensure_mergeable_map("r1.0").unwrap()
            } else {
                cells.insert_container("r1.0", LoroMap::new()).unwrap()
            }
        };
        make(&a).insert("v", "42").unwrap();
        make(&b).insert("b", "1").unwrap();
        a.commit();
        b.commit();
        merge_updates(&a, &b);
        let mut keys: Vec<String> = Vec::new();
        if let Some(ValueOrContainer::Container(loro::Container::Map(m))) = a.get_map("cells").get("r1.0") {
            keys = m.keys().map(|k| k.to_string()).collect();
            keys.sort();
        }
        keys
    };
    format!(
        "insert_container: fields surviving {:?}; ensure_mergeable_map: {:?} (want [\"b\", \"v\"])",
        run(false),
        run(true)
    )
}
