//! Decks workload: a 50-slide deck, 30 nodes per slide with nested groups
//! (slide -> group -> group -> object), and two peers moving objects between
//! slides, regrouping them and changing z-order concurrently.
//!
//! decks-core's `Slide::objects` is a flat Vec today; the groups are here
//! because grouping is the obvious next step for Decks and is where a tree
//! CRDT earns its keep (cycles need a parent to move into a child).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::rng::Rng;

pub const SLIDES: usize = 50;
pub const ROOT: &str = "root";

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    F(f64),
    S(String),
}

impl Val {
    fn render(&self) -> String {
        match self {
            Val::F(f) => format!("{f:?}"),
            Val::S(s) => format!("{s:?}"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum DOp {
    Add { id: String, parent: String, index: usize, z: f64, fields: Vec<(&'static str, Val)> },
    Set { id: String, field: &'static str, val: Val },
    /// `index` is the position among the new parent's children *after* the
    /// node is taken out (Loro's `mov_to` semantics). `z` is the fractional
    /// position the parent-pointer encodings store instead.
    Move { id: String, parent: String, index: usize, z: f64 },
    Delete { id: String },
}

#[derive(Clone, Debug)]
pub struct Node {
    pub parent: String,
    pub children: Vec<String>,
    pub z: f64,
    pub fields: BTreeMap<&'static str, Val>,
    pub deleted: bool,
}

impl Node {
    pub fn kind(&self) -> &str {
        match self.fields.get("kind") {
            Some(Val::S(s)) => s,
            _ => "",
        }
    }
}

/// One peer's view of the deck.
#[derive(Clone)]
pub struct Tree {
    pub nodes: BTreeMap<String, Node>,
}

impl Tree {
    pub fn new() -> Tree {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            ROOT.to_string(),
            Node { parent: String::new(), children: vec![], z: 0.0, fields: BTreeMap::new(), deleted: false },
        );
        Tree { nodes }
    }

    pub fn apply(&mut self, op: &DOp) {
        match op {
            DOp::Add { id, parent, index, z, fields } => {
                let node = Node {
                    parent: parent.clone(),
                    children: vec![],
                    z: *z,
                    fields: fields.iter().cloned().collect(),
                    deleted: false,
                };
                self.nodes.insert(id.clone(), node);
                self.nodes.get_mut(parent).unwrap().children.insert(*index, id.clone());
            }
            DOp::Set { id, field, val } => {
                self.nodes.get_mut(id).unwrap().fields.insert(field, val.clone());
            }
            DOp::Move { id, parent, index, z } => {
                let old = self.nodes[id].parent.clone();
                self.nodes.get_mut(&old).unwrap().children.retain(|c| c != id);
                self.nodes.get_mut(parent).unwrap().children.insert(*index, id.clone());
                let n = self.nodes.get_mut(id).unwrap();
                n.parent = parent.clone();
                n.z = *z;
            }
            DOp::Delete { id } => {
                let old = self.nodes[id].parent.clone();
                self.nodes.get_mut(&old).unwrap().children.retain(|c| c != id);
                self.nodes.get_mut(id).unwrap().deleted = true;
            }
        }
    }

    /// Fractional position for `index` among `parent`'s children, ignoring
    /// `moving` (which is about to leave its current slot).
    pub fn z_for(&self, parent: &str, index: usize, moving: Option<&str>) -> f64 {
        let sibs: Vec<f64> = self.nodes[parent]
            .children
            .iter()
            .filter(|c| Some(c.as_str()) != moving)
            .map(|c| self.nodes[c].z)
            .collect();
        let left = if index > 0 { sibs.get(index - 1).copied() } else { None };
        let right = sibs.get(index).copied();
        match (left, right) {
            (None, None) => 0.0,
            (Some(l), None) => l + 1.0,
            (None, Some(r)) => r - 1.0,
            (Some(l), Some(r)) => (l + r) / 2.0,
        }
    }

    pub fn is_ancestor<'a>(&'a self, anc: &str, mut node: &'a str) -> bool {
        loop {
            if node == anc {
                return true;
            }
            match self.nodes.get(node) {
                Some(n) if !n.parent.is_empty() => node = &n.parent,
                _ => return false,
            }
        }
    }

    fn slide_of<'a>(&'a self, mut node: &'a str) -> String {
        loop {
            let n = &self.nodes[node];
            if n.parent == ROOT {
                return node.to_string();
            }
            node = &n.parent;
        }
    }

    pub fn dump(&self) -> String {
        let mut out = String::new();
        dump_node(self, ROOT, &mut out);
        out
    }
}

fn dump_node(t: &Tree, id: &str, out: &mut String) {
    let n = &t.nodes[id];
    out.push_str(id);
    out.push('{');
    for (k, v) in &n.fields {
        out.push_str(k);
        out.push('=');
        out.push_str(&v.render());
        out.push(',');
    }
    out.push('}');
    if !n.children.is_empty() {
        out.push('[');
        for c in &n.children {
            dump_node(t, c, out);
        }
        out.push(']');
    }
}

/// A generic node-list form every backend can produce: `(id, parent, z,
/// deleted, fields)`. Tree-shaped backends fill `z` with the sibling index.
pub struct FlatNode {
    pub id: String,
    pub parent: String,
    pub order: f64,
    pub deleted: bool,
    pub fields: BTreeMap<&'static str, Val>,
}

/// What a backend reports after reading its document back.
pub struct DeckRead {
    pub dump: String,
    /// Ids met on a walk from the root, with multiplicity (duplicates show).
    pub visible: Vec<String>,
    /// Live (not deleted) nodes that no walk from the root reaches, because
    /// their parent chain loops (a cycle) or points at a missing node.
    pub in_cycles: usize,
    pub dangling: usize,
    /// Visible id -> the parent(s) it appears under (several if duplicated).
    pub parents: HashMap<String, Vec<String>>,
}

impl DeckRead {
    pub fn from_tree(t: &Tree, visible: Vec<String>, in_cycles: usize, dangling: usize) -> DeckRead {
        let base = |k: &str| k.split('#').next().unwrap_or(k).to_string();
        let mut parents: HashMap<String, Vec<String>> = HashMap::new();
        for (k, n) in &t.nodes {
            if k != ROOT {
                parents.entry(base(k)).or_default().push(base(&n.parent));
            }
        }
        DeckRead { dump: t.dump(), visible, in_cycles, dangling, parents }
    }
}

/// Rebuild the visible tree from parent pointers (Automerge/yrs encoding).
pub fn read_parent_pointers(flat: Vec<FlatNode>) -> DeckRead {
    let by_id: HashMap<&str, &FlatNode> = flat.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut kids: HashMap<&str, Vec<&FlatNode>> = HashMap::new();
    for n in &flat {
        if !n.deleted {
            kids.entry(n.parent.as_str()).or_default().push(n);
        }
    }
    for v in kids.values_mut() {
        v.sort_by(|a, b| a.order.total_cmp(&b.order).then_with(|| a.id.cmp(&b.id)));
    }
    let mut t = Tree::new();
    let mut visible = Vec::new();
    let mut stack = vec![ROOT.to_string()];
    while let Some(p) = stack.pop() {
        for n in kids.get(p.as_str()).map(Vec::as_slice).unwrap_or(&[]) {
            visible.push(n.id.clone());
            t.nodes.insert(
                n.id.clone(),
                Node { parent: p.clone(), children: vec![], z: n.order, fields: n.fields.clone(), deleted: false },
            );
            t.nodes.get_mut(&p).unwrap().children.push(n.id.clone());
            stack.push(n.id.clone());
        }
    }
    let seen: HashSet<&str> = visible.iter().map(String::as_str).collect();
    let (mut in_cycles, mut dangling) = (0, 0);
    for n in flat.iter().filter(|n| !n.deleted && !seen.contains(n.id.as_str())) {
        // Walk up: reaching a deleted ancestor is legitimate (the subtree was
        // deleted); a loop is a cycle; a missing parent is dangling.
        let mut cur = n;
        let mut steps = 0;
        loop {
            if cur.deleted {
                break;
            }
            match by_id.get(cur.parent.as_str()) {
                _ if cur.parent == ROOT => break,
                Some(p) => cur = p,
                None => {
                    dangling += 1;
                    break;
                }
            }
            steps += 1;
            if steps > flat.len() {
                in_cycles += 1;
                break;
            }
        }
    }
    DeckRead::from_tree(&t, visible, in_cycles, dangling)
}

fn obj_fields(rng: &mut Rng, kind: &'static str, n: usize) -> Vec<(&'static str, Val)> {
    let mut f = vec![
        ("kind", Val::S(kind.into())),
        ("x", Val::F(rng.below(1200) as f64)),
        ("y", Val::F(rng.below(700) as f64)),
    ];
    if kind != "group" {
        f.push(("w", Val::F(rng.between(20, 600) as f64)));
        f.push(("h", Val::F(rng.between(20, 400) as f64)));
        f.push(("rot", Val::F(0.0)));
    }
    if kind == "text" {
        f.push(("text", Val::S(format!("Point {n} about the quarterly results"))));
    }
    f
}

const KINDS: [&str; 4] = ["text", "rect", "circle", "image"];

/// The base deck, as the ops that build it. 30 nodes per slide:
/// 12 top-level objects, groups gA{3 objects, gB{4}} and gC{3, gD{4}}.
pub fn base_ops(seed: u64) -> Vec<DOp> {
    let mut rng = Rng::new(seed);
    let mut t = Tree::new();
    let mut ops = Vec::new();
    let mut n = 0usize;
    fn push(t: &mut Tree, ops: &mut Vec<DOp>, op: DOp) {
        t.apply(&op);
        ops.push(op);
    }
    for s in 0..SLIDES {
        let sid = format!("s{s}");
        let z = t.z_for(ROOT, s, None);
        let fields = vec![("kind", Val::S("slide".into())), ("title", Val::S(format!("Slide {s}")))];
        push(&mut t, &mut ops, DOp::Add { id: sid.clone(), parent: ROOT.into(), index: s, z, fields });
        let mut add = |t: &mut Tree, ops: &mut Vec<DOp>, parent: &str, group: bool| {
            n += 1;
            let kind = if group { "group" } else { KINDS[n % 4] };
            let id = if group { format!("g{n:x}") } else { format!("o{n:x}") };
            let index = t.nodes[parent].children.len();
            let z = t.z_for(parent, index, None);
            let fields = obj_fields(&mut rng, kind, n);
            push(t, ops, DOp::Add { id: id.clone(), parent: parent.into(), index, z, fields });
            id
        };
        for _ in 0..2 {
            for _ in 0..6 {
                add(&mut t, &mut ops, &sid, false);
            }
            let g1 = add(&mut t, &mut ops, &sid, true);
            for _ in 0..3 {
                add(&mut t, &mut ops, &g1, false);
            }
            let g2 = add(&mut t, &mut ops, &g1, true);
            for _ in 0..4 {
                add(&mut t, &mut ops, &g2, false);
            }
        }
    }
    ops
}

pub fn base_tree(ops: &[DOp]) -> Tree {
    let mut t = Tree::new();
    for op in ops {
        t.apply(op);
    }
    t
}

pub struct DSession {
    pub actions: Vec<Vec<DOp>>,
    pub final_tree: Tree,
}

impl DSession {
    pub fn op_count(&self) -> usize {
        self.actions.iter().map(Vec::len).sum()
    }
}

fn live_nodes(t: &Tree, pred: impl Fn(&Node) -> bool) -> Vec<String> {
    // Live = reachable from the root (not deleted, not under a deleted node).
    let mut out = Vec::new();
    let mut stack = vec![ROOT.to_string()];
    while let Some(p) = stack.pop() {
        for c in &t.nodes[&p].children {
            if pred(&t.nodes[c]) {
                out.push(c.clone());
            }
            stack.push(c.clone());
        }
    }
    out.sort();
    out
}

fn mv(t: &Tree, id: &str, parent: &str, index: usize) -> DOp {
    let z = t.z_for(parent, index, Some(id));
    DOp::Move { id: id.into(), parent: parent.into(), index, z }
}

fn max_index(t: &Tree, id: &str, parent: &str) -> usize {
    let n = t.nodes[parent].children.len();
    if t.nodes[id].parent == parent {
        n - 1
    } else {
        n
    }
}

/// Random editing actions for one peer, continuing from `tree`.
pub fn gen_random(seed: u64, peer: char, tree: &Tree, n_actions: usize) -> DSession {
    let mut rng = Rng::new(seed);
    let mut t = tree.clone();
    let mut actions = Vec::new();
    let mut added = 0usize;
    let slides: Vec<String> = t.nodes[ROOT].children.clone();
    for _ in 0..n_actions {
        let objects = live_nodes(&t, |n| !matches!(n.kind(), "slide" | "group"));
        let groups = live_nodes(&t, |n| n.kind() == "group");
        let movable: Vec<&String> = objects.iter().chain(groups.iter()).collect();
        let slides_now = t.nodes[ROOT].children.clone();
        let mut action = Vec::new();
        let o = rng.pick(&objects).clone();
        match rng.below(100) {
            0..=29 => {
                action.push(DOp::Set { id: o.clone(), field: "x", val: Val::F(rng.below(1200) as f64) });
                action.push(DOp::Set { id: o, field: "y", val: Val::F(rng.below(700) as f64) });
            }
            30..=39 => {
                action.push(DOp::Set { id: o.clone(), field: "w", val: Val::F(rng.between(20, 600) as f64) });
                action.push(DOp::Set { id: o, field: "h", val: Val::F(rng.between(20, 400) as f64) });
            }
            40..=44 => action.push(DOp::Set { id: o, field: "rot", val: Val::F(rng.below(360) as f64) }),
            45..=52 => {
                let text = format!("Edited by {peer} #{}", rng.below(10_000));
                action.push(DOp::Set { id: o, field: "text", val: Val::S(text) });
            }
            // z-order within the same parent (bring forward / send back ...)
            53..=67 => {
                let id = rng.pick(&movable).to_string();
                let p = t.nodes[&id].parent.clone();
                let idx = rng.below(max_index(&t, &id, &p) + 1);
                action.push(mv(&t, &id, &p, idx));
            }
            // move to another slide
            68..=82 => {
                let id = rng.pick(&movable).to_string();
                let s = rng.pick(&slides_now).clone();
                let idx = rng.below(max_index(&t, &id, &s) + 1);
                action.push(mv(&t, &id, &s, idx));
            }
            // regroup: into a group that is not inside the node itself
            83..=87 => {
                let id = rng.pick(&movable).to_string();
                let targets: Vec<&String> = groups.iter().filter(|g| !t.is_ancestor(&id, g)).collect();
                if let Some(g) = targets.get(rng.below(targets.len().max(1))) {
                    let idx = rng.below(max_index(&t, &id, g) + 1);
                    action.push(mv(&t, &id, g, idx));
                }
            }
            88..=92 => {
                added += 1;
                let parent = if rng.chance(0.7) { rng.pick(&slides_now).clone() } else { rng.pick(&groups).clone() };
                let index = rng.below(t.nodes[&parent].children.len() + 1);
                let z = t.z_for(&parent, index, None);
                let kind = *rng.pick(&KINDS);
                let fields = obj_fields(&mut rng, kind, added);
                action.push(DOp::Add { id: format!("{peer}n{added:x}"), parent, index, z, fields });
            }
            93..=97 => action.push(DOp::Delete { id: o }),
            _ => {
                let s = rng.pick(&slides).clone();
                let idx = rng.below(SLIDES);
                action.push(mv(&t, &s, ROOT, idx));
            }
        }
        for op in &action {
            t.apply(op);
        }
        if !action.is_empty() {
            actions.push(action);
        }
    }
    DSession { actions, final_tree: t }
}

/// The deliberately conflicting actions each peer performs first, from the
/// same base: the cases where tree CRDTs differ.
pub struct Adversarial {
    pub a: Vec<Vec<DOp>>,
    pub b: Vec<Vec<DOp>>,
    pub tree_a: Tree,
    pub tree_b: Tree,
    pub summary: Vec<(&'static str, usize)>,
}

pub fn gen_adversarial(seed: u64, base: &Tree) -> Adversarial {
    let mut rng = Rng::new(seed);
    let (mut ta, mut tb) = (base.clone(), base.clone());
    let (mut a, mut b) = (Vec::new(), Vec::new());
    let slides: Vec<String> = base.nodes[ROOT].children.clone();
    // Top-level objects of each slide, consumed so no object is reused.
    let mut pool: Vec<String> = slides
        .iter()
        .flat_map(|s| base.nodes[s].children.iter().filter(|c| base.nodes[*c].kind() != "group").cloned())
        .collect();
    let mut take = |rng: &mut Rng| pool.remove(rng.below(pool.len()));
    let mut step = |ta: &mut Tree, tb: &mut Tree, oa: DOp, ob: DOp| {
        ta.apply(&oa);
        tb.apply(&ob);
        a.push(vec![oa]);
        b.push(vec![ob]);
    };
    for _ in 0..20 {
        let o = take(&mut rng);
        let cur = ta.slide_of(&o);
        let others: Vec<&String> = slides.iter().filter(|s| **s != cur).collect();
        let (p, q) = (rng.pick(&others).to_string(), rng.pick(&others).to_string());
        let (ia, ib) = (rng.below(max_index(&ta, &o, &p) + 1), rng.below(max_index(&tb, &o, &q) + 1));
        let (oa, ob) = (mv(&ta, &o, &p, ia), mv(&tb, &o, &q, ib));
        step(&mut ta, &mut tb, oa, ob);
    }
    for k in 0..5 {
        // top-level group of slide 2k and of slide 2k+1
        let g = |t: &Tree, s: &str| {
            t.nodes[s].children.iter().find(|c| t.nodes[*c].kind() == "group").unwrap().clone()
        };
        let (g1, g2) = (g(&ta, &slides[2 * k + 10]), g(&ta, &slides[2 * k + 11]));
        let (oa, ob) = (mv(&ta, &g1, &g2, 0), mv(&tb, &g2, &g1, 0));
        step(&mut ta, &mut tb, oa, ob);
    }
    for _ in 0..5 {
        let o = take(&mut rng);
        let cur = tb.slide_of(&o);
        let q = slides.iter().find(|s| **s != cur).unwrap().clone();
        let ob = mv(&tb, &o, &q, 0);
        step(&mut ta, &mut tb, DOp::Delete { id: o }, ob);
    }
    for _ in 0..10 {
        let o = take(&mut rng);
        let p = ta.nodes[&o].parent.clone();
        let last = max_index(&ta, &o, &p);
        let (oa, ob) = (mv(&ta, &o, &p, last), mv(&tb, &o, &p, 0));
        step(&mut ta, &mut tb, oa, ob);
    }
    let summary = vec![
        ("same object moved to different slides", 20),
        ("group A into B while B into A (cycle)", 5),
        ("delete vs concurrent move", 5),
        ("bring-to-front vs send-to-back", 10),
    ];
    Adversarial { a, b, tree_a: ta, tree_b: tb, summary }
}

/// Every node that should still exist after merging two sessions: created
/// (base or by either peer) and not explicitly deleted by either.
pub fn expected_alive(base: &Tree, sessions: &[&[Vec<DOp>]]) -> (HashSet<String>, HashSet<String>) {
    let mut alive: HashSet<String> = base.nodes.keys().filter(|k| *k != ROOT).cloned().collect();
    let mut deleted = HashSet::new();
    for s in sessions {
        for op in s.iter().flatten() {
            match op {
                DOp::Add { id, .. } => {
                    alive.insert(id.clone());
                }
                DOp::Delete { id } => {
                    deleted.insert(id.clone());
                }
                _ => {}
            }
        }
    }
    for d in &deleted {
        alive.remove(d);
    }
    (alive, deleted)
}

#[derive(Debug, Default)]
pub struct WellFormed {
    pub visible: usize,
    pub duplicated: usize,
    pub missing: usize,
    pub resurrected: usize,
    pub in_cycles: usize,
    pub dangling: usize,
}

impl WellFormed {
    pub fn ok(&self) -> bool {
        self.duplicated == 0 && self.missing == 0 && self.in_cycles == 0 && self.dangling == 0
    }
}

pub fn well_formed(read: &DeckRead, alive: &HashSet<String>, deleted: &HashSet<String>) -> WellFormed {
    let uniq: HashSet<&String> = read.visible.iter().collect();
    WellFormed {
        visible: uniq.len(),
        duplicated: read.visible.len() - uniq.len(),
        missing: alive.iter().filter(|id| !uniq.contains(id)).count(),
        resurrected: deleted.iter().filter(|id| uniq.contains(id)).count(),
        in_cycles: read.in_cycles,
        dangling: read.dangling,
    }
}
