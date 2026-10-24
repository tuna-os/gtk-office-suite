//! The update profile (L2), spec section 7, receiver side, plus the writer's
//! sequencing and payload encoding (spec 4.4).
//!
//! A [`Receiver`] is fed every retained update (in any order, repeatedly) and
//! applies, per `(principal, device)` stream, the ones it supports in `seq`
//! order. A gap in `seq` stops that stream (rule 2) until the receiver has
//! reloaded from the latest version with [`Receiver::reload`].

use std::collections::{BTreeMap, HashSet};

use base64::Engine as _;

use crate::{FileRef, Payload, Stamped, Update, Version};

/// Spec 7 rule 5 and 4.4: an inline payload is at most this many bytes.
pub const MAX_INLINE_BYTES: u64 = 34_000;

/// The unit `seq` counts in.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamKey {
    pub principal: String,
    /// Empty when the binding cannot attribute a device.
    pub device: String,
}

pub fn stream_of(update: &Stamped<Update>) -> StreamKey {
    StreamKey {
        principal: update.sender.clone(),
        device: update.device.clone().unwrap_or_default(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PayloadError {
    #[error("inline payload of {0} bytes is over the {MAX_INLINE_BYTES} byte limit; use a file")]
    TooLarge(u64),
    #[error("payload data is not base64: {0}")]
    NotBase64(String),
    #[error("payload says {declared} bytes but carries {actual}")]
    SizeMismatch { declared: u64, actual: u64 },
    #[error("payload is a file; fetch and verify it first")]
    IsFile,
}

/// Spec 7 rule 5: bytes up to the limit go inline as base64.
pub fn inline_payload(bytes: &[u8]) -> Result<Payload, PayloadError> {
    let size = bytes.len() as u64;
    if size > MAX_INLINE_BYTES {
        return Err(PayloadError::TooLarge(size));
    }
    Ok(Payload::Base64 {
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
        size,
    })
}

/// A file payload (an update over the inline limit).
pub fn file_payload(file: FileRef) -> Payload {
    Payload::File { file }
}

/// The bytes of an inline payload, checked against its declared size.
pub fn decode_inline(payload: &Payload) -> Result<Vec<u8>, PayloadError> {
    match payload {
        Payload::File { .. } => Err(PayloadError::IsFile),
        Payload::Base64 { data, size } => {
            if *size > MAX_INLINE_BYTES {
                return Err(PayloadError::TooLarge(*size));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| PayloadError::NotBase64(e.to_string()))?;
            if bytes.len() as u64 != *size {
                return Err(PayloadError::SizeMismatch {
                    declared: *size,
                    actual: bytes.len() as u64,
                });
            }
            Ok(bytes)
        }
    }
}

/// Writer side: numbers one `(principal, device)` stream (spec 4.4: from 0,
/// plus 1 per update).
#[derive(Debug, Clone)]
pub struct StreamWriter {
    algorithm: String,
    doc_model: String,
    next_seq: u64,
}

impl StreamWriter {
    pub fn new(algorithm: impl Into<String>, doc_model: impl Into<String>) -> Self {
        StreamWriter {
            algorithm: algorithm.into(),
            doc_model: doc_model.into(),
            next_seq: 0,
        }
    }

    /// Build the next update. The sequence number is consumed even if the
    /// caller then fails to send it: that is exactly a lost update, which the
    /// receiver is required to notice.
    pub fn next(&mut self, base_version: &str, bytes: &[u8]) -> Result<Update, PayloadError> {
        let payload = inline_payload(bytes)?;
        let seq = self.next_seq;
        self.next_seq += 1;
        Ok(Update {
            dcp: 1,
            algorithm: self.algorithm.clone(),
            doc_model: self.doc_model.clone(),
            base_version: base_version.to_owned(),
            seq,
            payload,
        })
    }
}

/// What the receiving application provides (spec 7 rules 1 and 3).
pub trait Target {
    /// Rule 1: apply only an `algorithm` and `doc_model` the program supports.
    fn supports(&self, algorithm: &str, doc_model: &str) -> bool;
    /// Rule 3: apply to a copy and validate the result; an `Err` drops the
    /// update before the live state changes.
    fn dry_run(&self, algorithm: &str, doc_model: &str, payload: &[u8]) -> Result<(), String>;
    fn apply(&mut self, algorithm: &str, doc_model: &str, payload: &[u8]);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// Rule 1: unsupported algorithm or document model. Counts toward `seq`.
    Unsupported,
    /// Based on a different version than the receiver loaded. Counts toward `seq`.
    OtherBase,
    /// A second update with a `seq` the stream already used.
    DuplicateSeq,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dropped {
    /// The payload is malformed. Counts toward `seq`.
    BadPayload(PayloadError),
    /// Rule 3: failed the dry run. Counts toward `seq`.
    Invalid(String),
}

/// Rule 2: update `expected` of `stream` never arrived, though `found` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    pub stream: StreamKey,
    pub expected: u64,
    pub found: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// Identifiers of updates applied by this call, in application order.
    pub applied: Vec<String>,
    pub skipped: Vec<(String, Skip)>,
    pub dropped: Vec<(String, Dropped)>,
    /// Streams stopped by this call.
    pub gaps: Vec<Gap>,
}

#[derive(Debug, Default)]
struct StreamState {
    next: u64,
    stopped: Option<Gap>,
}

/// Applies the updates of one loaded version.
#[derive(Debug, Default)]
pub struct Receiver {
    base_version: Option<String>,
    streams: BTreeMap<StreamKey, StreamState>,
}

impl Receiver {
    /// A receiver for the document state at `base_version` (`None`: accept
    /// updates based on any version).
    pub fn new(base_version: Option<String>) -> Self {
        Receiver {
            base_version,
            ..Receiver::default()
        }
    }

    /// Streams currently stopped by a gap; the caller must recover by loading
    /// the latest version (spec 7 rule 2) and calling [`Self::reload`].
    pub fn stopped(&self) -> Vec<&Gap> {
        self.streams
            .values()
            .filter_map(|s| s.stopped.as_ref())
            .collect()
    }

    pub fn is_stopped(&self) -> bool {
        self.streams.values().any(|s| s.stopped.is_some())
    }

    /// The caller loaded `version` as the new state. Forget every stream
    /// position and gap: only updates based on `version` apply from here on
    /// (which is also what rule 4 asks of a reader after a compaction, since
    /// every update `covers` includes is based on an older version).
    pub fn reload(&mut self, version: &Stamped<Version>) {
        self.base_version = Some(version.id.clone());
        self.streams.clear();
    }

    /// Process every update in `retained` this receiver has not yet handled.
    pub fn ingest(&mut self, retained: &[Stamped<Update>], target: &mut dyn Target) -> Report {
        let mut by_stream: BTreeMap<StreamKey, Vec<&Stamped<Update>>> = BTreeMap::new();
        for u in retained {
            by_stream.entry(stream_of(u)).or_default().push(u);
        }
        let mut report = Report::default();
        for (key, mut log) in by_stream {
            // seq order; the binding timestamp then id decide between two
            // updates that claim the same seq.
            log.sort_by(|a, b| (a.body.seq, a.ts_ms, &a.id).cmp(&(b.body.seq, b.ts_ms, &b.id)));
            let start = if self.streams.contains_key(&key) {
                0
            } else {
                self.start_of(&log)
            };
            let state = self.streams.entry(key.clone()).or_insert(StreamState {
                next: start,
                stopped: None,
            });
            if state.stopped.is_some() {
                continue;
            }
            let mut seen = HashSet::new();
            for u in log {
                if !seen.insert(u.body.seq) {
                    report.skipped.push((u.id.clone(), Skip::DuplicateSeq));
                    continue;
                }
                if u.body.seq < state.next {
                    continue; // handled by an earlier call
                }
                if u.body.seq > state.next {
                    let gap = Gap {
                        stream: key.clone(),
                        expected: state.next,
                        found: u.body.seq,
                    };
                    state.stopped = Some(gap.clone());
                    report.gaps.push(gap);
                    break;
                }
                state.next += 1;
                Self::handle(&self.base_version, u, target, &mut report);
            }
        }
        report
    }

    /// Where a stream the receiver sees for the first time begins.
    ///
    /// With no base version the whole log counts, from 0 (rule 2). With one,
    /// earlier updates belong to versions the loaded state has superseded
    /// (and a compaction may have dropped them), so the stream begins at its
    /// first update based on the loaded version, or after everything
    /// retained if there is none yet. A loss between the loaded version and
    /// that first update cannot be told from a stream that simply began
    /// there; see the spec corrections in the Spike results.
    fn start_of(&self, log: &[&Stamped<Update>]) -> u64 {
        let Some(base) = &self.base_version else {
            return 0;
        };
        log.iter()
            .filter(|u| &u.body.base_version == base)
            .map(|u| u.body.seq)
            .min()
            .unwrap_or_else(|| log.iter().map(|u| u.body.seq).max().map_or(0, |m| m + 1))
    }

    fn handle(
        base_version: &Option<String>,
        u: &Stamped<Update>,
        target: &mut dyn Target,
        report: &mut Report,
    ) {
        let body = &u.body;
        if !target.supports(&body.algorithm, &body.doc_model) {
            report.skipped.push((u.id.clone(), Skip::Unsupported));
            return;
        }
        if base_version
            .as_deref()
            .is_some_and(|b| b != body.base_version)
        {
            report.skipped.push((u.id.clone(), Skip::OtherBase));
            return;
        }
        let bytes = match decode_inline(&body.payload) {
            Ok(b) => b,
            Err(e) => {
                report.dropped.push((u.id.clone(), Dropped::BadPayload(e)));
                return;
            }
        };
        if let Err(why) = target.dry_run(&body.algorithm, &body.doc_model, &bytes) {
            report.dropped.push((u.id.clone(), Dropped::Invalid(why)));
            return;
        }
        target.apply(&body.algorithm, &body.doc_model, &bytes);
        report.applied.push(u.id.clone());
    }
}

/// Spec 7 rule 6, as a check an algorithm profile runs against its own
/// [`Target`]: applying the same updates in different orders MUST give
/// byte-identical state. Per-stream order is fixed by rule 2, so what varies
/// is how the streams interleave; `trials` pseudo-random interleavings
/// (deterministic, seeded by the trial number) are delivered to a fresh
/// receiver and target, and every final state must equal the first one.
pub fn order_independent<T: Target>(
    updates: &[Stamped<Update>],
    mut fresh: impl FnMut() -> T,
    state_of: impl Fn(&T) -> Vec<u8>,
    trials: u64,
) -> Result<(), String> {
    let mut streams: BTreeMap<StreamKey, Vec<&Stamped<Update>>> = BTreeMap::new();
    for u in updates {
        streams.entry(stream_of(u)).or_default().push(u);
    }
    for log in streams.values_mut() {
        log.sort_by_key(|u| u.body.seq);
    }
    let mut reference: Option<Vec<u8>> = None;
    for trial in 0..trials {
        let mut cursors: Vec<(usize, &Vec<&Stamped<Update>>)> =
            streams.values().map(|l| (0, l)).collect();
        let mut rng = trial
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let (mut delivered, mut receiver, mut target) = (Vec::new(), Receiver::new(None), fresh());
        while cursors.iter().any(|(at, l)| *at < l.len()) {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let live: Vec<usize> = (0..cursors.len())
                .filter(|i| cursors[*i].0 < cursors[*i].1.len())
                .collect();
            let pick = live[(rng >> 33) as usize % live.len()];
            let (at, log) = &mut cursors[pick];
            delivered.push(log[*at].clone());
            *at += 1;
            receiver.ingest(&delivered, &mut target);
        }
        let state = state_of(&target);
        match &reference {
            None => reference = Some(state),
            Some(first) if *first != state => {
                return Err(format!(
                    "interleaving {trial} gave {state:?}, interleaving 0 gave {first:?}"
                ));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{integrity, Author, Covers, Locator};

    /// Toy algorithm `append/1` on model `text/1`: the state is the version's
    /// bytes plus every payload applied, in order. `BAD` fails the dry run.
    struct Append(Vec<u8>);

    impl Target for Append {
        fn supports(&self, algorithm: &str, doc_model: &str) -> bool {
            algorithm == "append/1" && doc_model == "text/1"
        }
        fn dry_run(&self, _: &str, _: &str, payload: &[u8]) -> Result<(), String> {
            if payload == b"BAD" {
                Err("instruction not valid".into())
            } else {
                Ok(())
            }
        }
        fn apply(&mut self, _: &str, _: &str, payload: &[u8]) {
            self.0.extend_from_slice(payload);
        }
    }

    fn upd(
        id: &str,
        ts: u64,
        sender: &str,
        device: Option<&str>,
        seq: u64,
        payload: &str,
    ) -> Stamped<Update> {
        upd_with(id, ts, sender, device, "append/1", "$v1", seq, payload)
    }

    #[allow(clippy::too_many_arguments)]
    fn upd_with(
        id: &str,
        ts: u64,
        sender: &str,
        device: Option<&str>,
        algorithm: &str,
        base: &str,
        seq: u64,
        payload: &str,
    ) -> Stamped<Update> {
        let mut s = Stamped::new(
            id,
            ts,
            sender,
            Update {
                dcp: 1,
                algorithm: algorithm.into(),
                doc_model: "text/1".into(),
                base_version: base.into(),
                seq,
                payload: inline_payload(payload.as_bytes()).unwrap(),
            },
        );
        s.device = device.map(String::from);
        s
    }

    fn a(id: &str, seq: u64, p: &str) -> Stamped<Update> {
        upd(id, seq * 10, "@a:x", Some("D1"), seq, p)
    }

    fn state(t: &Append) -> String {
        String::from_utf8(t.0.clone()).unwrap()
    }

    #[test]
    fn applies_in_seq_order_whatever_the_arrival_order() {
        let log = [a("u2", 2, "c"), a("u0", 0, "a"), a("u1", 1, "b")];
        let mut r = Receiver::new(Some("$v1".into()));
        let mut t = Append(b"base:".to_vec());
        let rep = r.ingest(&log, &mut t);
        assert_eq!(state(&t), "base:abc");
        assert_eq!(rep.applied, ["u0", "u1", "u2"]);
        assert!(rep.gaps.is_empty());
    }

    #[test]
    fn a_gap_stops_the_stream_and_nothing_past_it_is_applied_rule_2() {
        let log = [
            a("u0", 0, "a"),
            a("u1", 1, "b"),
            a("u3", 3, "d"),
            a("u4", 4, "e"),
        ];
        let mut r = Receiver::new(None);
        let mut t = Append(Vec::new());
        let rep = r.ingest(&log, &mut t);
        assert_eq!(state(&t), "ab");
        assert_eq!(
            rep.gaps,
            [Gap {
                stream: stream_of(&log[0]),
                expected: 2,
                found: 3
            }]
        );
        assert!(r.is_stopped());
    }

    #[test]
    fn a_stopped_stream_stays_stopped_even_if_the_missing_update_turns_up() {
        let mut r = Receiver::new(None);
        let mut t = Append(Vec::new());
        r.ingest(&[a("u0", 0, "a"), a("u2", 2, "c")], &mut t);
        let rep = r.ingest(&[a("u0", 0, "a"), a("u1", 1, "b"), a("u2", 2, "c")], &mut t);
        assert_eq!(state(&t), "a", "recovery is a reload, not a quiet catch-up");
        assert!(rep.applied.is_empty());
    }

    #[test]
    fn a_stream_that_starts_after_zero_is_a_gap_at_zero() {
        let mut r = Receiver::new(None);
        let mut t = Append(Vec::new());
        let rep = r.ingest(&[a("u1", 1, "b")], &mut t);
        assert_eq!(rep.gaps[0].expected, 0);
        assert_eq!(state(&t), "");
    }

    #[test]
    fn a_gap_in_one_stream_does_not_stop_another() {
        let log = [
            upd("x0", 1, "@a:x", Some("D1"), 0, "a"),
            upd("x2", 3, "@a:x", Some("D1"), 2, "c"),
            upd("y0", 2, "@b:x", Some("D9"), 0, "1"),
            upd("y1", 4, "@b:x", Some("D9"), 1, "2"),
        ];
        let mut r = Receiver::new(None);
        let mut t = Append(Vec::new());
        let rep = r.ingest(&log, &mut t);
        assert_eq!(state(&t), "a12");
        assert_eq!(rep.gaps.len(), 1);
        assert_eq!(rep.gaps[0].stream.principal, "@a:x");
    }

    #[test]
    fn two_devices_of_one_principal_are_two_streams() {
        let log = [
            upd("d1a", 1, "@a:x", Some("D1"), 0, "a"),
            upd("d2a", 2, "@a:x", Some("D2"), 0, "x"),
            upd("d1b", 3, "@a:x", Some("D1"), 1, "b"),
            upd("d2b", 4, "@a:x", Some("D2"), 1, "y"),
        ];
        let mut r = Receiver::new(None);
        let mut t = Append(Vec::new());
        let rep = r.ingest(&log, &mut t);
        assert!(
            rep.gaps.is_empty(),
            "same seq on two devices is not a duplicate: {rep:?}"
        );
        assert_eq!(rep.applied.len(), 4);
    }

    #[test]
    fn without_a_device_a_principal_is_one_stream() {
        let log = [
            upd("u0", 1, "@a:x", None, 0, "a"),
            upd("u1", 2, "@a:x", None, 1, "b"),
        ];
        let mut r = Receiver::new(None);
        let mut t = Append(Vec::new());
        r.ingest(&log, &mut t);
        assert_eq!(state(&t), "ab");
    }

    #[test]
    fn unknown_algorithm_is_ignored_but_still_counts_toward_seq_rule_1() {
        let log = [
            upd_with("u0", 1, "@a:x", Some("D"), "append/1", "$v1", 0, "a"),
            upd_with("u1", 2, "@a:x", Some("D"), "loro/1", "$v1", 1, "ZZ"),
            upd_with("u2", 3, "@a:x", Some("D"), "append/1", "$v1", 2, "c"),
        ];
        let mut r = Receiver::new(None);
        let mut t = Append(Vec::new());
        let rep = r.ingest(&log, &mut t);
        assert_eq!(
            state(&t),
            "ac",
            "nothing from the unsupported update is shown"
        );
        assert_eq!(rep.skipped, [("u1".to_string(), Skip::Unsupported)]);
        assert!(rep.gaps.is_empty(), "not mistaken for a lost update");
    }

    #[test]
    fn unknown_document_model_is_ignored_too() {
        let mut u = upd("u0", 1, "@a:x", Some("D"), 0, "a");
        u.body.doc_model = "tables/1".into();
        let mut r = Receiver::new(None);
        let mut t = Append(Vec::new());
        let rep = r.ingest(&[u], &mut t);
        assert_eq!(rep.skipped.len(), 1);
        assert_eq!(state(&t), "");
    }

    #[test]
    fn an_update_that_fails_the_dry_run_is_dropped_and_reported_rule_3() {
        let log = [a("u0", 0, "a"), a("u1", 1, "BAD"), a("u2", 2, "c")];
        let mut r = Receiver::new(None);
        let mut t = Append(Vec::new());
        let rep = r.ingest(&log, &mut t);
        assert_eq!(state(&t), "ac", "the last valid state is kept");
        assert_eq!(
            rep.dropped,
            [(
                "u1".to_string(),
                Dropped::Invalid("instruction not valid".into())
            )]
        );
        assert!(rep.gaps.is_empty());
    }

    #[test]
    fn malformed_payloads_are_dropped_not_applied() {
        let mut bad_size = a("u0", 0, "abc");
        bad_size.body.payload = Payload::Base64 {
            data: "YWJj".into(),
            size: 7,
        };
        let mut bad_b64 = a("u1", 1, "x");
        bad_b64.body.payload = Payload::Base64 {
            data: "!!!".into(),
            size: 1,
        };
        let mut file = a("u2", 2, "x");
        file.body.payload = file_payload(integrity::file_ref(
            "application/octet-stream",
            b"x",
            Locator::Url {
                href: "https://x/y".into(),
            },
        ));
        let mut r = Receiver::new(None);
        let mut t = Append(Vec::new());
        let rep = r.ingest(&[bad_size, bad_b64, file], &mut t);
        assert_eq!(state(&t), "");
        assert_eq!(rep.dropped.len(), 3);
        assert!(matches!(
            rep.dropped[0].1,
            Dropped::BadPayload(PayloadError::SizeMismatch { .. })
        ));
        assert!(matches!(
            rep.dropped[1].1,
            Dropped::BadPayload(PayloadError::NotBase64(_))
        ));
        assert!(matches!(
            rep.dropped[2].1,
            Dropped::BadPayload(PayloadError::IsFile)
        ));
    }

    #[test]
    fn updates_based_on_another_version_are_not_applied() {
        let log = [
            upd_with("u0", 1, "@a:x", Some("D"), "append/1", "$old", 0, "a"),
            upd_with("u1", 2, "@a:x", Some("D"), "append/1", "$v1", 1, "b"),
        ];
        let mut r = Receiver::new(Some("$v1".into()));
        let mut t = Append(Vec::new());
        let rep = r.ingest(&log, &mut t);
        assert_eq!(state(&t), "b");
        assert_eq!(rep.applied, ["u1"]);
        assert!(
            rep.gaps.is_empty(),
            "older updates are behind the loaded version, not a gap: {rep:?}"
        );
    }

    #[test]
    fn a_straggler_based_on_an_older_version_is_skipped_but_counted() {
        let log = [
            upd_with("u0", 1, "@a:x", Some("D"), "append/1", "$v1", 0, "a"),
            upd_with("u1", 2, "@a:x", Some("D"), "append/1", "$v2", 1, "b"),
            upd_with("u2", 3, "@a:x", Some("D"), "append/1", "$v1", 2, "STALE"),
            upd_with("u3", 4, "@a:x", Some("D"), "append/1", "$v2", 3, "c"),
        ];
        let mut r = Receiver::new(Some("$v2".into()));
        let mut t = Append(Vec::new());
        let rep = r.ingest(&log, &mut t);
        assert_eq!(state(&t), "bc");
        assert_eq!(rep.skipped, [("u2".to_string(), Skip::OtherBase)]);
        assert!(rep.gaps.is_empty());
    }

    #[test]
    fn ingesting_again_applies_only_what_is_new() {
        let mut r = Receiver::new(None);
        let mut t = Append(Vec::new());
        r.ingest(&[a("u0", 0, "a")], &mut t);
        let rep = r.ingest(&[a("u0", 0, "a"), a("u1", 1, "b")], &mut t);
        assert_eq!(state(&t), "ab");
        assert_eq!(rep.applied, ["u1"]);
    }

    #[test]
    fn a_repeated_seq_is_reported_not_applied_twice() {
        let log = [a("u0", 0, "a"), upd("u0b", 5, "@a:x", Some("D1"), 0, "Z")];
        let mut r = Receiver::new(None);
        let mut t = Append(Vec::new());
        let rep = r.ingest(&log, &mut t);
        assert_eq!(state(&t), "a");
        assert_eq!(rep.skipped, [("u0b".to_string(), Skip::DuplicateSeq)]);
    }

    fn version(id: &str, covers: Option<(&str, &str)>) -> Stamped<Version> {
        Stamped::new(
            id,
            100,
            "@a:x",
            Version {
                dcp: 1,
                parents: vec![],
                file: integrity::file_ref(
                    "text/plain",
                    b"v",
                    Locator::Url {
                        href: "https://x/y".into(),
                    },
                ),
                created_by: Author {
                    principal: "@a:x".into(),
                    device: "D".into(),
                },
                message: None,
                covers: covers.map(|(algorithm, through)| Covers {
                    algorithm: algorithm.into(),
                    through: through.into(),
                }),
            },
        )
    }

    #[test]
    fn reload_clears_a_gap_and_follows_the_new_version() {
        let mut log = vec![a("u0", 0, "a"), a("u2", 2, "c")];
        let mut r = Receiver::new(Some("$v1".into()));
        let mut t = Append(b"v1:".to_vec());
        r.ingest(&log, &mut t);
        assert!(r.is_stopped());
        assert_eq!(state(&t), "v1:a");

        // Recovery: load the latest version (it contains everything).
        let v2 = version("$v2", None);
        r.reload(&v2);
        assert!(!r.is_stopped());
        let mut fresh = Append(b"v2:".to_vec());
        let rep = r.ingest(&log, &mut fresh);
        assert_eq!(
            state(&fresh),
            "v2:",
            "updates based on v1 are not replayed onto v2"
        );
        assert!(
            rep.gaps.is_empty(),
            "the old hole is behind the version now: {rep:?}"
        );

        // New updates on top of v2 continue the stream.
        log.push(upd_with(
            "u3",
            30,
            "@a:x",
            Some("D1"),
            "append/1",
            "$v2",
            3,
            "d",
        ));
        let rep = r.ingest(&log, &mut fresh);
        assert_eq!(state(&fresh), "v2:d");
        assert_eq!(rep.applied, ["u3"]);
    }

    #[test]
    fn a_loss_after_reload_is_still_a_gap() {
        let log = [
            a("u0", 0, "a"),
            upd_with("u3", 30, "@a:x", Some("D1"), "append/1", "$v2", 3, "d"),
            upd_with("u5", 50, "@a:x", Some("D1"), "append/1", "$v2", 5, "f"),
        ];
        let mut r = Receiver::new(None);
        r.reload(&version("$v2", None));
        let mut t = Append(Vec::new());
        let rep = r.ingest(&log, &mut t);
        assert_eq!(state(&t), "d");
        assert_eq!(
            rep.gaps,
            [Gap {
                stream: stream_of(&log[0]),
                expected: 4,
                found: 5
            }]
        );
    }

    #[test]
    fn compaction_covers_older_updates_and_lets_a_log_start_late_rule_4() {
        // The version v2 covers append/1 through u1 (rule 4). The writer then
        // dropped u0 and u1, so the retained log starts at seq 2. After
        // loading v2 only u2 and later apply and nothing is mistaken for a gap.
        let retained = [
            upd_with("u2", 30, "@a:x", Some("D1"), "append/1", "$v2", 2, "c"),
            upd_with("u3", 40, "@a:x", Some("D1"), "append/1", "$v2", 3, "d"),
        ];
        let v2 = version("$v2", Some(("append/1", "u1")));
        let mut r = Receiver::new(None);
        r.reload(&v2);
        let mut t = Append(b"v2:".to_vec());
        let rep = r.ingest(&retained, &mut t);
        assert_eq!(state(&t), "v2:cd");
        assert!(rep.gaps.is_empty());
    }

    #[test]
    fn compaction_covered_updates_still_in_the_log_are_not_replayed() {
        let log = [
            a("u0", 0, "a"),
            a("u1", 1, "b"),
            upd_with("u2", 30, "@a:x", Some("D1"), "append/1", "$v2", 2, "c"),
        ];
        let v2 = version("$v2", Some(("append/1", "u1")));
        let mut r = Receiver::new(None);
        r.reload(&v2);
        let mut t = Append(b"v2:".to_vec());
        let rep = r.ingest(&log, &mut t);
        assert_eq!(state(&t), "v2:c");
        assert_eq!(rep.applied, ["u2"]);
    }

    #[test]
    fn inline_payload_limit_is_34000_bytes_rule_5() {
        assert!(inline_payload(&vec![7u8; 34_000]).is_ok());
        assert_eq!(
            inline_payload(&vec![7u8; 34_001]),
            Err(PayloadError::TooLarge(34_001))
        );
        let p = inline_payload(b"hello").unwrap();
        assert_eq!(decode_inline(&p).unwrap(), b"hello");
        // The base64 text of the largest payload still fits the schema's maxLength.
        let Payload::Base64 { data, .. } = inline_payload(&vec![255u8; 34_000]).unwrap() else {
            panic!()
        };
        assert!(data.len() <= 48_000);
    }

    #[test]
    fn writer_numbers_a_stream_from_zero() {
        let mut w = StreamWriter::new("append/1", "text/1");
        let u: Vec<u64> = (0..3).map(|_| w.next("$v1", b"x").unwrap().seq).collect();
        assert_eq!(u, [0, 1, 2]);
        assert!(w.next("$v1", &vec![0u8; 34_001]).is_err());
    }

    #[test]
    fn a_writer_built_update_validates_against_the_schema() {
        let mut w = StreamWriter::new("append/1", "text/1");
        let u = w.next("$v1", b"hello").unwrap();
        crate::schema::validate_typed(crate::schema::Kind::Update, &u).unwrap();
    }

    /// A commutative toy algorithm: the state is the set of payloads.
    struct SetDoc(std::collections::BTreeSet<Vec<u8>>);

    impl Target for SetDoc {
        fn supports(&self, _: &str, _: &str) -> bool {
            true
        }
        fn dry_run(&self, _: &str, _: &str, _: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn apply(&mut self, _: &str, _: &str, payload: &[u8]) {
            self.0.insert(payload.to_vec());
        }
    }

    fn two_streams() -> Vec<Stamped<Update>> {
        vec![
            upd("a0", 1, "@a:x", Some("D1"), 0, "x"),
            upd("a1", 2, "@a:x", Some("D1"), 1, "y"),
            upd("b0", 3, "@b:x", Some("D2"), 0, "p"),
            upd("b1", 4, "@b:x", Some("D2"), 1, "q"),
        ]
    }

    #[test]
    fn determinism_rule_6_holds_for_a_commutative_algorithm() {
        let r = order_independent(
            &two_streams(),
            || SetDoc(Default::default()),
            |t| t.0.iter().flatten().copied().collect(),
            50,
        );
        assert_eq!(r, Ok(()));
    }

    #[test]
    fn determinism_check_can_fail_an_order_dependent_algorithm() {
        // `append` depends on how the streams interleave, which is exactly
        // what rule 6 forbids of a real profile.
        let r = order_independent(&two_streams(), || Append(Vec::new()), |t| t.0.clone(), 50);
        assert!(r.is_err(), "the harness must be able to fail");
    }
}
