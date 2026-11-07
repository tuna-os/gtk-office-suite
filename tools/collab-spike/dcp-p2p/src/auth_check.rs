//! Authorising an entry from the p2panda-auth state (a read-time rule; p2panda-spaces has none).
//!
//! p2panda-auth decides who may change membership. It has no notion of "may write a version":
//! every member holding the group key can encrypt anything. This file is what the binding adds:
//!
//! * (a) the author must have had at least the required access level in the group state at the
//!   causal point the author claims (`heads`, the auth operations it had processed), and
//! * (b) no removal or demotion of the author may exist that is outside the causal past of those
//!   heads (p2panda-auth's "strong removal": an action concurrent with the author's removal is
//!   invalid). Without (b) a removed member could keep writing by claiming an old causal point.
//!
//! Both are computed from the operation graph on every read, so a replica that learns of a removal
//! late reaches the same verdict as one that knew at once.

use std::collections::{BTreeSet, HashSet};

use p2panda_auth::group::{GroupAction, GroupMember};
use p2panda_auth::traits::Operation as AuthOp;
use p2panda_auth::AccessLevel;
use p2panda_core::{Hash, VerifyingKey};
use p2panda_spaces::SpacesStoreState;
use petgraph::visit::{Bfs, Reversed};

/// The author's access level in the group state after exactly the operations in `heads` (and
/// their ancestors). `None`: not a member there, or a head this replica does not know.
pub fn level_at(
    state: &SpacesStoreState<()>,
    heads: &BTreeSet<Hash>,
    who: &VerifyingKey,
) -> Option<AccessLevel> {
    let g = &state.groups_y.inner;
    if heads.is_empty() || heads.iter().any(|h| !g.operations.contains_key(h)) {
        return None;
    }
    let set: HashSet<Hash> = heads.iter().copied().collect();
    let at = g.state_at(&set).ok()?;
    let members = at.get(&state.group_id)?;
    members
        .access_levels()
        .into_iter()
        .find_map(|(m, a)| (m == GroupMember::Individual(*who)).then_some(a.level))
}

/// Is there a removal or demotion of `who` that the claimed causal point does not include?
pub fn removed_unseen(
    state: &SpacesStoreState<()>,
    heads: &BTreeSet<Hash>,
    who: &VerifyingKey,
) -> bool {
    let g = &state.groups_y.inner;
    let mut known: HashSet<Hash> = HashSet::new();
    let rev = Reversed(&g.graph);
    for h in heads {
        if !g.graph.contains_node(*h) {
            continue;
        }
        let mut bfs = Bfs::new(&rev, *h);
        while let Some(n) = bfs.next(&rev) {
            known.insert(n);
        }
    }
    for (id, op) in g.operations.iter() {
        if known.contains(id) || g.ignore.contains(id) {
            continue;
        }
        let hit = match op.action() {
            GroupAction::Remove { member } => member == GroupMember::Individual(*who),
            GroupAction::Demote { member, .. } => member == GroupMember::Individual(*who),
            _ => false,
        };
        if hit {
            return true;
        }
    }
    false
}

pub fn authorized(
    state: &SpacesStoreState<()>,
    heads: &BTreeSet<Hash>,
    who: &VerifyingKey,
    need: AccessLevel,
) -> bool {
    match level_at(state, heads, who) {
        Some(l) if l >= need => !removed_unseen(state, heads, who),
        _ => false,
    }
}
