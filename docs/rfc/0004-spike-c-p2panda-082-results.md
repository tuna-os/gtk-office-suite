# RFC-0004 Spike C, re-run on p2panda 0.8.2

Date: 2026-11-26 · Tracks: issue #2499 ("use p2panda for collaboration as gnome is going to use it in the future"), the
Renovate hold of #2084/#2085 · Re-measures: [Spike C](0004-spike-c-results.md) and the p2panda rows of the
[relay spike](0004-spike-relay-results.md), the findings that put iroh/p2panda out as "runner-up" in
[RFC-0005](0005-collaboration-in-the-apps.md) · Code: [`tools/collab-spike/dcp-p2p/`](../../tools/collab-spike/dcp-p2p/)

This is the evidence step only: the Spike C binding (`dcp-p2p`) ported from p2panda 0.7.1 to 0.8.2 with the smallest
changes that build and pass, then the same tests and measurements run again. Nothing in the product changes.

**Every number here comes from one local machine** (4 vCPU Intel Xeon @ 2.10 GHz, 15 GB, rustc/cargo 1.100.0),
not from CI, with all nodes on loopback in one process. Spike C's numbers came from a CI runner (EPYC 9V74), so
they are not comparable in absolute seconds. To compare like with like, **0.7.1 was measured again on the same
machine** with the same test binary source (the `main` code plus the counters below), before the port, and both
versions twice. One machine and two runs each is evidence, not a benchmark.

## Recommendation

**p2panda 0.8.2 does not clear the bar RFC-0005 used.** The reason RFC-0005 gave ("the reader's cost grows faster
than linearly, about 81 s at 1,000 entries") is unchanged: on this machine a reader needed **95.5 s and 102.0 s**
on 0.8.2 against **100.0 s and 100.0 s** on 0.7.1 to catch up on 1,000 entries, received **359 MB and 462 MB**
against **447 MB and 456 MB**, and held a 2.13 MB p2panda-spaces state on both. The cause named in Spike C is
still in the source: p2panda-spaces' encryption orderer keeps every message in the state it reads and rewrites
per message (`encryption/orderer.rs:44`, the same "TODO: We keep all messages in memory currently which is bad").

What did change, and matters for a later look:

- **Role change works** (finding 3, part 1). `Space::promote` and `Space::demote` exist and a role change through
  the binding is now one auth operation plus one key-agreement message instead of a removal and an addition
  (measured, below). `SpacesArgs::SpaceUpdate` is still `unimplemented!()` (`manager.rs:422`).
- **p2panda-spaces now authorises writes itself**: an application message carries a `proof` (auth heads) that the
  receiver checks (`space.rs:586`, `verify_claimed_write_access`), and the plaintext of an author who has since
  lost write access is withheld (`space.rs:612-634`). Two of the binding's own work-arounds are now duplicated
  upstream (see "Port notes").
- **p2panda-net can now gate sync** (`Authoriser`, allow- and blocklists, and a `SyncHook` that accepts or rejects
  a session), which was one of the two reasons it was not used. The other is unchanged:
  `SyncHandle::initiate_session` is still `#[cfg(test)]` (`src/sync/handle.rs:99`).
- **p2panda-blobs is still an empty crate** (`src/lib.rs` is the `cfg_attr` line and
  `// TODO: Needs refactoring since p2panda-net refactor.`).
- Content published by a member that has not applied a removal is **still readable** by the removed member
  (finding 2, measured). That is the nature of a serverless group key, not a library defect, so no version will
  change it.

So the decision RFC-0005 took stands. If GNOME's direction (#2499) is the reason to come back, the thing to watch
upstream is orderer persistence and pruning in p2panda-spaces (the TODO above), and a public way to start a sync
with a given peer in p2panda-net; and the high-level `p2panda` node API, which 0.8 rebuilt around encrypted spaces
and which this binding does not use, would deserve its own spike.

## Versions

| Crate | Spike C | Now | Note |
|---|---|---|---|
| p2panda-core, -store, -sync, -auth, -encryption, -spaces | 0.7.1, `=` pinned | **0.8.2** (2026-11-25), `=` pinned | 0.8.0 2026-11-09, 0.8.1 2026-11-13 |
| p2panda-net, -blobs, -stream, `p2panda` | not used | 0.8.2, read, not used | sources read from the crates.io tarballs |
| sqlx / libsqlite3-sys | 0.8.6 / 0.30.1 | 0.9.1 / 0.30.1 | `libsqlite3-sys` is unchanged, so the crate stays its own workspace |
| iroh / noq / iroh-blobs | 1.4.1 / 1.4.0 / 0.103.1 | 1.4.1 / 1.4.0 / 0.105.0 | iroh-blobs was 0.105.0 on `main` before this port; the 0.7.1 re-measurement below used it too |
| rustc / cargo | 1.99.1 (CI) | 1.100.0 (local) | |

Resolved from `dcp-p2p/Cargo.lock` (committed with the port).

## What changed upstream (0.7.1 to 0.8.2)

From the [CHANGELOG](https://github.com/p2panda/p2panda/blob/main/CHANGELOG.md) (sections 0.8.0, 0.8.1, 0.8.2) and
the 0.8.2 sources, against the four findings RFC-0005 cites:

| Finding (RFC-0005, measured on 0.7.1) | Upstream in 0.8.x | Addressed? |
|---|---|---|
| 1. Reader catch-up: ~81 s for 1,000 entries, 438 MB received for a 2.1 MB state, CPU-bound | No entry about spaces state size or orderer pruning. "spaces: Atomic read -> modify -> write state handling" (#1513) keeps the per-message rewrite. "sync: Memory-leak caused by not cleaned-up sync sessions" and "Dropped operations during sync when channel ran full" (#1612) are fixes, not a cost change. The orderer TODO is still at `encryption/orderer.rs:44` | **No** (measured below) |
| 2. Content from a member that has not applied a removal is readable by the removed member | "stream: Filter out spaces application messages from concurrently removed members" (#1291) and "spaces: Validate write authority when processing application messages" (#1295) are about the *author* of a message, not about who can decrypt it | **No** (measured below; not fixable without a server) |
| 3a. p2panda-spaces role change `unimplemented!()` | "node: API for promoting and demoting space members" (#1311); `Space::promote`/`demote` (`space.rs:212`, `228`); the encryption mapping now handles Promote as Add and Demote as Remove (`encryption/message.rs:130`, `146`). `SpaceUpdate` still `unimplemented!()` (`manager.rs:422`) | **Yes** for promote/demote (measured below) |
| 3b. p2panda-blobs an empty crate | `src/lib.rs` unchanged; the other 560 lines still compile into nothing | **No** |
| 3c. p2panda-net not usable as published (no per-peer sync, no membership gate) | "net: `SyncHook` intercepting sync sessions to accept or reject them" (#1451), "net: Introduce an `Authoriser` for maintaining and enforcing allow- and blocklists" (#1321), "node: ConnectionAuthoriserHook updating authoriser based on space membership" (#1359), "Sync session management re-write" (#1612); iroh 1.5.0. `initiate_session` is still `#[cfg(test)]` (`src/sync/handle.rs:99`). `codec` is `pub mod` (`src/lib.rs:243`) | **Partly**: gating yes, driving a session with a chosen peer no |
| 4. Leases advisory; `commenter` unsupported | `AccessLevel` is still `Pull, Read, Write, Manage` (`p2panda-auth/src/access.rs:17`). Advisory leases are a property of having no coordinator | **No** |

Other 0.8 changes the port had to follow: p2panda-core signs with a `Signer` trait (#1202); p2panda-store moved to
sqlx 0.9 and `AnyOperation` on `LogStore` (#1322, #1391) and renamed write methods with a `_tx` suffix (#1515);
p2panda-spaces persists its own state in one transaction per call, returns output structs (#1453), dropped the
resolver type parameter (#1298), renamed `SpaceMembership` to `KeyAgreement` (#1596) and replaced the key-bundle
message with `SpacesArgs::Member` (cross-signed, #1332); p2panda-sync's `LogSync` takes a bounded `mpsc` sender
instead of a `broadcast` one.

## Port notes

`cargo check --all-targets`, with and without `--features relay-test`, gives no errors and no warnings after these
changes. Commit 943317094 (#2085) recorded "14 errors from reshaped store, spaces and signing APIs" for the bump.

| Where | 0.7.1 | 0.8.2, and what the port does |
|---|---|---|
| `Cargo.toml:25-31` | `=0.7.1` | `=0.8.2` for the six crates |
| `src/msg.rs:30` | | `impl From<Operation<DcpExt>> for DcpMsg`: p2panda-spaces 0.8 reads messages back through `SpacesMessageStore`, which needs it |
| `src/msg.rs:113-133` (`DcpForge::append`) | the forge began, committed or rolled back its own transaction | **No transaction.** 0.8 calls the forge inside its own state transaction on the same `SqliteStore` ("make sure the forge itself is running within the same transaction", `store.rs`); a second `begin` would wait for ever. `get_latest_entry_tx` is now on `LogStore<AnyOperation, ..>`; `insert_operation` is `insert_operation_tx`; the header builder takes `&impl Signer` |
| `src/replica.rs:41` | `Manager<S, F, (), StrongRemoveResolver<()>>` | `Manager<S, F, ()>` |
| `src/replica.rs:163-168` | `Doc::persist` wrote the returned groups and space states | `Doc::state_changed` only drops the binding's cached copy: 0.8 persists its own state |
| `src/replica.rs:173-180` | read the space state inside `begin`/`commit` | committed read with `get_space_state` |
| `src/replica.rs:190-195` | `manager.space(id) -> Option<Space>` | `has_space` then `space(id).created()`, which resolves at once for a known space |
| `src/replica.rs:150-154`, `352-362`; `src/ops.rs:57-61` | `manager.me()` made the pre-keys; `key_bundle_message()` forged a card on each call | `rotate_key_bundle()` makes the pre-key and forges the card once, into the keys log, in `new_doc`; the card is kept on the `Doc` |
| `src/ops.rs:65` | `SpacesArgs::KeyBundle` | `SpacesArgs::Member(_)` |
| `src/ops.rs:91-101`, `225-253`, `276-310` | tuples of states and messages | `SpaceOutput.messages`; `take_output` marks them applied and returns the key-agreement message's hash (what `revoke` waits for peers to apply) |
| `src/ops.rs:154-180`, `255-274` (`grant`, `change_role`) | a role change was `remove` then `add` (two key rotations), because promote/demote panicked | **Behaviour change:** `Space::promote` or `Space::demote`. This is the one place the port uses something new, because it is finding 3 |
| `src/ops.rs:333-341` | `groups_y.inner.heads()` | `groups_y.heads(&[state.group_id])` (the method is now on the state, per group) |
| `src/ops.rs:356` | `publish -> (state, msg)` | `publish -> (msg, event)` |
| `src/ops.rs:761-779` (`raw_op_headers`, a test hook) | `get_log_entries` | the `log_entries` stream, which carries the stored header bytes |
| `src/apply.rs:90-105` | an operation was ready when its `dependencies()` were applied | also waits for an application message's `proof`: 0.8 checks the proof when it processes the message and `dependencies()` does not list it, so a message that arrived before its proof's auth operations would be refused for good |
| `src/apply.rs:116`, `136-151` | refused `Promote`/`Demote` and matched `Auth`, `SpaceMembership`, `KeyBundle` | processes `Promote`/`Demote`; matches `Group`, `KeyAgreement`, `Member`; still refuses `SpaceUpdate` (still `unimplemented!()`) |
| `src/apply.rs:31`, `175-176` | `insert_operation`; `process -> (groups, space, events)` + persist | `insert_operation_tx` (inside the binding's own batch transaction, as before); `process -> events` |
| `src/net.rs:259-281` (`run_session`) | `broadcast` channel drained after the session | bounded `mpsc` channel drained **while** the session runs (`tokio::join!`), since `LogSync` now awaits room on it |
| `tests/header_limit.rs:27-28` | | the new `proof` field. The 512-element header limit is still there (`p2panda-core/src/operation/any.rs:203`), so `ext.rs`' chunking stays |
| `tests/p2p.rs:327-340` | asserted the owner *decrypted* the removed editor's entry and the binding's rule refused it | asserts what 0.8 does instead: the operation is received and processed (applied 1, pending 0, rejected 0) and p2panda-spaces emits no plaintext for it (entries 2 -> 2). The test's point (a removed editor's version is listed by nobody) is asserted unchanged |
| `tests/p2p.rs:405-448` | | **New:** `a_role_change_is_one_promote_or_demote_operation` |
| `tests/measure.rs` | | (separate commit) `convergence_of_n_entries` also prints the reader's iroh socket bytes received, the process CPU during the wait and the replica's `ops_received`, `ops_applied`, `sync_sessions`, the in-process stand-ins for the relay spike's veth and `/proc` counters |

Not done, as asked: replacing `src/codec.rs` with `p2panda_net::codec` (now public, but depending on p2panda-net
pulls iroh 1.5, iroh-gossip and ractor for 81 lines), replacing iroh-blobs with p2panda-blobs (empty), and using
p2panda-net's driver (no `initiate_session`).

**The negative control still fails the journey, but only at the forger's own replica.** With the binding's reader
rule off (`authorize = false`), `forged_version_from_a_viewer_is_not_accepted` fails with
`"viewer: a viewer's forged version was accepted by a reader"`: the journey checks owner, editor and viewer in that
order, and the owner and editor no longer accept the forgery, because p2panda-spaces 0.8 refuses an application
message whose author has no write access at its claimed `proof`. Only the viewer's own replica, which adds its own
entry locally without receiving it, lists it. So on 0.8 the network-level gate is p2panda's, and the binding's rule
(`auth_check.rs`) is a second gate that also covers a replica's own writes.

## Tests

Debug builds with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`, in `tools/collab-spike/dcp-p2p`:

| Command | Result |
|---|---|
| `cargo test --test conformance -- --nocapture --test-threads=1` | **2 passed**: all 11 section-12 journeys pass, 0 skipped; the negative control fails the forged-version journey as it must |
| `cargo test --test p2p -- --nocapture --test-threads=1` | **15 passed** (the 14 of Spike C and the new role-change test) |
| `cargo test --test header_limit` | **5 passed** |
| `cargo test --test measure` | 5 ignored (measurements, run in release below) |
| `cargo test --features relay-test --test relay -- --nocapture` | **1 passed** (in-process relay; as in Spike C the selected path was IP, not the relay) |
| `cargo test --test wan` | **20 failed, not run**: every test panics at `tests/wan.rs:115` because the network namespaces `dcp-a`... do not exist. They need root, `ip netns` (this machine has no `ip` binary) and an `iroh-relay` server (`scripts/netns-wan.sh`); CI runs them only in the relay workflow. With `DCP_SKIP_NETNS=1` they report 20 skipped. No relay-spike (shaped-link) number was re-measured |
| release: `cargo test --release --test p2p -- --nocapture --test-threads=1` | **15 passed** |

The first full run failed two tests, both fixed in the port (not by weakening what they prove): the removed-editor
test asserted the 0.7.1 mechanism (see the table), and the new role-change test counted the promoted member's own
version as a demotion operation (it now counts from after that version has arrived).

## The findings, old against new

### Finding 1: reader catch-up on 1,000 entries

Command (release, one test at a time, as Spike C):

```
cargo test --release --test measure -- --ignored --test-threads=1 --nocapture convergence_of_n_entries
```

For 0.7.1 the same test binary, built from `main` plus the counters commit before the port, was run on the same
machine, twice each, alternating. N inline updates of 100 bytes, one writer, one reader, both editors:

| N = 1,000 | Spike C (CI, 0.7.1) | Relay spike (CI, 0.7.1, unshaped) | 0.7.1 here, run 1 / run 2 | **0.8.2 here, run 1 / run 2** |
|---|---|---|---|---|
| reader complete after the last publish | 81.3 s | 64.4 s (EPYC 7763), 43.7 s (Xeon 6973P-C) | 100.0 s / 100.0 s | **95.5 s / 102.0 s** |
| bytes received by the reader | not measured | 438.0 MB (veth rx) | 447.0 MB / 456.4 MB | **359.0 MB / 461.9 MB** |
| of which during the wait | | | 396.1 MB / 406.1 MB | 342.3 MB / 445.5 MB |
| CPU during the wait (whole process, both nodes) | | 66.0 s (reader process) | 129.2 s / 129.1 s | 116.4 s / 140.5 s |
| operations received by the reader / applied | | 172,067 / 1,006 | 156,527 / 1,006 and 122,726 / 1,006 | 95,711 / 1,006 and 80,475 / 1,006 |
| sync sessions, reader / writer | | 768 / 768 | 440 / 860 and 322 / 863 | 224 / 610 and 188 / 697 |
| persisted p2panda-spaces state, reader / writer | 2.13 MB / 43.6 kB | | 2,127,427 / 43,605 B | **2,129,436 / 43,614 B** |
| bytes per stored operation (100-byte payload) | ~2,170 | | 2,161 / 2,162 | 2,204 / 2,204 |
| writer publish of 1,000 | 5.7 s | | 7.2 s / 6.9 s | 5.2 s / 4.1 s |

Smaller N, the same runs (reader complete after the last publish): N = 100: 0.7.1 677 / 675 ms, 0.8.2 641 / 647 ms;
N = 300: 0.7.1 6,757 / 6,729 ms, 0.8.2 7,188 / 6,773 ms. A late joiner pulling 308 operations in one session:
0.7.1 4,560 ms, 0.8.2 4,272 ms (`late_joiner_pulls_n_entries_in_one_session`).

0.8.2 output, run 1 (`MEASURE` lines verbatim):

```
MEASURE n=1000: reader iroh socket bytes received 358978049 since the first publish, 342301407 during the wait; process CPU during the wait 116410 ms (both nodes); reader ops_received 95711 ops_applied 1006 sync_sessions 224 (writer sync_sessions 610)
MEASURE n=1000: persisted p2panda-spaces state 43614 bytes at the writer, 2129436 bytes at the reader
MEASURE n=1000: publish 5171 ms (5.171 ms per entry at the writer), reader complete yes after 95509 ms past the last publish; 1006 operations stored, 2218032 header bytes in total = 2204 bytes per operation (payload 100 bytes)
```

0.7.1 output, run 1, same machine:

```
MEASURE n=1000: reader iroh socket bytes received 447018045 since the first publish, 396051163 during the wait; process CPU during the wait 129170 ms (both nodes); reader ops_received 156527 ops_applied 1006 sync_sessions 440 (writer sync_sessions 860)
MEASURE n=1000: persisted p2panda-spaces state 43605 bytes at the writer, 2127427 bytes at the reader
MEASURE n=1000: publish 7199 ms (7.199 ms per entry at the writer), reader complete yes after 99998 ms past the last publish; 1006 operations stored, 2174707 header bytes in total = 2161 bytes per operation (payload 100 bytes)
```

Reading it: **no change beyond run-to-run noise.** The two 0.8.2 runs bracket the two 0.7.1 runs (95.5 and 102.0 s
against 100.0 and 100.0 s; 359 and 462 MB against 447 and 456 MB). The reader's state is the same 2.13 MB, the
cost per entry still grows with N (about 6.4 ms per entry at 100, 23 ms at 300 and 95 to 102 ms at 1,000), and the
bytes received are still about 170 to 220 times the state. The "bytes received" here are iroh's socket counters
in the reader's process (UDP payload on loopback), not a veth counter, so they are close to, not the same
quantity as, the relay spike's 438 MB. The absolute seconds are higher than Spike C's 81 s because the machine is
slower; the same 0.7.1 code took 100 s here.

### Finding 2: a removed member reads what a member that had not applied the removal published

Command: `cargo test [--release] --test p2p -- --nocapture --test-threads=1`, test
`content_from_a_member_that_has_not_seen_the_removal_is_readable_by_the_removed_member` (`tests/p2p.rs:346`,
unchanged by the port).

| | 0.7.1 (Spike C) | **0.8.2** |
|---|---|---|
| removed viewer listed and read the version | yes (release); in two debug runs it did not, a binding race since fixed | **yes, in debug and in release** |
| `revoke()` returned after | 3 ms (release) / 49 ms (debug) | 4 ms (release) / 38 ms (debug) |

```
test content_from_a_member_that_has_not_seen_the_removal_is_readable_by_the_removed_member ... MEASURE revoke with the only other member unreachable returned after 4 ms
MEASURE removed member read a version published after its removal by a member that had not applied it: yes
```

The test asserts that the removed viewer fetches the plaintext (`assert_eq!(viewer.fetch_file(..), secret)`), so
"yes" means the bytes were read. The other revocation tests are unchanged: the removed member still receives later
ciphertext and decrypts none of it (`a_revoked_member_receives_ciphertext_but_cannot_decrypt_later_entries`), and
the blob gate refuses it (`io: connection lost`).

### Finding 3: role change, p2panda-blobs, p2panda-net

**Role change: yes.** New test `a_role_change_is_one_promote_or_demote_operation` (`tests/p2p.rs:410`): the owner
promotes the viewer to editor, the promoted member publishes a version that the third member (the editor) lists,
the owner demotes it back, and a publish by the demoted member is refused with `Forbidden`.

```
test a_role_change_is_one_promote_or_demote_operation ... MEASURE promote viewer -> editor: 2 new operations at the owner
MEASURE demote editor -> viewer: 2 new operations at the owner
MEASURE role change through p2panda-spaces promote/demote: works
```

Two operations is one `Group` message (promote or demote) and one `KeyAgreement` message. The 0.7.1 binding needed
four (a removal and an addition, each two) and two key rotations; Read <-> Write changes no secret membership, so
0.8 makes no key rotation for it (`space.rs:543-575`). The conformance journey `roles_enforced_at_store`, which
promotes and demotes a member, passes with this path. On 0.7.1 the same call would reach `unimplemented!()`; that
was read, not run, then and was not run now.

**p2panda-blobs: still empty** (read: `p2panda-blobs-0.8.2/src/lib.rs`). **p2panda-net: partly** (read, not run):
it can now refuse sync sessions and connections by an allow/blocklist or a hook, which answers "sync serves any
peer that knows the topic"; it still cannot be asked to sync with a given peer outside its own tests.

### Finding 4 (not re-measured)

Unchanged in source: `AccessLevel` has no comment level, and a lease without a coordinator is advisory. The lease
tests pass on 0.8.2 (`partitioned_sessions_both_acquire_the_lease_and_the_merge_picks_one`: both acquired while
apart, one holder 126 ms after healing in debug; `leases_work_with_clocks_a_minute_apart` and its negative control
as before).

## Not verified

- **CI.** Nothing here ran on the CI runners Spike C used; `collab-spike.yml` was not run on this branch.
- **The relay spike's shaped links** (`tests/wan.rs`): not run, see Tests.
- **More than two runs** of the 1,000-entry measurement per version, and other machines.
- Where the reader's CPU goes between decoding re-sent operations and p2panda-spaces' per-message state rewrite was
  not separated, on either version.
- p2panda-net's `Authoriser` and `SyncHook`, the high-level `p2panda` node API with encrypted spaces, and
  `ConnectionAuthoriserHook`: read in the CHANGELOG and source, not built or run.
- Renovate still holds the p2panda crates below 0.8 (`renovate.json`, "allowedVersions": "<0.8", until #2084).
  This branch moves them to 0.8.2 and does not change that rule.
