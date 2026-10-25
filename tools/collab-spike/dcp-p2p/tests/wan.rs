//! dcp-p2p over a real `iroh-relay`, with every peer in its own Linux network namespace and the
//! links shaped with `tc netem`. Results: `docs/rfc/0004-spike-relay-results.md`.
//!
//! Needs root, the namespaces and the relay's CA: `tools/collab-spike/scripts/netns-wan.sh up`,
//! then `IROH_RELAY_BIN=... netns-wan.sh relay-up`, then (as root)
//! `cargo test --release --test wan -- --test-threads=1 --nocapture`. Shape the links first with
//! `netns-wan.sh shape <rtt_ms> <loss_pct> <rate>`; the shaping is not this file's business, only
//! `DCP_WAN_CONDITION` (a label) is.
//!
//! Each peer is a `dcp-p2p-node` process (`ip netns exec dcp-N ...`) driven through JSON lines;
//! `Node` is a `DocumentStore` proxy, so the spec section 12 journeys run unchanged.
//!
//! A test that needs the namespaces FAILS when they are missing. Only `DCP_SKIP_NETNS=1` skips
//! them, and the test prints `SKIP`.
//!
//! Environment: DCP_WAN_CONDITION (label), DCP_WAN_SCRIPT, DCP_WAN_NODE_BIN, DCP_WAN_RELAY_URL
//! (default https://10.77.0.1), DCP_WAN_CA (DER, default /tmp/dcp-wan/ca.der), DCP_WAN_OUT
//! (default /tmp/dcp-wan-out), DCP_WAN_NODE_LOG (RUST_LOG of the nodes), DCP_WAN_SCALE (timeout
//! multiplier, default 1), DCP_WAN_BIG=1 (also 1,000 entries), DCP_WAN_UPGRADE_REPS (default 5).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::future::Future;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parking_lot::Mutex;
use dcp_conformance::fixture::Faults;
use dcp_conformance::runner::{journey_names, JOURNEYS};
use dcp_conformance::{
    new_file, run_one, standard_meta, Capabilities, Failure, Fixture, Forger, Outcome, RealTime, StoreFactory,
    ALGORITHM, BASE_FILE, DOC_MODEL,
};
use dcp_core::view::read_view;
use dcp_core::{DocId, DocumentStore, Lease, Meta, NewFile, Role, Stamped, StoreError, Update, Version};
use dcp_p2p::wire::{stamped_from_json, stamped_list_from_json, stamped_to_json};
use iroh::{EndpointAddr, TransportAddr};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tokio::sync::oneshot;

const SYNC_ALPN: &str = "dcp/p2p-sync/1";

// ------------------------------------------------------------------------------ environment

struct Env {
    script: String,
    node_bin: String,
    relay_url: String,
    ca_der: String,
    out: String,
    condition: String,
    node_log: String,
    scale: f64,
}

static ENV: LazyLock<Env> = LazyLock::new(|| {
    let g = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let e = Env {
        script: g("DCP_WAN_SCRIPT", concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/netns-wan.sh")),
        node_bin: g("DCP_WAN_NODE_BIN", env!("CARGO_BIN_EXE_dcp-p2p-node")),
        relay_url: g("DCP_WAN_RELAY_URL", "https://10.77.0.1"),
        ca_der: g("DCP_WAN_CA", "/tmp/dcp-wan/ca.der"),
        out: g("DCP_WAN_OUT", "/tmp/dcp-wan-out"),
        condition: g("DCP_WAN_CONDITION", "unshaped"),
        node_log: g("DCP_WAN_NODE_LOG", "warn"),
        scale: g("DCP_WAN_SCALE", "1").parse().unwrap_or(1.0),
    };
    std::fs::create_dir_all(&e.out).expect("output directory");
    e
});

fn env() -> &'static Env {
    &ENV
}

/// A timeout, scaled for the shaped conditions.
fn secs(s: u64) -> Duration {
    Duration::from_secs_f64(s as f64 * env().scale)
}

fn big() -> bool {
    std::env::var("DCP_WAN_BIG").as_deref() == Ok("1")
}

fn topology_present() -> bool {
    let out = Command::new("ip").args(["netns", "list"]).output();
    match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout).to_string();
            ["dcp-a", "dcp-b", "dcp-c", "dcp-r"].iter().all(|n| s.lines().any(|l| l.split_whitespace().next() == Some(n)))
        }
        Err(_) => false,
    }
}

fn script(args: &[&str]) -> String {
    let e = env();
    let out = Command::new(&e.script).args(args).output().unwrap_or_else(|err| panic!("running {} {args:?}: {err}", e.script));
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{} {args:?} failed:\n{text}", e.script);
    text
}

/// Gate a test on the namespaces, make sure the relay runs, set the firewall mode. `false`: skip.
fn begin(name: &str, mode: &str) -> bool {
    if !topology_present() {
        if std::env::var("DCP_SKIP_NETNS").as_deref() == Ok("1") {
            eprintln!("SKIP {name}: the network namespaces dcp-a, dcp-b, dcp-c, dcp-r are not there (DCP_SKIP_NETNS=1)");
            return false;
        }
        panic!(
            "{name}: the network namespaces dcp-a, dcp-b, dcp-c, dcp-r are missing. Run `sudo tools/collab-spike/scripts/netns-wan.sh up` \
             and `relay-up` (as root), or set DCP_SKIP_NETNS=1 to skip this test"
        );
    }
    script(&["relay-up"]);
    script(&["mode", mode]);
    eprintln!("== {name}: condition {}, firewall mode {mode}", env().condition);
    true
}

macro_rules! begin {
    ($name:expr, $mode:expr) => {
        if !begin($name, $mode) {
            return;
        }
    };
}

/// Print and append one result line.
fn record(test: &str, mut v: Value) {
    let o = v.as_object_mut().expect("a JSON object");
    o.insert("condition".into(), json!(env().condition));
    o.insert("test".into(), json!(test));
    let line = v.to_string();
    eprintln!("MEASURE {line}");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(format!("{}/results.jsonl", env().out)) {
        let _ = writeln!(f, "{line}");
    }
}

fn dump(name: &str, v: &Value) {
    let _ = std::fs::write(format!("{}/{}-{}.json", env().out, env().condition, name), serde_json::to_string_pretty(v).unwrap_or_default());
}

fn ms(d: Duration) -> u64 {
    d.as_millis() as u64
}

fn median(v: &mut [u64]) -> u64 {
    v.sort_unstable();
    v[v.len() / 2]
}

// ------------------------------------------------------------------------------ node process

enum Waiter {
    Async(oneshot::Sender<Value>),
    Sync(std::sync::mpsc::Sender<Value>),
}

/// One peer: a `dcp-p2p-node` process in a network namespace, and a `DocumentStore` for it.
struct Node {
    label: String,
    idx: usize,
    principal: String,
    device: String,
    /// From starting the process until it printed `ready` (startup plus reaching the relay).
    ready_ms: u64,
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    pending: Arc<Mutex<HashMap<u64, Waiter>>>,
    next: AtomicU64,
}

static SUMMARIES: Mutex<Vec<(String, Value)>> = Mutex::new(Vec::new());
static NODE_SEQ: AtomicU64 = AtomicU64::new(0);

fn decode_err(e: &Value) -> StoreError {
    let msg = e["msg"].as_str().unwrap_or("").to_string();
    // The node prints the Display of the error; keep the kind, drop the prefix.
    let strip = |p: &str| msg.strip_prefix(p).unwrap_or(&msg).to_string();
    match e["kind"].as_str().unwrap_or("") {
        "NotFound" => StoreError::NotFound(strip("not found: ")),
        "Forbidden" => StoreError::Forbidden(strip("forbidden: ")),
        "Conflict" => StoreError::Conflict(strip("conflict: ")),
        "RateLimited" => StoreError::RateLimited { retry_after_ms: e["retry_after_ms"].as_u64() },
        "Invalid" => StoreError::Invalid(strip("invalid: ")),
        _ => StoreError::Transport(strip("transport: ")),
    }
}

fn dec<T: DeserializeOwned>(v: Value) -> Result<T, StoreError> {
    serde_json::from_value(v).map_err(|e| StoreError::Transport(format!("decoding a node's answer: {e}")))
}

impl Node {
    /// Start peer `idx` (0, 1, 2 = namespace dcp-a, dcp-b, dcp-c); panics when it cannot.
    async fn spawn(idx: usize, label: &str) -> Arc<Node> {
        match Node::try_spawn(idx, label).await {
            Ok(n) => n,
            Err(e) => panic!("{e}"),
        }
    }

    /// Like `spawn`, but says why instead of panicking: the process did not become ready, or
    /// could not reach the relay within 30 s.
    async fn try_spawn(idx: usize, label: &str) -> Result<Arc<Node>, String> {
        let t_spawn = Instant::now();
        let e = env();
        let ns = ["dcp-a", "dcp-b", "dcp-c"][idx];
        let ip = format!("10.77.{}.2", idx + 1);
        let seq = NODE_SEQ.fetch_add(1, Ordering::SeqCst);
        let log_path = format!("{}/node-{}-{:04}-{}.log", e.out, e.condition, seq, label);
        let log = std::fs::File::create(&log_path).expect("node log");
        let mut child = Command::new("ip")
            .args(["netns", "exec", ns, e.node_bin.as_str(), "--bind", format!("{ip}:0").as_str(), "--relay", e.relay_url.as_str(), "--relay-ca", e.ca_der.as_str()])
            .args(["--sync-ms", "200", "--device", label])
            .env("RUST_LOG", &e.node_log)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log))
            .spawn()
            .expect("ip netns exec (is this running as root?)");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let pending: Arc<Mutex<HashMap<u64, Waiter>>> = Arc::default();
        let (ready_tx, ready_rx) = oneshot::channel::<Value>();
        let p2 = pending.clone();
        std::thread::spawn(move || {
            let mut ready_tx = Some(ready_tx);
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                if v.get("ready").is_some() {
                    if let Some(t) = ready_tx.take() {
                        let _ = t.send(v);
                    }
                    continue;
                }
                let Some(id) = v.get("id").and_then(Value::as_u64) else { continue };
                let w = p2.lock().remove(&id);
                match w {
                    Some(Waiter::Async(t)) => {
                        let _ = t.send(v);
                    }
                    Some(Waiter::Sync(t)) => {
                        let _ = t.send(v);
                    }
                    None => {}
                }
            }
            // The process is gone: drop every sender so the waiting calls fail.
            p2.lock().clear();
        });
        let ready = match tokio::time::timeout(Duration::from_secs(120), ready_rx).await {
            Ok(Ok(v)) => v,
            other => {
                let tail = std::fs::read_to_string(&log_path).unwrap_or_default();
                let _ = child.kill();
                return Err(format!("node {label} in {ns} did not become ready ({other:?}); its log:\n{tail}"));
            }
        };
        let ready_ms = ms(t_spawn.elapsed());
        if ready["online"].as_bool() != Some(true) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "node {label} in {ns} did not reach the relay {} within 30 s (ready line after {ready_ms} ms); its log:\n{}",
                e.relay_url,
                std::fs::read_to_string(&log_path).unwrap_or_default()
            ));
        }
        Ok(Arc::new(Node {
            label: label.to_string(),
            idx,
            principal: ready["principal"].as_str().unwrap().to_string(),
            device: ready["device"].as_str().unwrap().to_string(),
            ready_ms,
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            pending,
            next: AtomicU64::new(1),
        }))
    }

    fn send(&self, m: &str, a: Value, w: Waiter) -> u64 {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        self.pending.lock().insert(id, w);
        let line = json!({"id": id, "m": m, "a": a}).to_string();
        let mut s = self.stdin.lock();
        if writeln!(s, "{line}").and_then(|_| s.flush()).is_err() {
            self.pending.lock().remove(&id);
        }
        id
    }

    fn unwrap_resp(resp: Value) -> Result<Value, StoreError> {
        match resp.get("err") {
            Some(e) => Err(decode_err(e)),
            None => Ok(resp["ok"].clone()),
        }
    }

    async fn call(&self, m: &str, a: Value) -> Result<Value, StoreError> {
        let (tx, rx) = oneshot::channel();
        let id = self.send(m, a, Waiter::Async(tx));
        match tokio::time::timeout(Duration::from_secs(600), rx).await {
            Ok(Ok(resp)) => Self::unwrap_resp(resp),
            Ok(Err(_)) => Err(StoreError::Transport(format!("node {} died during {m}", self.label))),
            Err(_) => {
                self.pending.lock().remove(&id);
                Err(StoreError::Transport(format!("node {}: {m} did not answer in 600 s", self.label)))
            }
        }
    }

    fn call_sync(&self, m: &str, a: Value, max: Duration) -> Result<Value, StoreError> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.send(m, a, Waiter::Sync(tx));
        match rx.recv_timeout(max) {
            Ok(resp) => Self::unwrap_resp(resp),
            Err(_) => Err(StoreError::Transport(format!("node {}: {m} did not answer", self.label))),
        }
    }

    async fn addr(&self) -> EndpointAddr {
        dec(self.call("addr", json!({})).await.expect("addr")).expect("an EndpointAddr")
    }

    async fn add_peer(&self, addr: &EndpointAddr) {
        self.call("add_peer", json!({"addr": addr})).await.expect("add_peer");
    }

    async fn join(&self, doc: &DocId, creator: &str) -> String {
        dec(self.call("join", json!({"doc": doc, "creator": creator})).await.expect("join")).unwrap()
    }

    async fn register_card(&self, doc: &DocId, card: &str) {
        self.call("register_card", json!({"doc": doc, "card": card})).await.expect("register_card");
    }

    async fn set_online(&self, online: bool) {
        self.call("set_online", json!({"online": online})).await.expect("set_online");
    }

    async fn sync_now(&self, doc: &DocId, peer: &str) -> Result<Value, StoreError> {
        self.call("sync_now", json!({"doc": doc, "peer": peer})).await
    }

    async fn net_report(&self) -> Rep {
        Rep(self.call("net_report", json!({})).await.expect("net_report"))
    }

    /// (total CPU, CPU of the store's own thread) in microseconds, and the resident set in kB.
    async fn cpu(&self) -> (u64, u64, u64) {
        let v = self.call("cpu", json!({})).await.expect("cpu");
        (
            v["cpu_us_total"].as_u64().unwrap_or(0),
            v["cpu_us_by_thread"]["dcp-p2p"].as_u64().unwrap_or(0),
            v["rss_kb"].as_u64().unwrap_or(0),
        )
    }

    fn ns(&self) -> &'static str {
        ["dcp-a", "dcp-b", "dcp-c"][self.idx]
    }

    /// Bytes received and sent on this peer's veth, as the namespace's kernel counts them.
    fn iface_bytes(&self) -> (u64, u64) {
        let dev = format!("vn-{}", ["a", "b", "c"][self.idx]);
        let read = |f: &str| -> u64 {
            let o = Command::new("ip")
                .args(["netns", "exec", self.ns(), "cat", &format!("/sys/class/net/{dev}/statistics/{f}")])
                .output()
                .expect("ip netns exec cat");
            String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0)
        };
        (read("rx_bytes"), read("tx_bytes"))
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        // The path summary of every node that ever ran, taken as it goes away.
        if let Ok(rep) = self.call_sync("net_report", json!({}), Duration::from_secs(10)) {
            SUMMARIES.lock().push((self.label.clone(), Rep(rep).summary()));
        }
        let mut c = self.child.lock();
        let _ = c.kill();
        let _ = c.wait();
    }
}

#[async_trait]
impl DocumentStore for Node {
    fn principal(&self) -> String {
        self.principal.clone()
    }
    fn device(&self) -> String {
        self.device.clone()
    }
    async fn create_document(&self, meta: &Meta, first: Option<NewFile>) -> Result<DocId, StoreError> {
        let file = first.map(|f| json!({"mime": f.mime, "hex": hex::encode(f.bytes)}));
        dec(self.call("create_document", json!({"meta": meta, "file": file})).await?)
    }
    async fn meta(&self, doc: &DocId) -> Result<Meta, StoreError> {
        dec(self.call("meta", json!({"doc": doc})).await?)
    }
    async fn role(&self, doc: &DocId) -> Result<Role, StoreError> {
        dec(self.call("role", json!({"doc": doc})).await?)
    }
    async fn grant(&self, doc: &DocId, principal: &str, role: Role) -> Result<(), StoreError> {
        self.call("grant", json!({"doc": doc, "principal": principal, "role": role})).await.map(|_| ())
    }
    async fn revoke(&self, doc: &DocId, principal: &str) -> Result<(), StoreError> {
        self.call("revoke", json!({"doc": doc, "principal": principal})).await.map(|_| ())
    }
    async fn versions(&self, doc: &DocId) -> Result<Vec<Stamped<Version>>, StoreError> {
        stamped_list_from_json(&self.call("versions", json!({"doc": doc})).await?).map_err(StoreError::Transport)
    }
    async fn fetch_file(&self, doc: &DocId, version: &Stamped<Version>) -> Result<Vec<u8>, StoreError> {
        let h: String = dec(self.call("fetch_file", json!({"doc": doc, "version": stamped_to_json(version)})).await?)?;
        hex::decode(h).map_err(|e| StoreError::Transport(e.to_string()))
    }
    async fn publish_version(
        &self,
        doc: &DocId,
        parents: Vec<String>,
        file: NewFile,
        message: Option<String>,
    ) -> Result<Stamped<Version>, StoreError> {
        let f = json!({"mime": file.mime, "hex": hex::encode(file.bytes)});
        stamped_from_json(&self.call("publish_version", json!({"doc": doc, "parents": parents, "file": f, "message": message})).await?).map_err(StoreError::Transport)
    }
    async fn lease(&self, doc: &DocId) -> Result<Option<Stamped<Lease>>, StoreError> {
        {
            let v = self.call("lease", json!({"doc": doc})).await?;
            if v.is_null() { Ok(None) } else { stamped_from_json(&v).map(Some).map_err(StoreError::Transport) }
        }
    }
    async fn write_lease(&self, doc: &DocId, lease: Lease) -> Result<Stamped<Lease>, StoreError> {
        stamped_from_json(&self.call("write_lease", json!({"doc": doc, "lease": lease})).await?).map_err(StoreError::Transport)
    }
    async fn now_ms(&self, doc: &DocId) -> Result<u64, StoreError> {
        dec(self.call("now_ms", json!({"doc": doc})).await?)
    }
    async fn publish_update(&self, doc: &DocId, update: Update) -> Result<Stamped<Update>, StoreError> {
        stamped_from_json(&self.call("publish_update", json!({"doc": doc, "update": update})).await?).map_err(StoreError::Transport)
    }
    async fn updates(&self, doc: &DocId) -> Result<Vec<Stamped<Update>>, StoreError> {
        stamped_list_from_json(&self.call("updates", json!({"doc": doc})).await?).map_err(StoreError::Transport)
    }
}

struct EditorFaults(Arc<Node>);
impl Faults for EditorFaults {
    fn truncate_next_upload(&self) {
        self.0.call_sync("truncate_next_upload", json!({}), Duration::from_secs(30)).expect("truncate_next_upload");
    }
}

struct ViewerForger(Arc<Node>);
#[async_trait]
impl Forger for ViewerForger {
    async fn forge_version(&self, doc: &DocId, version: Version) -> Result<Option<String>, Failure> {
        match self.0.call("forge_version", json!({"doc": doc, "version": version})).await {
            Ok(v) => Ok(Some(v.as_str().unwrap_or("").to_string())),
            Err(StoreError::Forbidden(_)) => Ok(None),
            Err(e) => Err(Failure::new(format!("forging failed unexpectedly: {e}"))),
        }
    }
}

// ------------------------------------------------------------------------------ addresses, setup

/// What the peers are told about each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Addrs {
    /// The endpoint id and the relay URL, no IP address.
    RelayOnly,
    /// The endpoint id, the relay URL and the peer's bound IP address.
    WithDirect,
}

fn shown(a: &EndpointAddr, how: Addrs) -> EndpointAddr {
    match how {
        Addrs::WithDirect => a.clone(),
        Addrs::RelayOnly => EndpointAddr::from_parts(a.id, a.relay_urls().map(|u| TransportAddr::Relay(u.clone()))),
    }
}

async fn mesh(nodes: &[&Arc<Node>], how: Addrs) {
    let mut addrs = Vec::new();
    for n in nodes {
        addrs.push(n.addr().await);
    }
    for (i, a) in nodes.iter().enumerate() {
        for (j, _) in nodes.iter().enumerate() {
            if i != j {
                a.add_peer(&shown(&addrs[j], how)).await;
            }
        }
    }
}

async fn wait_for<T, F, Fut>(what: &str, max: Duration, mut probe: F) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let deadline = Instant::now() + max;
    loop {
        if let Some(v) = probe().await {
            return v;
        }
        if Instant::now() >= deadline {
            panic!("timed out after {max:?} waiting for: {what}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_soft<T, F, Fut>(max: Duration, interval: Duration, mut probe: F) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let deadline = Instant::now() + max;
    loop {
        if let Some(v) = probe().await {
            return Some(v);
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(interval).await;
    }
}

async fn invite(owner: &Arc<Node>, guest: &Arc<Node>, doc: &DocId) {
    let card = guest.join(doc, &owner.principal()).await;
    owner.register_card(doc, &card).await;
}

/// Two nodes, one document, `b` a viewer who has read it. `a` is the owner.
async fn pair(how: Addrs, role: Role) -> (Arc<Node>, Arc<Node>, DocId) {
    let (a, b) = tokio::join!(Node::spawn(0, "a"), Node::spawn(1, "b"));
    mesh(&[&a, &b], how).await;
    let doc = a.create_document(&standard_meta(), Some(new_file(b"wan"))).await.expect("create_document");
    invite(&a, &b, &doc).await;
    a.grant(&doc, &b.principal(), role).await.expect("grant");
    wait_for("b has its role and the first version", secs(120), || async {
        if b.role(&doc).await.ok()? != role {
            return None;
        }
        (b.versions(&doc).await.ok()?.len() == 1).then_some(())
    })
    .await;
    (a, b, doc)
}

/// Owner, editor, viewer, one document with one version, everything synced.
async fn trio(how: Addrs) -> (Arc<Node>, Arc<Node>, Arc<Node>, DocId) {
    let (o, e, v) = tokio::join!(Node::spawn(0, "owner"), Node::spawn(1, "editor"), Node::spawn(2, "viewer"));
    mesh(&[&o, &e, &v], how).await;
    let doc = o.create_document(&standard_meta(), Some(new_file(BASE_FILE))).await.expect("create_document");
    invite(&o, &e, &doc).await;
    invite(&o, &v, &doc).await;
    o.grant(&doc, &e.principal(), Role::Editor).await.expect("grant editor");
    o.grant(&doc, &v.principal(), Role::Viewer).await.expect("grant viewer");
    for (n, role) in [(&e, Role::Editor), (&v, Role::Viewer)] {
        wait_for(&format!("{} has role {role:?} and the first version", n.label), secs(120), || async {
            if n.role(&doc).await.ok()? != role {
                return None;
            }
            n.meta(&doc).await.ok()?;
            (n.versions(&doc).await.ok()?.len() == 1).then_some(())
        })
        .await;
    }
    (o, e, v, doc)
}

async fn head_ids(n: &Arc<Node>, doc: &DocId) -> Vec<String> {
    match read_view(&**n, doc).await {
        Ok(v) => v.head_ids().into_iter().map(str::to_owned).collect(),
        Err(_) => Vec::new(),
    }
}

async fn first_version(n: &Arc<Node>, doc: &DocId) -> String {
    n.versions(doc).await.unwrap().into_iter().find(|v| v.body.parents.is_empty()).unwrap().id
}

// ------------------------------------------------------------------------------ path analysis

/// A `net_report` answer (see `dcp_p2p::pathlog`), read only through iroh's own path data.
struct Rep(Value);

impl Rep {
    fn arr(&self, k: &str) -> Vec<&Value> {
        self.0[k].as_array().map(|a| a.iter().collect()).unwrap_or_default()
    }
    fn metric(&self, k: &str) -> u64 {
        self.0["metrics"][k].as_u64().unwrap_or(0)
    }
    fn sync_conns(&self, peer: &str) -> Vec<&Value> {
        self.arr("conns").into_iter().filter(|c| c["alpn"] == SYNC_ALPN && c["peer"] == peer).collect()
    }
    /// An IP path existed (open at the start, or opened later, or selected) on any connection to `peer`.
    fn ip_seen(&self, peer: &str) -> bool {
        self.arr("events").into_iter().any(|e| {
            e["peer"] == peer && e["transport"] == "ip" && matches!(e["event"].as_str(), Some("initial" | "opened" | "selected"))
        }) || self.arr("live").into_iter().any(|p| p["peer"] == peer && p["transport"] == "ip")
    }
    /// The transport of the selected path of every open sync connection to `peer`.
    fn selected_now(&self, peer: &str) -> Vec<String> {
        self.arr("live")
            .into_iter()
            .filter(|p| p["peer"] == peer && p["alpn"] == SYNC_ALPN && p["selected"] == true)
            .map(|p| p["transport"].as_str().unwrap_or("?").to_string())
            .collect()
    }
    /// Bytes (sent plus received) on paths of `transport`, optionally for one peer: open paths plus
    /// the final statistics of closed ones.
    fn bytes(&self, peer: Option<&str>, transport: &str) -> u64 {
        let ok = |p: &Value| p["transport"] == transport && peer.map_or(true, |x| p["peer"] == x);
        let live: u64 = self.arr("live").into_iter().filter(|p| ok(p)).map(|p| p["tx_bytes"].as_u64().unwrap_or(0) + p["rx_bytes"].as_u64().unwrap_or(0)).sum();
        let closed: u64 = self
            .arr("events")
            .into_iter()
            .filter(|e| e["event"] == "closed" && ok(e))
            .map(|e| e["tx_bytes"].as_u64().unwrap_or(0) + e["rx_bytes"].as_u64().unwrap_or(0))
            .sum();
        live + closed
    }
    /// The dial-side connections to `peer`: (id, start, handshake done).
    fn dials(&self, peer: &str) -> Vec<(u64, u64, u64)> {
        self.arr("conns")
            .into_iter()
            .filter(|c| c["peer"] == peer && c["side"] == "dial")
            .filter_map(|c| Some((c["id"].as_u64()?, c["start_us"].as_u64()?, c["connected_us"].as_u64()?)))
            .collect()
    }
    /// An IP path was already open on connection `id` when tracking began (right after the handshake).
    fn conn_ip_at_handshake(&self, id: u64) -> bool {
        self.arr("events").into_iter().any(|e| e["conn"] == id && e["event"] == "initial" && e["transport"] == "ip")
    }
    /// When IP was first selected on connection `id`.
    fn conn_first_selected_ip_us(&self, id: u64) -> Option<u64> {
        self.arr("events").into_iter().filter(|e| e["conn"] == id && e["event"] == "selected" && e["transport"] == "ip").filter_map(|e| e["t_us"].as_u64()).min()
    }
    fn conn_closed(&self, id: u64) -> bool {
        self.arr("conns").into_iter().any(|c| c["id"] == id && !c["closed_us"].is_null())
    }
    fn events_after(&self, peer: &str, t_us: u64, event: &str, transport: &str) -> Option<u64> {
        self.arr("events")
            .into_iter()
            .filter(|e| e["peer"] == peer && e["event"] == event && e["transport"] == transport)
            .filter_map(|e| e["t_us"].as_u64())
            .filter(|t| *t >= t_us)
            .min()
    }
    fn summary(&self) -> Value {
        let conns = self.arr("conns");
        let ids_with = |transport: &str, event: &str| -> usize {
            let mut ids: Vec<u64> = self.arr("events").into_iter().filter(|e| e["transport"] == transport && e["event"] == event).filter_map(|e| e["conn"].as_u64()).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len()
        };
        let mut ip_ids: Vec<u64> = self.arr("events").into_iter().filter(|e| e["transport"] == "ip").filter_map(|e| e["conn"].as_u64()).collect();
        ip_ids.sort_unstable();
        ip_ids.dedup();
        let ip_conns = ip_ids.len();
        json!({
            "conns": conns.len(),
            "conns_dialled": conns.iter().filter(|c| c["side"] == "dial").count(),
            "conns_with_an_ip_path": ip_conns,
            "conns_that_selected_ip": ids_with("ip", "selected"),
            "conns_that_selected_relay": ids_with("relay", "selected"),
            "ip_bytes": self.bytes(None, "ip"),
            "relay_bytes": self.bytes(None, "relay"),
            "send_ipv4": self.metric("send_ipv4"),
            "send_relay": self.metric("send_relay"),
            "recv_data_ipv4": self.metric("recv_data_ipv4"),
            "recv_data_relay": self.metric("recv_data_relay"),
            "holepunch_attempts": self.metric("holepunch_attempts"),
        })
    }
}

/// `Ok` when iroh's own path data says the two nodes talked through the relay and nothing else.
fn relay_only(a: &Rep, b: &Rep, a_id: &str, b_id: &str) -> Result<(), String> {
    for (who, r, peer) in [("a", a, b_id), ("b", b, a_id)] {
        if r.sync_conns(peer).is_empty() {
            return Err(format!("{who} has no sync connection to its peer"));
        }
        if r.ip_seen(peer) {
            return Err(format!("{who}: an IP path to the peer existed"));
        }
        let ip = r.bytes(Some(peer), "ip");
        if ip > 0 {
            return Err(format!("{who}: {ip} bytes crossed an IP path"));
        }
        let sel = r.selected_now(peer);
        if sel.is_empty() || sel.iter().any(|t| t != "relay") {
            return Err(format!("{who}: selected paths are {sel:?}, not all relay"));
        }
        if r.bytes(Some(peer), "relay") == 0 {
            return Err(format!("{who}: the relay path carried no bytes"));
        }
    }
    Ok(())
}

/// `Ok` when iroh's own path data says a sync connection of each node selected a direct IP path
/// that has carried bytes.
fn direct(a: &Rep, b: &Rep, a_id: &str, b_id: &str) -> Result<(), String> {
    for (who, r, peer) in [("a", a, b_id), ("b", b, a_id)] {
        let sel = r.selected_now(peer);
        if !sel.iter().any(|t| t == "ip") {
            return Err(format!("{who}: selected paths are {sel:?}, none is direct"));
        }
        if r.bytes(Some(peer), "ip") == 0 {
            return Err(format!("{who}: no bytes crossed an IP path"));
        }
    }
    Ok(())
}

async fn reports(a: &Arc<Node>, b: &Arc<Node>) -> (Rep, Rep) {
    (a.net_report().await, b.net_report().await)
}

/// Wait until both nodes select a direct path (or `max` passes).
async fn wait_direct(a: &Arc<Node>, b: &Arc<Node>, max: Duration) -> Option<Duration> {
    let t = Instant::now();
    wait_soft(max, Duration::from_millis(250), || async {
        let (ra, rb) = reports(a, b).await;
        direct(&ra, &rb, &a.principal, &b.principal).ok().map(|_| t.elapsed())
    })
    .await
}

fn print_summaries(test: &str) {
    let mut s = SUMMARIES.lock();
    let mut tot = (0u64, 0u64, 0u64, 0u64, 0u64);
    for (_, v) in s.iter() {
        tot.0 += v["conns"].as_u64().unwrap_or(0);
        tot.1 += v["conns_with_an_ip_path"].as_u64().unwrap_or(0);
        tot.2 += v["conns_that_selected_ip"].as_u64().unwrap_or(0);
        tot.3 += v["ip_bytes"].as_u64().unwrap_or(0);
        tot.4 += v["relay_bytes"].as_u64().unwrap_or(0);
    }
    record(
        test,
        json!({"kind": "paths", "nodes": s.len(), "conns": tot.0, "conns_with_an_ip_path": tot.1, "conns_that_selected_ip": tot.2, "ip_bytes": tot.3, "relay_bytes": tot.4,
               "per_node": s.iter().map(|(l, v)| json!({"node": l, "summary": v})).collect::<Vec<_>>()}),
    );
    s.clear();
}

// ------------------------------------------------------------------------------ conformance

struct WanFactory {
    how: Addrs,
    mode: &'static str,
}

#[async_trait]
impl StoreFactory for WanFactory {
    fn name(&self) -> String {
        format!("p2p-iroh-p2panda-wan-{}", self.mode)
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            grant_revoke: true,
            revocation: true,
            arbitrary_lease_writes: true,
            racy_lease: true,
            history: true,
            arbitrary_parents: true,
            updates: true,
            upload_faults: true,
            forgery: true,
        }
    }

    async fn fresh(&self) -> Result<Fixture, Failure> {
        let (owner, editor, viewer, doc) = trio(self.how).await;
        let mut fx = Fixture::from_existing(
            owner,
            editor.clone(),
            viewer.clone(),
            doc,
            Arc::new(RealTime),
            self.capabilities(),
            Some(Arc::new(EditorFaults(editor))),
            secs(30),
        )
        .await?;
        fx.forger = Some(Arc::new(ViewerForger(viewer)));
        Ok(fx)
    }
}

/// Every journey of spec section 12, timed one by one.
async fn journeys(test: &str, mode: &'static str, how: Addrs) {
    let factory = WanFactory { how, mode };
    let mut failed = Vec::new();
    for name in journey_names() {
        let t = Instant::now();
        let r = run_one(&factory, name).await;
        let spec = JOURNEYS.iter().find(|j| j.name == name).map(|j| j.spec_test).unwrap_or("?");
        let (outcome, why) = match &r.outcome {
            Outcome::Passed => ("pass", String::new()),
            Outcome::Failed(f) => {
                failed.push(name);
                ("FAIL", f.to_string())
            }
            Outcome::Skipped { missing } => ("skipped", missing.join(",")),
        };
        record(test, json!({"kind": "journey", "mode": mode, "journey": name, "spec_test": spec, "outcome": outcome, "ms": ms(t.elapsed()), "why": why}));
    }
    print_summaries(test);
    assert!(failed.is_empty(), "journeys failed under condition {}, mode {mode}: {failed:?}", env().condition);
}

// ------------------------------------------------------------------------------ tests: journeys

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn journeys_relay_only() {
    begin!("journeys_relay_only", "relay-only");
    journeys("journeys_relay_only", "relay-only", Addrs::RelayOnly).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn journeys_direct_allowed() {
    begin!("journeys_direct_allowed", "direct");
    journeys("journeys_direct_allowed", "direct", Addrs::WithDirect).await;
}

macro_rules! both_modes {
    ($relay_only:ident, $direct:ident, $f:ident) => {
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn $relay_only() {
            begin!(stringify!($relay_only), "relay-only");
            $f(stringify!($relay_only), "relay-only", Addrs::RelayOnly).await;
        }
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn $direct() {
            begin!(stringify!($direct), "direct");
            $f(stringify!($direct), "direct", Addrs::WithDirect).await;
        }
    };
}

// ------------------------------------------------------------------------------ tests: scenarios

/// The viewer is offline while the editor publishes five versions, then returns.
async fn catch_up(test: &str, mode: &str, how: Addrs) {
    let (_owner, editor, viewer, doc) = trio(how).await;
    viewer.set_online(false).await;
    let mut parent = first_version(&editor, &doc).await;
    for i in 0..5 {
        let v = editor
            .publish_version(&doc, vec![parent.clone()], new_file(format!("edit {i} while the viewer is away").as_bytes()), None)
            .await
            .unwrap();
        parent = v.id;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(head_ids(&viewer, &doc).await, vec![first_version(&viewer, &doc).await], "an offline viewer must not see edits made while it was away");
    let t0 = Instant::now();
    viewer.set_online(true).await;
    wait_for("the viewer catches up", secs(120), || async { (head_ids(&viewer, &doc).await == vec![parent.clone()]).then_some(()) }).await;
    let took = t0.elapsed();
    let v = read_view(&*viewer, &doc).await.unwrap();
    assert!(v.rejected.is_empty());
    assert_eq!(v.heads[0].bytes, b"edit 4 while the viewer is away");
    let rep = viewer.net_report().await;
    record(test, json!({"kind": "catch_up", "mode": mode, "versions": 5, "ms": ms(took), "viewer_paths": rep.summary()}));
}
both_modes!(catch_up_relay_only, catch_up_direct, catch_up);

/// Owner and editor are both offline and each publishes a child of the same version.
async fn concurrent_heads(test: &str, mode: &str, how: Addrs) {
    let (owner, editor, viewer, doc) = trio(how).await;
    let first = first_version(&owner, &doc).await;
    owner.set_online(false).await;
    editor.set_online(false).await;
    let a = owner.publish_version(&doc, vec![first.clone()], new_file(b"owner offline edit"), None).await.unwrap();
    let b = editor.publish_version(&doc, vec![first.clone()], new_file(b"editor offline edit"), None).await.unwrap();
    let t0 = Instant::now();
    owner.set_online(true).await;
    editor.set_online(true).await;
    for n in [&owner, &editor, &viewer] {
        wait_for("two heads everywhere", secs(120), || async { (head_ids(n, &doc).await.len() == 2).then_some(()) }).await;
    }
    let two_heads = t0.elapsed();
    let mut ids = head_ids(&viewer, &doc).await;
    ids.sort();
    let mut want = vec![a.id.clone(), b.id.clone()];
    want.sort();
    assert_eq!(ids, want, "the viewer sees both heads, none hidden");
    let t1 = Instant::now();
    let merge = owner.publish_version(&doc, vec![a.id.clone(), b.id.clone()], new_file(b"merged"), None).await.unwrap();
    for n in [&owner, &editor, &viewer] {
        wait_for("one head everywhere after the merge", secs(120), || async { (head_ids(n, &doc).await == vec![merge.id.clone()]).then_some(()) }).await;
    }
    record(test, json!({"kind": "concurrent_heads", "mode": mode, "two_heads_everywhere_ms": ms(two_heads), "merge_visible_everywhere_ms": ms(t1.elapsed())}));
}
both_modes!(concurrent_heads_relay_only, concurrent_heads_direct, concurrent_heads);

/// How long a lease write takes to show at the other node: what lease rule 3's settle interval
/// has to cover.
async fn lease_latency(test: &str, mode: &str, how: Addrs) {
    let (a, b, doc) = pair(how, Role::Editor).await;
    let base = a.versions(&doc).await.unwrap().remove(0).id;
    let mut lat = Vec::new();
    for i in 0..30u64 {
        let claim = dcp_core::lease::Claim::new(a.principal(), format!("s{i}"), "t", &base, 60_000);
        let lease = claim.lease_over(None);
        let t = Instant::now();
        let w = a.write_lease(&doc, lease).await.unwrap();
        loop {
            if b.lease(&doc).await.ok().flatten().map(|l| l.id) == Some(w.id.clone()) {
                break;
            }
            assert!(t.elapsed() < secs(60), "the lease never arrived");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        lat.push(ms(t.elapsed()));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    lat.sort_unstable();
    record(
        test,
        json!({"kind": "lease_latency", "mode": mode, "writes": lat.len(), "min_ms": lat[0], "median_ms": lat[lat.len() / 2], "p95_ms": lat[lat.len() * 95 / 100], "max_ms": lat[lat.len() - 1],
               "over_500ms": lat.iter().filter(|x| **x > 500).count()}),
    );
}
both_modes!(lease_latency_relay_only, lease_latency_direct, lease_latency);

fn blob_bytes(n: usize) -> Vec<u8> {
    let mut data = vec![0u8; n];
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    for chunk in data.chunks_mut(8) {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let b = x.to_le_bytes();
        chunk.copy_from_slice(&b[..chunk.len()]);
    }
    data
}

/// Connection setup, a session over an open connection, and files.
async fn connect_and_files(test: &str, mode: &str, how: Addrs) {
    // Cold: connect plus the first session between two fresh nodes, against a second session.
    let mut cold = Vec::new();
    let mut warm = Vec::new();
    for _ in 0..3 {
        let (a, b, doc) = pair(how, Role::Viewer).await;
        b.set_online(false).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        a.set_online(false).await;
        a.set_online(true).await;
        b.set_online(true).await;
        let t = Instant::now();
        let r = b.sync_now(&doc, &a.principal).await;
        let c = ms(t.elapsed());
        if r.is_err() {
            continue;
        }
        let t = Instant::now();
        b.sync_now(&doc, &a.principal).await.unwrap();
        cold.push(c);
        warm.push(ms(t.elapsed()));
    }
    record(
        test,
        json!({"kind": "connect", "mode": mode, "runs": cold.len(), "cold_median_ms": if cold.is_empty() { 0 } else { median(&mut cold.clone()) }, "cold_all_ms": cold, "warm_median_ms": if warm.is_empty() { 0 } else { median(&mut warm.clone()) }, "warm_all_ms": warm}),
    );
    assert!(!cold.is_empty(), "no cold sync succeeded");

    // Files: 1 MiB and 8 MiB.
    for mib in [1usize, 8] {
        let (a, b, doc) = pair(how, Role::Editor).await;
        let base = a.versions(&doc).await.unwrap().remove(0).id;
        let data = blob_bytes(mib * 1024 * 1024);
        let t = Instant::now();
        let v = a.publish_version(&doc, vec![base], NewFile { mime: dcp_conformance::MIME.into(), bytes: data.clone() }, None).await.unwrap();
        let publish_ms = ms(t.elapsed());
        let done = Instant::now();
        wait_for("b lists the version", secs(900), || async { b.versions(&doc).await.ok()?.iter().any(|x| x.id == v.id).then_some(()) }).await;
        let listed_ms = ms(done.elapsed());
        let fetches = b.call("blob_fetches", json!({"doc": doc})).await.unwrap();
        let big = fetches.as_array().unwrap().iter().max_by_key(|f| f["bytes"].as_u64().unwrap_or(0)).cloned().unwrap_or(Value::Null);
        let bytes = big["bytes"].as_u64().unwrap_or(0);
        let micros = big["micros"].as_u64().unwrap_or(1).max(1);
        let got = b.fetch_file(&doc, &v).await.unwrap();
        assert_eq!(got, data, "the file arrived intact");
        record(
            test,
            json!({"kind": "file", "mode": mode, "mib": mib, "writer_publish_ms": publish_ms, "reader_lists_ms_after_publish": listed_ms, "fetch_sealed_bytes": bytes,
                   "fetch_ms": micros / 1000, "fetch_mib_per_s": (bytes as f64 / 1048576.0) / (micros as f64 / 1e6)}),
        );
    }
}
both_modes!(connect_and_files_relay_only, connect_and_files_direct, connect_and_files);

fn update(base: &str, seq: u64, size: usize) -> Update {
    Update {
        dcp: 1,
        algorithm: ALGORITHM.into(),
        doc_model: DOC_MODEL.into(),
        base_version: base.into(),
        seq,
        payload: dcp_core::updates::inline_payload(&vec![b'x'; size]).unwrap(),
    }
}

/// Item 4: the reader's cost of N 100-byte entries, split into CPU and network.
async fn state_growth(test: &str, mode: &str, how: Addrs) {
    let sizes: &[usize] = if big() { &[100, 300, 1000] } else { &[100, 300] };
    for &n in sizes {
        let (a, b, doc) = pair(how, Role::Editor).await;
        let base = a.versions(&doc).await.unwrap().remove(0).id;
        let (cpu_b0, thr_b0, _) = b.cpu().await;
        let (cpu_a0, thr_a0, _) = a.cpu().await;
        let (rx0, tx0) = b.iface_bytes();
        let (arx0, atx0) = a.iface_bytes();
        let t = Instant::now();
        for i in 0..n {
            a.publish_update(&doc, update(&base, i as u64, 100)).await.unwrap();
        }
        let publish_ms = ms(t.elapsed());
        let (cpu_b_pub, thr_b_pub, _) = b.cpu().await;
        let published = Instant::now();
        let interval = if n <= 100 { Duration::from_millis(100) } else { Duration::from_millis(500) };
        let seen = wait_soft(secs(1800), interval, || async { (b.updates(&doc).await.ok()?.len() == n).then_some(()) }).await;
        let lag_ms = ms(published.elapsed());
        let total_ms = ms(t.elapsed());
        let (cpu_b1, thr_b1, rss_b) = b.cpu().await;
        let (cpu_a1, thr_a1, _) = a.cpu().await;
        let (rx1, tx1) = b.iface_bytes();
        let (arx1, atx1) = a.iface_bytes();
        let cb = b.call("counters", json!({"doc": doc})).await.unwrap_or(Value::Null);
        let ca = a.call("counters", json!({"doc": doc})).await.unwrap_or(Value::Null);
        let state_b = b.call("space_state_bytes", json!({"doc": doc})).await.ok().and_then(|v| v.as_u64()).unwrap_or(0);
        let state_a = a.call("space_state_bytes", json!({"doc": doc})).await.ok().and_then(|v| v.as_u64()).unwrap_or(0);
        let rx = rx1 - rx0;
        let reader_thr_ms = (thr_b1 - thr_b0) / 1000;
        let tail_thr_ms = (thr_b1 - thr_b_pub) / 1000;
        let tail_cpu_ms = (cpu_b1 - cpu_b_pub) / 1000;
        record(
            test,
            json!({"kind": "state_growth", "mode": mode, "n": n, "complete": seen.is_some(),
                   "writer_publish_ms": publish_ms, "reader_complete_ms_after_last_publish": lag_ms, "total_ms": total_ms,
                   "reader_cpu_ms_total": (cpu_b1 - cpu_b0) / 1000, "reader_store_thread_cpu_ms": reader_thr_ms,
                   "reader_cpu_ms_after_last_publish": tail_cpu_ms, "reader_store_thread_cpu_ms_after_last_publish": tail_thr_ms,
                   "writer_cpu_ms_total": (cpu_a1 - cpu_a0) / 1000, "writer_store_thread_cpu_ms": (thr_a1 - thr_a0) / 1000,
                   "reader_veth_rx_bytes": rx, "reader_veth_tx_bytes": tx1 - tx0, "writer_veth_rx_bytes": arx1 - arx0, "writer_veth_tx_bytes": atx1 - atx0,
                   "reader_state_bytes": state_b, "writer_state_bytes": state_a, "reader_rss_kb": rss_b,
                   "reader_ops_received": cb["ops_received"], "reader_ops_applied": cb["ops_applied"], "reader_sync_sessions": cb["sync_sessions"], "reader_sync_failures": cb["sync_failures"],
                   "writer_ops_received": ca["ops_received"], "writer_sync_sessions": ca["sync_sessions"]}),
        );
        assert!(seen.is_some(), "the reader never received all {n} entries in {:?}", secs(1800));
    }
}
both_modes!(state_growth_relay_only, state_growth_direct, state_growth);

// ------------------------------------------------------------------------------ tests: paths

/// Item 1: with UDP between the peers dropped by the firewall, and no direct address given, iroh's
/// own path data shows relay and nothing else.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn path_relay_only_when_udp_is_blocked() {
    begin!("path_relay_only_when_udp_is_blocked", "relay-only");
    let test = "path_relay_only_when_udp_is_blocked";
    let (a, b, doc) = pair(Addrs::RelayOnly, Role::Viewer).await;
    // Traffic, and time for a hole punch to (wrongly) succeed.
    let mut parent = first_version(&a, &doc).await;
    for i in 0..5 {
        parent = a.publish_version(&doc, vec![parent], new_file(format!("relay edit {i}").as_bytes()), None).await.unwrap().id;
        tokio::time::sleep(secs(2)).await;
    }
    wait_for("b has the head", secs(120), || async { (head_ids(&b, &doc).await == vec![parent.clone()]).then_some(()) }).await;
    let (ra, rb) = reports(&a, &b).await;
    dump("path_relay_only-a", &ra.0);
    dump("path_relay_only-b", &rb.0);
    let verdict = relay_only(&ra, &rb, &a.principal, &b.principal);
    let fw = script(&["counters"]);
    record(
        test,
        json!({"kind": "path_proof", "case": "udp blocked, addresses withheld", "relay_only_verdict": format!("{verdict:?}"), "a": ra.summary(), "b": rb.summary(),
               "a_selected": ra.selected_now(&b.principal), "b_selected": rb.selected_now(&a.principal), "firewall_forward_counters": fw}),
    );
    verdict.expect("relay-only: iroh's path data must show only the relay");
}

/// The negative control of the assertion above, and the upgrade time: with UDP allowed and the
/// same withheld addresses, the "relay-only" assertion must FAIL (the connection upgrades).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn path_control_udp_allowed_upgrades_and_the_relay_only_assertion_fails() {
    let test = "path_control_udp_allowed_upgrades";
    begin!(test, "direct");
    let reps: usize = std::env::var("DCP_WAN_UPGRADE_REPS").ok().and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut upgrade_ms = Vec::new();
    let mut dial_ms = Vec::new();
    let mut failures = 0;
    for rep in 0..reps {
        let t_start = Instant::now();
        let (a, b, _doc) = pair(Addrs::RelayOnly, Role::Viewer).await;
        let first_sync_ms = ms(t_start.elapsed());
        let up = wait_direct(&a, &b, secs(90)).await;
        let (ra, rb) = reports(&a, &b).await;
        if rep == 0 {
            dump("path_control-a", &ra.0);
            dump("path_control-b", &rb.0);
        }
        let control = relay_only(&ra, &rb, &a.principal, &b.principal);
        // The first connection of the pair (earliest dial start over both nodes): when did iroh select
        // an IP path on it, from iroh's own event timestamps taken in the node process?
        let mut first: Option<(u64, u64, u64, &Rep)> = None;
        for (r, peer) in [(&ra, &b.principal), (&rb, &a.principal)] {
            for (id, start, connected) in r.dials(peer) {
                if first.map_or(true, |f| start < f.1) {
                    first = Some((id, start, connected, r));
                }
            }
        }
        let (mut after_handshake, mut after_dial, mut ip_open_at_handshake) = (None, None, None);
        if let Some((id, start, connected, r)) = first {
            ip_open_at_handshake = Some(r.conn_ip_at_handshake(id));
            if let Some(sel) = r.conn_first_selected_ip_us(id) {
                after_handshake = Some(sel.saturating_sub(connected) / 1000);
                after_dial = Some(sel.saturating_sub(start) / 1000);
            }
        }
        record(
            test,
            json!({"kind": "upgrade", "rep": rep, "upgraded": up.is_some(), "poll_ms_from_first_sync": up.map(ms), "first_sync_ms_after_spawn": first_sync_ms,
                   "ip_selected_ms_after_handshake": after_handshake, "ip_selected_ms_after_dial_start": after_dial, "ip_path_open_when_the_handshake_finished": ip_open_at_handshake,
                   "relay_only_assertion_on_this_run": format!("{control:?}"), "a": ra.summary(), "b": rb.summary()}),
        );
        match up {
            Some(_) => {
                // The control: on this run the relay-only assertion must have failed.
                assert!(control.is_err(), "negative control: the relay-only assertion passed on a run that upgraded to a direct path");
                direct(&ra, &rb, &a.principal, &b.principal).expect("direct");
                if let Some(x) = after_handshake {
                    upgrade_ms.push(x);
                }
                if let Some(x) = after_dial {
                    dial_ms.push(x);
                }
            }
            None => failures += 1,
        }
    }
    record(
        test,
        json!({"kind": "upgrade_summary", "reps": reps, "not_upgraded": failures, "ip_selected_after_handshake_ms": upgrade_ms,
               "median_after_handshake_ms": if upgrade_ms.is_empty() { Value::Null } else { json!(median(&mut upgrade_ms.clone())) },
               "median_after_dial_ms": if dial_ms.is_empty() { Value::Null } else { json!(median(&mut dial_ms.clone())) }}),
    );
    assert_eq!(failures, 0, "{failures} of {reps} runs never selected a direct path within {:?}", secs(90));
}

/// With the peer's IP address given, the first connection is direct.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn path_direct_when_addresses_are_provided() {
    let test = "path_direct_when_addresses_are_provided";
    begin!(test, "direct");
    let (a, b, doc) = pair(Addrs::WithDirect, Role::Viewer).await;
    let _ = doc;
    let up = wait_direct(&a, &b, secs(60)).await;
    let (ra, rb) = reports(&a, &b).await;
    dump("path_provided-a", &ra.0);
    dump("path_provided-b", &rb.0);
    let verdict = direct(&ra, &rb, &a.principal, &b.principal);
    let ctrl = relay_only(&ra, &rb, &a.principal, &b.principal);
    record(test, json!({"kind": "path_proof", "case": "udp allowed, addresses provided", "direct_verdict": format!("{verdict:?}"), "relay_only_assertion": format!("{ctrl:?}"), "ms_until_direct": up.map(ms), "a": ra.summary(), "b": rb.summary()}));
    verdict.expect("direct");
    assert!(ctrl.is_err(), "negative control: the relay-only assertion passed on a direct connection");
}

/// Publish at `a`, wait for `b` to list it. Returns the time it took, or `None`.
async fn push_one(a: &Arc<Node>, b: &Arc<Node>, doc: &DocId, text: &str, max: Duration) -> Option<Duration> {
    let head = head_ids(a, doc).await;
    let parent = head.first().cloned().unwrap_or_default();
    let v = a.publish_version(doc, vec![parent], new_file(text.as_bytes()), None).await.ok()?;
    let t = Instant::now();
    wait_soft(max, Duration::from_millis(100), || async { b.versions(doc).await.ok()?.iter().any(|x| x.id == v.id).then_some(()) }).await?;
    Some(t.elapsed())
}

/// Item 3: kill the relay while the connection is direct. Does it survive, and does a connection
/// made after the relay is gone work?
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_killed_while_the_connection_is_direct() {
    let test = "relay_killed_while_direct";
    begin!(test, "direct");
    let (a, b, doc) = pair(Addrs::RelayOnly, Role::Editor).await;
    wait_direct(&a, &b, secs(90)).await.expect("the connection must be direct before the relay is killed");
    let (ra0, _) = reports(&a, &b).await;
    let conn_id = ra0.arr("conns").into_iter().filter(|c| c["alpn"] == SYNC_ALPN && c["peer"] == b.principal.as_str()).filter_map(|c| c["id"].as_u64()).next();
    script(&["relay-down"]);
    let t_kill = Instant::now();
    // Publish both ways, repeatedly, for a while: the established direct connection has to carry it.
    let mut results = Vec::new();
    for i in 0..6 {
        let (from, to) = if i % 2 == 0 { (&a, &b) } else { (&b, &a) };
        let took = push_one(from, to, &doc, &format!("no relay {i}"), secs(30)).await;
        results.push(took.map(ms));
        tokio::time::sleep(secs(3)).await;
    }
    let (ra1, rb1) = reports(&a, &b).await;
    dump("relay_killed_direct-a", &ra1.0);
    let survived = conn_id.map(|id| !ra1.conn_closed(id)).unwrap_or(false);
    let still_direct = direct(&ra1, &rb1, &a.principal, &b.principal);
    // A connection made after the relay is gone: b goes offline and back (it dials again).
    b.set_online(false).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let miss = push_one(&a, &b, &doc, "published while b is offline", secs(5)).await;
    b.set_online(true).await;
    let t_redial = Instant::now();
    let redial = wait_soft(secs(60), Duration::from_millis(100), || async {
        let head = head_ids(&a, &doc).await;
        (head.len() == 1 && head_ids(&b, &doc).await == head).then_some(())
    })
    .await
    .map(|_| t_redial.elapsed());
    record(
        test,
        json!({"kind": "relay_killed", "case": "direct connection", "conn_id": conn_id, "sync_connection_survived": survived,
               "still_direct_verdict": format!("{still_direct:?}"), "visible_ms_each_publish_after_kill": results,
               "seconds_since_kill_at_end_of_series": t_kill.elapsed().as_secs(),
               "offline_b_saw_the_publish_while_offline": miss.is_some(),
               "reconnect_after_relay_killed_new_connection_ms": redial.map(ms),
               "relay_conns_failed_a": ra1.metric("relay_conns_failed"), "relay_conns_closed_a": ra1.metric("relay_conns_closed")}),
    );
    script(&["relay-up"]);
    assert!(survived, "the direct connection was closed when the relay went away");
    assert!(results.iter().all(Option::is_some), "a publish did not arrive over the surviving direct connection: {results:?}");
}

/// Item 3: kill the relay while the peers can only talk through it, then bring it back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_killed_while_the_connection_is_relay_only() {
    let test = "relay_killed_while_relay_only";
    begin!(test, "relay-only");
    let (a, b, doc) = pair(Addrs::RelayOnly, Role::Editor).await;
    assert!(push_one(&a, &b, &doc, "before the kill", secs(60)).await.is_some(), "no sync before the kill");
    let (ra0, _) = reports(&a, &b).await;
    let ids0: Vec<u64> = ra0.arr("conns").into_iter().filter(|c| c["alpn"] == SYNC_ALPN).filter_map(|c| c["id"].as_u64()).collect();
    script(&["relay-down"]);
    let t_kill = Instant::now();
    // Publish at a: it must NOT reach b while the relay is gone (this is what proves the dependency).
    let head = head_ids(&a, &doc).await;
    let v = a.publish_version(&doc, head.clone(), new_file(b"published while the relay is dead"), None).await.unwrap();
    let during = wait_soft(secs(20), Duration::from_millis(250), || async { b.versions(&doc).await.ok()?.iter().any(|x| x.id == v.id).then_some(()) }).await;
    let (ra_mid, _) = reports(&a, &b).await;
    let closed_during = ids0.iter().filter(|id| ra_mid.conn_closed(**id)).count();
    let dead_s = t_kill.elapsed().as_secs();
    script(&["relay-up"]);
    let t_back = Instant::now();
    let recovered = wait_soft(secs(300), Duration::from_millis(250), || async { b.versions(&doc).await.ok()?.iter().any(|x| x.id == v.id).then_some(()) }).await.map(|_| t_back.elapsed());
    let (ra1, rb1) = reports(&a, &b).await;
    dump("relay_killed_relay_only-a", &ra1.0);
    let ids1: Vec<u64> = ra1.arr("conns").into_iter().filter(|c| c["alpn"] == SYNC_ALPN).filter_map(|c| c["id"].as_u64()).collect();
    let survivors: Vec<u64> = ids0.iter().copied().filter(|id| !ra1.conn_closed(*id)).collect();
    record(
        test,
        json!({"kind": "relay_killed", "case": "relay-only connection", "visible_at_b_while_relay_dead": during.is_some(), "sync_connections_closed_while_dead": closed_during,
               "seconds_dead_before_restart": dead_s,
               "recovered_after_relay_returned_ms": recovered.map(ms), "same_connection_survived": !survivors.is_empty(), "sync_conn_ids_before": ids0, "sync_conn_ids_after": ids1,
               "relay_conns_success_a": ra1.metric("relay_conns_success"), "relay_conns_failed_a": ra1.metric("relay_conns_failed"), "relay_conns_closed_a": ra1.metric("relay_conns_closed"),
               "b_relay_conns_failed": rb1.metric("relay_conns_failed")}),
    );
    assert!(during.is_none(), "negative control: a publish crossed a dead relay with UDP blocked, so the connection was not relay-only");
    assert!(recovered.is_some(), "the relay-only connection did not recover within {:?} of the relay returning", secs(300));
}

/// Item 3 (extra): the direct path is cut by the firewall mid-session; the connection falls back
/// to the relay, and re-upgrades when UDP is allowed again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_path_blocked_mid_session_falls_back_to_the_relay() {
    let test = "direct_path_blocked_midsession";
    begin!(test, "direct");
    let (a, b, doc) = pair(Addrs::RelayOnly, Role::Editor).await;
    wait_direct(&a, &b, secs(90)).await.expect("direct before the block");
    script(&["mode", "relay-only"]);
    let t_block = Instant::now();
    let t_block_us = dcp_p2p::pathlog::now_us();
    let first = push_one(&a, &b, &doc, "udp blocked", secs(120)).await;
    let first_ms = first.map(ms);
    // When did a select the relay path again?
    let fallback = wait_soft(secs(120), Duration::from_millis(250), || async {
        let ra = a.net_report().await;
        ra.events_after(&b.principal, t_block_us, "selected", "relay").map(|t| (t - t_block_us) / 1000)
    })
    .await;
    // Data keeps flowing through the relay.
    let second = push_one(&b, &a, &doc, "still blocked", secs(120)).await.map(ms);
    script(&["mode", "direct"]);
    let t_open_us = dcp_p2p::pathlog::now_us();
    let t_open = Instant::now();
    let reup = wait_soft(secs(180), Duration::from_millis(250), || async {
        let ra = a.net_report().await;
        ra.events_after(&b.principal, t_open_us, "selected", "ip").map(|t| (t - t_open_us) / 1000)
    })
    .await;
    let third = push_one(&a, &b, &doc, "unblocked", secs(120)).await.map(ms);
    let (ra, _) = reports(&a, &b).await;
    dump("direct_blocked-a", &ra.0);
    record(
        test,
        json!({"kind": "direct_blocked", "first_publish_visible_ms_after_block": first_ms, "relay_selected_ms_after_block": fallback, "second_publish_ms": second,
               "ip_reselected_ms_after_unblock": reup, "third_publish_ms": third, "secs_blocked_total": t_open.elapsed().as_secs().saturating_sub(0), "since_block_s": t_block.elapsed().as_secs()}),
    );
    assert!(first.is_some(), "no publish crossed after the direct path was blocked");
    assert!(second.is_some(), "the relay did not keep carrying the session");
}

// ------------------------------------------------------------------------------ tests: ports

/// Restores `egress open` when dropped, so a failing test does not leave the namespaces closed.
struct EgressGuard;
impl Drop for EgressGuard {
    fn drop(&mut self) {
        let _ = Command::new(&env().script).args(["egress", "open"]).output();
    }
}

/// Item 5: what a client needs outbound. With the peers' namespaces allowed only TCP 443 to the
/// relay (no UDP at all, no TCP 80, no UDP 7842), the node still reaches the relay and the pair
/// syncs. The firewall counters say what iroh tried and was refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ports_tcp_443_to_the_relay_is_enough() {
    let test = "ports_tcp_443_to_the_relay_is_enough";
    begin!(test, "relay-only");
    let _guard = EgressGuard;
    script(&["egress", "tcp443-only"]);
    let t = Instant::now();
    let (na, nb) = tokio::join!(Node::try_spawn(0, "a"), Node::try_spawn(1, "b"));
    let (a, b) = match (na, nb) {
        (Ok(a), Ok(b)) => (a, b),
        (a, b) => {
            let why = format!("{:?} / {:?}", a.err(), b.err());
            let counters = script(&["egress-counters"]);
            record(test, json!({"kind": "ports", "case": "tcp 443 to the relay only", "reached_relay": false, "why": why.chars().take(1500).collect::<String>(), "egress_counters": counters}));
            panic!("a node could not come online with only TCP 443 to the relay allowed: {why}");
        }
    };
    let ready = (a.ready_ms, b.ready_ms);
    mesh(&[&a, &b], Addrs::RelayOnly).await;
    let doc = a.create_document(&standard_meta(), Some(new_file(b"tcp 443 only"))).await.expect("create_document");
    invite(&a, &b, &doc).await;
    a.grant(&doc, &b.principal(), Role::Editor).await.expect("grant");
    wait_for("b has its role and the first version", secs(120), || async {
        if b.role(&doc).await.ok()? != Role::Editor {
            return None;
        }
        (b.versions(&doc).await.ok()?.len() == 1).then_some(())
    })
    .await;
    let took = push_one(&a, &b, &doc, "over tcp 443 only", secs(60)).await;
    let (ra, rb) = reports(&a, &b).await;
    let verdict = relay_only(&ra, &rb, &a.principal, &b.principal);
    let counters = script(&["egress-counters"]);
    record(
        test,
        json!({"kind": "ports", "case": "tcp 443 to the relay only", "reached_relay": true, "ready_ms_a_b": [ready.0, ready.1], "setup_to_first_sync_ms": ms(t.elapsed()),
               "publish_visible_ms": took.map(ms), "relay_only_verdict": format!("{verdict:?}"), "a": ra.summary(), "egress_counters": counters}),
    );
    assert!(took.is_some(), "a publish did not cross with only TCP 443 open");
    verdict.expect("relay-only");
}

/// The negative control of the test above: with nothing allowed out, a node cannot come online.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ports_control_nothing_allowed_out_means_no_relay() {
    let test = "ports_control_nothing_allowed_out";
    begin!(test, "relay-only");
    let _guard = EgressGuard;
    script(&["egress", "none"]);
    let r = Node::try_spawn(0, "a").await;
    let counters = script(&["egress-counters"]);
    record(test, json!({"kind": "ports", "case": "nothing allowed out", "node_came_online": r.is_ok(), "egress_counters": counters}));
    assert!(r.is_err(), "negative control: a node came online with every outbound packet dropped, so the TCP-443-only result proves nothing");
}
