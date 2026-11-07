//! The version graph: heads, parent validation and conflict detection.
//!
//! Spec 4.2 version rules 4 to 7 and spec section 6 (head selection, offline
//! divergence, merging).

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use crate::{Stamped, Version};

/// Spec 4.2: `parents` has at most 8 entries.
pub const MAX_PARENTS: usize = 8;

/// Why a version's `parents` is not acceptable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParentError {
    /// Version rule 6: lists itself.
    #[error("lists itself as a parent")]
    SelfParent,
    /// Version rule 6: lists a descendant (the parent chain loops back).
    #[error("lists a descendant, {0}, as a parent")]
    ListsDescendant(String),
    /// Version rule 6: lists an identifier that is not in the retained history.
    #[error("lists an unknown parent {0}")]
    UnknownParent(String),
    #[error("lists {0} twice")]
    DuplicateParent(String),
    #[error("lists {0} parents, at most {MAX_PARENTS} are allowed")]
    TooManyParents(usize),
    /// Version rules 4 and 7: only the first version has `parents: []`; a
    /// save is a child of the version the editor started from.
    #[error("has no parents but the document already has versions")]
    RootAfterFirst,
}

/// A version the reader rejected on structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    pub id: String,
    pub error: ParentError,
}

/// Newest first by binding timestamp (spec 6.1), the identifier breaking
/// ties so the order is deterministic.
pub fn newest_first(a: &Stamped<Version>, b: &Stamped<Version>) -> std::cmp::Ordering {
    (Reverse(a.ts_ms), Reverse(&a.id)).cmp(&(Reverse(b.ts_ms), Reverse(&b.id)))
}

/// Heads of `versions`: those no other listed version names as a parent
/// (spec section 3), newest first.
///
/// Pass the versions the reader accepted; a version already rejected must not
/// be able to demote its parent from head. Duplicate identifiers count once.
pub fn heads(versions: &[Stamped<Version>]) -> Vec<&Stamped<Version>> {
    let parents: HashSet<&str> = versions
        .iter()
        .flat_map(|v| v.body.parents.iter().map(String::as_str))
        .collect();
    let mut seen = HashSet::new();
    let mut out: Vec<&Stamped<Version>> = versions
        .iter()
        .filter(|v| !parents.contains(v.id.as_str()) && seen.insert(v.id.as_str()))
        .collect();
    out.sort_by(|a, b| newest_first(a, b));
    out
}

/// What opening a document finds (spec 6.1 and version rule 7).
#[derive(Debug, PartialEq, Eq)]
pub enum Heads<'a> {
    /// A document with no version has no content (rule 7).
    NoContent,
    Single(&'a Stamped<Version>),
    /// Rule 5: show the conflict, offer each head, newest first. Never pick
    /// one silently.
    Conflict(Vec<&'a Stamped<Version>>),
}

pub fn resolve(versions: &[Stamped<Version>]) -> Heads<'_> {
    let mut h = heads(versions);
    match h.len() {
        0 => Heads::NoContent,
        1 => Heads::Single(h.remove(0)),
        _ => Heads::Conflict(h),
    }
}

/// `Some(heads)`, newest first, when the document has more than one head.
pub fn conflict(versions: &[Stamped<Version>]) -> Option<Vec<&Stamped<Version>>> {
    match resolve(versions) {
        Heads::Conflict(h) => Some(h),
        _ => None,
    }
}

/// The `parents` of a merge version: every merged head (spec 6.3), in the
/// order given, which is what makes the merge a single head again.
pub fn merge_parents(heads: &[&Stamped<Version>]) -> Vec<String> {
    heads.iter().map(|h| h.id.clone()).collect()
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Version rule 6: "identifiers older than the retained history MAY be
    /// unknown". Set when the binding says it dropped old versions
    /// (spec 6.4); otherwise an unknown parent is a rejection.
    pub allow_unknown_parents: bool,
}

/// Check every version's parents (version rules 6, and the schema's
/// uniqueness and 8-parent bounds) and return the rejections.
///
/// Rejected versions are not cascaded: a child that lists a rejected version
/// is judged on its own parents, which exist (the store has them) even though
/// the reader discards them.
pub fn validate(versions: &[Stamped<Version>], options: Options) -> Vec<Rejected> {
    let known: HashSet<&str> = versions.iter().map(|v| v.id.as_str()).collect();
    let cyclic = on_cycles(versions);
    let mut out = Vec::new();
    for v in versions {
        if let Some(error) = check_one(v, &known, &cyclic, options) {
            out.push(Rejected {
                id: v.id.clone(),
                error,
            });
        }
    }
    out
}

fn check_one(
    v: &Stamped<Version>,
    known: &HashSet<&str>,
    cyclic: &HashMap<&str, String>,
    options: Options,
) -> Option<ParentError> {
    let parents = &v.body.parents;
    if parents.len() > MAX_PARENTS {
        return Some(ParentError::TooManyParents(parents.len()));
    }
    let mut seen = HashSet::new();
    for p in parents {
        if !seen.insert(p.as_str()) {
            return Some(ParentError::DuplicateParent(p.clone()));
        }
    }
    if parents.iter().any(|p| *p == v.id) {
        return Some(ParentError::SelfParent);
    }
    if let Some(descendant) = cyclic.get(v.id.as_str()) {
        return Some(ParentError::ListsDescendant(descendant.clone()));
    }
    if !options.allow_unknown_parents {
        if let Some(p) = parents.iter().find(|p| !known.contains(p.as_str())) {
            return Some(ParentError::UnknownParent(p.clone()));
        }
    }
    None
}

/// Versions that sit on a parent-chain cycle, each mapped to a parent of its
/// own that is on the same cycle. A cycle exists exactly when some version
/// lists a descendant (itself included, which `check_one` reports first).
///
/// Iterative Tarjan strongly-connected components over child to parent edges,
/// linear in versions plus parent links.
fn on_cycles(versions: &[Stamped<Version>]) -> HashMap<&str, String> {
    let index_of: HashMap<&str, usize> = versions
        .iter()
        .enumerate()
        .map(|(i, v)| (v.id.as_str(), i))
        .collect();
    let edges: Vec<Vec<usize>> = versions
        .iter()
        .map(|v| {
            v.body
                .parents
                .iter()
                .filter_map(|p| index_of.get(p.as_str()).copied())
                .collect()
        })
        .collect();

    let n = versions.len();
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut component = vec![usize::MAX; n];
    let mut stack = Vec::new();
    let mut counter = 0;
    let mut components = 0;

    for root in 0..n {
        if index[root] != usize::MAX {
            continue;
        }
        // (node, next edge to look at)
        let mut work = vec![(root, 0usize)];
        index[root] = counter;
        low[root] = counter;
        counter += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some((node, next)) = work.last_mut() {
            let node = *node;
            if *next < edges[node].len() {
                let to = edges[node][*next];
                *next += 1;
                if index[to] == usize::MAX {
                    index[to] = counter;
                    low[to] = counter;
                    counter += 1;
                    stack.push(to);
                    on_stack[to] = true;
                    work.push((to, 0));
                } else if on_stack[to] {
                    low[node] = low[node].min(index[to]);
                }
            } else {
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    low[parent] = low[parent].min(low[node]);
                }
                if low[node] == index[node] {
                    while let Some(member) = stack.pop() {
                        on_stack[member] = false;
                        component[member] = components;
                        if member == node {
                            break;
                        }
                    }
                    components += 1;
                }
            }
        }
    }

    let mut size = vec![0usize; components];
    for c in &component {
        size[*c] += 1;
    }
    let mut out = HashMap::new();
    for (i, v) in versions.iter().enumerate() {
        if size[component[i]] > 1 {
            if let Some(&p) = edges[i].iter().find(|&&p| component[p] == component[i]) {
                out.insert(v.id.as_str(), versions[p].id.clone());
            }
        }
    }
    out
}

/// Split `versions` into the ones the reader accepts and the rejections.
pub fn split(
    versions: &[Stamped<Version>],
    options: Options,
) -> (Vec<Stamped<Version>>, Vec<Rejected>) {
    let rejected = validate(versions, options);
    let bad: HashSet<&str> = rejected.iter().map(|r| r.id.as_str()).collect();
    let accepted = versions
        .iter()
        .filter(|v| !bad.contains(v.id.as_str()))
        .cloned()
        .collect();
    (accepted, rejected)
}

/// Spec 6.4 retention: drop versions stamped before `cutoff_ms`, but keep
/// every head and every version a retained version lists as a parent. (The
/// parents' own parents may then be unknown, which is what
/// [`Options::allow_unknown_parents`] is for.)
pub fn prune(versions: &[Stamped<Version>], cutoff_ms: u64) -> Vec<Stamped<Version>> {
    let head_ids: HashSet<&str> = heads(versions).iter().map(|h| h.id.as_str()).collect();
    let kept = |v: &Stamped<Version>| v.ts_ms >= cutoff_ms || head_ids.contains(v.id.as_str());
    let parents_of_kept: HashSet<&str> = versions
        .iter()
        .filter(|v| kept(v))
        .flat_map(|v| v.body.parents.iter().map(String::as_str))
        .collect();
    versions
        .iter()
        .filter(|v| kept(v) || parents_of_kept.contains(v.id.as_str()))
        .cloned()
        .collect()
}

/// The writer's side of the rules: `parents` for a version about to be
/// published on top of `existing`. A save is a child of what the editor
/// started from (rule 4), so every parent must be a version the writer can
/// see; only the first version is parentless (rule 7). Parents must be
/// unique and at most 8. Self and descendant cannot arise for a new version.
pub fn check_new_parents(
    existing: &[Stamped<Version>],
    parents: &[String],
) -> Result<(), ParentError> {
    if parents.len() > MAX_PARENTS {
        return Err(ParentError::TooManyParents(parents.len()));
    }
    let known: HashSet<&str> = existing.iter().map(|v| v.id.as_str()).collect();
    let mut seen = HashSet::new();
    for p in parents {
        if !seen.insert(p.as_str()) {
            return Err(ParentError::DuplicateParent(p.clone()));
        }
        if !known.contains(p.as_str()) {
            return Err(ParentError::UnknownParent(p.clone()));
        }
    }
    if parents.is_empty() && !existing.is_empty() {
        return Err(ParentError::RootAfterFirst);
    }
    Ok(())
}

/// The members of `members` that no other member descends from, once each, in
/// the order given: the smallest set of parents that has every one of them as
/// an ancestor-or-self (a version already reachable through another parent is
/// left out, so a child of `[b]` is not also a child of `a` when `b` descends
/// from `a`). Identifiers not in `versions` are kept as they are (their own
/// ancestors are unknown).
pub fn antichain(versions: &[Stamped<Version>], members: &[String]) -> Vec<String> {
    let parents: HashMap<&str, &[String]> = versions
        .iter()
        .map(|v| (v.id.as_str(), v.body.parents.as_slice()))
        .collect();
    let mut reachable: HashSet<&str> = HashSet::new();
    for m in members {
        // Every proper ancestor of `m`.
        let mut seen: HashSet<&str> = HashSet::new();
        let mut stack: Vec<&str> = parents
            .get(m.as_str())
            .map(|p| p.iter().map(String::as_str).collect())
            .unwrap_or_default();
        while let Some(a) = stack.pop() {
            if !seen.insert(a) {
                continue;
            }
            if let Some(ps) = parents.get(a) {
                stack.extend(ps.iter().map(String::as_str));
            }
        }
        reachable.extend(
            seen.into_iter()
                .filter(|a| *a != m.as_str()),
        );
    }
    let mut seen = HashSet::new();
    members
        .iter()
        .filter(|m| !reachable.contains(m.as_str()) && seen.insert(m.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{integrity, Author, Locator};

    fn v(id: &str, ts: u64, parents: &[&str]) -> Stamped<Version> {
        Stamped::new(
            id,
            ts,
            "@a:x",
            Version {
                dcp: 1,
                parents: parents.iter().map(|p| p.to_string()).collect(),
                file: integrity::file_ref(
                    "text/plain",
                    id.as_bytes(),
                    Locator::Url {
                        href: "https://x/y".into(),
                    },
                ),
                created_by: Author {
                    principal: "@a:x".into(),
                    device: "D".into(),
                },
                message: None,
                covers: None,
            },
        )
    }

    fn ids<'a>(h: &[&'a Stamped<Version>]) -> Vec<&'a str> {
        h.iter().map(|v| v.id.as_str()).collect()
    }

    #[test]
    fn empty_document_has_no_content_rule_7() {
        assert_eq!(resolve(&[]), Heads::NoContent);
        assert!(heads(&[]).is_empty());
        assert!(conflict(&[]).is_none());
    }

    #[test]
    fn a_chain_has_one_head() {
        let vs = [v("a", 1, &[]), v("b", 2, &["a"]), v("c", 3, &["b"])];
        assert_eq!(ids(&heads(&vs)), ["c"]);
        assert!(matches!(resolve(&vs), Heads::Single(h) if h.id == "c"));
    }

    #[test]
    fn two_versions_sharing_a_parent_are_two_heads_rule_5() {
        let vs = [v("a", 1, &[]), v("b", 2, &["a"]), v("c", 3, &["a"])];
        let c = conflict(&vs).expect("must be a conflict");
        assert_eq!(ids(&c), ["c", "b"], "newest first by binding timestamp");
        assert!(matches!(resolve(&vs), Heads::Conflict(h) if h.len() == 2));
    }

    #[test]
    fn heads_are_ordered_newest_first_with_a_deterministic_tiebreak() {
        let vs = [
            v("a", 1, &[]),
            v("x", 5, &["a"]),
            v("y", 5, &["a"]),
            v("z", 9, &["a"]),
        ];
        assert_eq!(ids(&heads(&vs)), ["z", "y", "x"]);
        let rev: Vec<_> = vs.iter().rev().cloned().collect();
        assert_eq!(
            ids(&heads(&rev)),
            ["z", "y", "x"],
            "input order must not matter"
        );
    }

    #[test]
    fn merging_every_head_returns_to_one_head_spec_6_3() {
        let mut vs = vec![v("a", 1, &[]), v("b", 2, &["a"]), v("c", 3, &["a"])];
        let parents = merge_parents(&heads(&vs));
        assert_eq!(parents, ["c", "b"]);
        let p: Vec<&str> = parents.iter().map(String::as_str).collect();
        vs.push(v("m", 4, &p));
        assert_eq!(ids(&heads(&vs)), ["m"]);
        assert!(conflict(&vs).is_none());
    }

    #[test]
    fn merging_only_some_heads_leaves_a_conflict() {
        let vs = [
            v("a", 1, &[]),
            v("b", 2, &["a"]),
            v("c", 3, &["a"]),
            v("d", 4, &["a"]),
            v("m", 5, &["b", "c"]),
        ];
        assert_eq!(ids(&heads(&vs)), ["m", "d"]);
    }

    #[test]
    fn a_valid_graph_has_no_rejections() {
        let vs = [v("a", 1, &[]), v("b", 2, &["a"]), v("c", 3, &["a", "b"])];
        assert!(validate(&vs, Options::default()).is_empty());
    }

    #[test]
    fn self_parent_is_rejected_rule_6() {
        let vs = [v("a", 1, &[]), v("b", 2, &["a", "b"])];
        let r = validate(&vs, Options::default());
        assert_eq!(
            r,
            [Rejected {
                id: "b".into(),
                error: ParentError::SelfParent
            }]
        );
    }

    #[test]
    fn listing_a_descendant_is_rejected_rule_6() {
        // a lists c, but c descends from b which descends from a.
        let vs = [v("a", 1, &["c"]), v("b", 2, &["a"]), v("c", 3, &["b"])];
        let r = validate(&vs, Options::default());
        let bad: HashSet<&str> = r.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(
            bad,
            HashSet::from(["a", "b", "c"]),
            "all three sit on the loop"
        );
        assert!(r
            .iter()
            .all(|x| matches!(x.error, ParentError::ListsDescendant(_))));
    }

    #[test]
    fn two_cycle_is_rejected_and_an_innocent_child_is_not() {
        let vs = [v("a", 1, &["b"]), v("b", 2, &["a"]), v("c", 3, &["b"])];
        let r = validate(&vs, Options::default());
        let bad: HashSet<&str> = r.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(bad, HashSet::from(["a", "b"]));
        let (accepted, _) = split(&vs, Options::default());
        assert_eq!(ids(&heads(&accepted)), ["c"]);
    }

    #[test]
    fn unknown_parent_is_rejected_unless_history_was_trimmed() {
        let vs = [v("b", 2, &["gone"]), v("c", 3, &["b"])];
        let r = validate(&vs, Options::default());
        assert_eq!(
            r,
            [Rejected {
                id: "b".into(),
                error: ParentError::UnknownParent("gone".into())
            }]
        );
        assert!(validate(
            &vs,
            Options {
                allow_unknown_parents: true
            }
        )
        .is_empty());
    }

    #[test]
    fn a_rejected_version_cannot_demote_its_parent_from_head() {
        // `bad` claims `a` as parent and also an unknown one: rejected, so `a`
        // is still the head of what the reader accepted.
        let vs = [v("a", 1, &[]), v("bad", 2, &["a", "nowhere"])];
        let (accepted, rejected) = split(&vs, Options::default());
        assert_eq!(rejected.len(), 1);
        assert_eq!(ids(&heads(&accepted)), ["a"]);
    }

    #[test]
    fn duplicates_and_too_many_parents_are_rejected() {
        let many: Vec<String> = (0..9).map(|i| format!("p{i}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        let mut vs: Vec<_> = many.iter().map(|p| v(p, 1, &[])).collect();
        vs.push(v("big", 2, &refs));
        vs.push(v("dup", 3, &["p0", "p0"]));
        let r = validate(&vs, Options::default());
        assert!(r.contains(&Rejected {
            id: "big".into(),
            error: ParentError::TooManyParents(9)
        }));
        assert!(r.contains(&Rejected {
            id: "dup".into(),
            error: ParentError::DuplicateParent("p0".into())
        }));
    }

    #[test]
    fn long_chain_validates_without_recursion_limits() {
        let mut vs = vec![v("n0", 0, &[])];
        for i in 1..20_000u64 {
            let parent = format!("n{}", i - 1);
            vs.push(v(&format!("n{i}"), i, &[parent.as_str()]));
        }
        assert!(validate(&vs, Options::default()).is_empty());
        assert_eq!(heads(&vs).len(), 1);
    }

    #[test]
    fn writer_side_parents_must_be_known_unique_and_nonempty_after_the_first() {
        let vs = [v("a", 1, &[]), v("b", 2, &["a"])];
        check_new_parents(&vs, &["b".into()]).unwrap();
        check_new_parents(&vs, &["a".into(), "b".into()]).unwrap();
        check_new_parents(&[], &[]).unwrap();
        assert_eq!(
            check_new_parents(&vs, &[]),
            Err(ParentError::RootAfterFirst)
        );
        assert_eq!(
            check_new_parents(&vs, &["zz".into()]),
            Err(ParentError::UnknownParent("zz".into()))
        );
        assert_eq!(
            check_new_parents(&[], &["a".into()]),
            Err(ParentError::UnknownParent("a".into()))
        );
        assert_eq!(
            check_new_parents(&vs, &["a".into(), "a".into()]),
            Err(ParentError::DuplicateParent("a".into()))
        );
        let nine: Vec<String> = (0..9).map(|i| format!("p{i}")).collect();
        assert_eq!(
            check_new_parents(&vs, &nine),
            Err(ParentError::TooManyParents(9))
        );
    }

    #[test]
    fn an_old_version_never_overwritten_rule_4_a_save_adds_a_child() {
        // Publishing on top of `a` adds a child; `a` stays in the retained
        // set and the new version is one of two heads.
        let mut vs = vec![v("a", 1, &[]), v("b", 2, &["a"])];
        check_new_parents(&vs, &["a".into()]).unwrap();
        vs.push(v("c", 3, &["a"]));
        assert!(vs.iter().any(|x| x.id == "a"));
        assert_eq!(ids(&heads(&vs)), ["c", "b"]);
    }

    #[test]
    fn retention_keeps_every_head_and_the_parents_of_what_it_keeps_rule_6_4() {
        // a(1) <- b(2) <- c(3) <- d(10); e(4) is a stale second head.
        let vs = [
            v("a", 1, &[]),
            v("b", 2, &["a"]),
            v("c", 3, &["b"]),
            v("d", 10, &["c"]),
            v("e", 4, &["c"]),
        ];
        let kept = prune(&vs, 10);
        let kept_ids: HashSet<&str> = kept.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(
            kept_ids,
            HashSet::from(["c", "d", "e"]),
            "d by age, e as a head, c as their parent; a and b dropped"
        );
        assert_eq!(
            ids(&heads(&kept)),
            ["d", "e"],
            "no head was lost, conflict still visible"
        );
        // the dropped ancestors are now unknown, which the reader must tolerate
        // only when the binding says history was trimmed
        assert!(validate(&kept, Options::default())
            .iter()
            .all(|r| r.id == "c"));
        assert!(validate(
            &kept,
            Options {
                allow_unknown_parents: true
            }
        )
        .is_empty());
    }

    #[test]
    fn retention_with_nothing_old_drops_nothing() {
        let vs = [v("a", 5, &[]), v("b", 6, &["a"])];
        assert_eq!(prune(&vs, 0).len(), 2);
    }

    #[test]
    fn a_second_root_is_a_second_head_not_a_silent_choice_rule_7() {
        // Only the first version has no parents; a second parentless version
        // is not a child of anything, so it is shown as a conflict.
        let vs = [v("a", 1, &[]), v("z", 2, &[])];
        assert!(conflict(&vs).is_some());
    }

    fn strings(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn an_antichain_drops_every_member_another_member_descends_from() {
        // a <- b <- c, a <- d, and e is a second root.
        let vs = [
            v("a", 1, &[]),
            v("b", 2, &["a"]),
            v("c", 3, &["b"]),
            v("d", 4, &["a"]),
            v("e", 5, &[]),
        ];
        assert_eq!(antichain(&vs, &strings(&["a", "c"])), ["c"]);
        assert_eq!(antichain(&vs, &strings(&["a", "b", "c"])), ["c"]);
        assert_eq!(antichain(&vs, &strings(&["c", "d"])), ["c", "d"]);
        assert_eq!(antichain(&vs, &strings(&["a", "c", "d"])), ["c", "d"]);
        assert_eq!(antichain(&vs, &strings(&["c", "e", "c"])), ["c", "e"]);
        assert_eq!(antichain(&vs, &strings(&["a"])), ["a"]);
        assert!(antichain(&vs, &[]).is_empty());
        // An identifier the graph does not know has no known ancestors.
        assert_eq!(antichain(&vs, &strings(&["x", "d"])), ["x", "d"]);
    }

    #[test]
    fn an_antichain_of_a_long_chain_does_not_recurse() {
        let mut vs = vec![v("n0", 0, &[])];
        for i in 1..20_000u64 {
            let (id, parent) = (format!("n{i}"), format!("n{}", i - 1));
            vs.push(v(&id, i, &[parent.as_str()]));
        }
        assert_eq!(antichain(&vs, &strings(&["n0", "n19999"])), ["n19999"]);
    }
}
