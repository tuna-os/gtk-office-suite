#!/usr/bin/env bash
# Ran on branch plan/collab-in-apps-exp-a (a throwaway: the four dcp crates copied to tools/collab-promoted/ and made members of the product workspace, plus a seam crate collab-session and a tables `collab` feature). Kept here so the numbers in docs/rfc/0005 can be reproduced; not part of any build.
# RFC-0005 experiment: cold compile cost and release size of option A (in the product workspace)
# and option B (a standalone sidecar workspace, spike lockfile).
set -uo pipefail
OUT=${OUT:-$PWD/results}; mkdir -p "$OUT"; S="$OUT/compile.txt"; : > "$S"
say() { echo "$*" | tee -a "$S"; }
A=$PWD
say "nproc=$(nproc) mem=$(free -m | awk '/Mem:/{print $2}')MB rustc=$(rustc --version)"
timeit() { local label=$1; shift; local t0=$(date +%s); "$@" >"$OUT/build-$label.log" 2>&1; local rc=$?; say "$label: exit=$rc $(( $(date +%s)-t0 )) s"; }

say "### A: product workspace, cold target dir, --locked"
export CARGO_TARGET_DIR=$PWD/target-a
timeit "A0 reference: release tables-core (default, what the product already compiles)" cargo build --locked --release -p tables-core
timeit "A1 dev: dcp-core" cargo build --locked -p dcp-core
timeit "A2 dev: collab-session +streams" cargo build --locked -p collab-session --features streams
timeit "A3 dev: collab-session +streams +matrix (adds matrix-sdk)" cargo build --locked -p collab-session --features streams,matrix
timeit "A4 release: probe (matrix+streams)" cargo build --locked --release -p collab-session --features matrix,streams --bin probe
say "A probe release size: $(stat -c %s target-a/release/probe) bytes; stripped: $(strip -o /tmp/probe-a target-a/release/probe && stat -c %s /tmp/probe-a) bytes"
say "A: default-build test: cargo build --locked -p tables-core is up to date after A4? $(cargo build --locked --release -p tables-core 2>&1 | tail -1)"
du -sh target-a | tee -a "$S"
timeit "A5 release: dcp-matrix with sqlite (bundled C sqlite)" cargo build --locked --release -p collab-session --features matrix-sqlite
unset CARGO_TARGET_DIR

say; say "### B: sidecar workspace (own lockfile), cold target dir, --locked"
SC=/tmp/sc; rm -rf "$SC"; mkdir -p "$SC/tools/collab-spike" "$SC/docs"
cp -r docs/collab-spec "$SC/docs/"
for d in dcp-core dcp-conformance dcp-matrix dcp-streams; do cp -r tools/collab-spike/$d "$SC/tools/collab-spike/$d"; done
mkdir -p "$SC/tools/collab-spike/dcp-sidecar/src"
cat > "$SC/Cargo.toml" <<'T'
[workspace]
resolver = "2"
members = ["tools/collab-spike/dcp-core", "tools/collab-spike/dcp-conformance", "tools/collab-spike/dcp-matrix", "tools/collab-spike/dcp-streams", "tools/collab-spike/dcp-sidecar"]
[workspace.package]
version = "0.0.0"
edition = "2021"
publish = false
license = "GPL-3.0-or-later"
[workspace.dependencies]
async-trait = "0.1"
base64 = "0.23"
hex = "0.4"
jsonschema = "0.30"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.11"
thiserror = "2"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time", "sync"] }
T
cat > "$SC/tools/collab-spike/dcp-sidecar/Cargo.toml" <<'T'
[package]
name = "dcp-sidecar"
version.workspace = true
edition.workspace = true
publish.workspace = true
license.workspace = true
[dependencies]
dcp-core = { path = "../dcp-core" }
dcp-matrix = { path = "../dcp-matrix" }
dcp-streams = { path = "../dcp-streams" }
tokio.workspace = true
T
cp "$A/tools/collab-promoted/collab-session/src/bin/probe.rs" "$SC/tools/collab-spike/dcp-sidecar/src/main.rs"
sed -i 's/use collab_session::{connect, Binding};//' "$SC/tools/collab-spike/dcp-sidecar/src/main.rs"
cat >> "$SC/tools/collab-spike/dcp-sidecar/src/main.rs" <<'R'

enum Binding {
    Matrix { homeserver: String, user_id: String, device_id: String, access_token: String },
    Streams { server: String, device: String },
}
async fn connect(b: Binding) -> Result<Box<dyn dcp_core::DocumentStore>, dcp_core::StoreError> {
    match b {
        Binding::Matrix { homeserver, user_id, device_id, access_token } => {
            let creds = dcp_matrix::Credentials { user_id, device_id, access_token };
            Ok(Box::new(dcp_matrix::MatrixStore::connect(dcp_matrix::MatrixConfig::new(homeserver), &creds).await?))
        }
        Binding::Streams { server, device } => Ok(Box::new(dcp_streams::StreamsStore::new(
            dcp_streams::Http::new(&server), dcp_streams::Identity::generate(), dcp_streams::StreamsConfig::new(&device)))),
    }
}
R
# Start from the spike's committed lockfile so versions are the measured ones; cargo prunes what the 5 members do not use.
cp tools/collab-spike/Cargo.lock "$SC/Cargo.lock"
( cd "$SC" && cargo metadata --format-version 1 >/dev/null 2>"$OUT/sc-meta.err"; tail -2 "$OUT/sc-meta.err" )
say "B sidecar lockfile: packages=$(grep -c '^name = ' $SC/Cargo.lock) lines=$(wc -l < $SC/Cargo.lock) (spike full lock: $(grep -c '^name = ' tools/collab-spike/Cargo.lock) packages)"
cp "$SC/Cargo.lock" "$OUT/Cargo.lock.sidecar"
cp "$A/Cargo.lock" "$OUT/Cargo.lock.A"
export CARGO_TARGET_DIR=$SC/target
cd "$SC"
timeit "B1 release: dcp-sidecar (matrix+streams, cold)" cargo build --locked --release -p dcp-sidecar
say "B sidecar release size: $(stat -c %s target/release/dcp-sidecar) bytes; stripped: $(strip -o /tmp/sidecar-b target/release/dcp-sidecar && stat -c %s /tmp/sidecar-b) bytes"
du -sh target | tee -a "$S"
