#![no_main]

use dcp_core::model::{Stamped, Update};
use dcp_core::schema::{parse, Kind};
use dcp_core::updates::{decode_inline, Receiver, Target};
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;

// The receiver side of the update log (#1995): updates arrive from other
// collaborators through the Matrix and Streams bindings, are parsed against
// the update schema, and go through `Receiver::ingest` (sequencing, gaps,
// base versions, inline payload decoding) before collabd hands the bytes to
// the app. The input is a JSON array of what a binding would stamp.
//
// Beyond "does not panic", two properties the sidecar relies on:
// - an update is applied at most once, however often the log repeats it;
// - reading the same retained log again applies nothing new.
#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    ts_ms: u64,
    #[serde(default)]
    sender: String,
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    base: Option<String>,
    update: serde_json::Value,
}

struct Collect(Vec<Vec<u8>>);

impl Target for Collect {
    fn supports(&self, algorithm: &str, _doc_model: &str) -> bool {
        algorithm == "loro/1"
    }
    fn dry_run(&self, _: &str, _: &str, payload: &[u8]) -> Result<(), String> {
        // Something to tell a dropped update from an applied one by.
        if payload.first() == Some(&0xFF) {
            Err("refused".into())
        } else {
            Ok(())
        }
    }
    fn apply(&mut self, _: &str, _: &str, payload: &[u8]) {
        self.0.push(payload.to_vec());
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(envelopes) = serde_json::from_slice::<Vec<Envelope>>(data) else { return };
    let base = envelopes.first().and_then(|e| e.base.clone());
    let mut log = Vec::new();
    for (i, e) in envelopes.into_iter().enumerate() {
        let Ok(update) = parse::<Update>(Kind::Update, &e.update) else { continue };
        let _ = decode_inline(&update.payload);
        log.push(Stamped { id: format!("${i}"), ts_ms: e.ts_ms, sender: e.sender, body: update, device: e.device });
    }
    let mut receiver = Receiver::new(base);
    let mut target = Collect(Vec::new());
    let first = receiver.ingest(&log, &mut target);
    let mut applied = first.applied.clone();
    applied.sort();
    applied.dedup();
    assert_eq!(applied.len(), first.applied.len(), "an update applied twice in one pass");
    assert_eq!(target.0.len(), first.applied.len(), "the report and the target disagree");
    for id in &first.applied {
        assert!(!first.dropped.iter().any(|(d, _)| d == id), "{id} both applied and dropped");
    }
    let again = receiver.ingest(&log, &mut target);
    assert!(again.applied.is_empty(), "re-reading the log applied {:?} again", again.applied);
});
