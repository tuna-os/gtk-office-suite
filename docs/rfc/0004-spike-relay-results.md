# RFC-0004 Spike C, round 2: dcp-p2p over a real iroh-relay, in separate network namespaces, on shaped links

Date: 2026-10-25 · Tracks: [RFC-0004](0004-interoperable-document-collaboration.md), [DCP draft 1 §8.5](../DOCUMENT-COLLABORATION-SPEC.md), [RFC-0001](0001-crdt-collaboration.md) (the Flatpak question) · Follows [Spike C](0004-spike-c-results.md), whose "not verified" list this closes in part · Code: [`tools/collab-spike/dcp-p2p/`](../../tools/collab-spike/dcp-p2p/) (`tests/wan.rs`, `src/bin/dcp-p2p-node.rs`, `src/bin/dcp-p2p-footprint.rs`, `src/pathlog.rs`) and [`tools/collab-spike/scripts/netns-wan.sh`](../../tools/collab-spike/scripts/netns-wan.sh) (branch `spike/collab-relay`) · Workflow: [`collab-spike.yml`](../../.github/workflows/collab-spike.yml) (the spike ran it as its own `collab-spike-relay.yml`, merged into it at integration; now the jobs `p2p-relay` for the in-process relay test and `wan-plan`, `wan-probe`, `wan-build`, `wan` and `wan-footprint`; the spike's `check` is the `check` job)

**Runs.** Everything in the tables comes from CI run
[40179059127](https://github.com/tuna-os/gtk-office-suite/actions/runs/40179059127) (commit
`53e228528b849bdfc67f81b6b46dde2b077752b1`; conclusion **success**: `check`, `probe`, `build`, 14 `wan` jobs,
`footprint`), except where a section names another run. Artifacts: `wan-<condition>-<journeys|rest>` (logs, raw
`results.jsonl`, the path dumps), `footprint`, `relay-probe`, `wan-bin` (the binaries and `versions.txt`). Earlier runs of the
same workflow on this branch that failed or were superseded: 40174628555 (the relay binary was not on `sudo`'s PATH),
40175038124 and 40175614808 (compile errors found by `check`), 40176037256 (a smoke run, below).

**Machine and versions.** GitHub-hosted `ubuntu-24.04`, kernel 6.17.0-1022-azure, 4 vCPU, 16 GB, shared. **The CPU model differs
between jobs** (`setup.txt`): in the matrix run the `rest` jobs of unshaped, rtt50, rtt150, loss5 and worst ran on an AMD EPYC
7763, those of loss1 and rate1m on an EPYC 9V45, the `journeys` jobs on 7763 or 9V74. CPU seconds are therefore only compared
below between rows on the same CPU. rustc/cargo 1.99.1. iroh 1.4.1, noq 1.4.0, iroh-blobs 0.103.1, p2panda 0.7.1 (the dcp-p2p lockfile).
The relay is **`iroh-relay` 1.4.1** (`iroh-relay --version` prints `iroh-relay 1.4.1`), installed with
`cargo install iroh-relay --version =1.4.1 --features server --locked` (the lockfile shipped in the crate), the same number as the
`iroh` in `dcp-p2p/Cargo.lock`. iptables v1.8.10 (nf_tables), iproute2 6.1.0, OpenSSL 3.0.13, tshark from the Ubuntu archive.

## Recommendation

**The relay and path questions Spike C left open now have measured answers, and they change several sentences of 8.5 (see Spec corrections).** Nothing
failed: all 154 journey runs (11 journeys × 2 modes × 7 link conditions) passed, and every assertion below held in every
condition. What the runs do not cover (NAT, a real WAN, jitter) is in [Not verified](#not-verified).

1. **"Relay-only" was not shown in Spike C, and it is not what withholding addresses does.** Spike C gave each node the other's
   relay URL and no IP address and called the result a relay run. With UDP between the peers reachable, iroh opens a direct path
   about three round trips after the handshake (median 156 ms at 50 ms RTT, 463 ms at 150 ms) and selects it. A connection
   stays on the relay only when direct UDP is impossible. With the firewall dropping all peer-to-peer packets, iroh's own path
   data showed 1,515 connections over 7 conditions with **no IP path ever opened and 0 IP bytes**; with UDP allowed and the same
   withheld addresses the same assertion failed in **35 of 35** runs (the negative control).
2. **A relay is needed to start and to recover, not to continue.** An already-direct connection survived the relay being killed in 7 of 7 conditions and kept
   carrying publishes; a relay-only connection delivered nothing while the relay was dead (7 of 7) and recovered 3.5 to 22.6 s
   after the relay came back.
3. **A silently dead direct path costs 15 to 20 s.** With the firewall cutting an upgraded connection, the next publish became
   visible after 15.6 to 21.2 s; iroh moved to the relay path after 15.0 to 19.9 s (iroh's `PATH_MAX_IDLE_TIMEOUT` is 15 s).
4. **Lease rule 3's 500 ms settle interval is not enough on a WAN.** A lease write took a median of 437 to 507 ms and up to 525 ms
   to show at the other node at 150 ms RTT (11 to 15 of 30 writes exceeded 500 ms), and up to 662 ms at 5 % loss with no
   delay; with 150 ms, 5 % loss and 1 Mbit/s the maximum was 955 ms.
5. **The reader cost of p2panda-spaces is CPU-bound unless the link is slow.** Without a rate limit the reader's CPU time was
   79 to 102 % of the wait at 300 and 1,000 entries (delay and loss alone did not change that); at 1 Mbit/s the wait was within
   4 % of the time the received bytes need on the link (60 s against 58 s at 1,000 entries) and the CPU was busy for 52 % of it.
6. **For a private deployment a client needs outbound TCP 443 to the relay and UDP to its peers, and nothing else.** UDP 7842
   and TCP 80 to the relay are optional (with them blocked a node was ready after 3.0 to 3.6 s); no DNS is needed with an
   IP-literal relay URL; and
   the endpoint contacts no discovery service when built with iroh's `Minimal` preset (0 packets on the wire in the capture;
   the `N0` preset put 374 packets on it).

## Method

### Topology

```
 dcp-a 10.77.1.2 ─┐                        ┌─ relay 10.77.0.1 (iroh-relay 1.4.1 in dcp-r:
 dcp-b 10.77.2.2 ─┼── veth, netem both ──  dcp-r  (forwarding on)     HTTPS 443, HTTP 80, QUIC address discovery UDP 7842)
 dcp-c 10.77.3.2 ─┘   ends of every link   └─ FORWARD: ACCEPT (mode direct) or DROP all (mode relay-only)
```

Each peer is a `dcp-p2p-node` process started with `ip netns exec dcp-N`, driven over JSON lines on stdin/stdout. A `DocumentStore`
proxy (`Node` in `tests/wan.rs`) makes the unchanged `dcp-conformance` journeys run against processes in different namespaces. The
relay runs in `dcp-r` from `iroh-relay --config-path` with a TLS certificate from a test CA (OpenSSL, P-256, certificate SAN
`IP:10.77.0.1`); the peers trust that CA only (`CaTlsConfig::custom_roots`, new `P2pConfig::relay_ca_der`), and reach the relay
at `https://10.77.0.1/`. Relay traffic is INPUT/OUTPUT on `dcp-r`, so the FORWARD rule never touches it. A peer reaches the
relay in one hop and another peer in two.

Modes: **relay-only**: FORWARD drops everything, and the peers are told each other's endpoint id and relay URL only. **direct**:
FORWARD accepts, and the peers are told the IP address too (the "addresses provided" case); the upgrade tests give the peers the
relay URL only and still allow UDP. Mode switches while the peers run (used by the mid-session test).

### Shaping (`netns-wan.sh shape <rtt_ms> <loss_pct> <rate>`)

`tc qdisc ... root netem limit 10000 [delay] [loss] [rate]` on **both ends of every veth**: delay R/4 per end, loss L/2 per end,
rate per end. So a peer-to-peer path (two links) has round trip R and loses about L per direction, a peer-to-relay path R/2 with
L/2 loss per direction, and a relayed peer-to-peer path (peer, relay, peer) R. The delay is fixed (no jitter), loss is random and
independent, and `rate` is netem's. Measured with `ping` in each job (`setup.txt`, 30 pings; minimum round trip):

| condition | `shape` | peer to peer | peer to relay | loss seen (30 pings, a to b / a to relay) |
|---|---|---|---|---|
| unshaped | 0 0 - | 0.045 ms | 0.023 ms | 0 / 0 |
| rtt50 | 50 0 - | 50.1 ms | 25.0 ms | 0 / 0 |
| rtt150 | 150 0 - | 150.1 ms | 75.1 ms | 0 / 0 |
| loss1 | 0 1 - | 0.04 ms | 0.02 ms | 0 / 0 |
| loss5 | 0 5 - | 0.04 ms | 0.03 ms | 10 % / 6.7 % (4 and 2 crossings of 2.5 %) |
| rate1m | 0 0 1mbit | 3.2 ms | 1.6 ms | 0 / 0 |
| worst | 150 5 1mbit | 153.3 ms | 76.7 ms | 3.3 % / 3.3 % |

`scale` in the matrix multiplies the timeouts inside the tests (1 to 5); it never changes what is asserted.

### What each test asserts, and its negative control

Path data comes only from iroh 1.4.1's own API, read in the node process (`src/pathlog.rs`): `Connection::paths()` (a `PathList`
whose `Path`s say `is_ip()`, `is_relay()`, `is_selected()`, `stats()`), `Connection::path_events()` (`Opened`, `Selected`,
`Closed` with final statistics) and `Endpoint::metrics().socket` (`send_ipv4`, `send_relay`, `holepunch_attempts`,
`paths_direct`, ...). Timestamps are taken in the node process.

| test (`tests/wan.rs`) | asserts | negative control |
|---|---|---|
| `path_relay_only_when_udp_is_blocked` | both nodes: a sync connection exists, no IP path was ever open, 0 bytes on IP paths, the selected path of every sync connection is the relay, the relay path carried bytes | `path_control_udp_allowed_...`: same setup with UDP allowed; the assertion must return `Err` (35 of 35 runs) |
| `path_control_udp_allowed_upgrades_...` | an IP path is selected within 90 s (×scale); time from the handshake to the first selected IP path of the first connection | the relay-only assertion fails on every run |
| `path_direct_when_addresses_are_provided` | the sync connections select an IP path that carried bytes | the relay-only assertion must fail |
| `relay_killed_while_the_connection_is_direct` | the same sync connection is not closed, six publishes (alternating direction) arrive | n/a (survival is the property) |
| `relay_killed_while_the_connection_is_relay_only` | recovery after the relay returns | a publish must **not** arrive while the relay is dead |
| `direct_path_blocked_mid_session_...` | data still flows after the block; time to relay selection and back | n/a |
| `ports_tcp_443_to_the_relay_is_enough` | with only TCP 443 to the relay allowed (iptables OUTPUT in each peer namespace), two nodes come online and sync, relay-only verdict `Ok` | `ports_control_nothing_allowed_out...`: with everything dropped a node must **not** come online |
| `journeys_relay_only`, `journeys_direct_allowed` | `run_all`-equivalent: each of the 11 journeys passes | `path_...` tests; the per-run path summary (below) shows the mode was real |
| `catch_up_*`, `concurrent_heads_*`, `lease_latency_*`, `connect_and_files_*`, `state_growth_*` | the scenario's own assertions (versions arrive, heads, file bytes equal) | as in `tests/p2p.rs` |

A test that needs the namespaces **fails** when they are missing, with the command to create them; only `DCP_SKIP_NETNS=1` skips
it, and the test prints `SKIP`. The footprint script fails if the `minimal` variant sends any packet or if the `n0` variant
sends none (a blind capture proves nothing).

### Commands (CI runs the same)

```
sudo tools/collab-spike/scripts/netns-wan.sh up
sudo tools/collab-spike/scripts/netns-wan.sh shape 150 5 1mbit            # rtt_ms loss_pct rate
sudo env IROH_RELAY_BIN=$(command -v iroh-relay) tools/collab-spike/scripts/netns-wan.sh relay-up
sudo env DCP_WAN_CONDITION=worst DCP_WAN_SCALE=5 DCP_WAN_BIG=1 DCP_WAN_OUT=out DCP_WAN_NODE_BIN=target/release/dcp-p2p-node \
     DCP_WAN_SCRIPT=$PWD/tools/collab-spike/scripts/netns-wan.sh  target/release/deps/wan-<hash> --test-threads=1 --nocapture
sudo tools/collab-spike/scripts/netns-footprint.sh target/release/dcp-p2p-footprint out/     # item 5
python3 tools/collab-spike/scripts/wan-results.py <downloaded artifacts>                      # tables from results.jsonl
```

Hosted runners allow all of it: namespaces, veth, `tc netem` (`sch_netem`) and iptables worked as root in the first probe (run
40174628555) and tcpdump in the `wan` jobs, so the fallback to a userspace delay proxy was not needed.

### How this was iterated, and what changed after seeing data

The first full-stack run was a smoke run (40176037256, one condition, the path tests only). It showed one thing the test got
wrong: the upgrade time was read from the first connection a node *dialled*, and that connection had already learned the peer's
address from the peer's earlier dial, so it reported 0 ms. The test now follows the earliest connection of the pair
(earliest dial start over both nodes) and reports the time from the end of its handshake to the first selected IP path. The
tcp-443-only test and its control were added after the smoke run as well; no assertion was weakened or removed after any result,
and the matrix run is the first in which all of it ran. Compile errors in `tests/wan.rs` and the node were found by the `check`
job (runs 40175038124, 40175614808) before any measurement.

**Lockfiles.** The committed `tools/collab-spike/dcp-p2p/Cargo.lock` on main was stale: `cargo metadata --locked` on a clean
checkout of main fails, because the lock has no `parking_lot 0.13`, which `dcp-core` and `dcp-conformance` require. It is regenerated here (adding
`parking_lot 0.13`, `lock_api 0.5`, `parking_lot_core 0.10` and the new direct dependency edges; no existing version changed,
iroh stays 1.4.1) and `cargo check --all-targets --locked`, with and without `relay-test`, passes in the `check` job. The other
workspace's `tools/collab-spike/Cargo.lock` is also stale on main (the `collab-spike.yml` jobs on this branch fail with "cannot
update the lock file ... because --locked was passed"; run 40179059128); it is outside this spike and untouched.

## Results

### 1. Is a connection direct or relayed? (item 1)

**Path proof, firewall-enforced.** Same setup in 7 conditions: `path_relay_only_when_udp_is_blocked` returned `Ok` in 7 of 7
(`a_selected`/`b_selected` all `relay`); `path_direct_when_addresses_are_provided` returned `Ok` in 7 of 7 with its relay-only
control failing as required (`Err("a: an IP path to the peer existed")`). The firewall was seen to be doing work: in the first
probe, with `relay-only`, pings between the namespaces failed and the FORWARD DROP counter rose (6 packets, `probe`), and
peer-to-relay pings always worked.

**The same assertion at scale.** Each journeys job records, per node as it exits, iroh's paths for every connection. Summed over
the 7 conditions (231 nodes per mode):

| mode | connections | with an IP path | that selected an IP path | bytes on IP paths | bytes on relay paths |
|---|---|---|---|---|---|
| relay-only | 1,515 | **0** | 0 | **0** | 47,117,772 |
| direct (addresses provided) | 1,476 | 1,476 | 1,476 | 54,564,012 | 17,198,419 |

(The relay path of a direct connection stays open and carries traffic too: 24 % of the bytes counted over all paths of the
direct-mode connections. Path statistics count every path of a connection, selected or not; that was not separated.)

**One timeline** (condition rtt150, upgrade test, node a, times from the dial start; `rtt150-path_control-a.json`): handshake
done through the relay at 151 ms (one round trip, relay path selected); IP path opened and selected at 615.7 ms. iroh's own
metrics for the pair: `holepunch_attempts` 2, `paths_direct` 2, `paths_relay` 3, `num_conns_direct` 2.

**Upgrade time** (UDP allowed, addresses withheld, 5 fresh node pairs per condition, the first connection of the pair; from the
handshake's end to the first selected IP path of that connection; no NAT, so hole punching here is trivially possible):

| condition | upgraded | median ms after handshake | median ms after dial start | values (ms after handshake) |
|---|---|---|---|---|
| unshaped | 5/5 | 1 | 3 | 1, 1, 1, 1, 1 |
| rtt50 | 5/5 | 156 | 207 | 155, 155, 156, 156, 156 |
| rtt150 | 5/5 | 463 | 615 | 463, 463, 463, 463, 464 |
| loss1 | 5/5 | 0 | 1 | 1, 0, 0, 0, 0 |
| loss5 | 5/5 | 1 | 3 | 1, 4, 1, 1, 1 |
| rate1m | 5/5 | 430 | 476 | 429, 431, 430, 431, 429 |
| worst | 5/5 | 715 | 983 | 709, 715, 787, 654 (a fifth run's first connection had no selected-IP event; the pair did upgrade) |

So the upgrade takes about three round trips of the peer-to-peer path after the relay handshake (3.1 × RTT at 50 and 150 ms).
At 1 Mbit/s it took 430 ms with a 3 ms round trip, so the rate limit, not the delay, set it there. Spike C's in-process relay
test, read with the same API, is next.

**Spike C's in-process relay test, read with the path API** (`tests/relay.rs`, job `relay-inproc`, run
[40192160497](https://github.com/tuna-os/gtk-office-suite/actions/runs/40192160497), commit `f45846ac`, debug build, loopback, no
firewall, iroh's in-process relay server). The nodes were given the relay URL and no direct address, as in Spike C. iroh reports,
for the connections of both nodes: a relay path existed (`Relay(https://127.0.0.1:44763/)`, not selected) **and an IP path to the
peer's loopback address existed and was selected** (`Ip(127.0.0.1:60447)`, `selected: true`, rtt 0.6 to 2.0 ms against 4.3 to
14 ms for the relay path); the test asserts the relay path existed and prints the rest. So Spike C's "relay-only addresses" run
synced over a **direct** path (a relay path existed alongside it), and its measured "20.9 ms with relay-only addresses
against 20.5 ms with direct addresses" compared two direct connections (this run: 25.5 against 25.4 ms, same debug build). What it did not
show is relay-only operation; the firewall-enforced test above does.

### 2. The conformance journeys on shaped links, both modes (item 3)

All 11 journeys pass in all 7 conditions in both modes (154 of 154, none skipped). Time per journey in seconds, relay-only / direct:

| journey | unshaped | rtt50 | rtt150 | loss1 | loss5 | rate1m | worst |
|---|---|---|---|---|---|---|---|
| convergence_on_one_head (1) | 0.9 / 1.0 | 1.9 / 1.9 | 3.3 / 3.6 | 1.0 / 1.0 | 1.3 / 4.1 | 2.5 / 2.7 | 5.7 / 7.3 |
| second_head_shown_not_hidden (2) | 0.9 / 0.9 | 1.9 / 1.8 | 3.5 / 3.5 | 1.0 / 1.0 | 1.8 / 1.8 | 2.7 / 2.8 | 7.0 / 5.7 |
| corrupt_file_rejected (3) | 0.6 / 0.6 | 1.4 / 1.1 | 2.4 / 2.4 | 0.7 / 1.6 | 0.9 / 1.8 | 1.8 / 2.2 | 4.4 / 4.4 |
| truncated_upload_never_published (3) | 0.7 / 0.7 | 1.4 / 1.4 | 2.5 / 2.5 | 0.7 / 0.8 | 0.9 / 2.0 | 1.9 / 2.2 | 5.0 / 5.5 |
| lease_exclusion_and_readback (4) | 2.6 / 2.4 | 3.2 / 3.3 | 4.8 / 5.2 | 2.7 / 2.5 | 2.9 / 3.7 | 3.6 / 3.7 | 8.6 / 9.5 |
| lease_expiry_releases (5) | 12.2 / 12.3 | 12.8 / 12.7 | 13.8 / 13.8 | 12.4 / 13.1 | 12.7 / 13.6 | 13.2 / 13.3 | 16.4 / 15.4 |
| roles_enforced_at_store (6) | 1.4 / 1.4 | 2.4 / 2.4 | 4.6 / 4.6 | 1.5 / 1.5 | 2.9 / 2.6 | 3.0 / 3.1 | 8.2 / 5.9 |
| forged_version_from_a_viewer_is_not_accepted (6) | 0.7 / 0.6 | 1.3 / 1.1 | 2.4 / 2.4 | 0.8 / 0.7 | 2.3 / 1.7 | 2.0 / 2.4 | 5.0 / 5.5 |
| revocation_hides_new_content (7) | 0.7 / 0.7 | 1.6 / 1.6 | 3.3 / 3.3 | 0.9 / 0.9 | 1.1 / 1.8 | 2.2 / 2.4 | 7.0 / 4.5 |
| unknown_algorithm_falls_back_to_versions (8) | 0.7 / 0.7 | 1.2 / 1.2 | 2.5 / 2.5 | 1.0 / 0.8 | 0.8 / 1.1 | 2.1 / 2.1 | 4.5 / 3.6 |
| update_stream_gap_recovery (9) | 1.1 / 1.1 | 1.7 / 1.8 | 3.7 / 3.7 | 2.2 / 2.2 | 1.5 / 1.4 | 2.6 / 2.6 | 6.6 / 5.1 |

Every journey but `lease_expiry_releases` took 9.5 s or less; that one is 12 s because of its own 4 s lease and 500 ms settle.
The fixture's settle bound was 30 s ×scale, so nothing came near a timeout. **A passing lease journey does not show that the
500 ms settle covers propagation** (next section); the journeys wait on the fixture's settle bound.

**Timing scenarios** (relay-only / direct; `tests/wan.rs`, single run per cell, three for the connect row):

| condition | offline catch-up of 5 versions (ms) | two heads visible everywhere after reconnect (ms) | merge visible everywhere (ms) | cold connect + first session (ms, median of 3) | warm session (ms) |
|---|---|---|---|---|---|
| unshaped | 53 / 53 | 56 / 61 | 58 / 58 | 10 / 10 | 3 / 2 |
| rtt50 | 363 / 362 | 365 / 364 | 262 / 263 | 169 / 167 | 103 / 103 |
| rtt150 | 877 / 879 | 932 / 932 | 628 / 628 | 485 / 483 | 307 / 305 |
| loss1 | 52 / 51 | 54 / 1,031 | 55 / 56 | 8 / 7 | 2 / 1 |
| loss5 | 260 / 154 | 56 / 1,044 | 472 / 58 | 12 / 10 | 2 / 4 |
| rate1m | 1,447 / 1,649 | 880 / 931 | 469 / 677 | 258 / 249 | 144 / 107 |
| worst | 2,484 / 1,601 | 2,485 / 1,345 | 1,768 / 989 | 1,025 / 624 | 566 / 397 |

(The 1,031 and 1,044 ms entries are one lost round each: the anti-entropy interval is 200 ms and a session that loses a packet
waits for QUIC's loss recovery. Offline catch-up is about 6 round trips at 50 and 150 ms RTT.) The unshaped figures agree with
Spike C's loopback ones (52 ms catch-up).

**Files** (fetch time at the reader in ms, relay-only / direct; 1 MiB and 8 MiB; `32 MiB` was not run, it would take 4.5 minutes
at 1 Mbit/s):

| condition | 1 MiB | 8 MiB | 8 MiB, MiB/s |
|---|---|---|---|
| unshaped | 10 / 6 | 45 / 21 | 177 / 365 |
| rtt50 | 464 / 461 | 800 / 794 | 10.0 / 10.1 |
| rtt150 | 1,379 / 1,369 | 2,384 / 2,384 | 3.4 / 3.4 |
| loss1 | 4 / 2 | 21 / 13 | 365 / 580 |
| loss5 | 11 / 6 | 303 / 52 | 26 / 152 |
| rate1m | 10,474 / 9,293 | 81,848 / 72,070 | 0.10 / 0.11 |
| worst | 13,077 / 21,428 | 89,539 / 197,854 | 0.09 / 0.04 |

At 1 Mbit/s a file takes the link's time (8 MiB in 72 to 82 s against 67 s for the bits). In the `worst` condition the
relay's TCP stream beat direct QUIC over UDP (90 s against 198 s for 8 MiB, 13 s against 21 s for 1 MiB), the only condition in which
the relay path was faster.

### 3. Lease propagation: what lease rule 3's settle interval must cover

Thirty lease writes per cell at one node, polled every 5 ms at the other (`lease_latency_*`); ms, min / median / p95 / max, and
the number of the 30 that exceeded 500 ms:

| condition | relay-only | > 500 ms | direct | > 500 ms |
|---|---|---|---|---|
| unshaped | 10 / 17 / 17 / 17 | 0 | 9 / 11 / 18 / 18 | 0 |
| rtt50 | 101 / 110 / 113 / 114 | 0 | 98 / 110 / 113 / 113 | 0 |
| rtt150 | 256 / 437 / 523 / 525 | 11 | 251 / 507 / 521 / 522 | 15 |
| loss1 | 7 / 8 / 9 / 17 | 0 | 7 / 8 / 9 / 63 | 0 |
| loss5 | 9 / 17 / 629 / 662 | 2 | 9 / 15 / 104 / 166 | 0 |
| rate1m | 63 / 65 / 76 / 88 | 0 | 51 / 52 / 157 / 175 | 0 |
| worst | 325 / 472 / 884 / 955 | 11 | 296 / 460 / 710 / 735 | 12 |

Propagation is about 2.2 round trips at 50 ms (110 ms) and 3 round trips at 150 ms (about 450 ms): a write triggers a session
(open stream, hello, status, then p2panda-sync's Have / PreSync / Operation exchange). The spec's 500 ms (the Matrix value, kept
for this binding in Spike C because loopback needed 17 ms) is exceeded by half the writes at 150 ms RTT and by outliers at 5 %
loss. Thirty writes per cell is a small sample; the maxima are lower bounds.

### 4. The relay is killed (item 3)

`relay-down` sends SIGKILL to `iroh-relay`; the peers keep running. Outage lengths are 20 s ×scale.

**Already direct** (upgraded first; `relay_killed_while_the_connection_is_direct`): in 7 of 7 conditions the same sync connection
was still open and still direct after the relay had been dead for 24 to 126 s, and six publishes alternating in both directions
all arrived (visible after 101 to 103 ms unshaped, 304 ms at 50 ms RTT, 610 to 914 ms at 150 ms RTT, 811 to 3,456 ms in `worst`); a
peer that went offline and back re-dialled the other with the relay dead and was current in 104 ms to 1.0 s (iroh kept the
peer's addresses in the endpoint; a **fresh process** that never knew the peer's address could not do this, and was not tested).
iroh logged 11 to 15 failed relay reconnects per node meanwhile (`relay_conns_failed`).

**Relay-only** (`relay_killed_while_the_connection_is_relay_only`): a publish at a did **not** reach b while the relay was dead
in 7 of 7 conditions (the control). After the relay returned the publish arrived after:

| condition | unshaped | rtt50 | rtt150 | loss1 | loss5 | rate1m | worst |
|---|---|---|---|---|---|---|---|
| relay dead for (s) | 20 | 30 | 41 | 31 | 60 | 60 | 100 |
| sync connections closed meanwhile (of those open) | 0 | 2 | 2 | 1 | 3 | 2 | 2 |
| arrived after the relay returned (s) | 12.3 | 3.5 | 17.9 | 19.9 | 18.1 | 8.6 | 22.6 |

A relay-only connection survives a relay outage shorter than 30 s (iroh's `RELAY_PATH_MAX_IDLE_TIMEOUT`); after that QUIC closes
it and dcp-p2p's anti-entropy dials again, which then waits for the relay clients to reconnect (3.5 to 22.6 s here). The publish
arrived in every case.

**A silent cut of an upgraded path** (`direct_path_blocked_mid_session_...`: UDP between the peers is dropped by the firewall while
the connection is direct; the relay stays up). Seconds after the block:

| condition | unshaped | rtt50 | rtt150 | loss1 | loss5 | rate1m | worst |
|---|---|---|---|---|---|---|---|
| iroh selects the relay path | 19.9 | 15.0 | 15.4 | 19.9 | 19.9 | 15.0 | 15.1 |
| the next publish is visible | 21.2 | 15.6 | 16.9 | 21.2 | 21.2 | 17.5 | 17.9 |

Nothing was lost, but for 15 to 21 s every publish and lease write sat unseen. After the firewall was opened again iroh selected
the IP path again within 12 ms to 1.9 s. The 15 s is iroh's `PATH_MAX_IDLE_TIMEOUT` (`socket.rs`); the extra 5 s in 3 of 7
conditions (those with no delay and no rate limit) was not explained.

### 5. The cost of p2panda-spaces state growth over the shaped link (item 4)

N inline updates of 100 bytes from one writer to one reader (both editors); the reader's CPU time is read from `/proc/self/task`
(CLK_TCK 100) in the node process, in total and for the store's own thread (named `dcp-p2p`, where p2panda runs), and the bytes
from the reader's veth counters. 1,000 entries only in the conditions marked big (`unshaped`, `rtt150`, `rate1m`, `worst`). CPU model of these jobs: EPYC 7763 for
unshaped, rtt50, rtt150, loss5, worst; EPYC 9V45 for loss1 and rate1m.
Direct mode shown; relay-only gave the same picture (its wait differed from direct by up to 42 %, `worst` at N = 300: 29.3 s
against 20.7 s; all rows are in `results.jsonl`). "CPU after the last publish" below is the reader's CPU during the wait.

| condition | N | reader complete after the last publish (s) | reader CPU in that wait (s) | of which store thread (s) | reader veth rx (MB) | the same bytes at 1 Mbit/s (s) |
|---|---|---|---|---|---|---|
| unshaped | 100 | 0.57 | 0.57 | 0.53 | 3.7 | |
| unshaped | 300 | 5.8 | 5.7 | 5.4 | 40.3 | |
| unshaped | 1,000 | 64.4 | 66.0 | 63.0 | 438.0 | |
| rtt150 | 100 | 1.7 | 0.65 | 0.60 | 0.9 | |
| rtt150 | 300 | 5.2 | 4.5 | 4.3 | 4.5 | |
| rtt150 | 1,000 | 49.3 | 50.2 | 48.1 | 81.9 | |
| loss5 | 300 | 5.3 | 5.3 | 5.0 | 32.0 | |
| rate1m | 100 | 5.7 | 0.43 | 0.40 | 0.73 | 5.8 |
| rate1m | 300 | 15.7 | 2.6 | 2.5 | 2.0 | 16.0 |
| rate1m | 1,000 | 60.1 | 31.2 | 29.8 | 7.2 | 57.9 |
| worst | 300 | 20.7 | 4.4 | 4.2 | 1.6 | 12.8 |
| worst | 1,000 | 99.1 | 48.0 | 45.9 | 6.1 | 48.6 |

Reading it:

- **Delay and loss do not change what limits the reader.** At 50 and 150 ms RTT, 1 % and 5 % loss, and at 300 and 1,000 entries,
  the reader's CPU time during the wait is 79 to 102 % of the wait (87 to 102 % in direct mode; the store thread alone 75 to
  98 %): it is CPU-bound, as on loopback (Spike C: 81 s for 1,000; here 64 s on a different machine). The store thread
  is 95 to 96 % of the reader's CPU. At 100 entries and 150 ms RTT the wait (1.7 s) is round trips, not CPU (0.65 s).
- **At 1 Mbit/s the link limits it.** The wait equals the time the received bytes need at 1 Mbit/s (5.7 s against 5.8 s,
  15.7 against 16.0, 60.1 against 57.9) and the CPU is busy for 8 %, 17 % and 52 % of it at N = 100, 300 and 1,000.
- **In `worst` (150 ms, 5 % loss, 1 Mbit/s) the two add up** (99 s wall for 48 s of link time and 48 s of CPU): QUIC under loss
  and delay on a thin link did not keep the reader fed.
- **The bytes are not the state.** The state is 2.1 MB at N = 1,000 but the reader received **438 MB** unshaped, 82 MB at 150 ms RTT
  and 7 MB at 1 Mbit/s, and its total CPU for the same final state was 72.8 s unshaped, 53.3 s at 150 ms RTT and 48.0 s in `worst` (all on the
  same EPYC 7763; the 31 s at 1 Mbit/s is on a faster CPU and not comparable). The counters added for the follow-up run
  (below) show where the bytes go.

**State-growth counters** (follow-up run [40192160497](https://github.com/tuna-os/gtk-office-suite/actions/runs/40192160497), commit
`f45846ac`, `[growth]` switch: the state-growth test alone in two conditions; the library code is that of the matrix run, only
the test's counters were added; different runners: **Intel Xeon 6973P-C** for unshaped, EPYC 9V45 for rate1m). Reader side, direct
mode; "received" is `Counters::ops_received` (operations decoded from sessions, duplicates included), "applied" `ops_applied`
(the 1,000 updates plus the document's own entries):

| condition | N | wait after last publish (s) | reader CPU in the wait (s) | reader veth rx (MB) | operations received | operations applied | sync sessions (reader / writer) |
|---|---|---|---|---|---|---|---|
| unshaped | 100 | 0.44 | 0.38 | 3.3 | 1,472 | 106 | 109 / 109 |
| unshaped | 300 | 3.7 | 3.7 | 34.6 | 15,574 | 306 | 268 / 268 |
| unshaped | 1,000 | 43.7 | 44.4 | 381.2 | **172,067** | 1,006 | 768 / 768 |
| rate1m | 100 | 6.0 | 0.38 | 0.8 | 270 | 106 | 9 / 12 |
| rate1m | 300 | 14.6 | 2.6 | 1.9 | 627 | 306 | 10 / 12 |
| rate1m | 1,000 | 56.7 | 31.0 | 6.4 | 2,803 | 1,006 | 53 / 53 |

(Relay-only: 149,606 received for 1,006 applied unshaped at N = 1,000, 1,959 at 1 Mbit/s.) **On a fast link each operation was
received 171 times**: 768 sessions for 1,000 publishes, each carrying the backlog the reader had not yet applied, at about 2,200
bytes an operation (381 MB / 172,067 = 2,215 B, the 2,170 B per operation of Spike C). At 1 Mbit/s a session takes long enough that
about 53 of them ran and each operation was received 2.8 times. [INFERENCE] The mechanism: a session sends what the receiver's
stored log heights lack, the reader's heights advance only as fast as it applies (O(N) per operation), and the writer starts a
session on every publish, so a fast writer re-sends an ever longer backlog; this was not traced in p2panda-sync. The same final
state took the reader 44 s here on a Xeon 6973P-C and 64 s on an EPYC 7763 in the matrix run, so absolute seconds in this
section depend on the CPU model; the ratios within a job do not.

So the 81 s of Spike C is a library cost (p2panda-spaces rewriting its state per message) that no network condition removes
at a fast link, and that a slow link merely hides behind the transfer time.

### 6. What a client needs outbound; can discovery be switched off (item 5, the RFC-0001 Flatpak question)

**Private relay, observed.** Node a's traffic in each of the 14 `wan` jobs, captured on its veth (`wire-summary.txt`,
`tcpdump -s 128`, `tshark`): only three kinds of destination appeared and **no DNS query, no other host** (each summary also
has one `icmp` line, an artefact: ICMP errors embed the header of the UDP packet they answer; the script now skips them):

```
tcp 10.77.0.1:443   (relay: the relay protocol over TLS; it also carries every relayed byte)
tcp 10.77.0.1:80    (relay: a captive-portal style probe, a few dozen packets)
udp 10.77.0.1:7842  (relay: QUIC address discovery, "QAD")
udp 10.77.N.2:<ephemeral>   (each peer: direct QUIC; a fresh random port per endpoint)
```

**TCP 443 alone is enough** (`ports_tcp_443_to_the_relay_is_enough`): with each peer namespace allowed *only* TCP to the relay's
port 443 (no UDP at all, no TCP 80, no UDP 7842), both nodes came online, synced a document and delivered a publish in 101 ms
(unshaped; 607 ms at 150 ms RTT; 1,724 ms in `worst`), over the relay, with iroh's path data showing relay only. The
firewall counters show what iroh tried and was refused: 2 TCP SYNs to port 80, 8 UDP packets to 7842, 9 UDP packets
to peers. **What it cost**: the node was ready after 3.0 to 3.6 s in all 7 conditions, against 0.42 s for start-up, document creation
and first sync together with everything allowed (the upgrade test, unshaped); the probes that cannot succeed have to time out
(inferred from the 3 s, not from a log). Control: with everything dropped a node does not come online
(`node_came_online: false` in 7 of 7 conditions).

**So a client needs**: TCP 443 to the relay (required to start, and for every peer that cannot be reached directly), and
UDP to its peers' addresses on any port (optional: it is what makes a connection direct; each endpoint binds one UDP socket,
the OS chooses the port unless the application sets one). UDP 7842 and TCP 80 to the relay are optional (faster start-up). With an
**IP-literal relay URL no DNS is needed**; a relay URL with a host name needs the system resolver (not tested here). In a Flatpak
app all of this is `--share=network` in `finish-args`: the permission is all or nothing, so these are the ports a site firewall,
not Flatpak, has to allow. iroh's builder has a `proxy_url` for relays behind an HTTP proxy; it was read, not run.

**Can discovery be disabled?** Yes, and dcp-p2p already does: `Endpoint::builder(presets::Minimal)` with `RelayMode::Custom` (or
`Disabled`) adds no address-lookup service. Captured in a namespace NATed to the internet
(`netns-footprint.sh`, run 40179059127, job `footprint`; the probe has two endpoints in one process and one dials the other):

| variant | packets the endpoints put on the wire | what they contacted |
|---|---|---|
| `minimal` (Minimal preset, no relay, dial by IP) | **0** (the script fails otherwise) | nothing |
| `n0-relay` (Minimal + n0's default relay map, dial by id and relay URL) | 309 | DNS (the system resolver) for the four relays; then TCP 443 and UDP 7842 to each of `aps1-1`, `euc1-1`, `use1-1`, `usw1-1.relay.n0.iroh.link` (5.223.65.62, 91.99.237.97, 87.99.151.110, 5.78.69.43), TCP 80 to two; home relay `usw1-1`; UDP to the other endpoint's public address seen by QAD |
| `n0` (`presets::N0`, dial by endpoint id alone) | 374 (the capture is not blind) | the same, plus `dns.iroh.link` (49.13.207.244): TCP 443 for the pkarr publish (`https://dns.iroh.link/pkarr`) and a DNS query for `_iroh.<z-base-32 endpoint id>.dns.iroh.link` to resolve the peer; the connection took 823 ms against 18 ms with the relay URL given |

With the `N0` preset each endpoint **publishes its id and its home relay URL (no IP address) to `dns.iroh.link`** (iroh's debug log
in `n0.log`: `Publishing endpoint info to pkarr data=EndpointData { addrs: [Relay(https://usw1-1.relay.n0.iroh.link./)] ...}`). A
private deployment must therefore use `Minimal` with its own relay and must not use `presets::N0`; the `relay` that dcp-p2p
takes as configuration already works this way. DNS-over-the-system-resolver was to Azure's resolver (168.63.129.16) from the
namespace; ICMP from the namespace to 1.1.1.1 got no answer on the runner, which does not matter for iroh.

## Spec corrections

Proposed; not applied (`docs/DOCUMENT-COLLABORATION-SPEC.md` is not owned by this spike). Section numbers are those of draft 1.

1. **8.5 "Measured once", sentence 3** ("No WAN, no NAT, no n0 relay: every timing below is loopback."): replace with *"Spike C's
   timings are loopback. A second run (RFC-0004 relay spike, CI run 40179059127) put the peers in separate Linux network
   namespaces behind an `iroh-relay` 1.4.1 with a test CA, links shaped by `tc netem` to 50 and 150 ms RTT, 1 % and 5 % loss and
   1 Mbit/s: there is still no NAT, no jitter, no real WAN and no n0 relay for dcp-p2p traffic."* All 11 journeys passed in both
   modes in all 7 conditions.
2. **8.5 "Measured" paragraph**, the sentence "a relay test synced a document, but whether the traffic stayed on the relay was not
   observed": replace with the new **Paths and relays** paragraph below.
3. **New paragraph, "Paths and relays."** *"A connection is direct or relayed as iroh's `Connection::paths()` says (`is_selected`,
   `is_ip`, `is_relay`); an implementation MUST NOT infer it from the addresses it was given. With UDP reachable between the
   peers, a connection given only a relay URL upgraded to a direct path about three round trips after the handshake (156 ms at
   50 ms RTT, 463 ms at 150 ms). With peer-to-peer UDP dropped, no IP path was ever opened (1,515 connections). An established
   direct connection survived the relay being killed (7 of 7 conditions); a relay-only connection delivers nothing while the
   relay is down and recovers 3.5 to 22.6 s after it returns. A direct path that is cut silently is abandoned after 15 s
   (iroh `PATH_MAX_IDLE_TIMEOUT`) and traffic moves to the relay (15.0 to 19.9 s in the runs; the next publish was visible
   after 15.6 to 21.2 s); a relay that stays dead for 30 s (`RELAY_PATH_MAX_IDLE_TIMEOUT`) closes relay-only connections. A
   binding MUST therefore treat 20 s as the time to notice a lost path and MUST NOT assume faster fail-over in L3."*
3b. **8.5 "Measured"**: Spike C's "20.9 ms with relay-only addresses against 20.5 ms with direct addresses" (spike-c-results.md, the
   latency table) compared two direct connections; drop the comparison from any text that quotes it.
4. **8.5 "Leases and time", the settle-interval sentence** ("the settle interval SHOULD be at least 500 ms ... not measured on a WAN"):
   replace with *"Propagation of a lease write between two nodes was a median of 110 ms at 50 ms RTT, 437 to 507 ms at 150 ms
   RTT (maximum 525 ms; 11 to 15 of 30 writes exceeded 500 ms), up to 662 ms at 5 % loss with no delay, and up to 955 ms at 150 ms,
   5 % loss and 1 Mbit/s. The settle interval SHOULD be at least 2 s unless every member is on one low-latency, loss-free
   network; 500 ms is enough only there."* Lease rule 3 itself says "500 ms" for Matrix; the p2p binding should state its own
   value. (The journeys pass with 500 ms at 150 ms RTT, so a passing journey is not evidence for the value.)
5. **8.5 "Availability"**: add the WAN figures: offline catch-up of five versions 53 ms unshaped, 363 ms at 50 ms RTT, 877 ms at 150 ms,
   1.4 to 2.5 s at 1 Mbit/s; two divergent heads visible everywhere 54 ms to 2.5 s after reconnecting (1.0 s where one lost round was needed); a 1 MiB file
   fetched in 0.5 s at 50 ms RTT, 1.4 s at 150 ms and 9 to 21 s at 1 Mbit/s.
6. **8.5 "Updates (L2) and scale"**: add *"The reader is CPU-bound without a rate limit (at 300 and 1,000 entries CPU time was 79 to 102 % of the wait, 95 % of
   it in the store's own thread) and network-bound at 1 Mbit/s (wait within 4 % of the transfer time of the bytes received);
   delay and loss alone do not change that. At 1,000 entries the reader needed 49 s (150 ms RTT) to 64 s (no shaping) and
   60 to 99 s at 1 Mbit/s. On an unshaped link the reader received 438 MB for 1,000 entries whose state is 2.1 MB."* (The
   counters run: each operation was received 171 times, 768 sessions for 1,000 publishes.)
7. **8.5 "Flatpak" sentence** ("reaching another device needs a UDP socket and, when peers are not directly reachable, a relay, both
   of which need `--share=network`"): replace with *"A client needs outbound TCP 443 to the relay and, to be direct, UDP to its
   peers' addresses on any port. UDP 7842 and TCP 80 to the relay are optional (blocking them made a node ready after 3.0 to
   3.6 s instead of under 0.5 s). With a relay URL given as an IP address no DNS is needed. The endpoint MUST be built with iroh's `Minimal` preset
   (as dcp-p2p is): the `N0` preset contacts `dns.iroh.link` and four n0 relays and publishes the endpoint id and home relay
   there. In Flatpak the permission is `--share=network`, which cannot be limited to ports."*
8. **8.5 "Not measured"**: replace "A WAN, NAT traversal, hole punching, production relays and address discovery" with
   "NAT traversal (all hole punching here crossed a routed network, not a NAT), jitter, a real WAN, production relays carrying
   dcp-p2p traffic, host-name relay URLs, IPv6"; keep "two devices in Flatpak".

## Not verified

- **NAT.** The namespaces are routed, not NATed. Every hole punch here was between peers that could reach each other's bound
  address; hole-punch *success* behind real NATs (port-restricted, symmetric, carrier-grade) is untested, and the 3-round-trip
  upgrade time is for a network where punching is trivially possible. `holepunch_attempts` was read but its success counterpart
  is only visible as the IP path appearing.
- **A real WAN.** netem here is a fixed delay (no jitter, no reordering), independent random loss (no bursts) and netem's `rate`
  (not a token bucket with a burst and a shared queue). A 5 % loss netem is not a lossy radio. One run per cell on a shared
  4-vCPU runner; the upgrade test repeats five times, the lease test thirty writes, the connect test three pairs.
- **dcp-p2p over the n0 production relays.** The footprint probe dialled between two endpoints in one process behind a NAT;
  it did not run the binding, nor measure latency or capacity of n0's relays.
- **Relay operation**: more than one relay and home-relay fail-over, the relay's access control (`access = ...`), its rate limits,
  ACME/Let's Encrypt, a relay URL with a host name (so DNS), IPv6, the relay under many clients, relay restarts that lose
  its state across a version change.
- **A fresh process after the relay died** with no address for the peer (only an endpoint that had learned the peer's address was
  tested after the relay was killed).
- **Flatpak**: nothing here ran inside an installed Flatpak on two devices; the port list comes from captures outside Flatpak.
  `HTTP proxy` (`proxy_url`) was read, not run. mDNS/local discovery is off in this binding and was not run.
- Spike C's **"not verified"** items that are not network items stand: three or more writers, concurrent membership changes,
  restart (all state is in memory), more than 1,000 entries, presence, the `commenter` role, a hostile peer, interop with other
  implementations; the late-joiner pull of `measure.rs` and the 32 MiB file were not ported to the namespaces.
- iroh's own **path and socket counters** are read as documented in the 1.4.1 source (`path_watcher.rs`, `socket/metrics.rs`);
  their definitions were not cross-checked against a packet capture byte for byte (only the destinations were). The "bytes on
  relay paths" of direct-mode connections is a sum over every path of a connection, selected or not.
- The extra 5 s in 3 of the 7 mid-session fall-back times and the fifth `worst` upgrade timestamp are unexplained; the
  mechanism of the 171-fold re-send (above) is an inference from the counters, not a trace of p2panda-sync.
- One failed sync session per unshaped state-growth run (`sync_failures` 1 at the reader in all four unshaped rows of the counters
  run) was not examined.
- **Hardware drift**: the CPU model differed between jobs (see the top) and the same state-growth test took 64 s and 44 s on two
  runners. Only ratios inside one job are comparable.
