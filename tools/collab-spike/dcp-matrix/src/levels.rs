//! A sender's power level AT the event (spec 8.1 reader rule, question 15).
//!
//! A reader that judges a DCP event's sender by the room's *current* power
//! levels drops what a since-demoted editor legitimately published (and then
//! the owner's later save built on it), and accepts what a since-promoted user
//! forged earlier (Spike F; Spike Q15 in `docs/rfc/0004-spike-q15-results.md`).
//! The level has to be the one in force at the event. Four ways to get it are
//! implemented here and measured against a real Synapse:
//!
//! * [`SenderRule::Context`]: `GET /rooms/{id}/context/{eventId}?limit=0`, whose
//!   `state` is the room state at that event, one request per event, cached by
//!   event ID (state at an event never changes).
//! * [`SenderRule::SdkContext`]: the same endpoint through matrix-sdk's own
//!   `Room::event_with_context` (the SDK has no other state-at-event call).
//! * [`SenderRule::TimelineWalk`]: the reader already pages `/messages`, which
//!   carries `m.room.create` and every `m.room.power_levels` event in stream
//!   order; walk them oldest to newest and judge each DCP event by the table
//!   the walk holds when it reaches it. No extra request.
//! * [`SenderRule::PrevContentWalk`]: the same stream read newest to oldest,
//!   starting from the newest table and undoing each `m.room.power_levels`
//!   event through its `unsigned.prev_content`. Needs no history older than the
//!   event being judged.
//!
//! And the two rules it replaces, kept as negative controls:
//! [`SenderRule::CurrentTable`] (draft 1's rule) and [`SenderRule::Trust`] (no
//! check).
//!
//! The walks and [`power_from_state`] are pure functions of events: they run
//! offline from a cached transcript.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::state::{Power, Snapshot, LEVEL_EDITOR};

pub const POWER_LEVELS: &str = "m.room.power_levels";
pub const CREATE: &str = "m.room.create";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SenderRule {
    /// No check of the sender's level (a deliberate defect: negative control).
    Trust,
    /// The room's current power levels, with the server trusted when the
    /// current table gates `m.room.encrypted`: draft 1's rule, which Spike F
    /// showed wrong (negative control).
    CurrentTable,
    /// `/context/{eventId}?limit=0` (raw HTTP, state filtered to the create and
    /// power-levels events), cached by event ID.
    Context,
    /// matrix-sdk's `Room::event_with_context` with a context size of 0.
    SdkContext,
    /// Forward walk of the power-levels events of the `/messages` pass.
    TimelineWalk,
    /// Backward walk of the same pass through `unsigned.prev_content`.
    PrevContentWalk,
}

impl Default for SenderRule {
    fn default() -> Self {
        SenderRule::Context
    }
}

impl SenderRule {
    /// The rules that judge by the level in force at the event.
    pub const AT_EVENT: [SenderRule; 4] =
        [SenderRule::Context, SenderRule::SdkContext, SenderRule::TimelineWalk, SenderRule::PrevContentWalk];

    pub fn name(self) -> &'static str {
        match self {
            SenderRule::Trust => "trust-sender",
            SenderRule::CurrentTable => "current-table",
            SenderRule::Context => "context",
            SenderRule::SdkContext => "sdk-context",
            SenderRule::TimelineWalk => "timeline-walk",
            SenderRule::PrevContentWalk => "prev-content-walk",
        }
    }

    pub fn at_event(self) -> bool {
        Self::AT_EVENT.contains(&self)
    }
}

/// What the table in force at an event says about its sender.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Level {
    /// The sender's level: `users[sender]`, else `users_default` (a room
    /// version 12 creator has unbounded power).
    pub level: i64,
    /// What the table asks for the event's wire type (`events[type]`, else
    /// `events_default`).
    pub required: i64,
}

impl Level {
    /// The reader rule: the sender had the editor level, and the level the table
    /// itself asked for that event type, when the event was sent.
    pub fn permitted(&self) -> bool {
        self.level >= self.required.max(LEVEL_EDITOR)
    }
}

/// `power`'s verdict on a sender and an event of `wire_type` (what the
/// homeserver sees: `m.room.encrypted` in an encrypted room).
pub fn level_in(power: &Power, sender: &str, wire_type: &str) -> Level {
    Level {
        level: power.level_of(sender),
        required: power.events.get(wire_type).copied().unwrap_or(power.events_default),
    }
}

/// One judged event, for tests and logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SenderCheck {
    pub id: String,
    pub sender: String,
    /// `None`: the rule could not establish the table at the event.
    pub level: Option<Level>,
}

/// The table in a state-at-event answer; `None` when it holds no power-levels
/// event (the answer is then no evidence).
pub fn power_from_state(state: &[Value]) -> Option<Power> {
    let snap = Snapshot::from_events(state);
    if snap.power_levels_content.is_null() {
        None
    } else {
        Some(snap.power)
    }
}

/// A `m.room.create` or `m.room.power_levels` event of the pass, with the
/// position it had in it (1 is the newest event; a larger number is older).
#[derive(Clone, Debug)]
pub(crate) struct Mark {
    pub pos: usize,
    pub event: Value,
}

/// A DCP event to judge.
#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub pos: usize,
    pub id: String,
    pub sender: String,
    pub wire_type: String,
}

enum Step<'a> {
    Mark(&'a Value),
    Target(&'a Target),
}

fn steps<'a>(marks: &'a [Mark], targets: &'a [Target]) -> Vec<(usize, Step<'a>)> {
    let mut v: Vec<(usize, Step<'a>)> = marks.iter().map(|m| (m.pos, Step::Mark(&m.event))).collect();
    v.extend(targets.iter().map(|t| (t.pos, Step::Target(t))));
    v
}

fn power_from_events(create: &Value, pl: &Value) -> Power {
    Snapshot::from_events(&[create.clone(), pl.clone()]).power
}

fn power_from_content(content: &Value, creators: &[String]) -> Power {
    let ev = json!({ "type": POWER_LEVELS, "state_key": "", "content": content });
    let mut p = Snapshot::from_events(&[ev]).power;
    p.creators = creators.to_vec();
    p
}

fn is_state(ev: &Value, ty: &str) -> bool {
    ev["type"].as_str() == Some(ty)
}

/// `unsigned.prev_content` (Synapse sends it there; some servers at the top level).
fn prev_content(ev: &Value) -> Option<&Value> {
    [&ev["unsigned"]["prev_content"], &ev["prev_content"]].into_iter().find(|v| v.is_object())
}

/// Oldest to newest: `m.room.create` then each `m.room.power_levels` replaces
/// the table, and every target is judged by the table held at its position.
/// `None` for a target that no create plus power-levels pair precedes.
pub(crate) fn walk_forward(marks: &[Mark], targets: &[Target]) -> HashMap<String, Option<Level>> {
    let mut all = steps(marks, targets);
    all.sort_by(|a, b| b.0.cmp(&a.0));
    let mut create: Option<&Value> = None;
    let mut table: Option<Power> = None;
    let mut out = HashMap::new();
    for (_, step) in all {
        match step {
            Step::Mark(ev) if is_state(ev, CREATE) => create = Some(ev),
            Step::Mark(ev) if is_state(ev, POWER_LEVELS) => table = create.map(|c| power_from_events(c, ev)),
            Step::Mark(_) => {}
            Step::Target(t) => {
                out.insert(t.id.clone(), table.as_ref().map(|p| level_in(p, &t.sender, &t.wire_type)));
            }
        }
    }
    out
}

/// Newest to oldest: start from the newest power-levels event of the pass (or
/// `current` when the pass holds none: nothing changed since), and at each
/// power-levels event step back to the table in its `prev_content`. A target
/// older than a power-levels event without `prev_content` is `None`. Needs
/// neither the room's creation nor any power-levels event older than the one
/// in force at the oldest target.
pub(crate) fn walk_backward(marks: &[Mark], targets: &[Target], current: &Power) -> HashMap<String, Option<Level>> {
    let mut all = steps(marks, targets);
    all.sort_by(|a, b| a.0.cmp(&b.0));
    let newest = marks.iter().filter(|m| is_state(&m.event, POWER_LEVELS)).min_by_key(|m| m.pos);
    let mut table: Option<Power> = Some(match newest {
        Some(m) => power_from_content(&m.event["content"], &current.creators),
        None => current.clone(),
    });
    let mut out = HashMap::new();
    for (_, step) in all {
        match step {
            Step::Mark(ev) if is_state(ev, POWER_LEVELS) => {
                table = prev_content(ev).map(|c| power_from_content(c, &current.creators));
            }
            Step::Mark(_) => {}
            Step::Target(t) => {
                out.insert(t.id.clone(), table.as_ref().map(|p| level_in(p, &t.sender, &t.wire_type)));
            }
        }
    }
    out
}

/// Counters of the state-at-event requests of one store.
#[derive(Default, Debug)]
pub(crate) struct LevelCounters {
    pub requests: AtomicU64,
    pub cache_hits: AtomicU64,
    pub rate_limited: AtomicU64,
    pub waited_ms: AtomicU64,
    pub request_ms: AtomicU64,
    pub bytes: AtomicU64,
}

/// The cost of resolving senders, as counted by one store (or the difference of
/// two readings, [`LevelStats::since`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LevelStats {
    /// HTTP requests for a state at an event, including the ones answered 429.
    pub requests: u64,
    pub cache_hits: u64,
    /// Requests answered 429 (`M_LIMIT_EXCEEDED`).
    pub rate_limited: u64,
    /// Milliseconds slept for those 429s.
    pub waited_ms: u64,
    /// Milliseconds inside requests (summed, so more than the elapsed time when
    /// they overlap).
    pub request_ms: u64,
    /// Response body bytes (raw HTTP only; the SDK does not expose its own).
    pub bytes: u64,
}

impl LevelStats {
    pub fn since(self, then: LevelStats) -> LevelStats {
        LevelStats {
            requests: self.requests - then.requests,
            cache_hits: self.cache_hits - then.cache_hits,
            rate_limited: self.rate_limited - then.rate_limited,
            waited_ms: self.waited_ms - then.waited_ms,
            request_ms: self.request_ms - then.request_ms,
            bytes: self.bytes - then.bytes,
        }
    }
}

/// `(rule, event ID)` to the level. State at an event never changes, so an
/// entry never goes stale.
#[derive(Default, Debug)]
pub(crate) struct Levels {
    map: Mutex<HashMap<(SenderRule, String), Level>>,
    pub counters: LevelCounters,
}

impl Levels {
    pub fn get(&self, rule: SenderRule, id: &str) -> Option<Level> {
        self.map.lock().ok()?.get(&(rule, id.to_owned())).copied()
    }

    pub fn put(&self, rule: SenderRule, id: &str, level: Level) {
        if let Ok(mut m) = self.map.lock() {
            m.insert((rule, id.to_owned()), level);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut m) = self.map.lock() {
            m.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.map.lock().map(|m| m.len()).unwrap_or(0)
    }

    pub fn stats(&self) -> LevelStats {
        let c = &self.counters;
        LevelStats {
            requests: c.requests.load(Ordering::Relaxed),
            cache_hits: c.cache_hits.load(Ordering::Relaxed),
            rate_limited: c.rate_limited.load(Ordering::Relaxed),
            waited_ms: c.waited_ms.load(Ordering::Relaxed),
            request_ms: c.request_ms.load(Ordering::Relaxed),
            bytes: c.bytes.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "@owner:localhost";
    const ED: &str = "@ed:localhost";
    const VW: &str = "@vw:localhost";

    fn create(v: &str) -> Value {
        json!({ "type": CREATE, "state_key": "", "sender": OWNER, "content": { "room_version": v, "type": "org.tunaos.dcp.document" } })
    }

    fn table(users: Value) -> Value {
        json!({ "users": users, "users_default": 0, "events_default": 0, "state_default": 100, "events": {} })
    }

    fn pl(content: Value, prev: Option<Value>) -> Value {
        let mut ev = json!({ "type": POWER_LEVELS, "state_key": "", "sender": OWNER, "content": content });
        if let Some(p) = prev {
            ev["unsigned"] = json!({ "prev_content": p });
        }
        ev
    }

    fn target(pos: usize, id: &str, sender: &str) -> Target {
        Target { pos, id: id.into(), sender: sender.into(), wire_type: "m.room.encrypted".into() }
    }

    /// Oldest first: create, PL1 (ed is an editor), a, PL2 (ed demoted), f, PL3
    /// (ed promoted again), b. Positions count from the newest (1).
    fn history(v: &str) -> (Vec<Mark>, Vec<Target>, Power) {
        let t1 = table(json!({ OWNER: 100, ED: 50 }));
        let t2 = table(json!({ OWNER: 100 }));
        let marks = vec![
            Mark { pos: 7, event: create(v) },
            Mark { pos: 6, event: pl(t1.clone(), None) },
            Mark { pos: 4, event: pl(t2.clone(), Some(t1.clone())) },
            Mark { pos: 2, event: pl(t1.clone(), Some(t2)) },
        ];
        let targets = vec![target(5, "a", ED), target(3, "f", ED), target(1, "b", ED), target(1_000, "never", ED)];
        let current = power_from_content(&t1, &[]);
        (marks, targets, current)
    }

    fn verdicts(m: &HashMap<String, Option<Level>>) -> Vec<(String, Option<bool>)> {
        let mut v: Vec<_> = m.iter().map(|(k, l)| (k.clone(), l.map(|l| l.permitted()))).collect();
        v.sort();
        v
    }

    #[test]
    fn both_walks_judge_by_the_table_at_the_event() {
        let (marks, targets, current) = history("10");
        // "never" is older than everything: no table yet.
        let want = vec![
            ("a".to_owned(), Some(true)),
            ("b".to_owned(), Some(true)),
            ("f".to_owned(), Some(false)),
            ("never".to_owned(), None),
        ];
        assert_eq!(verdicts(&walk_forward(&marks, &targets)), want);
        assert_eq!(verdicts(&walk_backward(&marks, &targets, &current)), want);
    }

    #[test]
    fn the_current_table_would_get_the_demoted_events_wrong() {
        // The current table (ed promoted again) accepts f, which the walks drop:
        // the control that the test above can fail.
        let (_, _, current) = history("10");
        assert!(level_in(&current, ED, "m.room.encrypted").permitted());
    }

    #[test]
    fn a_later_promotion_does_not_validate_an_earlier_event() {
        let t0 = table(json!({ OWNER: 100 }));
        let t1 = table(json!({ OWNER: 100, VW: 50 }));
        let marks = vec![
            Mark { pos: 5, event: create("10") },
            Mark { pos: 4, event: pl(t0.clone(), None) },
            Mark { pos: 2, event: pl(t1.clone(), Some(t0)) },
        ];
        let targets = vec![target(3, "forged", VW), target(1, "legit", VW)];
        let current = power_from_content(&t1, &[]);
        let want = vec![("forged".to_owned(), Some(false)), ("legit".to_owned(), Some(true))];
        assert_eq!(verdicts(&walk_forward(&marks, &targets)), want);
        assert_eq!(verdicts(&walk_backward(&marks, &targets, &current)), want);
    }

    #[test]
    fn a_pass_without_the_create_event_is_unresolved_forward_but_not_backward() {
        let (marks, targets, current) = history("10");
        let no_create: Vec<Mark> = marks.into_iter().filter(|m| m.pos != 7).collect();
        let fwd = walk_forward(&no_create, &targets);
        assert!(fwd.values().all(|l| l.is_none()), "{fwd:?}");
        let back = walk_backward(&no_create, &targets, &current);
        assert_eq!(back["a"].map(|l| l.permitted()), Some(true));
        assert_eq!(back["f"].map(|l| l.permitted()), Some(false));
    }

    #[test]
    fn a_missing_prev_content_makes_older_targets_unresolved() {
        let (mut marks, targets, current) = history("10");
        for m in &mut marks {
            if m.pos == 4 {
                m.event["unsigned"] = Value::Null;
            }
        }
        let back = walk_backward(&marks, &targets, &current);
        assert_eq!(back["b"].map(|l| l.permitted()), Some(true));
        assert_eq!(back["f"].map(|l| l.permitted()), Some(false));
        assert_eq!(back["a"], None, "older than the power-levels event that lost its prev_content");
    }

    #[test]
    fn the_required_level_of_the_event_type_counts() {
        let mut t = table(json!({ OWNER: 100, ED: 50 }));
        t["events"] = json!({ "m.room.encrypted": 75 });
        let p = power_from_content(&t, &[]);
        assert!(!level_in(&p, ED, "m.room.encrypted").permitted());
        assert!(level_in(&p, OWNER, "m.room.encrypted").permitted());
        assert!(level_in(&p, ED, "org.tunaos.dcp.version").permitted());
        assert!(!level_in(&p, VW, "org.tunaos.dcp.version").permitted());
    }

    #[test]
    fn a_state_answer_without_power_levels_is_no_evidence() {
        assert!(power_from_state(&[create("10")]).is_none());
        let t = table(json!({ OWNER: 100, ED: 50 }));
        let p = power_from_state(&[create("10"), pl(t, None)]).expect("table");
        assert_eq!(p.level_of(ED), 50);
    }

    #[test]
    fn a_v12_creator_has_unbounded_power_in_both_walks() {
        let t = table(json!({ ED: 50 }));
        let marks = vec![Mark { pos: 3, event: create("12") }, Mark { pos: 2, event: pl(t.clone(), None) }];
        let targets = vec![target(1, "o", OWNER)];
        let fwd = walk_forward(&marks, &targets);
        assert_eq!(fwd["o"].map(|l| l.level), Some(i64::MAX));
        let mut current = power_from_content(&t, &[]);
        current.creators = vec![OWNER.to_owned()];
        let back = walk_backward(&marks, &targets, &current);
        assert_eq!(back["o"].map(|l| l.level), Some(i64::MAX));
    }

    #[test]
    fn the_cache_keeps_rules_apart() {
        let l = Levels::default();
        l.put(SenderRule::Context, "$e", Level { level: 50, required: 0 });
        assert!(l.get(SenderRule::Context, "$e").is_some());
        assert!(l.get(SenderRule::SdkContext, "$e").is_none());
        l.clear();
        assert_eq!(l.len(), 0);
    }
}
