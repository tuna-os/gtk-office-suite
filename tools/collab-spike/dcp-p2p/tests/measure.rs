//! Measurements for docs/rfc/0004-spike-c-results.md. Run in release mode, one test at a time:
//! `cargo test --release --test measure -- --ignored --test-threads=1 --nocapture`.
//! Every figure is printed as a `MEASURE` line.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use dcp_conformance::{new_file, standard_meta, ALGORITHM, DOC_MODEL};
use dcp_core::{DocumentStore, Role, Update};
use dcp_p2p::P2pStore;

fn median(v: &mut Vec<u128>) -> u128 {
    v.sort_unstable();
    v[v.len() / 2]
}

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

async fn pair() -> (Arc<P2pStore>, Arc<P2pStore>, String) {
    let a = node(cfg()).await;
    let b = node(cfg()).await;
    mesh(&[&a, &b]).await;
    let doc = a.create_document(&standard_meta(), Some(new_file(b"measure"))).await.unwrap();
    invite(&a, &b, &doc).await;
    a.grant(&doc, &b.principal(), Role::Editor).await.unwrap();
    wait_for("b is an editor", Duration::from_secs(30), || async {
        (b.role(&doc).await.ok()? == Role::Editor).then_some(())
    })
    .await;
    (a, b, doc)
}

/// QUIC connect plus the first sync session between two fresh nodes on loopback, against a second
/// session over the connection that is already open.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn connect_time_on_loopback() {
    let mut cold = Vec::new();
    let mut warm = Vec::new();
    for _ in 0..7 {
        let a = node(cfg()).await;
        let b = node(cfg()).await;
        a.add_peer(b.addr()).await.unwrap();
        b.add_peer(a.addr()).await.unwrap();
        let doc = a.create_document(&standard_meta(), Some(new_file(b"x"))).await.unwrap();
        invite(&a, &b, &doc).await;
        a.grant(&doc, &b.principal(), Role::Viewer).await.unwrap();
        // Let the background loops settle so only the timed call is a first contact.
        b.set_online(false).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        a.set_online(false).await.unwrap();
        a.set_online(true).await.unwrap();
        b.set_online(true).await.unwrap();
        let t = Instant::now();
        let r = b.sync_now(&doc, &a.principal()).await;
        let c = t.elapsed().as_micros();
        if r.is_err() {
            continue;
        }
        let t = Instant::now();
        b.sync_now(&doc, &a.principal()).await.unwrap();
        cold.push(c);
        warm.push(t.elapsed().as_micros());
    }
    eprintln!("MEASURE connect+first sync session (loopback, no relay): runs {} median {} us min {} us max {} us", cold.len(), median(&mut cold.clone()), cold.iter().min().unwrap(), cold.iter().max().unwrap());
    eprintln!("MEASURE sync session over an open connection: median {} us min {} us max {} us", median(&mut warm.clone()), warm.iter().min().unwrap(), warm.iter().max().unwrap());
}

/// N small entries (inline updates of 100 bytes): how long they take to publish at the writer, to
/// reach a second node, and what each costs on the wire.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn convergence_of_n_entries() {
    for n in [10usize, 100, 300, 1000] {
        let (a, b, doc) = pair().await;
        let base = a.versions(&doc).await.unwrap().remove(0).id;
        let t = Instant::now();
        for i in 0..n {
            a.publish_update(&doc, update(&base, i as u64, 100)).await.unwrap();
        }
        let publish_ms = t.elapsed().as_millis();
        let published = Instant::now();
        let seen = wait_soft(Duration::from_secs(600), || async {
            (b.updates(&doc).await.ok()?.len() == n).then_some(())
        })
        .await;
        let lag = published.elapsed().as_millis();
        let headers = a.raw_op_headers(&doc).await.unwrap();
        let bytes: usize = headers.iter().map(|h| h.len()).sum();
        eprintln!(
            "MEASURE n={n}: persisted p2panda-spaces state {} bytes at the writer, {} bytes at the reader",
            a.space_state_bytes(&doc).await.unwrap(),
            b.space_state_bytes(&doc).await.unwrap()
        );
        eprintln!(
            "MEASURE n={n}: publish {publish_ms} ms ({} ms per entry at the writer), reader complete {} after {lag} ms past the last publish; {} operations stored, {} header bytes in total = {} bytes per operation (payload 100 bytes)",
            publish_ms as f64 / n as f64,
            if seen.is_some() { "yes" } else { "NO (timeout)" },
            headers.len(),
            bytes,
            bytes / headers.len().max(1)
        );
    }
}

/// A third node joins late and pulls everything in one session: wire bytes per operation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn late_joiner_pulls_n_entries_in_one_session() {
    for n in [100usize, 300] {
        let (a, b, doc) = pair().await;
        let base = a.versions(&doc).await.unwrap().remove(0).id;
        for i in 0..n {
            a.publish_update(&doc, update(&base, i as u64, 100)).await.unwrap();
        }
        wait_for("b has everything", Duration::from_secs(600), || async {
            (b.updates(&doc).await.ok()?.len() == n).then_some(())
        })
        .await;
        let c = node(cfg()).await;
        mesh(&[&a, &b, &c]).await;
        invite(&a, &c, &doc).await;
        // Take b offline so the pull is a single, clean session with a.
        b.set_online(false).await.unwrap();
        a.grant(&doc, &c.principal(), Role::Viewer).await.unwrap();
        // c learns the membership first (a short session), then the timed full pull is whatever remains.
        let t = Instant::now();
        let first = c.sync_now(&doc, &a.principal()).await;
        let first_ms = t.elapsed().as_millis();
        match first {
            Ok(s) => eprintln!(
                "MEASURE late joiner n={n}: first session {first_ms} ms moved {} operations ({} bytes) in, {} out",
                s.received_operations, s.received_bytes, s.sent_operations
            ),
            Err(e) => eprintln!("MEASURE late joiner n={n}: first session failed: {e}"),
        }
        let t2 = Instant::now();
        wait_for("c has all entries", Duration::from_secs(600), || async {
            (c.updates(&doc).await.ok()?.len() == n).then_some(())
        })
        .await;
        eprintln!("MEASURE late joiner n={n}: all {n} entries readable {} ms after the first session ended", t2.elapsed().as_millis());
    }
}

/// Files: the writer's time to seal, store and verify, and the reader's fetch (iroh-blobs).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn blob_transfer_1_and_32_mib() {
    for mib in [1usize, 32] {
        let (a, b, doc) = pair().await;
        let base = a.versions(&doc).await.unwrap().remove(0).id;
        let mut data = vec![0u8; mib * 1024 * 1024];
        // Not compressible, not constant: a cheap deterministic generator.
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        for chunk in data.chunks_mut(8) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let b = x.to_le_bytes();
            chunk.copy_from_slice(&b[..chunk.len()]);
        }
        let t = Instant::now();
        let v = a.publish_version(&doc, vec![base], dcp_core::NewFile { mime: dcp_conformance::MIME.into(), bytes: data.clone() }, None).await.unwrap();
        let publish_ms = t.elapsed().as_millis();
        let done = Instant::now();
        wait_for("b lists the version", Duration::from_secs(600), || async {
            b.versions(&doc).await.ok()?.iter().any(|x| x.id == v.id).then_some(())
        })
        .await;
        let listed_ms = done.elapsed().as_millis();
        let fetches = b.blob_fetches(&doc).await.unwrap();
        let big = fetches.iter().max_by_key(|f| f.bytes).unwrap();
        let secs = big.micros as f64 / 1e6;
        let t = Instant::now();
        let bytes = b.fetch_file(&doc, &v).await.unwrap();
        let read_ms = t.elapsed().as_millis();
        assert_eq!(bytes, data);
        eprintln!(
            "MEASURE {mib} MiB file: writer publish (seal+store+read back+entry) {publish_ms} ms; reader listed it {listed_ms} ms after the publish returned; iroh-blobs fetch of {} sealed bytes {:.0} ms = {:.1} MiB/s; reader decrypt+read {read_ms} ms",
            big.bytes,
            secs * 1000.0,
            (big.bytes as f64 / 1048576.0) / secs
        );
    }
}

/// How long a lease write takes to show at another node: what the settle interval of lease rule 3
/// has to cover on this binding (the anti-entropy interval is 200 ms; a write triggers a push).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measurement"]
async fn lease_propagation_latency() {
    let (a, b, doc) = pair().await;
    let base = a.versions(&doc).await.unwrap().remove(0).id;
    let mut lat = Vec::new();
    for i in 0..60u64 {
        let claim = dcp_core::lease::Claim::new(a.principal(), format!("s{i}"), "t", &base, 60_000);
        let lease = claim.lease_over(None);
        let t = Instant::now();
        let w = a.write_lease(&doc, lease).await.unwrap();
        // Poll every 2 ms so the resolution is not the poll interval.
        loop {
            if b.lease(&doc).await.ok().flatten().map(|l| l.id) == Some(w.id.clone()) {
                break;
            }
            assert!(t.elapsed() < Duration::from_secs(10), "the lease never arrived");
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        lat.push(t.elapsed().as_micros());
    }
    lat.sort_unstable();
    eprintln!(
        "MEASURE lease write -> visible at the other node over 60 writes: min {} us, median {} us, p95 {} us, max {} us",
        lat[0],
        lat[lat.len() / 2],
        lat[lat.len() * 95 / 100],
        lat[lat.len() - 1]
    );
}
