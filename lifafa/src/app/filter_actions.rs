//! What a filter pass does to one message: the forwards still to send, and
//! the mark-as-read, delete or move that follows them. Pure, so the rules of
//! the spec (which forward is skipped, which rule wins) can be tested.

use crate::config::{forward_key, FilterRule, Forwarded, ForwardedLog, FORWARD_WINDOW_SECS};
use crate::models::Message;

/// One forward to send: the destinations of one rule for one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ForwardJob {
    /// Identifies the forward in the [`ForwardedLog`].
    pub(super) key: String,
    /// Destination addresses, lowercased.
    pub(super) to: Vec<String>,
    /// The rule's label, for the log and for error notices.
    pub(super) rule: String,
}

/// What a pass does to one message.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct ActionPlan {
    /// Forwards not yet recorded and eligible, one per distinct key.
    pub(super) forwards: Vec<ForwardJob>,
    /// Mark the message read.
    pub(super) mark_read: bool,
    /// Delete the message for good.
    pub(super) delete: bool,
    /// Move the message here.
    pub(super) move_to: Option<String>,
    /// A forward that cannot be made (the message is encrypted), or one
    /// still waiting in the Outbox, holds the message: nothing else a rule
    /// does to it may run.
    pub(super) held: bool,
}

/// What a plan needs to know besides the rules and the message.
pub(super) struct PlanContext<'a> {
    /// The folder the message is in.
    pub(super) src_path: Option<&'a str>,
    /// The account's own addresses.
    pub(super) own: &'a [String],
    /// The forwards already made.
    pub(super) log: &'a ForwardedLog,
    /// Whether a destination folder exists on the account.
    pub(super) dest_ok: &'a dyn Fn(&str) -> bool,
    /// Whether the account's Outbox still holds a forward of the message.
    pub(super) in_outbox: &'a dyn Fn(&Message) -> bool,
    /// The time now, Unix seconds.
    pub(super) now: i64,
}

/// The reference a forward of `m` is recorded under: its Message-ID and
/// date, so a sender that reuses a Message-ID still gets each message
/// forwarded. Without a Message-ID, the sender, date and subject stand in,
/// which unlike a UID stay the same when the message changes folder.
fn message_ref(m: &Message) -> String {
    if m.message_id.is_empty() {
        format!(
            "noid:{}|{}|{}",
            m.from_addr.to_lowercase(),
            m.timestamp,
            m.subject
        )
    } else {
        format!("{}|{}", m.message_id, m.timestamp)
    }
}

/// Whether `rule` applies to `m` at all. A rule that forwards starts from
/// the moment its Forward was added: mail that was already in the folder, or
/// whose date is unknown, is left alone by every action of the rule, so a
/// new Forward with Delete never erases old mail it did not forward. Mail
/// older than the forward window is left alone too, which keeps the
/// [`ForwardedLog`] from having to remember it.
fn rule_applies(rule: &FilterRule, m: &Message, now: i64) -> bool {
    rule.forward_to.is_empty()
        || ((rule.forward_since == 0 || m.timestamp >= rule.forward_since)
            && m.timestamp >= now - FORWARD_WINDOW_SECS)
}

/// Whether `rule` may forward `m`: it is neither the account's own mail nor
/// from a destination, which is how a forward coming back into a filtered
/// mailbox is told apart.
fn may_forward(rule: &FilterRule, m: &Message, own: &[String]) -> bool {
    let from = m.from_addr.trim();
    let same = |a: &String| a.trim().eq_ignore_ascii_case(from);
    !(own.iter().any(same) || rule.forward_to.iter().any(same))
}

/// Plan the actions of the `matched` rules on `m`.
///
/// Every matching rule forwards. The first matching rule that deletes or has
/// a usable destination (other than where the message already is) decides
/// the delete or the move; later ones are ignored.
pub(super) fn plan_actions(matched: &[&FilterRule], m: &Message, cx: &PlanContext) -> ActionPlan {
    let mut plan = ActionPlan::default();
    let matching: Vec<&FilterRule> = matched
        .iter()
        .copied()
        .filter(|r| rule_applies(r, m, cx.now))
        .collect();
    let reference = message_ref(m);
    for rule in &matching {
        if rule.forward_to.is_empty() || !may_forward(rule, m, cx.own) {
            continue;
        }
        // Fetching an OpenPGP message for a forward would decrypt it in the
        // background and send it on as plain text. It is left for the user.
        if crate::models::preview_is_encrypted(&m.preview) {
            plan.held = true;
            continue;
        }
        let key = forward_key(&rule.account_email, &reference, &rule.forward_to);
        match cx.log.state(&key) {
            Some(Forwarded::Sent) => continue,
            // Queued in the Outbox: it has gone once the Outbox no longer
            // holds it and the original carries the forwarded mark. Until
            // then deleting the original could lose the only copy, and one
            // the user threw out of the Outbox was never sent at all.
            Some(Forwarded::Queued) => {
                if (cx.in_outbox)(m) || !m.forwarded {
                    plan.held = true;
                }
                continue;
            }
            None => {}
        }
        if plan.forwards.iter().any(|j| j.key == key) {
            continue;
        }
        plan.forwards.push(ForwardJob {
            key,
            to: rule.forward_to.clone(),
            rule: rule.label(),
        });
    }
    plan.mark_read = m.unread && matching.iter().any(|r| r.mark_read);
    // The first rule that moves or deletes decides. A move that does not
    // apply (the message is already there, or the folder is gone) blocks the
    // deletes of later rules, which would otherwise erase mail an earlier
    // rule meant to keep; a later move may still apply.
    let mut blocked = false;
    for rule in &matching {
        if rule.delete {
            if !blocked {
                plan.delete = true;
                break;
            }
        } else if !rule.dest_path.is_empty() {
            if (cx.dest_ok)(&rule.dest_path) && cx.src_path != Some(&rule.dest_path) {
                plan.move_to = Some(rule.dest_path.clone());
                break;
            }
            blocked = true;
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::msg;
    use crate::config::{FilterField, FilterMatch};

    fn rule() -> FilterRule {
        let mut r = FilterRule::draft_from_message("me@x.org", "billing@shop.example", "");
        r.field = FilterField::FromAddress;
        r.matcher = FilterMatch::Equals;
        r.dest_path = String::new();
        r
    }

    fn forwarding(to: &[&str]) -> FilterRule {
        let mut r = rule();
        r.forward_to = to.iter().map(|s| s.to_string()).collect();
        r
    }

    /// The clock of the tests: late enough that every test message is
    /// within the forward window.
    const NOW: i64 = 5_000;

    fn plan_in(
        rules: &[&FilterRule],
        m: &Message,
        src: Option<&str>,
        log: &ForwardedLog,
        dest_ok: &dyn Fn(&str) -> bool,
    ) -> ActionPlan {
        let own = ["me@x.org".to_string()];
        let cx = PlanContext {
            src_path: src,
            own: &own,
            log,
            dest_ok,
            in_outbox: &|_| false,
            now: NOW,
        };
        plan_actions(rules, m, &cx)
    }

    fn plan(rules: &[FilterRule], m: &Message, log: &ForwardedLog) -> ActionPlan {
        let refs: Vec<&FilterRule> = rules.iter().collect();
        plan_in(&refs, m, Some("INBOX"), log, &|_| true)
    }

    fn arrived(mut m: Message) -> Message {
        m.message_id = "m1".into();
        m.from_addr = "billing@shop.example".into();
        m.timestamp = 2_000;
        m.unread = true;
        m
    }

    #[test]
    fn a_message_without_an_id_is_forwarded_under_a_fallback_key() {
        let mut m = arrived(msg(1, 1, 7));
        m.message_id = String::new();
        let rules = [forwarding(&["admin@corp.example"])];
        let first = plan(&rules, &m, &ForwardedLog::default());
        assert_eq!(first.forwards.len(), 1);
        assert!(first.forwards[0].key.contains("noid:billing@shop.example"));
        let log = ForwardedLog::with_keys(vec![first.forwards[0].key.clone()]);
        assert!(plan(&rules, &m, &log).forwards.is_empty());
    }

    #[test]
    fn duplicate_and_reordered_destinations_forward_once() {
        let m = arrived(msg(1, 1, 1));
        let rules = [
            forwarding(&["a@corp.example", "b@corp.example"]),
            forwarding(&["B@corp.example", "a@corp.example"]),
        ];
        assert_eq!(plan(&rules, &m, &ForwardedLog::default()).forwards.len(), 1);
    }

    #[test]
    fn own_mail_and_mail_from_a_destination_are_not_forwarded() {
        let rules = [forwarding(&["admin@corp.example"])];
        let mut m = arrived(msg(1, 1, 1));
        m.from_addr = "ME@x.org".into();
        assert!(plan(&rules, &m, &ForwardedLog::default())
            .forwards
            .is_empty());
        m.from_addr = "admin@corp.example".into();
        assert!(plan(&rules, &m, &ForwardedLog::default())
            .forwards
            .is_empty());
    }

    #[test]
    fn a_forwarding_rule_leaves_old_mail_alone_entirely() {
        // A new rule with Forward and Delete must not touch what was in the
        // folder before it existed: not forwarded, not read, not deleted.
        let mut r = forwarding(&["admin@corp.example"]);
        r.forward_since = 3_000;
        r.mark_read = true;
        r.delete = true;
        let old = arrived(msg(1, 1, 1));
        let p = plan(&[r.clone()], &old, &ForwardedLog::default());
        assert_eq!(p, ActionPlan::default());
        let mut fresh = arrived(msg(1, 1, 2));
        fresh.timestamp = 4_000;
        let p = plan(&[r], &fresh, &ForwardedLog::default());
        assert_eq!(p.forwards.len(), 1);
        assert!(p.mark_read && p.delete);
    }

    #[test]
    fn rules_without_a_forward_are_not_limited_by_date() {
        let mut r = rule();
        r.forward_since = 3_000;
        r.delete = true;
        let old = arrived(msg(1, 1, 1));
        assert!(plan(&[r], &old, &ForwardedLog::default()).delete);
    }

    #[test]
    fn mail_with_an_unknown_date_is_left_alone_once_a_start_is_set() {
        let mut r = forwarding(&["admin@corp.example"]);
        r.forward_since = 3_000;
        let mut m = arrived(msg(1, 1, 1));
        m.timestamp = 0;
        assert!(plan(&[r], &m, &ForwardedLog::default()).forwards.is_empty());
    }

    #[test]
    fn first_move_or_delete_wins_and_all_forwards_run() {
        let mut mv = rule();
        mv.dest_path = "Admin".into();
        let mut del = rule();
        del.delete = true;
        let fwd = forwarding(&["admin@corp.example"]);
        let m = arrived(msg(1, 1, 1));
        let p = plan(
            &[fwd.clone(), mv.clone(), del.clone()],
            &m,
            &ForwardedLog::default(),
        );
        assert_eq!(p.move_to.as_deref(), Some("Admin"));
        assert!(!p.delete);
        assert_eq!(p.forwards.len(), 1);
        let p = plan(&[del, mv], &m, &ForwardedLog::default());
        assert!(p.delete && p.move_to.is_none());
    }

    #[test]
    fn a_move_that_does_not_apply_does_not_hand_the_message_to_a_later_delete() {
        // Rule 1 files into the folder the message is already in; rule 2
        // deletes. The message must stay.
        let mut mv = rule();
        mv.dest_path = "Boss".into();
        let mut del = rule();
        del.delete = true;
        let m = arrived(msg(1, 1, 1));
        let refs = [&mv, &del];
        let p = plan_in(&refs, &m, Some("Boss"), &ForwardedLog::default(), &|_| true);
        assert!(!p.delete && p.move_to.is_none());
        // Same when the first rule's folder is gone from the server.
        let p = plan_in(&refs, &m, Some("INBOX"), &ForwardedLog::default(), &|_| {
            false
        });
        assert!(!p.delete && p.move_to.is_none());
    }

    #[test]
    fn a_blocked_move_still_lets_a_later_move_apply() {
        let mut first = rule();
        first.dest_path = "Gone".into();
        let mut second = rule();
        second.dest_path = "Admin".into();
        let m = arrived(msg(1, 1, 1));
        let refs = [&first, &second];
        let p = plan_in(&refs, &m, Some("INBOX"), &ForwardedLog::default(), &|d| {
            d == "Admin"
        });
        assert_eq!(p.move_to.as_deref(), Some("Admin"));
    }

    #[test]
    fn encrypted_mail_is_held_not_forwarded() {
        let rules = [forwarding(&["admin@corp.example"])];
        let mut m = arrived(msg(1, 1, 1));
        m.preview = crate::models::ENCRYPTED_PREVIEW.into();
        let p = plan(&rules, &m, &ForwardedLog::default());
        assert!(p.forwards.is_empty());
        assert!(p.held);
    }

    #[test]
    fn mark_read_is_skipped_when_already_read() {
        let mut r = rule();
        r.mark_read = true;
        let mut m = arrived(msg(1, 1, 1));
        assert!(plan(&[r.clone()], &m, &ForwardedLog::default()).mark_read);
        m.unread = false;
        assert!(!plan(&[r], &m, &ForwardedLog::default()).mark_read);
    }

    #[test]
    fn a_destination_that_is_gone_or_is_the_source_moves_nothing() {
        let mut r = rule();
        r.dest_path = "INBOX".into();
        let m = arrived(msg(1, 1, 1));
        assert_eq!(
            plan(&[r.clone()], &m, &ForwardedLog::default()).move_to,
            None
        );
        r.dest_path = "Gone".into();
        let refs = [&r];
        let p = plan_in(&refs, &m, Some("INBOX"), &ForwardedLog::default(), &|_| {
            false
        });
        assert_eq!(p.move_to, None);
    }

    #[test]
    fn a_forward_waiting_in_the_outbox_holds_the_message() {
        let mut r = forwarding(&["admin@corp.example"]);
        r.delete = true;
        let m = arrived(msg(1, 1, 1));
        let key = plan(&[r.clone()], &m, &ForwardedLog::default()).forwards[0]
            .key
            .clone();
        let log = ForwardedLog::with_queued(key);
        let p = plan(&[r.clone()], &m, &log);
        assert!(p.held && p.forwards.is_empty());
        // Gone from the Outbox and marked forwarded: it went.
        let mut sent = m.clone();
        sent.forwarded = true;
        let p = plan(&[r.clone()], &sent, &log);
        assert!(!p.held && p.delete && p.forwards.is_empty());
        // Marked forwarded by something else while still queued: held.
        let refs = [&r];
        let own: [String; 0] = [];
        let cx = PlanContext {
            src_path: Some("INBOX"),
            own: &own,
            log: &log,
            dest_ok: &|_| true,
            in_outbox: &|_| true,
            now: NOW,
        };
        assert!(plan_actions(&refs, &sent, &cx).held);
    }

    #[test]
    fn mail_older_than_the_window_is_left_alone() {
        let mut r = forwarding(&["admin@corp.example"]);
        r.delete = true;
        let m = arrived(msg(1, 1, 1));
        let refs = [&r];
        let own: [String; 0] = [];
        let log = ForwardedLog::default();
        let cx = PlanContext {
            src_path: Some("INBOX"),
            own: &own,
            log: &log,
            dest_ok: &|_| true,
            in_outbox: &|_| false,
            now: m.timestamp + FORWARD_WINDOW_SECS + 1,
        };
        assert_eq!(plan_actions(&refs, &m, &cx), ActionPlan::default());
    }

    #[test]
    fn a_reused_message_id_with_another_date_is_forwarded_again() {
        let rules = [forwarding(&["admin@corp.example"])];
        let first = arrived(msg(1, 1, 1));
        let key = plan(&rules, &first, &ForwardedLog::default()).forwards[0]
            .key
            .clone();
        let log = ForwardedLog::with_keys(vec![key]);
        assert!(plan(&rules, &first, &log).forwards.is_empty());
        let mut second = arrived(msg(1, 1, 2));
        second.timestamp = 3_000;
        assert_eq!(plan(&rules, &second, &log).forwards.len(), 1);
    }
}
