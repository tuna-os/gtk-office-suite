//! Shared plumbing for the Nextcloud tests: where the instances are, who the
//! users are, and the rule that a missing instance FAILS the test unless
//! `DCP_SKIP_NEXTCLOUD=1` says to skip (and then the test says so).
#![allow(dead_code)]

use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dcp_conformance::wrappers::{Defect, Defective};
use dcp_conformance::{
    new_file, standard_meta, Capabilities, Failure, Faults, Fixture, Forger, RealTime, StoreFactory, BASE_FILE,
};
use dcp_core::{DocId, DocumentStore, Role, Version};
use dcp_webdav::ocm::accept_pending_remote_shares;
use dcp_webdav::{Dav, ServerClock, WebdavConfig, WebdavStore};

pub const PASSWORD: &str = "dcp-user-Passw0rd-spike";

pub fn url_a() -> String {
    std::env::var("DCP_NEXTCLOUD_A_URL").unwrap_or_else(|_| "http://nc-a".into())
}

pub fn url_b() -> String {
    std::env::var("DCP_NEXTCLOUD_B_URL").unwrap_or_else(|_| "http://nc-b".into())
}

pub fn password() -> String {
    std::env::var("DCP_NC_USER_PASSWORD").unwrap_or_else(|_| PASSWORD.into())
}

static TOKENS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
    std::sync::OnceLock::new();

fn tokens() -> &'static std::sync::Mutex<std::collections::HashMap<String, String>> {
    TOKENS.get_or_init(Default::default)
}

/// Every request with the account password makes Nextcloud verify it with a slow
/// password hash, about 200 ms. An app password is checked against a token and is
/// the way a client is meant to authenticate; the tests use one per user.
/// `DCP_NC_NO_APP_PASSWORDS=1` keeps the account password (the latency test
/// measures both).
pub async fn prime_app_passwords(base: &str) {
    if std::env::var("DCP_NC_NO_APP_PASSWORDS").as_deref() == Ok("1") {
        return;
    }
    for user in ["alice", "bob", "carol", "dave"] {
        let key = format!("{base}|{user}");
        if tokens().lock().unwrap().contains_key(&key) {
            continue;
        }
        let d = Dav::new(base, user, &password());
        match d.ocs("GET", "core/getapppassword", &[]).await {
            Ok(v) => {
                if let Some(t) = v["apppassword"].as_str() {
                    tokens().lock().unwrap().insert(key, t.to_owned());
                }
            }
            Err(e) => println!("no app password for {user}@{base}: {e}"),
        }
    }
}

pub fn password_for(base: &str, user: &str) -> String {
    tokens()
        .lock()
        .unwrap()
        .get(&format!("{base}|{user}"))
        .cloned()
        .unwrap_or_else(password)
}

/// `Some(())` if the test should run. With `DCP_SKIP_NEXTCLOUD=1` it prints
/// `SKIPPED <name>` and returns `None`. Otherwise it panics if `base` does not
/// answer `status.php` with `"installed":true`.
pub async fn require(name: &str, bases: &[String]) -> Option<()> {
    if std::env::var("DCP_SKIP_NEXTCLOUD").as_deref() == Ok("1") {
        println!("SKIPPED {name}: DCP_SKIP_NEXTCLOUD=1");
        return None;
    }
    for base in bases {
        let dav = Dav::new(base, "nobody", "nobody");
        let ok = match dav.request("GET", "/status.php", &[], None).await {
            Ok(r) => r.ok() && r.text().contains("\"installed\":true"),
            Err(_) => false,
        };
        assert!(
            ok,
            "Nextcloud is not reachable at {base} (start it with scripts/nextcloud-up.sh, or set DCP_SKIP_NEXTCLOUD=1 to skip {name} explicitly)"
        );
    }
    for base in bases {
        prime_app_passwords(base).await;
    }
    Some(())
}

pub fn dav(base: &str, user: &str) -> Dav {
    Dav::new(base, user, &password_for(base, user))
}

pub fn config(base: &str, user: &str) -> WebdavConfig {
    WebdavConfig {
        base_url: base.to_owned(),
        user: user.to_owned(),
        password: password_for(base, user),
        device: format!("{user}-device"),
        remote_scheme: "http".into(),
    }
}

/// A calibrated clock for `base`, shared by the handles of one test.
pub async fn clock(base: &str) -> Arc<ServerClock> {
    ServerClock::calibrate(&dav(base, "alice")).await.expect("clock calibration")
}

pub fn store(base: &str, user: &str, clock: &Arc<ServerClock>, fault: &Arc<AtomicBool>) -> WebdavStore {
    WebdavStore::with_parts(&config(base, user), clock.clone(), fault.clone())
}

pub fn unique(prefix: &str) -> String {
    let mut b = [0u8; 5];
    getrandom_fill(&mut b);
    format!("{prefix}-{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>())
}

fn getrandom_fill(b: &mut [u8]) {
    // /dev/urandom: the test crate does not depend on getrandom directly.
    use std::io::Read;
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(b)).expect("urandom");
}

/// Append one JSON measurement line to `$DCP_MEASURE_OUT` (if set) and print it.
pub fn measure(kind: &str, value: serde_json::Value) {
    let line = serde_json::json!({ "measure": kind, "value": value }).to_string();
    println!("MEASURE {line}");
    if let Ok(path) = std::env::var("DCP_MEASURE_OUT") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

pub fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1000.0 * 10.0).round() / 10.0
}

/// median and p95 of samples (milliseconds).
pub fn stats(mut v: Vec<f64>) -> serde_json::Value {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    let p = |q: f64| v[((n as f64 - 1.0) * q).round() as usize];
    serde_json::json!({ "n": n, "min": v[0], "median": p(0.5), "p95": p(0.95), "max": v[n - 1] })
}

// ------------------------------------------------------ conformance fixtures

pub struct NcFaults(pub Arc<AtomicBool>);

impl Faults for NcFaults {
    fn truncate_next_upload(&self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// The viewer's client writing straight into the document folder.
pub struct NcForger {
    pub viewer: Arc<WebdavStore>,
}

#[async_trait]
impl Forger for NcForger {
    async fn forge_version(&self, doc: &DocId, version: Version) -> Result<Option<String>, Failure> {
        let name = unique("forged");
        Ok(self.viewer.put_raw_version(doc, &name, &version).await?)
    }
}

pub fn capabilities() -> Capabilities {
    Capabilities {
        grant_revoke: true,
        // Revocation is access control on a server that holds the plaintext:
        // content published after it is unreadable to the revoked user, nothing
        // is encrypted.
        revocation: true,
        // The lease log takes any record.
        arbitrary_lease_writes: true,
        // Writes made one after the other get a compare-and-set: a taken name is
        // refused (412/423), which is all the journeys write. Under real concurrency
        // creating a name is not atomic on Nextcloud (a late creator can replace the
        // record: `lease_log_create_race_leaves_one_holder_after_the_read_backs`), so
        // `false` here does not license skipping lease rule 3's read-backs.
        racy_lease: false,
        history: true,
        arbitrary_parents: true,
        // No ordered update stream in WebDAV (L2).
        updates: false,
        upload_faults: true,
        forgery: true,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Topology {
    /// alice, bob and carol on `nc-a`.
    Single,
    /// alice on `nc-a`; bob and carol on `nc-b`, reaching the document only
    /// through Open Cloud Mesh shares they accepted there.
    Federated,
}

/// The lease duration `lease_expiry_releases` claims over Nextcloud, one instance or a
/// federation, in milliseconds (`Fixture::lease_ms`; the journey's default is 10 s).
///
/// Why this long: after the renewal the journey lets `3/4 * D` pass and the lease must
/// still be alive when it is read again, so the renewal's read-backs, the 500 ms settle
/// interval of lease rule 3 and the final acquire's reads all have to fit in `D / 4`
/// less 500 ms: 500 ms at 4 s, 4.5 s at 20 s. `docs/rfc/0004-spike-e-results.md` measured
/// a Nextcloud request at 50 to 120 ms on one server and found the 4 s margin to be about
/// 0.26 s by arithmetic, so a slow runner spends it: one instance on 35.0.2 failed
/// ("renewal did not keep the lease alive") in CI run 40180662063 and passed on 34.0.5 and
/// in the runs before. A federation (two server hops per operation) failed at 4 s in 7 of
/// 8 runs. 20 s is the duration `tests/lease_timing.rs` measured to pass on both
/// topologies and both builds in every run, and its margin is nine times that of 4 s.
/// The journey is the same, only its clock is wider; no assertion is weakened.
pub const NEXTCLOUD_LEASE_MS: u64 = 20_000;

pub struct NcFactory {
    pub topology: Topology,
    pub clock_a: Arc<ServerClock>,
    pub clock_b: Option<Arc<ServerClock>>,
    pub hide_newest_from_viewer: bool,
}

impl NcFactory {
    pub async fn single(hide_newest_from_viewer: bool) -> NcFactory {
        NcFactory {
            topology: Topology::Single,
            clock_a: clock(&url_a()).await,
            clock_b: None,
            hide_newest_from_viewer,
        }
    }

    pub async fn federated() -> NcFactory {
        NcFactory {
            topology: Topology::Federated,
            clock_a: clock(&url_a()).await,
            clock_b: Some(clock(&url_b()).await),
            hide_newest_from_viewer: false,
        }
    }
}

#[async_trait]
impl StoreFactory for NcFactory {
    fn name(&self) -> String {
        match (self.topology, self.hide_newest_from_viewer) {
            (_, true) => "nextcloud with a defective viewer".into(),
            (Topology::Single, _) => format!("nextcloud ({})", url_a()),
            (Topology::Federated, _) => {
                "nextcloud over OCM (alice@nc-a owner; bob@nc-b editor, carol@nc-b viewer)".into()
            }
        }
    }

    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    async fn fresh(&self) -> Result<Fixture, Failure> {
        let fault = Arc::new(AtomicBool::new(false));
        let owner = Arc::new(store(&url_a(), "alice", &self.clock_a, &fault));
        let (editor, viewer) = match self.topology {
            Topology::Single => (
                Arc::new(store(&url_a(), "bob", &self.clock_a, &fault)),
                Arc::new(store(&url_a(), "carol", &self.clock_a, &fault)),
            ),
            Topology::Federated => {
                let cb = self.clock_b.as_ref().expect("a federation has a second clock");
                (
                    Arc::new(store(&url_b(), "bob", cb, &fault)),
                    Arc::new(store(&url_b(), "carol", cb, &fault)),
                )
            }
        };
        let viewer_dyn: Arc<dyn DocumentStore> = if self.hide_newest_from_viewer {
            Arc::new(Defective::new(viewer.clone(), Defect::HideNewest))
        } else {
            viewer.clone()
        };
        let faults = Some(Arc::new(NcFaults(fault)) as Arc<dyn Faults>);
        let mut fx = match self.topology {
            Topology::Single => {
                Fixture::standard(
                    owner,
                    editor,
                    viewer_dyn,
                    Arc::new(RealTime),
                    capabilities(),
                    faults,
                    // Nextcloud is one database: what one handle wrote the next reads.
                    Duration::ZERO,
                )
                .await?
            }
            Topology::Federated => {
                let doc = owner.create_document(&standard_meta(), Some(new_file(BASE_FILE))).await?;
                owner.grant(&doc, &editor.principal(), Role::Editor).await?;
                owner.grant(&doc, &viewer.principal(), Role::Viewer).await?;
                // A remote share waits on the receiving server until its user accepts it.
                for (who, dav) in [("bob", editor.dav()), ("carol", viewer.dav())] {
                    let n = accept_pending_remote_shares(dav, &doc).await?;
                    if n != 2 {
                        return Err(Failure::new(format!("{who} had {n} pending shares of {doc}, expected 2")));
                    }
                }
                Fixture::from_existing(
                    owner,
                    editor,
                    viewer_dyn,
                    doc,
                    Arc::new(RealTime),
                    capabilities(),
                    faults,
                    // Two servers and a share between them: allow some time.
                    Duration::from_secs(5),
                )
                .await?
            }
        };
        // Spec test 5 needs the renewal, its read-backs and two more reads to fit inside the
        // renewed lease; see NEXTCLOUD_LEASE_MS for why 4 s does not, on either topology.
        fx.lease_ms = NEXTCLOUD_LEASE_MS;
        fx.forger = Some(Arc::new(NcForger { viewer }));
        Ok(fx)
    }
}
