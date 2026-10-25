//! Spike Q15 (spec 8.1, question 15): a reader judges a DCP event's sender by
//! the power level IN FORCE AT THE EVENT, not by the room's current table.
//!
//! Every scenario builds one room history on a real Synapse (events from
//! several principals while the owner changes their levels), writes down for
//! each event whether a correct reader accepts it ([`Ledger`]), and then reads
//! the room with
//!
//! * every at-event rule ([`SenderRule::AT_EVENT`]: `/context`, matrix-sdk's
//!   `event_with_context`, the forward walk of the power-levels events, the
//!   backward walk through `prev_content`) as two live readers, which must
//!   get every verdict right and agree on every level they found, and
//! * the rules they replace, as negative controls: draft 1's current-table
//!   reader and a reader that trusts the sender must get the specific events
//!   of the scenario WRONG (a control that does not fail fails the check).
//!
//! Scenarios 2 to 5 need a room whose table does not gate `m.room.encrypted`
//! (`RigOptions::draft0_power_levels`): that is where Synapse accepts the
//! forged events and only the reader can drop them (each forgery's acceptance
//! by the server is asserted, so the check cannot pass vacuously).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use dcp_conformance::{ensure, new_file, Failure, ALGORITHM, DOC_MODEL};
use dcp_core::{graph, updates, Author, DocId, DocumentStore, FileRef, Role, Update, Version};
use dcp_matrix::{MatrixStore, ReadReport, SenderRule};
use serde_json::json;

use crate::checks::{Check, CheckFuture};
use crate::profile;
use crate::rig::{Rig, Who};

type R = Result<(), Failure>;

macro_rules! scenarios {
    ($($name:ident),+ $(,)?) => {
        /// Every scenario, in this file's order.
        pub const ALL: &[Check] = &[
            $(Check { name: stringify!($name), run: |r| Box::pin($name(r)) }),+
        ];
    };
}

scenarios!(
    an_editor_demoted_after_publishing_keeps_their_version_and_the_owners_later_save,
    a_demoted_editors_later_events_are_dropped_and_their_earlier_ones_stay,
    a_later_promotion_does_not_validate_an_earlier_forged_event,
    a_late_joiner_sees_the_verdicts_of_a_live_reader,
    power_levels_changed_several_times_in_a_burst,
);

pub fn by_name(name: &str) -> &'static Check {
    ALL.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no scenario named {name}"))
}

// ---------------------------------------------------------------- ledger and readings

struct Entry {
    id: String,
    label: String,
    /// What a correct reader does with it.
    accepted: bool,
}

/// Every version and update of a scenario, with the verdict a correct reader
/// gives it.
struct Ledger {
    entries: Vec<Entry>,
}

impl Ledger {
    fn new(first: &str) -> Self {
        Ledger { entries: vec![Entry { id: first.to_owned(), label: "the first version".into(), accepted: true }] }
    }

    fn accept(&mut self, label: &str, id: &str) {
        self.entries.push(Entry { id: id.to_owned(), label: label.to_owned(), accepted: true });
    }

    fn drop_it(&mut self, label: &str, id: &str) {
        self.entries.push(Entry { id: id.to_owned(), label: label.to_owned(), accepted: false });
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    /// The events `reading` gets wrong, each as `"<label> (<id>, expected …)"`.
    fn wrong(&self, reading: &Reading) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| (reading.listed.contains(&e.id) || reading.updates.contains(&e.id)) != e.accepted)
            .map(|e| format!("{} ({}, expected {})", e.label, short(&e.id), if e.accepted { "accepted" } else { "dropped" }))
            .collect()
    }

    /// Some entry's label starts with `prefix` (so a typo in a control cannot pass).
    fn knows(&self, prefix: &str) -> bool {
        self.entries.iter().any(|e| e.label.starts_with(prefix))
    }
}

fn short(id: &str) -> &str {
    id.get(..9).unwrap_or(id)
}

/// What one reader sees under one rule.
struct Reading {
    /// Versions the reader accepts: not dropped for their sender, and with
    /// parents it knows (version rule 6).
    listed: BTreeSet<String>,
    updates: BTreeSet<String>,
    heads: Vec<String>,
    report: ReadReport,
    secs: f64,
}

async fn reading(store: &MatrixStore, doc: &DocId, rule: SenderRule) -> Result<Reading, Failure> {
    let t = Instant::now();
    let report = store.read_report_with(doc, rule).await?;
    let secs = t.elapsed().as_secs_f64();
    let (accepted, _rejected) = graph::split(&report.versions, graph::Options::default());
    let heads: Vec<String> = graph::heads(&accepted).iter().map(|v| v.id.clone()).collect();
    let listed: BTreeSet<String> = accepted.iter().map(|v| v.id.clone()).collect();
    let updates: BTreeSet<String> = report.updates.iter().map(|u| u.id.clone()).collect();
    Ok(Reading { listed, updates, heads, report, secs })
}

/// Wait until `store` has every event of the ledger decrypted and judged.
async fn ready(rig: &Rig, store: &MatrixStore, ledger: &Ledger) -> R {
    rig.until(&format!("{} to have read and judged all {} events", store.principal(), ledger.len()), || async {
        let rep = store.read_report_with(&rig.doc, SenderRule::TimelineWalk).await?;
        Ok(if rep.undecryptable == 0 && rep.checks.len() == ledger.len() {
            Ok(())
        } else {
            Err(format!("undecryptable {}, judged {} of {}", rep.undecryptable, rep.checks.len(), ledger.len()))
        })
    })
    .await
}

fn log_reading(rig: &Rig, scenario: &str, reader: &str, rule: SenderRule, r: &Reading, wrong: &[String]) {
    let s = &r.report.level_stats;
    println!(
        "READING {scenario} | reader {reader} | rule {:<17} | wrong {} | dropped {} unresolved {} | {:.2}s sender-check {} ms | requests {} cache-hits {} 429s {}",
        rule.name(),
        wrong.len(),
        r.report.rejected_unauthorised,
        r.report.rejected_unresolved,
        r.secs,
        r.report.sender_check_ms,
        s.requests,
        s.cache_hits,
        s.rate_limited,
    );
    crate::common::record(
        &rig.syn,
        "q15_reading",
        json!({
            "scenario": scenario, "reader": reader, "rule": rule.name(), "label": profile::label(&rig.syn),
            "wrong": wrong, "dropped_unauthorised": r.report.rejected_unauthorised,
            "unresolved": r.report.rejected_unresolved, "secs": r.secs, "sender_check_ms": r.report.sender_check_ms,
            "requests": s.requests, "cache_hits": s.cache_hits, "rate_limited": s.rate_limited, "bytes": s.bytes,
        }),
    );
}

fn levels_of(r: &Reading) -> BTreeMap<String, Option<(i64, i64)>> {
    r.report.checks.iter().map(|c| (c.id.clone(), c.level.map(|l| (l.level, l.required)))).collect()
}

/// `store` under every at-event rule: every verdict right, nothing unresolved,
/// the expected heads, and every rule found the same level for every event.
async fn at_event_readers_are_right(
    rig: &Rig,
    scenario: &str,
    reader: &str,
    store: &MatrixStore,
    ledger: &Ledger,
    heads: &[String],
) -> Result<BTreeMap<&'static str, Reading>, Failure> {
    let mut out = BTreeMap::new();
    for rule in SenderRule::AT_EVENT {
        let r = reading(store, &rig.doc, rule).await?;
        let wrong = ledger.wrong(&r);
        log_reading(rig, scenario, reader, rule, &r, &wrong);
        ensure!(
            wrong.is_empty(),
            "{scenario}: reader {reader} under the {} rule got events wrong: {wrong:?} (dropped as unauthorised {:?}, unresolved {})",
            rule.name(),
            r.report.unauthorised_ids,
            r.report.rejected_unresolved
        );
        ensure!(
            r.report.rejected_unresolved == 0,
            "{scenario}: reader {reader}, {}: {} events unresolved",
            rule.name(),
            r.report.rejected_unresolved
        );
        ensure!(r.heads == heads, "{scenario}: reader {reader}, {}: heads {:?}, expected {heads:?}", rule.name(), r.heads);
        out.insert(rule.name(), r);
    }
    let mut it = out.iter();
    let (first_name, first) = it.next().expect("rules");
    let want = levels_of(first);
    for (name, r) in it {
        let got = levels_of(r);
        ensure!(
            got == want,
            "{scenario}: reader {reader}: the {name} rule found other levels than the {first_name} rule: {got:?} against {want:?}"
        );
    }
    Ok(out)
}

/// The negative control: `rule` must get at least the events labelled
/// `must_be_wrong` wrong, or the check cannot tell a right reader from this one.
async fn control_fails(
    rig: &Rig,
    scenario: &str,
    reader: &str,
    store: &MatrixStore,
    ledger: &Ledger,
    rule: SenderRule,
    must_be_wrong: &[&str],
) -> R {
    let r = reading(store, &rig.doc, rule).await?;
    let wrong = ledger.wrong(&r);
    log_reading(rig, &format!("{scenario} CONTROL"), reader, rule, &r, &wrong);
    for label in must_be_wrong {
        ensure!(ledger.knows(label), "{scenario}: the control names \"{label}\", which no event of the scenario has");
        ensure!(
            wrong.iter().any(|w| w.starts_with(label)),
            "{scenario}: NEGATIVE CONTROL: the {} reader was expected to get \"{label}\" wrong, but its wrong verdicts are {wrong:?}; the check would not notice a reader like it",
            rule.name()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------- writer helpers

async fn is_gated(rig: &Rig) -> Result<bool, Failure> {
    Ok(rig.handles.owner.snapshot(&rig.doc).await?.power.server_gates_encrypted_timeline())
}

async fn needs_draft0(rig: &Rig, scenario: &str) -> R {
    ensure!(
        !is_gated(rig).await?,
        "{scenario} needs a room whose power levels do not gate m.room.encrypted (RigOptions::draft0_power_levels): the server must accept the forged events"
    );
    Ok(())
}

/// The owner sets `principal`'s role and waits until the server's table says so.
async fn set_role(rig: &Rig, principal: &str, role: Role) -> R {
    let owner = &rig.handles.owner;
    owner.grant(&rig.doc, principal, role).await?;
    rig.until(&format!("the power levels to show {principal} as {role:?}"), || async {
        let got = owner.snapshot(&rig.doc).await?.power.role_of(principal);
        Ok(if got == role { Ok(()) } else { Err(format!("role {got:?}")) })
    })
    .await
}

/// A version through the store (so the store's own role check applies).
async fn publish(rig: &Rig, who: Who, parents: Vec<String>, text: &str) -> Result<dcp_core::Stamped<Version>, Failure> {
    Ok(rig.native(who).publish_version(&rig.doc, parents, new_file(text.as_bytes()), None).await?)
}

/// A version straight to the homeserver, around the store's checks. In a
/// draft-0 room the server accepts it; a refusal fails the scenario.
async fn forge_version(rig: &Rig, who: Who, parents: Vec<String>, file: FileRef, text: &str) -> Result<String, Failure> {
    let store = rig.handles.get(who);
    let v = Version {
        dcp: 1,
        parents,
        file,
        created_by: Author { principal: store.principal(), device: store.device() },
        message: Some(text.into()),
        covers: None,
    };
    let content = serde_json::to_value(&v).map_err(|e| Failure::new(e.to_string()))?;
    profile::send_unchecked_patiently(store, &rig.doc, "org.tunaos.dcp.version", content)
        .await
        .map_err(|e| Failure::new(format!("the server refused {who:?}'s forged version ({e}): the scenario needs it accepted")))
}

async fn forge_update(rig: &Rig, who: Who, seq: u64, text: &str, base: &str) -> Result<String, Failure> {
    let u = Update {
        dcp: 1,
        algorithm: ALGORITHM.into(),
        doc_model: DOC_MODEL.into(),
        base_version: base.into(),
        seq,
        payload: updates::inline_payload(text.as_bytes()).map_err(|e| Failure::new(e.to_string()))?,
    };
    let content = serde_json::to_value(&u).map_err(|e| Failure::new(e.to_string()))?;
    profile::send_unchecked_patiently(rig.handles.get(who), &rig.doc, "org.tunaos.dcp.update", content)
        .await
        .map_err(|e| Failure::new(format!("the server refused {who:?}'s forged update ({e}): the scenario needs it accepted")))
}

async fn first_file(rig: &Rig) -> Result<FileRef, Failure> {
    let versions = rig.handles.owner.read_report_with(&rig.doc, SenderRule::Trust).await?.versions;
    versions
        .into_iter()
        .find(|v| v.id == rig.first)
        .map(|v| v.body.file)
        .ok_or_else(|| Failure::new("the first version is not listed"))
}

// ---------------------------------------------------------------- scenario 1

/// Scenario 1: an editor publishes, is demoted, the owner publishes on top of
/// the editor's version later. A reader that judges by the level at the event
/// accepts both. In a draft-0 room (nothing gates the timeline) the current-
/// table reader drops the editor's version, and then the owner's save for its
/// unknown parent (Spike F): the control. In a gated room that reader trusts
/// the server and is right too.
pub async fn an_editor_demoted_after_publishing_keeps_their_version_and_the_owners_later_save(rig: &Rig) -> R {
    let scenario = "S1 demoted editor";
    let gated = is_gated(rig).await?;
    let editor_id = rig.principal(Who::Editor);
    let mut ledger = Ledger::new(&rig.first);

    let v2 = publish(rig, Who::Editor, vec![rig.first.clone()], "v2, published while an editor").await?;
    ledger.accept("v2 by the editor, before the demotion", &v2.id);
    set_role(rig, &editor_id, Role::Viewer).await?;
    let o1 = publish(rig, Who::Owner, vec![v2.id.clone()], "the owner's save, built on v2").await?;
    ledger.accept("o1 by the owner, after the demotion", &o1.id);

    for (reader, who) in [("owner", Who::Owner), ("viewer", Who::Viewer)] {
        let store = rig.handles.get(who);
        ready(rig, store, &ledger).await?;
        at_event_readers_are_right(rig, scenario, reader, store, &ledger, &[o1.id.clone()]).await?;
        if gated {
            // The server refuses the ex-editor and the current-table reader trusts it: right here.
            let r = reading(store, &rig.doc, SenderRule::CurrentTable).await?;
            let wrong = ledger.wrong(&r);
            ensure!(wrong.is_empty(), "{scenario}: in a gated room the current-table reader is right, got {wrong:?}");
        } else {
            control_fails(rig, scenario, reader, store, &ledger, SenderRule::CurrentTable, &["v2 by the editor", "o1 by the owner"]).await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- scenario 2

/// Scenario 2: the demoted ex-editor publishes AFTER the demotion in a room
/// where Synapse accepts it: the reader drops exactly those events and keeps
/// the editor's earlier version and the owner's later save. Controls: the
/// current-table reader drops the earlier version (and the save built on it),
/// the reader that trusts senders accepts the forgeries.
pub async fn a_demoted_editors_later_events_are_dropped_and_their_earlier_ones_stay(rig: &Rig) -> R {
    let scenario = "S2 events after the demotion";
    needs_draft0(rig, scenario).await?;
    let editor_id = rig.principal(Who::Editor);
    let mut ledger = Ledger::new(&rig.first);

    let v2 = publish(rig, Who::Editor, vec![rig.first.clone()], "v2, published while an editor").await?;
    ledger.accept("v2 by the editor, before the demotion", &v2.id);
    set_role(rig, &editor_id, Role::Viewer).await?;
    let f = forge_version(rig, Who::Editor, vec![v2.id.clone()], v2.body.file.clone(), "forged by the ex-editor").await?;
    ledger.drop_it("forged version by the ex-editor, after the demotion", &f);
    let fu = forge_update(rig, Who::Editor, 0, "forged update by the ex-editor", &v2.id).await?;
    ledger.drop_it("forged update by the ex-editor, after the demotion", &fu);
    let o1 = publish(rig, Who::Owner, vec![v2.id.clone()], "the owner's save, built on v2").await?;
    ledger.accept("o1 by the owner, after the demotion", &o1.id);

    for (reader, who) in [("owner", Who::Owner), ("viewer", Who::Viewer)] {
        let store = rig.handles.get(who);
        ready(rig, store, &ledger).await?;
        at_event_readers_are_right(rig, scenario, reader, store, &ledger, &[o1.id.clone()]).await?;
        control_fails(rig, scenario, reader, store, &ledger, SenderRule::CurrentTable, &["v2 by the editor", "o1 by the owner"]).await?;
        control_fails(
            rig,
            scenario,
            reader,
            store,
            &ledger,
            SenderRule::Trust,
            &["forged version by the ex-editor", "forged update by the ex-editor"],
        )
        .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------- scenario 3

/// Scenario 3: a viewer forges a version and an update (the draft-0 server
/// accepts them), is promoted to editor LATER and publishes legitimately. The
/// promotion does not validate the earlier forgeries. Control: the
/// current-table reader sees an editor and accepts them retroactively.
pub async fn a_later_promotion_does_not_validate_an_earlier_forged_event(rig: &Rig) -> R {
    let scenario = "S3 later promotion";
    needs_draft0(rig, scenario).await?;
    let viewer_id = rig.principal(Who::Viewer);
    let file = first_file(rig).await?;
    let mut ledger = Ledger::new(&rig.first);

    let f = forge_version(rig, Who::Viewer, vec![rig.first.clone()], file, "forged while a viewer").await?;
    ledger.drop_it("forged version by the viewer, before the promotion", &f);
    let fu = forge_update(rig, Who::Viewer, 0, "forged update while a viewer", &rig.first).await?;
    ledger.drop_it("forged update by the viewer, before the promotion", &fu);
    set_role(rig, &viewer_id, Role::Editor).await?;
    let a = publish(rig, Who::Viewer, vec![rig.first.clone()], "published after the promotion").await?;
    ledger.accept("a by the promoted viewer, after the promotion", &a.id);

    let forged = ["forged version by the viewer", "forged update by the viewer"];
    for (reader, who) in [("owner", Who::Owner), ("editor", Who::Editor)] {
        let store = rig.handles.get(who);
        ready(rig, store, &ledger).await?;
        at_event_readers_are_right(rig, scenario, reader, store, &ledger, &[a.id.clone()]).await?;
        control_fails(rig, scenario, reader, store, &ledger, SenderRule::CurrentTable, &forged).await?;
        control_fails(rig, scenario, reader, store, &ledger, SenderRule::Trust, &forged).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------- scenarios 4 and 5

/// The burst history: the owner flips the editor between viewer and editor five
/// times with no pause, and the editor sends an event after every change.
/// Ends demoted. Returns the ledger and the owner's last save (the head).
async fn burst_history(rig: &Rig) -> Result<(Ledger, String), Failure> {
    let editor_id = rig.principal(Who::Editor);
    let owner = &rig.handles.owner;
    let file = first_file(rig).await?;
    let mut ledger = Ledger::new(&rig.first);

    let a1 = publish(rig, Who::Editor, vec![rig.first.clone()], "a1, editor").await?;
    ledger.accept("a1 by the editor (level 50)", &a1.id);

    owner.grant(&rig.doc, &editor_id, Role::Viewer).await?; // change 1: demoted
    let f1 = forge_version(rig, Who::Editor, vec![a1.id.clone()], file.clone(), "f1, viewer").await?;
    ledger.drop_it("f1 by the viewer (change 1)", &f1);

    owner.grant(&rig.doc, &editor_id, Role::Editor).await?; // change 2: promoted
    let a2 = publish(rig, Who::Editor, vec![a1.id.clone()], "a2, editor again").await?;
    ledger.accept("a2 by the editor (change 2)", &a2.id);

    owner.grant(&rig.doc, &editor_id, Role::Viewer).await?; // change 3: demoted
    let f2 = forge_version(rig, Who::Editor, vec![a2.id.clone()], file.clone(), "f2, viewer").await?;
    ledger.drop_it("f2 by the viewer (change 3)", &f2);
    let fu2 = forge_update(rig, Who::Editor, 0, "fu2, viewer", &a2.id).await?;
    ledger.drop_it("fu2 update by the viewer (change 3)", &fu2);

    owner.grant(&rig.doc, &editor_id, Role::Editor).await?; // change 4: promoted
    let a3 = publish(rig, Who::Editor, vec![a2.id.clone()], "a3, editor again").await?;
    ledger.accept("a3 by the editor (change 4)", &a3.id);

    owner.grant(&rig.doc, &editor_id, Role::Viewer).await?; // change 5: demoted for good
    let f3 = forge_version(rig, Who::Editor, vec![a3.id.clone()], file, "f3, viewer").await?;
    ledger.drop_it("f3 by the viewer (change 5)", &f3);

    let o1 = publish(rig, Who::Owner, vec![a3.id.clone()], "the owner's save, built on a3").await?;
    ledger.accept("o1 by the owner (after change 5)", &o1.id);
    Ok((ledger, o1.id))
}

const BURST_LEGITIMATE: &[&str] = &["a1 by the editor", "a2 by the editor", "a3 by the editor", "o1 by the owner"];
const BURST_FORGERIES: &[&str] = &["f1 by the viewer", "f2 by the viewer", "fu2 update by the viewer", "f3 by the viewer"];

/// Scenario 5: power levels changed five times in a burst, an event from the
/// editor between every two changes. Every event is judged by its own table:
/// the four accepted ones (three by the editor while an editor, the owner's
/// save) are accepted, the four sent while a viewer are dropped, and the
/// owner's save builds on the last accepted one. Controls: the current-table
/// reader (the editor ends demoted) drops the editor's legitimate events, the
/// trusting reader accepts the forgeries.
pub async fn power_levels_changed_several_times_in_a_burst(rig: &Rig) -> R {
    let scenario = "S5 burst of changes";
    needs_draft0(rig, scenario).await?;
    let (ledger, head) = burst_history(rig).await?;
    for (reader, who) in [("owner", Who::Owner), ("viewer", Who::Viewer)] {
        let store = rig.handles.get(who);
        ready(rig, store, &ledger).await?;
        at_event_readers_are_right(rig, scenario, reader, store, &ledger, &[head.clone()]).await?;
        control_fails(rig, scenario, reader, store, &ledger, SenderRule::CurrentTable, BURST_LEGITIMATE).await?;
        control_fails(rig, scenario, reader, store, &ledger, SenderRule::Trust, BURST_FORGERIES).await?;
    }
    Ok(())
}

/// Scenario 4: a user who joins after all of it (the burst history) and reads
/// the room's history gets the verdicts and the levels of a live reader, under
/// every at-event rule. The control: under draft 1's rule the late joiner
/// would get the editor's legitimate events wrong.
pub async fn a_late_joiner_sees_the_verdicts_of_a_live_reader(rig: &Rig) -> R {
    let scenario = "S4 late joiner";
    needs_draft0(rig, scenario).await?;
    let (ledger, head) = burst_history(rig).await?;
    let live = rig.handles.viewer.clone();
    ready(rig, &live, &ledger).await?;
    let live_readings = at_event_readers_are_right(rig, scenario, "live viewer", &live, &ledger, &[head.clone()]).await?;

    // Joins after every event above.
    let late = rig.syn.user("late").await;
    let late_id = late.principal();
    rig.handles.owner.grant(&rig.doc, &late_id, Role::Viewer).await?;
    late.role(&rig.doc).await?;
    ready(rig, &late, &ledger).await?;
    let late_readings = at_event_readers_are_right(rig, scenario, "late joiner", &late, &ledger, &[head.clone()]).await?;

    for (name, l) in &late_readings {
        let v = &live_readings[name];
        ensure!(
            l.listed == v.listed && l.updates == v.updates && l.heads == v.heads && levels_of(l) == levels_of(v),
            "{scenario}: under the {name} rule the late joiner and the live reader differ: listed {:?} against {:?}, updates {:?} against {:?}, heads {:?} against {:?}",
            l.listed,
            v.listed,
            l.updates,
            v.updates,
            l.heads,
            v.heads
        );
    }
    control_fails(rig, scenario, "late joiner", &late, &ledger, SenderRule::CurrentTable, BURST_LEGITIMATE).await?;
    control_fails(rig, scenario, "late joiner", &late, &ledger, SenderRule::Trust, BURST_FORGERIES).await?;
    Ok(())
}
