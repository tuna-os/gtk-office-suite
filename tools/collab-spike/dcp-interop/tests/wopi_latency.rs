//! How long a WOPI client waits for the host, per operation, when the host's
//! backing `MatrixStore` is paced by the homeserver. Collabora abandons a WOPI
//! call after about 3 s (its log: "Timeout while requesting [POST ...] after
//! 3003ms", seen in the spike-F churn run with a spent message budget), so what
//! matters is how many calls take longer. A scripted editor session on the wire
//! (no Collabora): per cycle `GetFile`, `LOCK`, two `PutFile`, `REFRESH_LOCK`,
//! `UNLOCK`, six cycles; every response must be 200, and the latency of each
//! request is reported (`LATENCY` lines and `DCP_MEASURE_OUT`). No latency is
//! asserted: this measures; the profile (`relaxed`, `default`, `default-drained`)
//! is what changes it.

use std::collections::BTreeMap;

use dcp_interop::common;
use dcp_interop::profile;
use dcp_interop::rig::synapse;
use dcp_interop::{Rig, RigOptions, Who};
use serde_json::json;

const COLLABORA_WOPI_TIMEOUT_MS: u128 = 3_003;
const CYCLES: usize = 6;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wopi_request_latency_under_the_homeservers_limit() {
    let Some(syn) = synapse() else { return };
    let label = profile::label(&syn);
    let rig = Rig::new(&syn, RigOptions::honest()).await.expect("rig");
    let ed = rig.raw(Who::Editor);
    let mut samples: BTreeMap<&str, Vec<u128>> = BTreeMap::new();
    let before = rig.stats();
    for cycle in 0..CYCLES {
        let lock = format!("latency-lock-{cycle}");
        let mut record = |op: &'static str, r: &dcp_interop::raw::Resp| {
            assert_eq!(r.status, 200, "{op} in cycle {cycle}: HTTP {} {:?}", r.status, r.text());
            samples.entry(op).or_default().push(r.elapsed_ms);
        };
        record("GetFile", &ed.get_file(None).await.expect("GetFile"));
        record("LOCK", &ed.lock(&lock).await.expect("LOCK"));
        for i in 0..2 {
            let bytes = format!("cycle {cycle} save {i}");
            record("PutFile", &ed.put_file(Some(&lock), None, bytes.as_bytes()).await.expect("PutFile"));
        }
        record("REFRESH_LOCK", &ed.refresh_lock(&lock).await.expect("REFRESH_LOCK"));
        record("UNLOCK", &ed.unlock(&lock).await.expect("UNLOCK"));
    }
    let spent = profile::stats_since(rig.stats(), before);
    let mut rows = Vec::new();
    let (mut total, mut slow) = (0usize, 0usize);
    for (op, v) in &mut samples {
        v.sort_unstable();
        let over = v.iter().filter(|ms| **ms > COLLABORA_WOPI_TIMEOUT_MS).count();
        total += v.len();
        slow += over;
        println!(
            "LATENCY {label:<15} {op:<13} n {:>2}  min {:>5} ms  median {:>5} ms  max {:>5} ms  over Collabora's {COLLABORA_WOPI_TIMEOUT_MS} ms: {over}",
            v.len(),
            v[0],
            v[v.len() / 2],
            v[v.len() - 1]
        );
        rows.push(json!({"op": op, "n": v.len(), "min_ms": v[0], "median_ms": v[v.len() / 2], "max_ms": v[v.len() - 1], "over_3003_ms": over, "all_ms": v}));
    }
    println!(
        "LATENCY {label:<15} total: {slow} of {total} WOPI calls took longer than {COLLABORA_WOPI_TIMEOUT_MS} ms; the host's store sent {} events, was answered 429 {} times, waited {:.1} s for the limiter",
        spent.sent,
        spent.rate_limited,
        spent.waited_ms as f64 / 1000.0
    );
    common::record(&syn, "wopi_latency", json!({"label": label, "rows": rows, "calls": total, "over_3003_ms": slow, "sent": spent.sent, "rate_limited": spent.rate_limited, "waited_secs": spent.waited_ms as f64 / 1000.0}));
}
