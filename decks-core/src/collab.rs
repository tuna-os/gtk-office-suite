// SPDX-License-Identifier: GPL-3.0-or-later
//! The deck as a Loro document: RFC-0001 Phase 3, behind the `collab`
//! feature (off by default). No transport yet: a [`Replica`] records local
//! op groups into its document, exchanges updates with another replica
//! in-process, and rebuilds the deck from the merged document.
//!
//! # Encoding
//!
//! One Loro `MovableTree`, `deck`, with fractional-index sibling order (the
//! encoding the spike measured, `docs/rfc/0001-spike-results.md`):
//! - one root node; its children are the slides, in order;
//! - a slide's children are its objects, in z-order.
//!
//! Each node's fields are last-writer-wins registers in its metadata map:
//! - a slide: `title`, `bg`, `notes`, `master` and `tr` (the transition),
//!   and `builds`, whose entries name their object by its node, not by its
//!   index, so a concurrent insert or move doesn't retarget a build;
//! - an object: `v`, its kind, and one register `f.<field>` per field of
//!   that kind (`f.x`, `f.text`, `f.style`, ...), so one person moving a box
//!   while another edits its text keeps both.
//!
//! A deleted slide or object keeps its node and gets `d` set: a tombstone
//! that no move clears, so a delete wins over a concurrent move or edit,
//! as the RFC decides (Loro's own `delete` would lose to a concurrent
//! move). Undo clears the tombstone. Tombstoned nodes are collected when
//! history is compacted (Phase 4).
//!
//! # How a local edit gets in
//!
//! Decks' ops are addressed by stable ids (`ops.rs`), so each op maps onto
//! the tree one to one: an insert creates a node (or, for an undone delete,
//! clears its tombstone), a move moves only the node that moved, a set
//! writes only the registers that changed. The replica keeps its own copy
//! of the deck and applies each op to it, so it knows where things are
//! after every op of a group.
//!
//! # Known limits
//! - A remote change rebuilds the deck from the document, with fresh local
//!   ids, so the local undo history no longer applies to it (as for Tables).
//! - Text is one register per field: two people typing in the same box at
//!   once keep one person's text. Rich text as a Loro `Text` is Letters'
//!   phase.
//! - `master` is an index into the deck's masters, which aren't replicated
//!   yet.

use std::collections::{BTreeMap, HashMap, HashSet};

use loro::{ExportMode, LoroDoc, LoroMap, LoroTree, LoroValue, TreeID, TreeParentId, ValueOrContainer};

use crate::builds::{Build, BuildEffect};
use crate::engine::{Slide, SlideObject, Transition};
use crate::ops::{apply, ensure_ids, Op, OpError};

const DELETED: &str = "d";

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("the deck types serialize")
}

fn string_at(map: &LoroMap, key: &str) -> Option<String> {
    match map.get(key) {
        Some(ValueOrContainer::Value(LoroValue::String(s))) => Some(s.to_string()),
        _ => None,
    }
}

fn is_deleted(map: &LoroMap) -> bool {
    matches!(map.get(DELETED), Some(ValueOrContainer::Value(LoroValue::Bool(true))))
}

/// Set `key` to `text`, unless it already is.
fn put(map: &LoroMap, key: &str, text: &str) {
    if string_at(map, key).as_deref() != Some(text) {
        map.insert(key, text).expect("map insert");
    }
}

fn set_deleted(map: &LoroMap, deleted: bool) {
    if is_deleted(map) != deleted {
        map.insert(DELETED, deleted).expect("map insert");
    }
}

/// An object as its kind and one encoded register per field.
fn object_fields(object: &SlideObject) -> (String, BTreeMap<String, String>) {
    let value = serde_json::to_value(object).expect("objects serialize");
    let serde_json::Value::Object(outer) = value else { panic!("an object serializes as its kind and fields") };
    let (kind, inner) = outer.into_iter().next().expect("an object has a kind");
    let fields = match inner {
        serde_json::Value::Object(fields) => fields.into_iter().map(|(k, v)| (k, v.to_string())).collect(),
        _ => BTreeMap::new(),
    };
    (kind, fields)
}

fn write_object(meta: &LoroMap, object: &SlideObject) {
    let (kind, fields) = object_fields(object);
    put(meta, "v", &kind);
    let stale: Vec<String> = meta
        .keys()
        .map(|k| k.to_string())
        .filter(|k| k.strip_prefix("f.").is_some_and(|name| !fields.contains_key(name)))
        .collect();
    for key in stale {
        meta.delete(&key).expect("map delete");
    }
    for (name, text) in &fields {
        put(meta, &format!("f.{name}"), text);
    }
}

/// The object a node holds, or `None` when its registers don't make one
/// (every peer then leaves it out alike).
fn read_object(meta: &LoroMap) -> Option<SlideObject> {
    let kind = string_at(meta, "v")?;
    let keys: Vec<String> = meta.keys().map(|k| k.to_string()).collect();
    let mut fields = serde_json::Map::new();
    for key in keys {
        if let Some(name) = key.strip_prefix("f.") {
            fields.insert(name.to_string(), serde_json::from_str(&string_at(meta, &key)?).ok()?);
        }
    }
    let mut outer = serde_json::Map::new();
    outer.insert(kind, serde_json::Value::Object(fields));
    serde_json::from_value(serde_json::Value::Object(outer)).ok()
}

/// A build as the document stores it: its object's node, effect and
/// direction.
type StoredBuild = (String, BuildEffect, bool);

/// One peer's copy of a shared deck.
pub struct Replica {
    doc: LoroDoc,
    tree: LoroTree,
    root: TreeID,
    /// The deck as this replica last wrote or read it, with its local ids.
    deck: Vec<Slide>,
    /// Each local slide and object id's node.
    nodes: HashMap<u64, TreeID>,
}

impl Replica {
    fn with_doc(doc: LoroDoc, peer: u64) -> Result<Self, String> {
        doc.set_peer_id(peer).map_err(|e| e.to_string())?;
        let tree = doc.get_tree("deck");
        tree.enable_fractional_index(0);
        let root = match tree.roots().first() {
            Some(root) => *root,
            None => tree.create(None).map_err(|e| e.to_string())?,
        };
        Ok(Replica { doc, tree, root, deck: Vec::new(), nodes: HashMap::new() })
    }

    /// Start a shared document from `slides`, as peer `peer`. The slides
    /// get their ids (`ops::ensure_ids`) if they have none yet.
    pub fn new(peer: u64, slides: &mut [Slide]) -> Result<Self, String> {
        ensure_ids(slides);
        let mut me = Self::with_doc(LoroDoc::new(), peer)?;
        me.deck = slides.to_vec();
        for s in slides.iter() {
            let node = me.tree.create(me.root).map_err(|e| e.to_string())?;
            me.nodes.insert(s.ids.slide, node);
            for (object, id) in s.objects.iter().zip(&s.ids.objects) {
                let o = me.tree.create(node).map_err(|e| e.to_string())?;
                me.nodes.insert(*id, o);
                write_object(&me.meta(o), object);
            }
            me.write_slide(node, s);
        }
        me.doc.commit();
        Ok(me)
    }

    /// A second peer on a copy of this document, with the deck it shows.
    pub fn fork(&self, peer: u64) -> Result<(Replica, Vec<Slide>), String> {
        self.doc.commit();
        let mut other = Self::with_doc(self.doc.fork(), peer)?;
        let deck = other.rebuild()?;
        Ok((other, deck))
    }

    /// A peer that has only the history in `updates` (from
    /// [`Replica::export_all`]): the whole document replayed from its ops.
    pub fn from_updates(peer: u64, updates: &[u8]) -> Result<(Replica, Vec<Slide>), String> {
        let doc = LoroDoc::new();
        doc.import(updates).map_err(|e| e.to_string())?;
        let mut me = Self::with_doc(doc, peer)?;
        let deck = me.rebuild()?;
        Ok((me, deck))
    }

    /// The document's whole history, as updates.
    pub fn export_all(&self) -> Vec<u8> {
        self.doc.commit();
        self.doc.export(ExportMode::all_updates()).expect("export")
    }

    /// Take in what `other` has that this replica doesn't. The deck then
    /// needs [`Replica::rebuild`] before the next local edit is recorded.
    pub fn merge_from(&mut self, other: &Replica) -> Result<(), String> {
        other.doc.commit();
        let updates = other.doc.export(ExportMode::updates(&self.doc.oplog_vv())).map_err(|e| e.to_string())?;
        self.doc.import(&updates).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn meta(&self, node: TreeID) -> LoroMap {
        self.tree.get_meta(node).expect("a node of this tree")
    }

    // ---------------------------------------------------------------- local edits

    /// Record into the document an op group that has just been applied to
    /// this replica's deck (the one [`Replica::new`] took or
    /// [`Replica::rebuild`] returned), as one change.
    pub fn record(&mut self, ops: &[Op]) -> Result<(), OpError> {
        // A slide shown although the document has it deleted (every slide
        // was deleted at once) comes back with the next local edit.
        for s in &self.deck {
            if let Some(node) = self.nodes.get(&s.ids.slide) {
                set_deleted(&self.meta(*node), false);
            }
        }
        for op in ops {
            if apply(&mut self.deck, op)?.is_empty() {
                continue; // a target that is gone: the op did nothing
            }
            self.write(op);
        }
        self.doc.commit();
        Ok(())
    }

    fn slide_at(&self, id: u64) -> usize {
        self.deck.iter().position(|s| s.ids.slide == id).expect("an applied op's slide")
    }

    /// The nodes of the slides (or of slide `si`'s objects) either side of
    /// position `at`.
    fn neighbours(&self, ids: &[u64], at: usize) -> (Option<TreeID>, Option<TreeID>) {
        let node = |i: usize| ids.get(i).and_then(|id| self.nodes.get(id)).copied();
        (at.checked_sub(1).and_then(node), node(at + 1))
    }

    /// A new node under `parent`, between `prev` and `next`.
    fn create(&self, parent: TreeID, prev: Option<TreeID>, next: Option<TreeID>) -> TreeID {
        let kids = self.tree.children(parent).unwrap_or_default();
        let index = match (prev, next) {
            (Some(p), _) => kids.iter().position(|k| *k == p).map_or(kids.len(), |i| i + 1),
            (None, Some(n)) => kids.iter().position(|k| *k == n).unwrap_or(0),
            (None, None) => kids.len(),
        };
        self.tree.create_at(parent, index).expect("tree create")
    }

    /// Move `node` under `parent`, between `prev` and `next`. Only this
    /// node moves, so a concurrent move of another node isn't undone.
    fn place(&self, node: TreeID, parent: TreeID, prev: Option<TreeID>, next: Option<TreeID>) {
        match (prev, next) {
            (Some(p), _) => self.tree.mov_after(node, p).expect("tree move"),
            (None, Some(n)) => self.tree.mov_before(node, n).expect("tree move"),
            (None, None) => {
                if self.tree.parent(node) != Some(TreeParentId::from(parent)) {
                    self.tree.mov(node, parent).expect("tree move");
                }
            }
        }
    }

    /// Put `order`'s nodes in that order under `parent`, moving as few as
    /// it takes: the nodes already in order stay put.
    fn order(&self, parent: TreeID, order: &[TreeID]) {
        let wanted: HashSet<TreeID> = order.iter().copied().collect();
        let now: Vec<TreeID> = self.tree.children(parent).unwrap_or_default().into_iter().filter(|k| wanted.contains(k)).collect();
        if now == order {
            return;
        }
        for (j, node) in order.iter().enumerate() {
            match j.checked_sub(1).map(|i| order[i]) {
                Some(prev) => self.tree.mov_after(*node, prev).expect("tree move"),
                None => self.place(*node, parent, None, now.first().copied().filter(|n| n != node)),
            }
        }
    }

    fn write_slide(&self, node: TreeID, s: &Slide) {
        let meta = self.meta(node);
        put(&meta, "title", &s.title);
        put(&meta, "bg", &s.background);
        put(&meta, "notes", &s.notes);
        put(&meta, "master", &json(&s.master_idx));
        put(&meta, "tr", &json(&s.transition));
        let builds: Vec<StoredBuild> = s
            .builds
            .iter()
            .filter_map(|b| Some((self.nodes.get(s.ids.objects.get(b.object)?)?.to_string(), b.effect, b.out)))
            .collect();
        put(&meta, "builds", &json(&builds));
    }

    /// Write `op`, just applied to the replica's deck, into the tree.
    fn write(&mut self, op: &Op) {
        match op {
            Op::InsertSlide { at, slide } => {
                let s = self.deck[*at].clone();
                let slide_ids: Vec<u64> = self.deck.iter().map(|s| s.ids.slide).collect();
                let (prev, next) = self.neighbours(&slide_ids, *at);
                // An id the op brings is an undone delete: the same node
                // comes back. A new slide gets its id from `apply`.
                let known = if slide.ids.slide != 0 { self.nodes.get(&s.ids.slide).copied() } else { None };
                let node = match known {
                    Some(node) => {
                        set_deleted(&self.meta(node), false);
                        self.place(node, self.root, prev, next);
                        node
                    }
                    None => {
                        let node = self.create(self.root, prev, next);
                        self.nodes.insert(s.ids.slide, node);
                        node
                    }
                };
                let mut objects = Vec::new();
                for (object, id) in s.objects.iter().zip(&s.ids.objects) {
                    let o = match self.nodes.get(id).copied() {
                        Some(o) => {
                            set_deleted(&self.meta(o), false);
                            o
                        }
                        None => {
                            let o = self.tree.create(node).expect("tree create");
                            self.nodes.insert(*id, o);
                            o
                        }
                    };
                    write_object(&self.meta(o), object);
                    objects.push(o);
                }
                self.order(node, &objects);
                self.write_slide(node, &s);
            }
            Op::DeleteSlide { slide } | Op::DeleteObject { id: slide, .. } => {
                if let Some(node) = self.nodes.get(slide) {
                    set_deleted(&self.meta(*node), true);
                }
            }
            Op::MoveSlide { slide, to } => {
                let slide_ids: Vec<u64> = self.deck.iter().map(|s| s.ids.slide).collect();
                let (prev, next) = self.neighbours(&slide_ids, *to);
                self.place(self.nodes[slide], self.root, prev, next);
            }
            Op::SetSlide { slide, .. } => {
                let s = &self.deck[self.slide_at(*slide)];
                self.write_slide(self.nodes[slide], s);
            }
            Op::InsertObject { slide, at, id, .. } => {
                let s = self.deck[self.slide_at(*slide)].clone();
                let parent = self.nodes[slide];
                let (prev, next) = self.neighbours(&s.ids.objects, *at);
                let node = match self.nodes.get(id).copied() {
                    Some(node) => {
                        set_deleted(&self.meta(node), false);
                        self.place(node, parent, prev, next);
                        node
                    }
                    None => {
                        let node = self.create(parent, prev, next);
                        self.nodes.insert(*id, node);
                        node
                    }
                };
                write_object(&self.meta(node), &s.objects[*at]);
            }
            Op::MoveObject { slide, id, to } => {
                let s = &self.deck[self.slide_at(*slide)];
                let (prev, next) = self.neighbours(&s.ids.objects, *to);
                self.place(self.nodes[id], self.nodes[slide], prev, next);
            }
            Op::SetObject { slide, id, .. } => {
                let s = &self.deck[self.slide_at(*slide)];
                let at = s.ids.objects.iter().position(|o| o == id).expect("an applied op's object");
                write_object(&self.meta(self.nodes[id]), &s.objects[at]);
            }
        }
    }

    // ---------------------------------------------------------------- the document's deck

    /// The deck the document holds, built from nothing but the document,
    /// with fresh local ids. It becomes this replica's deck.
    pub fn rebuild(&mut self) -> Result<Vec<Slide>, String> {
        self.doc.commit();
        let all = self.tree.children(self.root).unwrap_or_default();
        let mut live: Vec<TreeID> = all.iter().copied().filter(|n| !is_deleted(&self.meta(*n))).collect();
        // A deck keeps one slide: if every slide is deleted (two peers each
        // deleted a different one), the first stays, on every peer alike.
        if live.is_empty() {
            live.extend(all.first());
        }
        let mut nodes = HashMap::new();
        let mut next = 0u64;
        let mut id_for = |node: TreeID| {
            next += 1;
            nodes.insert(next, node);
            next
        };
        let mut deck = Vec::new();
        for node in live {
            let meta = self.meta(node);
            let mut s = Slide {
                title: string_at(&meta, "title").unwrap_or_default(),
                background: string_at(&meta, "bg").unwrap_or_default(),
                objects: Vec::new(),
                notes: string_at(&meta, "notes").unwrap_or_default(),
                master_idx: string_at(&meta, "master").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(None),
                transition: string_at(&meta, "tr").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Transition::None),
                builds: Vec::new(),
                ids: Default::default(),
            };
            s.ids.slide = id_for(node);
            let mut index_of: HashMap<String, usize> = HashMap::new();
            for o in self.tree.children(node).unwrap_or_default() {
                let meta = self.meta(o);
                if is_deleted(&meta) {
                    continue;
                }
                let Some(object) = read_object(&meta) else { continue };
                index_of.insert(o.to_string(), s.objects.len());
                s.objects.push(object);
                s.ids.objects.push(id_for(o));
            }
            let stored: Vec<StoredBuild> = string_at(&meta, "builds").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            s.builds = stored
                .into_iter()
                .filter_map(|(o, effect, out)| Some(Build { object: *index_of.get(&o)?, effect, out }))
                .collect();
            deck.push(s);
        }
        if deck.is_empty() {
            return Err("the document has no slides".into());
        }
        self.nodes = nodes;
        self.deck = deck.clone();
        Ok(deck)
    }
}

#[cfg(test)]
#[path = "collab_tests.rs"]
mod tests;
