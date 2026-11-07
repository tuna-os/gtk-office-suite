//! A sender's power level AT the event (spec 8.1 reader rule, question 15).
//!
//! A reader that judges a DCP event's sender by the room's *current* power
//! levels drops what a since-demoted editor legitimately published (and then
//! the owner's later save built on it), and accepts what a since-promoted user
//! forged earlier (Spike F; Spike Q15 in `docs/rfc/0004-spike-q15-results.md`).
//! The level has to be the one in force at the event. Five ways to get it are
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
//!   the walk holds when it reaches it. No extra request. Needs the pass to
//!   reach back to the room's creation.
//! * [`SenderRule::PrevContentWalk`]: the same stream read newest to oldest,
//!   starting from the newest table and undoing each `m.room.power_levels`
//!   event through its `unsigned.prev_content`. Needs no history older than the
//!   event being judged.
//! * [`SenderRule::Hybrid`] (the default): the forward walk, and `/context` for
//!   the events the walk cannot place (a window of the history that does not
//!   reach back to the room's creation).
//!
//! And the two rules they replace, kept as negative controls:
//! [`SenderRule::CurrentTable`] (draft 1's rule) and [`SenderRule::Trust`] (no
//! check).
//!
//! The walks and [`power_from_state`] are pure functions of events: they run
//! offline from a cached transcript ([`judge_transcript`]).

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
    /// [`SenderRule::TimelineWalk`], then [`SenderRule::Context`] for each event
    /// the walk could not place.
    Hybrid,
    /// [`SenderRule::TimelineWalk`] without the check that each power-levels event
    /// replaces the one before it in the pass (negative control: silently wrong
    /// when the reader cannot see a power-levels change).
    TimelineWalkUnverified,
}

impl Default for SenderRule {
    fn default() -> Self {
        SenderRule::Hybrid
    }
}

impl SenderRule {
    /// The rules that judge by the level in force at the event.
    pub const AT_EVENT: [SenderRule; 5] = [
        SenderRule::Context,
        SenderRule::SdkContext,
        SenderRule::TimelineWalk,
        SenderRule::PrevContentWalk,
        SenderRule::Hybrid,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SenderRule::Trust => "trust-sender",
            SenderRule::CurrentTable => "current-table",
            SenderRule::Context => "context",
            SenderRule::SdkContext => "sdk-context",
            SenderRule::TimelineWalk => "timeline-walk",
            SenderRule::PrevContentWalk => "prev-content-walk",
            SenderRule::Hybrid => "hybrid",
            SenderRule::TimelineWalkUnverified => "timeline-walk-unverified",
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

/// What `power` asks of an event of `event_type` (`events[event_type]`, else
/// `events_default` for a timeline event and `state_default` for a state event).
pub fn required_level(power: &Power, event_type: &str, is_state: bool) -> i64 {
    power.events.get(event_type).copied().unwrap_or(if is_state { power.state_default } else { power.events_default })
}

/// `power`'s verdict on a sender and an event of `event_type` (a state event or
/// not): the sender's level (`users[sender]`, else `users_default`) and what the
/// table asks for the event type.
pub fn level_for(power: &Power, sender: &str, event_type: &str, is_state: bool) -> Level {
    Level { level: power.level_of(sender), required: required_level(power, event_type, is_state) }
}

/// [`level_for`] for a timeline event, which is what a DCP version or update is
/// (`wire_type` is what the homeserver sees: `m.room.encrypted` in an encrypted room).
pub fn level_in(power: &Power, sender: &str, wire_type: &str) -> Level {
    level_for(power, sender, wire_type, false)
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
///
/// With `verify_chain` a table is also trusted only while the NEXT
/// power-levels event of the pass says it replaced exactly this one
/// (`unsigned.replaces_state` is this event's ID): when the history the reader
/// may see hides a power-levels change (a room whose history visibility is
/// `joined`, a reader who joined after it), the walk would otherwise judge the
/// events between the two visible tables by a table that was no longer in
/// force, silently. The events between them are `None`. Without `verify_chain`
/// (a negative control) they are judged by the stale table.
pub(crate) fn walk_forward(marks: &[Mark], targets: &[Target], verify_chain: bool) -> HashMap<String, Option<Level>> {
    let mut pls: Vec<&Mark> = marks.iter().filter(|m| is_state(&m.event, POWER_LEVELS)).collect();
    pls.sort_by(|a, b| b.pos.cmp(&a.pos));
    // Positions of the power-levels events whose successor in the pass does not replace them.
    let mut stale_after: Vec<usize> = Vec::new();
    if verify_chain {
        for pair in pls.windows(2) {
            let (this, next) = (pair[0], pair[1]);
            let linked = match (this.event["event_id"].as_str(), next.event["unsigned"]["replaces_state"].as_str()) {
                (Some(id), Some(replaces)) => id == replaces,
                _ => false,
            };
            if !linked {
                stale_after.push(this.pos);
            }
        }
    }
    let mut all = steps(marks, targets);
    all.sort_by(|a, b| b.0.cmp(&a.0));
    let mut create: Option<&Value> = None;
    let mut table: Option<Power> = None;
    // False between two visible tables when the later one does not replace the earlier one.
    let mut trusted = true;
    let mut out = HashMap::new();
    for (pos, step) in all {
        match step {
            Step::Mark(ev) if is_state(ev, CREATE) => create = Some(ev),
            Step::Mark(ev) if is_state(ev, POWER_LEVELS) => {
                table = create.map(|c| power_from_events(c, ev));
                trusted = !stale_after.contains(&pos);
            }
            Step::Mark(_) => {}
            Step::Target(t) => {
                let level = table.as_ref().filter(|_| trusted).map(|p| level_in(p, &t.sender, &t.wire_type));
                out.insert(t.id.clone(), level);
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

/// The pass shows the room's `m.room.create` with `m.federate` false: every
/// event of the room came from the creator's homeserver, so the order of the
/// pass is the order of the room's state and a walk of it is exact. For a room
/// other homeservers can join (or when the create event is not in the pass) the
/// state at an event is the result of state resolution and only the server
/// can say what it was.
pub(crate) fn non_federated(marks: &[Mark]) -> bool {
    marks.iter().any(|m| is_state(&m.event, CREATE) && m.event["content"]["m.federate"].as_bool() == Some(false))
}

/// The walks over a cached `/messages` pass, with no client and no network: the
/// raw events of the pass, newest first (state and timeline events alike, as
/// the server returned them), and for each timeline event (judged by the type
/// the server saw, `m.room.encrypted` in an encrypted room) the level its
/// sender had at it, or `None` when the cache does not hold what is needed
/// (never a guess from some other table). `rule` is
/// [`SenderRule::PrevContentWalk`] or, for any other value, the forward walk.
pub fn judge_transcript(rule: SenderRule, events: &[Value]) -> HashMap<String, Option<Level>> {
    let mut marks = Vec::new();
    let mut targets = Vec::new();
    for (i, ev) in events.iter().enumerate() {
        let pos = i + 1;
        let ty = ev["type"].as_str().unwrap_or("");
        if ev.get("state_key").is_some() {
            if ev["state_key"].as_str() == Some("") && (ty == CREATE || ty == POWER_LEVELS) {
                marks.push(Mark { pos, event: ev.clone() });
            }
        } else if let (Some(id), Some(sender)) = (ev["event_id"].as_str(), ev["sender"].as_str()) {
            targets.push(Target { pos, id: id.to_owned(), sender: sender.to_owned(), wire_type: ty.to_owned() });
        }
    }
    if rule == SenderRule::PrevContentWalk {
        let create = marks.iter().find(|m| is_state(&m.event, CREATE));
        let newest = marks.iter().filter(|m| is_state(&m.event, POWER_LEVELS)).min_by_key(|m| m.pos);
        let current = match (create, newest) {
            (Some(c), Some(p)) => power_from_events(&c.event, &p.event),
            _ => Power::default(),
        };
        walk_backward(&marks, &targets, &current)
    } else {
        walk_forward(&marks, &targets, true)
    }
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

    /// A power-levels event whose ID is derived from its content, so that the one
    /// built with `prev` = the earlier content `replaces_state` the earlier one.
    fn pl(content: Value, prev: Option<Value>) -> Value {
        let id = |c: &Value| format!("$pl-{c}");
        let mut ev = json!({ "type": POWER_LEVELS, "state_key": "", "sender": OWNER, "event_id": id(&content), "content": content });
        if let Some(p) = prev {
            ev["unsigned"] = json!({ "prev_content": p, "replaces_state": id(&p) });
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
        assert_eq!(verdicts(&walk_forward(&marks, &targets, true)), want);
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
        assert_eq!(verdicts(&walk_forward(&marks, &targets, true)), want);
        assert_eq!(verdicts(&walk_backward(&marks, &targets, &current)), want);
    }

    #[test]
    fn a_pass_without_the_create_event_is_unresolved_forward_but_not_backward() {
        let (marks, targets, current) = history("10");
        let no_create: Vec<Mark> = marks.into_iter().filter(|m| m.pos != 7).collect();
        let fwd = walk_forward(&no_create, &targets, true);
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
    fn a_state_event_is_asked_for_state_default_unless_the_table_names_its_type() {
        let mut t = table(json!({ OWNER: 100, ED: 50 }));
        t["state_default"] = json!(75);
        t["events_default"] = json!(10);
        t["events"] = json!({ "org.tunaos.dcp.lease": 50 });
        let p = power_from_content(&t, &[]);
        assert_eq!(level_for(&p, ED, "m.room.topic", true).required, 75, "state_default");
        assert_eq!(level_for(&p, ED, "m.room.encrypted", false).required, 10, "events_default");
        assert_eq!(level_for(&p, ED, "org.tunaos.dcp.lease", true).required, 50, "the table names the type");
        assert_eq!(level_for(&p, VW, "org.tunaos.dcp.lease", true).level, 0, "users_default");
        assert!(level_for(&p, ED, "org.tunaos.dcp.lease", true).permitted());
        assert!(!level_for(&p, ED, "m.room.topic", true).permitted(), "50 is below the state_default of 75");
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
        let fwd = walk_forward(&marks, &targets, true);
        assert_eq!(fwd["o"].map(|l| l.level), Some(i64::MAX));
        let mut current = power_from_content(&t, &[]);
        current.creators = vec![OWNER.to_owned()];
        let back = walk_backward(&marks, &targets, &current);
        assert_eq!(back["o"].map(|l| l.level), Some(i64::MAX));
    }

    #[test]
    fn only_a_room_that_does_not_federate_may_be_walked() {
        let mut c = create("10");
        assert!(!non_federated(&[Mark { pos: 1, event: c.clone() }]), "m.federate absent means true");
        c["content"]["m.federate"] = json!(false);
        assert!(non_federated(&[Mark { pos: 1, event: c.clone() }]));
        c["content"]["m.federate"] = json!(true);
        assert!(!non_federated(&[Mark { pos: 1, event: c }]));
        assert!(!non_federated(&[]), "no create event in the pass");
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

    #[test]
    fn a_cached_transcript_is_judged_offline_and_without_power_levels_it_is_not_judged() {
        // Newest first, as /messages returns it: ed, demoted, then ed's event; create and the tables before.
        let t1 = table(json!({ OWNER: 100, ED: 50 }));
        let t2 = table(json!({ OWNER: 100 }));
        let ev = |id: &str, sender: &str| json!({ "type": "m.room.encrypted", "event_id": id, "sender": sender, "content": {} });
        let transcript = vec![
            ev("$late", ED),
            pl(t2.clone(), Some(t1.clone())),
            ev("$early", ED),
            pl(t1, None),
            create("10"),
        ];
        // Through a string, as a cache on disk would hold it.
        let cached = serde_json::to_string(&transcript).unwrap();
        let events: Vec<Value> = serde_json::from_str(&cached).unwrap();
        for rule in [SenderRule::TimelineWalk, SenderRule::PrevContentWalk] {
            let v = judge_transcript(rule, &events);
            assert_eq!(v["$early"].map(|l| l.permitted()), Some(true), "{rule:?}");
            assert_eq!(v["$late"].map(|l| l.permitted()), Some(false), "{rule:?}");
        }
        // The control: drop the power-levels events from the cache and nothing can be judged.
        let stripped: Vec<Value> = events.into_iter().filter(|e| e["type"] != POWER_LEVELS).collect();
        let v = judge_transcript(SenderRule::TimelineWalk, &stripped);
        assert!(v.values().all(|l| l.is_none()), "{v:?}");
    }

    #[test]
    fn a_power_levels_change_the_reader_cannot_see_breaks_the_forward_walk_but_not_the_backward_one() {
        // PL0 is visible (ed is an editor), PL1 (ed demoted) is NOT in the pass, PL2 is visible and replaces PL1.
        let t0 = table(json!({ OWNER: 100, ED: 50 }));
        let t1 = table(json!({ OWNER: 100 }));
        let t2 = table(json!({ OWNER: 100, VW: 50 }));
        let marks = vec![
            Mark { pos: 6, event: create("10") },
            Mark { pos: 5, event: pl(t0, None) },
            Mark { pos: 2, event: pl(t2.clone(), Some(t1)) },
        ];
        // "between" is sent by the demoted ed, after PL1 and before PL2.
        let targets = vec![target(3, "between", ED), target(1, "after", OWNER)];
        let current = power_from_content(&t2, &[]);

        let checked = walk_forward(&marks, &targets, true);
        assert_eq!(checked["between"], None, "the walk must not judge it by PL0, which PL1 had replaced");
        assert_eq!(checked["after"].map(|l| l.permitted()), Some(true));
        // The control: without the chain check the same walk accepts the demoted editor's event.
        let unchecked = walk_forward(&marks, &targets, false);
        assert_eq!(unchecked["between"].map(|l| l.permitted()), Some(true), "the unverified walk is silently wrong here");
        // The backward walk has PL1 in PL2's prev_content and needs nothing else.
        let back = walk_backward(&marks, &targets, &current);
        assert_eq!(back["between"].map(|l| l.permitted()), Some(false));
        assert_eq!(back["after"].map(|l| l.permitted()), Some(true));
    }
}
