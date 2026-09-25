//! RFC-0001 Phase 1: CRDT library measurement spike.
//!
//! `crdt-spike all`                 run every backend x scenario, each in its
//!                                  own process (so peak RSS is per run), and
//!                                  print markdown tables
//! `crdt-spike run <backend> <scenario>`
//!                                  one run; prints `R\t<metric>\t<value>` lines
//! `crdt-spike probes`              the Letters marks and nested-map probes
//!
//! Backends: reference, automerge, automerge-naive (decks only), loro, yrs.
//! Scenarios: tables-seq, tables-conc, decks-seq, decks-conc.

// Built with one candidate only (see build-cost.sh), code for the others is
// legitimately unused.
#![cfg_attr(not(all(feature = "automerge", feature = "loro", feature = "yrs")), allow(dead_code, unused_imports))]

mod backend;
mod decks;
mod rng;
mod tables;

#[cfg(feature = "automerge")]
mod am;
#[cfg(feature = "loro")]
mod lo;
#[cfg(feature = "yrs")]
mod yr;

use std::collections::BTreeMap;
use std::fmt::Display;
use std::time::{Duration, Instant};

use backend::{DecksDoc, TablesDoc};
use decks::{base_ops, base_tree, expected_alive, gen_adversarial, gen_random, well_formed};
use tables::{check_merge, gen_session, replay_sheetmodel, View};

const SEED: u64 = 0x5eed_2026_0924;
const TABLE_OPS: usize = 20_000;
const DECK_ACTIONS: usize = 3_000;

fn emit(key: &str, val: impl Display) {
    println!("R\t{key}\t{val}");
}

fn ms(d: Duration) -> String {
    format!("{:.1}", d.as_secs_f64() * 1000.0)
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let t = Instant::now();
    let v = f();
    (v, t.elapsed())
}

fn peak_rss_mb() -> String {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find(|l| l.starts_with("VmHWM:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|kb| kb.parse::<f64>().ok())
        .map(|kb| format!("{:.1}", kb / 1024.0))
        .unwrap_or_else(|| "n/a".into())
}

fn kib(n: usize) -> String {
    format!("{:.1}", n as f64 / 1024.0)
}

// ------------------------------------------------------------------ Tables

fn reference_tables() {
    let base = View::base();
    let s = gen_session(SEED, 'a', &base, TABLE_OPS);
    let ((canon, sheet), t) = timed(|| replay_sheetmodel(&base, &s.actions));
    emit("ops", s.op_count());
    emit("actions (transactions)", s.actions.len());
    emit("apply ms", ms(t));
    emit("final rows", canon.rows.len());
    emit("final cell fields", canon.cells.len());
    emit("equals generator's own view", canon == s.final_view.canon());
    emit("state digest", format!("{:016x}", canon.digest()));
    emit("size KiB: raw keys+values (floor)", kib(canon.payload_bytes()));
    let xlsx = tables_core::io::save_sheets_to_xlsx_bytes(&[sheet], None).unwrap();
    emit("size KiB: xlsx via tables-core (no styles)", kib(xlsx.len()));
    let conc_a = gen_session(SEED + 1, 'a', &base, TABLE_OPS / 2);
    let conc_b = gen_session(SEED + 2, 'b', &base, TABLE_OPS / 2);
    emit("conc: ops A + B", format!("{} + {}", conc_a.op_count(), conc_b.op_count()));
    emit("peak RSS MiB", peak_rss_mb());
}

fn tables_seq<D: TablesDoc>() {
    let base = View::base();
    let s = gen_session(SEED, 'a', &base, TABLE_OPS);
    let (expected, _) = replay_sheetmodel(&base, &s.actions);
    let (mut d, t) = timed(|| {
        let mut d = D::base(&base);
        for a in &s.actions {
            d.apply(a);
        }
        d
    });
    emit("ops", s.op_count());
    emit("actions (transactions)", s.actions.len());
    emit("apply ms", ms(t));
    let (got, t) = timed(|| d.read());
    emit("read state ms", ms(t));
    emit("equals SheetModel replay", got == expected);
    emit("state digest", format!("{:016x}", got.digest()));
    let encs = d.encodings();
    for (name, bytes) in &encs {
        emit(&format!("size KiB: {name}"), kib(bytes.len()));
    }
    let (mut l, t) = timed(|| D::load(&encs[0].1));
    emit(&format!("load ms ({})", encs[0].0), ms(t));
    let (lr, t) = timed(|| l.read());
    emit("read after load ms", ms(t));
    emit("loaded equals SheetModel replay", lr == expected);
    drop(encs);
    emit("peak RSS MiB", peak_rss_mb());
}

fn tables_conc<D: TablesDoc>() {
    let base = View::base();
    let sa = gen_session(SEED + 1, 'a', &base, TABLE_OPS / 2);
    let sb = gen_session(SEED + 2, 'b', &base, TABLE_OPS / 2);
    let mut d0 = D::base(&base);
    let (mut a, mut b) = (d0.fork(1), d0.fork(2));
    drop(d0);
    let (_, t) = timed(|| {
        for x in &sa.actions {
            a.apply(x);
        }
        for x in &sb.actions {
            b.apply(x);
        }
    });
    emit("ops", sa.op_count() + sb.op_count());
    emit("apply ms (both peers)", ms(t));
    let (n_ab, t1) = timed(|| a.merge_from(&mut b));
    let (n_ba, t2) = timed(|| b.merge_from(&mut a));
    emit("merge ms (A pulls B)", ms(t1));
    emit("merge ms (B pulls A)", ms(t2));
    emit("delta KiB (B->A, A->B)", format!("{} / {}", kib(n_ab), kib(n_ba)));
    let (ca, cb) = (a.read(), b.read());
    emit("peers converge", ca == cb);
    emit("state digest", format!("{:016x}", ca.digest()));
    let chk = check_merge(&base, &sa, &sb, &ca);
    emit("merge oracle violations", chk.violations.len());
    for v in chk.violations.iter().take(5) {
        emit("  violation", v);
    }
    emit("rows after merge", ca.rows.len());
    emit("conflicting cell fields (A won / B won)", format!("{} ({} / {})", chk.conflicts, chk.a_wins, chk.b_wins));
    emit("edits to rows the other peer deleted", chk.orphaned_edits);
    let encs = a.encodings();
    for (name, bytes) in &encs {
        emit(&format!("size KiB: {name}"), kib(bytes.len()));
    }
    let (mut l, t) = timed(|| D::load(&encs[0].1));
    emit(&format!("load ms ({})", encs[0].0), ms(t));
    emit("loaded equals merged", l.read() == ca);
    emit("peak RSS MiB", peak_rss_mb());
}

// ------------------------------------------------------------------ Decks

fn reference_decks() {
    let ops = base_ops(SEED);
    let base = base_tree(&ops);
    emit("base nodes (slides + groups + objects)", base.nodes.len() - 1);
    let s = gen_random(SEED + 3, 'a', &base, DECK_ACTIONS);
    emit("actions", s.actions.len());
    emit("ops", s.op_count());
    let dump = s.final_tree.dump();
    emit("state digest", format!("{:016x}", fnv(&dump)));
    let adv = gen_adversarial(SEED + 4, &base);
    for (what, n) in &adv.summary {
        emit(&format!("conc adversarial: {what}"), n);
    }
    emit("peak RSS MiB", peak_rss_mb());
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn decks_seq<D: DecksDoc>() {
    let ops = base_ops(SEED);
    let base = base_tree(&ops);
    let s = gen_random(SEED + 3, 'a', &base, DECK_ACTIONS);
    let (mut d, t) = timed(|| {
        let mut d = D::base(&ops);
        for a in &s.actions {
            d.apply(a);
        }
        d
    });
    emit("actions", s.actions.len());
    emit("apply ms (incl. building the base deck)", ms(t));
    let (r, t) = timed(|| d.read());
    emit("read state ms", ms(t));
    let want = s.final_tree.dump();
    emit("equals reference tree", r.dump == want);
    emit("state digest", format!("{:016x}", fnv(&r.dump)));
    let encs = d.encodings();
    for (name, bytes) in &encs {
        emit(&format!("size KiB: {name}"), kib(bytes.len()));
    }
    let (mut l, t) = timed(|| D::load(&encs[0].1));
    emit(&format!("load ms ({})", encs[0].0), ms(t));
    emit("loaded equals reference tree", l.read().dump == want);
    emit("peak RSS MiB", peak_rss_mb());
}

fn decks_conc<D: DecksDoc>() {
    let ops = base_ops(SEED);
    let base = base_tree(&ops);
    let adv = gen_adversarial(SEED + 4, &base);
    let ra = gen_random(SEED + 5, 'a', &adv.tree_a, DECK_ACTIONS / 2);
    let rb = gen_random(SEED + 6, 'b', &adv.tree_b, DECK_ACTIONS / 2);
    let mut d0 = D::base(&ops);
    let (mut a, mut b) = (d0.fork(1), d0.fork(2));
    drop(d0);
    let (_, t) = timed(|| {
        for x in adv.a.iter().chain(&ra.actions) {
            a.apply(x);
        }
        for x in adv.b.iter().chain(&rb.actions) {
            b.apply(x);
        }
    });
    emit("actions", adv.a.len() + ra.actions.len() + adv.b.len() + rb.actions.len());
    emit("apply ms (both peers)", ms(t));
    let (n_ab, t1) = timed(|| a.merge_from(&mut b));
    let (n_ba, t2) = timed(|| b.merge_from(&mut a));
    emit("merge ms (A pulls B)", ms(t1));
    emit("merge ms (B pulls A)", ms(t2));
    emit("delta KiB (B->A, A->B)", format!("{} / {}", kib(n_ab), kib(n_ba)));
    let (ra_, rb_) = (a.read(), b.read());
    emit("peers converge", ra_.dump == rb_.dump);
    emit("state digest", format!("{:016x}", fnv(&ra_.dump)));
    let (alive, deleted) = expected_alive(&base, &[&adv.a, &ra.actions, &adv.b, &rb.actions]);
    let wf = well_formed(&ra_, &alive, &deleted);
    emit("well-formed (no dup, no loss, no cycle)", wf.ok());
    emit("visible nodes / expected", format!("{} / {}", wf.visible, alive.len()));
    emit("duplicated nodes", wf.duplicated);
    emit("lost nodes", wf.missing);
    emit("nodes stuck in cycles", wf.in_cycles);
    emit("nodes with missing parent", wf.dangling);
    emit("deleted nodes brought back by a concurrent move", format!("{} of {}", wf.resurrected, deleted.len()));
    // Outcomes of the deliberate conflicts (later random edits can touch
    // the same nodes, so these are indicative rather than exact).
    let (mut on_a, mut on_b, mut on_both, mut other) = (0, 0, 0, 0);
    for (xa, xb) in adv.a.iter().zip(&adv.b).take(20) {
        if let (decks::DOp::Move { id, parent: pa, .. }, decks::DOp::Move { parent: pb, .. }) = (&xa[0], &xb[0]) {
            let ps = ra_.parents.get(id).cloned().unwrap_or_default();
            match (ps.contains(pa), ps.contains(pb)) {
                (true, true) => on_both += 1,
                (true, false) => on_a += 1,
                (false, true) => on_b += 1,
                _ => other += 1,
            }
        }
    }
    emit(
        "same object moved to 2 slides: ends on A's / B's / both / elsewhere",
        format!("{on_a} / {on_b} / {on_both} / {other}"),
    );
    let revived = adv.a[25..30]
        .iter()
        .filter(|x| matches!(&x[0], decks::DOp::Delete { id } if ra_.parents.contains_key(id)))
        .count();
    emit("delete vs concurrent move: move wins (of 5)", revived);
    let cycles_left = adv.a[20..25]
        .iter()
        .filter(|x| matches!(&x[0], decks::DOp::Move { id, .. } if !ra_.parents.contains_key(id)))
        .count();
    emit("cycle pairs whose first group is not visible (of 5)", cycles_left);
    let encs = a.encodings();
    for (name, bytes) in &encs {
        emit(&format!("size KiB: {name}"), kib(bytes.len()));
    }
    let (mut l, t) = timed(|| D::load(&encs[0].1));
    emit(&format!("load ms ({})", encs[0].0), ms(t));
    emit("loaded equals merged", l.read().dump == ra_.dump);
    emit("peak RSS MiB", peak_rss_mb());
}

// ------------------------------------------------------------------ dispatch

fn run_pair<T: TablesDoc, D: DecksDoc>(scenario: &str) {
    match scenario {
        "tables-seq" => tables_seq::<T>(),
        "tables-conc" => tables_conc::<T>(),
        "decks-seq" => decks_seq::<D>(),
        "decks-conc" => decks_conc::<D>(),
        _ => panic!("unknown scenario {scenario}"),
    }
}

fn run(backend: &str, scenario: &str) {
    match backend {
        "reference" => match scenario {
            "tables-seq" => reference_tables(),
            "decks-seq" => reference_decks(),
            _ => {}
        },
        #[cfg(feature = "automerge")]
        "automerge" => run_pair::<am::AmTables, am::AmDeckPP>(scenario),
        #[cfg(feature = "automerge")]
        "automerge-naive" => match scenario {
            "decks-seq" => decks_seq::<am::AmDeckNaive>(),
            "decks-conc" => decks_conc::<am::AmDeckNaive>(),
            _ => {}
        },
        #[cfg(feature = "loro")]
        "loro" => run_pair::<lo::LoTables, lo::LoDeck>(scenario),
        #[cfg(feature = "yrs")]
        "yrs" => run_pair::<yr::YTables, yr::YDeckPP>(scenario),
        _ => eprintln!("backend {backend} not compiled in"),
    }
}

fn backends() -> Vec<&'static str> {
    let mut v = vec!["reference"];
    if cfg!(feature = "automerge") {
        v.push("automerge");
        v.push("automerge-naive");
    }
    if cfg!(feature = "loro") {
        v.push("loro");
    }
    if cfg!(feature = "yrs") {
        v.push("yrs");
    }
    v
}

/// Median for timing and memory metrics; otherwise the value, which must be
/// the same on every repetition (sizes and correctness are deterministic).
fn summarise(key: &str, vals: &[String]) -> String {
    let timing = key.contains(" ms") || key.contains("RSS");
    let nums: Option<Vec<f64>> = vals.iter().map(|v| v.parse::<f64>().ok()).collect();
    match nums {
        Some(mut n) if timing && !n.is_empty() => {
            n.sort_by(f64::total_cmp);
            format!("{:.1}", n[n.len() / 2])
        }
        _ => {
            let mut u: Vec<&String> = vals.iter().collect();
            u.dedup();
            u.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" / ")
        }
    }
}

fn all() {
    let exe = std::env::current_exe().unwrap();
    let reps: usize = std::env::var("CRDT_SPIKE_REPS").ok().and_then(|r| r.parse().ok()).unwrap_or(3);
    println!("Each cell: median of {reps} runs for times and peak RSS; other values are identical across runs.");
    for scenario in ["tables-seq", "tables-conc", "decks-seq", "decks-conc"] {
        let mut metrics: Vec<String> = Vec::new();
        let mut cols: Vec<(&str, BTreeMap<String, Vec<String>>)> = Vec::new();
        for b in backends() {
            if b == "automerge-naive" && scenario.starts_with("tables") {
                continue;
            }
            let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for rep in 0..reps {
                eprintln!("running {b} {scenario} ({}/{reps}) ...", rep + 1);
                let out = std::process::Command::new(&exe).args(["run", b, scenario]).output().unwrap();
                if !out.status.success() {
                    eprintln!("{b} {scenario} FAILED:\n{}", String::from_utf8_lossy(&out.stderr));
                }
                let mut seen: BTreeMap<String, String> = BTreeMap::new();
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let mut parts = line.splitn(3, '\t');
                    if parts.next() == Some("R") {
                        let (k, v) = (parts.next().unwrap().to_string(), parts.next().unwrap_or("").to_string());
                        if !metrics.contains(&k) {
                            metrics.push(k.clone());
                        }
                        seen.entry(k).and_modify(|e| e.push_str(&format!("; {v}"))).or_insert(v);
                    }
                }
                for (k, v) in seen {
                    m.entry(k).or_default().push(v);
                }
            }
            if !m.is_empty() {
                cols.push((b, m));
            }
        }
        println!("\n### {scenario}\n");
        print!("| metric |");
        for (b, _) in &cols {
            print!(" {b} |");
        }
        println!();
        print!("|---|");
        for _ in &cols {
            print!("---|");
        }
        println!();
        for k in &metrics {
            print!("| {k} |");
            for (_, m) in &cols {
                print!(" {} |", m.get(k).map(|v| summarise(k, v)).unwrap_or_else(|| "—".into()));
            }
            println!();
        }
    }
}

fn probes() {
    #[cfg(feature = "automerge")]
    {
        println!("## automerge\n");
        for l in am::letters_probe() {
            println!("{l}");
        }
        println!("nested map, concurrent creation: {}\n", am::nested_probe());
    }
    #[cfg(feature = "loro")]
    {
        println!("## loro\n");
        for l in lo::letters_probe() {
            println!("{l}");
        }
        println!("nested map, concurrent creation: {}\n", lo::nested_probe());
    }
    #[cfg(feature = "yrs")]
    {
        println!("## yrs\n");
        for l in yr::letters_probe() {
            println!("{l}");
        }
        println!("nested map, concurrent creation: {}\n", yr::nested_probe());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("run") => run(&args[2], &args[3]),
        Some("probes") => probes(),
        Some("all") | None => all(),
        Some(other) => eprintln!("unknown command {other}"),
    }
}
