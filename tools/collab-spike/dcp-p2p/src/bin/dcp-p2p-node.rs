//! One DCP peer as a process, for the relay spike (`docs/rfc/0004-spike-relay-results.md`): a
//! `P2pStore` driven by JSON lines on stdin and answered on stdout, so that a test can run each
//! peer in its own network namespace (`ip netns exec`) and drive the conformance journeys
//! through a `DocumentStore` proxy.
//!
//! Request:  `{"id": 7, "m": "publish_version", "a": {..}}`
//! Response: `{"id": 7, "ok": ..}` or `{"id": 7, "err": {"kind": "Forbidden", "msg": ".."}}`
//! The first line is `{"ready": true, ..}`. Binary values are lower-case hex.
//!
//! Options: `--bind ip:port` (default 127.0.0.1:0), `--relay URL`, `--relay-ca FILE` (DER),
//! `--sync-ms N` (anti-entropy interval, default 200), `--device NAME`.

use std::collections::BTreeMap;
use std::io::BufRead;
use std::sync::Arc;
use std::time::Duration;

use dcp_core::{DocumentStore, Lease, Meta, NewFile, Role, Stamped, StoreError, Update, Version};
use dcp_p2p::wire::{stamped_from_json, stamped_list_to_json, stamped_to_json};
use dcp_p2p::{P2pConfig, P2pStore};
use iroh::{EndpointAddr, RelayUrl};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

type R = Result<Value, StoreError>;

fn bad(msg: impl Into<String>) -> StoreError {
    StoreError::Invalid(msg.into())
}

fn field<T: DeserializeOwned>(a: &Value, k: &str) -> Result<T, StoreError> {
    let v = a.get(k).ok_or_else(|| bad(format!("missing argument {k}")))?;
    serde_json::from_value(v.clone()).map_err(|e| bad(format!("argument {k}: {e}")))
}

fn to<T: serde::Serialize>(v: &T) -> R {
    serde_json::to_value(v).map_err(|e| StoreError::Transport(format!("encoding the answer: {e}")))
}

fn file(a: &Value, k: &str) -> Result<NewFile, StoreError> {
    let f = a.get(k).ok_or_else(|| bad(format!("missing argument {k}")))?;
    let mime: String = field(f, "mime")?;
    let h: String = field(f, "hex")?;
    Ok(NewFile {
        mime,
        bytes: hex::decode(h).map_err(|e| bad(e.to_string()))?,
    })
}

fn err_json(e: &StoreError) -> Value {
    let (kind, retry) = match e {
        StoreError::NotFound(_) => ("NotFound", None),
        StoreError::Forbidden(_) => ("Forbidden", None),
        StoreError::Conflict(_) => ("Conflict", None),
        StoreError::RateLimited { retry_after_ms } => ("RateLimited", *retry_after_ms),
        StoreError::Invalid(_) => ("Invalid", None),
        StoreError::Transport(_) => ("Transport", None),
    };
    json!({"kind": kind, "msg": e.to_string(), "retry_after_ms": retry})
}

/// CPU time per thread name, in microseconds, from /proc (CLK_TCK is 100 on Linux x86-64; the
/// driver prints `getconf CLK_TCK` next to its figures), and the resident set.
fn cpu() -> Value {
    let mut by_name: BTreeMap<String, u64> = BTreeMap::new();
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir("/proc/self/task") {
        for ent in rd.flatten() {
            let Ok(stat) = std::fs::read_to_string(ent.path().join("stat")) else { continue };
            let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else { continue };
            let name = stat[open + 1..close].to_string();
            let rest: Vec<&str> = stat[close + 1..].split_whitespace().collect();
            // rest[0] is field 3 (state); utime and stime are fields 14 and 15.
            let (Some(u), Some(s)) = (rest.get(11), rest.get(12)) else { continue };
            let ticks: u64 = u.parse::<u64>().unwrap_or(0) + s.parse::<u64>().unwrap_or(0);
            *by_name.entry(name).or_default() += ticks * 10_000;
            total += ticks * 10_000;
        }
    }
    let mut rss_kb = 0u64;
    let mut hwm_kb = 0u64;
    if let Ok(st) = std::fs::read_to_string("/proc/self/status") {
        for l in st.lines() {
            let n = |l: &str| l.split_whitespace().nth(1).and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
            if l.starts_with("VmRSS:") {
                rss_kb = n(l);
            } else if l.starts_with("VmHWM:") {
                hwm_kb = n(l);
            }
        }
    }
    json!({"cpu_us_total": total, "cpu_us_by_thread": by_name, "rss_kb": rss_kb, "rss_peak_kb": hwm_kb})
}

async fn dispatch(s: &P2pStore, relay: &Option<RelayUrl>, m: &str, a: &Value) -> R {
    match m {
        "addr" => {
            let base = s.addr();
            let mut out = EndpointAddr::new(base.id);
            for ip in base.ip_addrs() {
                out = out.with_ip_addr(*ip);
            }
            if let Some(r) = relay {
                out = out.with_relay_url(r.clone());
            }
            to(&out)
        }
        "add_peer" => {
            let addr: EndpointAddr = field(a, "addr")?;
            s.add_peer(addr).await?;
            Ok(Value::Null)
        }
        "create_document" => {
            let meta: Meta = field(a, "meta")?;
            let first = if a.get("file").is_some_and(|f| !f.is_null()) { Some(file(a, "file")?) } else { None };
            to(&s.create_document(&meta, first).await?)
        }
        "meta" => to(&s.meta(&field::<String>(a, "doc")?).await?),
        "role" => to(&s.role(&field::<String>(a, "doc")?).await?),
        "grant" => {
            s.grant(&field::<String>(a, "doc")?, &field::<String>(a, "principal")?, field::<Role>(a, "role")?).await?;
            Ok(Value::Null)
        }
        "revoke" => {
            s.revoke(&field::<String>(a, "doc")?, &field::<String>(a, "principal")?).await?;
            Ok(Value::Null)
        }
        "versions" => Ok(stamped_list_to_json(&s.versions(&field::<String>(a, "doc")?).await?)),
        "fetch_file" => {
            let v: Stamped<Version> = stamped_from_json(a.get("version").ok_or_else(|| bad("missing argument version"))?).map_err(bad)?;
            Ok(json!(hex::encode(s.fetch_file(&field::<String>(a, "doc")?, &v).await?)))
        }
        "publish_version" => {
            let parents: Vec<String> = field(a, "parents")?;
            let message: Option<String> = field(a, "message")?;
            Ok(stamped_to_json(&s.publish_version(&field::<String>(a, "doc")?, parents, file(a, "file")?, message).await?))
        }
        "lease" => Ok(s.lease(&field::<String>(a, "doc")?).await?.map_or(Value::Null, |l| stamped_to_json(&l))),
        "write_lease" => Ok(stamped_to_json(&s.write_lease(&field::<String>(a, "doc")?, field::<Lease>(a, "lease")?).await?)),
        "now_ms" => to(&s.now_ms(&field::<String>(a, "doc")?).await?),
        "publish_update" => Ok(stamped_to_json(&s.publish_update(&field::<String>(a, "doc")?, field::<Update>(a, "update")?).await?)),
        "updates" => Ok(stamped_list_to_json(&s.updates(&field::<String>(a, "doc")?).await?)),
        "join" => Ok(json!(hex::encode(s.join(&field::<String>(a, "doc")?, &field::<String>(a, "creator")?).await?))),
        "register_card" => {
            let card = hex::decode(field::<String>(a, "card")?).map_err(|e| bad(e.to_string()))?;
            to(&s.register_card(&field::<String>(a, "doc")?, card).await?)
        }
        "set_online" => {
            s.set_online(field::<bool>(a, "online")?).await?;
            Ok(Value::Null)
        }
        "sync_now" => {
            let st = s.sync_now(&field::<String>(a, "doc")?, &field::<String>(a, "peer")?).await?;
            Ok(json!({
                "received_operations": st.received_operations,
                "received_bytes": st.received_bytes,
                "sent_operations": st.sent_operations,
                "sent_bytes": st.sent_bytes,
            }))
        }
        "counters" => {
            let c = s.counters(&field::<String>(a, "doc")?).await?;
            Ok(json!({
                "ops_received": c.ops_received, "ops_invalid": c.ops_invalid, "ops_rejected": c.ops_rejected,
                "ops_applied": c.ops_applied, "entries_invalid": c.entries_invalid,
                "sync_sessions": c.sync_sessions, "sync_failures": c.sync_failures, "sync_denied": c.sync_denied,
            }))
        }
        "entry_count" => to(&s.entry_count(&field::<String>(a, "doc")?).await?),
        "space_state_bytes" => to(&s.space_state_bytes(&field::<String>(a, "doc")?).await?),
        "blob_fetches" => {
            let f = s.blob_fetches(&field::<String>(a, "doc")?).await?;
            Ok(Value::Array(f.iter().map(|b| json!({"blob": b.blob, "bytes": b.bytes, "micros": b.micros})).collect()))
        }
        "truncate_next_upload" => {
            s.truncate_next_upload();
            Ok(Value::Null)
        }
        "forge_version" => to(&s.forge_version(&field::<String>(a, "doc")?, field::<Version>(a, "version")?).await?),
        "net_report" => to(&s.net_report().await?),
        "cpu" => Ok(cpu()),
        "quit" => std::process::exit(0),
        other => Err(bad(format!("unknown method {other}"))),
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    // Logging to stderr when RUST_LOG is set (the driver redirects stderr to a file).
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = P2pConfig {
        bind: arg(&args, "--bind").unwrap_or_else(|| "127.0.0.1:0".into()),
        sync_interval: Duration::from_millis(arg(&args, "--sync-ms").and_then(|s| s.parse().ok()).unwrap_or(200)),
        device: arg(&args, "--device"),
        ..P2pConfig::default()
    };
    let relay: Option<RelayUrl> = arg(&args, "--relay").map(|u| u.parse().expect("--relay is a URL"));
    if let Some(u) = &relay {
        cfg.relay = Some(iroh::RelayMap::from_iter([u.clone()]));
    }
    if let Some(p) = arg(&args, "--relay-ca") {
        cfg.relay_ca_der = vec![std::fs::read(&p).unwrap_or_else(|e| panic!("reading {p}: {e}"))];
    }
    let store = Arc::new(P2pStore::spawn(cfg).await.expect("the node starts"));
    let online = if relay.is_some() { store.wait_online(Duration::from_secs(30)).await.unwrap_or(false) } else { true };
    println!(
        "{}",
        json!({"ready": true, "principal": store.principal(), "device": store.device(), "online": online, "pid": std::process::id()})
    );

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(l) if !l.trim().is_empty() => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        // stdin closed: the driver is gone.
        std::process::exit(0);
    });
    while let Some(line) = rx.recv().await {
        let store = store.clone();
        let relay = relay.clone();
        tokio::spawn(async move {
            let req: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    println!("{}", json!({"id": null, "err": {"kind": "Invalid", "msg": format!("bad request: {e}")}}));
                    return;
                }
            };
            let id = req.get("id").cloned().unwrap_or(Value::Null);
            let m = req.get("m").and_then(Value::as_str).unwrap_or("").to_string();
            let a = req.get("a").cloned().unwrap_or(Value::Null);
            let out = match dispatch(&store, &relay, &m, &a).await {
                Ok(v) => json!({"id": id, "ok": v}),
                Err(e) => json!({"id": id, "err": err_json(&e)}),
            };
            println!("{out}");
        });
    }
}
