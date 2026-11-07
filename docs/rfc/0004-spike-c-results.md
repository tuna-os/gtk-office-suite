# RFC-0004 Spike C: iroh and p2panda binding results

Date: 2026-10-24 · Tracks: [RFC-0004](0004-interoperable-document-collaboration.md), [DCP draft 1 §8.4](../DOCUMENT-COLLABORATION-SPEC.md) · Code: [`tools/collab-spike/dcp-p2p/`](../../tools/collab-spike/dcp-p2p/) (branch `spike/collab-c`) · Workflow: [`collab-spike.yml`](../../.github/workflows/collab-spike.yml) · Raw data: CI artifacts `spike-c-check`, `spike-c-test`, `spike-c-measure`, `spike-c-relay`, `spike-c-flatpak`, `spike-c-upstream` of run [40139997921](https://github.com/tuna-os/gtk-office-suite/actions/runs/40139997921)

**Every number here comes from CI runs on a shared 4-vCPU GitHub-hosted runner (AMD EPYC 9V74,
16 GB, rustc 1.99.1) with all nodes on loopback, in one process or one sandbox.** There was no WAN
and no NAT. The journeys, the p2p tests and the header test ran in debug builds; the measurements
in the "Latency and throughput" section ran in release builds, and the p2p tests were run again in
release. They say what this binding and these crate versions did on our workloads; they are not a
benchmark of iroh or p2panda. Where a statement comes from reading source and was not run, the text
says so ("read, not run").

## Recommendation

**A conditional go for L0 and the version-carrying half of L1, and a go with a size limit for L2;
a no-go for the `commenter` role, for exclusive leases, and for using p2panda-net, p2panda-blobs or
p2panda-spaces' role-change API as they are.** DCP draft 1 can have a peer-to-peer binding on iroh
and p2panda 0.7.1, but only with the text proposed under [Proposed 8.5](#proposed-85), which differs
from what the RFC hoped in six ways that matter. The load-bearing results:

1. **The journeys pass.** `P2pStore` passes all 11 section-12 journeys of `dcp-conformance`
   through `run_all`, none skipped, with three iroh nodes on loopback and no relay or external
   service. A negative control (reader authorisation off) makes the forged-version journey fail for
   the intended reason, so the journey can fail.
2. **Most of "the p2panda stack" is not usable as published.** p2panda-blobs 0.7.1 is an empty
   crate; p2panda-net's sync cannot be driven per peer or gated by membership; p2panda-spaces 0.7.1
   cannot change a role (`unimplemented!()`), reports decrypted data without an author, keeps every
   message in the state it rewrites on each message, and needs one manager per document;
   p2panda-core 0.7.1 refuses a header with any byte string over 512 bytes, which silently breaks
   its own spaces messages beyond about 500 bytes. What works: p2panda-core's signed logs,
   p2panda-sync's `LogSync` protocol, p2panda-auth's group CRDT, p2panda-encryption's group key
   agreement, and iroh-blobs. The binding is those pieces plus about 3,300 lines of ours.
3. **A removed member cannot decrypt what is published after the removal, but only if the publisher
   has applied the removal.** With the membership gate switched off, the removed member's node
   received the later operations and decrypted none of them. A member that had not yet seen the
   removal published to the old key, and the removed member read it (measured). Revocation without a
   server is "after every reachable member has acknowledged"; `revoke()` waits for that.
4. **Leases cannot be exclusive without a coordinator.** Two partitioned sessions both acquired
   the lease and both read it back twice; after the partition healed the deterministic winner rule
   left one holder within one sync (measured). The binding offers an advisory lease with a Lamport
   order, and stamps it with the reader's own first-seen time so no two clocks have to agree
   (measured with clocks 60 s apart, with a negative control that fails).
5. **Availability needs overlap, not an always-on server.** A version published by a member who goes
   offline reaches another member through any third member that was online with both; if its only
   holder is offline it reaches nobody, and arrives when the holder returns (measured).
6. **p2panda-spaces 0.7.1 does not scale in entries.** A reader needed 0.59 s for 100 entries, 6.1 s
   for 300 and 81 s for 1,000 after the last publish, because the persisted state it rewrites on every
   message grows by the size of every message (2.1 MB at 1,000). Versions are fine; inline updates
   past a few hundred per document are not, on this library.

## Method

**Versions.** Pinned in `dcp-p2p/Cargo.toml` and recorded by CI from the resolved lockfile
(artifact `spike-c-check`):

| Crate | Version | Note |
|---|---|---|
| iroh (default features off, `tls-ring`) | **1.4.1** (2026-10-21) | 1.0.0 was released 2026-06-15; 1.4.0's notes: rustls security updates, better local-only networking |
| iroh-base / noq (QUIC) | 1.4.1 / 1.4.0 | |
| iroh-blobs | **0.103.1** (2026-10-06) | 0.103.0 is yanked: any peer could push blobs into a provider's store ([blog](https://www.iroh.computer/blog/iroh-blobs-push-fix)); its README says "not yet considered production quality" |
| p2panda-core, -store, -sync, -auth, -encryption, -spaces | **0.7.1** (2026-08-21), `=` pinned | pre-1.0, "APIs are not yet considered stable" |
| cbor-core / sqlx / libsqlite3-sys | 0.10.1 / 0.8.6 / 0.30.1 | |
| rustc / cargo | 1.99.1 / 1.99.1 on the runner | everything built with `cargo` 1.99 |

Not used: p2panda-net (see below), p2panda-blobs, p2panda-stream, the `p2panda` umbrella crate,
iroh-gossip, iroh-docs.

**Does the published stack build and pass on this toolchain?** p2panda-net 0.7.1 is `iroh = "1.0.3"`
based. Its own end-to-end test (`tests/e2e.rs`, gossip plus sync between two nodes on loopback),
built from the crates.io source on rustc 1.99.1 with the iroh cargo resolved for it (**1.0.3**), passed in 2.2 s
(job `upstream`, artifact `spike-c-upstream`). My own crate builds against iroh 1.4.1 and
iroh-blobs 0.103.1.

**The crate is its own workspace.** `dcp-p2p` cannot join the shared `tools/collab-spike`
workspace: `libsqlite3-sys` has `links = "sqlite3"`, p2panda-store needs sqlx 0.8 (libsqlite3-sys
^0.30) and `dcp-matrix` needs matrix-sdk-sqlite via rusqlite 0.40 (libsqlite3-sys ^0.38); cargo
allows one per resolution (first CI run, job `check`: "failed to select a version for
`libsqlite3-sys`"). The parent `Cargo.toml` now has `exclude = ["dcp-p2p"]` and the crate has its own
`Cargo.lock` (generated in CI; integration must commit it).

**What the binding is** (`dcp-p2p`, 3,272 lines of `src/`, 1,206 of tests):

- One `P2pStore` per principal. A principal is an Ed25519 key that is at the same time the iroh
  endpoint id (`SecretKey::from_bytes(signing_key.as_bytes())`), the p2panda author and the
  p2panda-auth actor, so the QUIC handshake authenticates the author.
- Each store runs on one OS thread with a current-thread runtime and a `LocalSet`, because the
  p2panda-spaces and p2panda-store traits return futures with no `Send` bound; the trait methods
  send a request to that thread.
- **Documents.** One document is one p2panda-spaces *space* with its own manager and its own
  SQLite database (see "What p2panda provides"). The document id is 32 random bytes (hex).
- **Entries.** Every DCP object is one p2panda `Operation` whose payload is a body that the
  *author signs* and p2panda-spaces then encrypts to the group: `{author, doc, heads, ts_us, nonce,
  env}`. The entry id is the BLAKE3 hash of the body. Meta, version, lease and update are
  `env` variants carrying the draft-1 objects unchanged.
- **Sync.** A session is one QUIC bidirectional stream between two members: a `Hello` naming the
  document, one status byte (the acceptor applies the membership rule), then p2panda-sync's
  `LogSync` protocol (Have / PreSync / Operation / Done). An anti-entropy loop (every 200 ms in the
  tests) and a trigger on every local publish start sessions with the document's other members.
- **Files.** iroh-blobs with an in-memory store; the file is sealed with a fresh XChaCha20-Poly1305
  key (p2panda-encryption's primitive) and the key travels inside the encrypted entry; a
  provider-side gate serves a blob only to a current member of a document that references it.
- **Roles and revocation.** p2panda-auth levels through p2panda-spaces: viewer = Read, editor =
  Write, owner = Manage. A reader-side rule (below) is what makes a role mean "may publish".
- **Time.** The store takes a `Clock`; `now_ms` is that clock; a lease is stamped with the time the
  reader first saw it on the reader's own clock.

**Why p2panda-sync over my own iroh sessions, and not p2panda-net's driver.** p2panda-net 0.7.1
would be the "whole stack" answer (endpoint, address book, discovery, gossip, `LogSync`). It was not
used because, in its source: `SyncHandle::initiate_session` is `#[cfg(test)]` only
(`src/sync/handle.rs:81`), so a sync with a given peer cannot be requested and catching up after an
absence cannot be measured or relied on; sync serves any peer that knows the topic, and allow- and
deny-lists are listed as future work (`src/lib.rs`, issue #925), so a removed member would keep
receiving every operation; and its API needs the actor stack and a `RelayUrl` for bootstrap nodes.
The binding uses p2panda-sync's `LogSync` protocol directly over its own iroh sessions, which keeps
p2panda's wire protocol and gives control of who is served and when. iroh-docs (a key-value store
with last-writer-wins entries) and iroh-gossip (ephemeral broadcast) were not a fit for an
append-only version graph; gossip would be the natural carrier for L3 presence and was not built.

**Tests** (`tools/collab-spike/dcp-p2p/tests/`): `conformance.rs` runs `run_all` and the negative
control; `p2p.rs` holds the tests conformance cannot see; `header_limit.rs` measures the p2panda-core
limit; `measure.rs` (ignored by default, run in release) prints the measurements; `relay.rs` runs a
relay through iroh's in-process relay server (`--features relay-test`). Every line this document
quotes as `MEASURE` is printed by one of them. A test that needs no service has nothing to skip:
no test here uses `DCP_SKIP_*`.

## What p2panda provides today, and what the binding had to build

| Need | p2panda piece | State in 0.7.1 | What the binding did |
|---|---|---|---|
| Signed, append-only entries | p2panda-core `Operation` | Works. Signature checked on decode. **A header may hold no CBOR byte string or array over 512 elements** (`operation/any.rs:192`, `length_limit(512)`), measured below | Chunks the spaces message into byte strings of 480 bytes (`ext.rs`) |
| Storage | p2panda-store `SqliteStore` | Works; in-memory has one connection, so a transaction blocks every other query | One database per document; short transactions |
| Log sync | p2panda-sync `LogSync` | Works (measured). One malformed header aborts the whole session | Own sessions, own membership gate, keeps the operations received before an abort |
| Discovery, address book, gossip | p2panda-net | Passes its own e2e test; sync cannot be driven per peer or gated | Not used |
| Blobs | **p2panda-blobs** | **An empty crate**: `src/lib.rs` is one `#![cfg_attr(docsrs, ...)]` line and `// TODO: Needs refactoring since p2panda-net refactor.`, with no `mod` declaration; the 560 other lines are files that nothing compiles | iroh-blobs 0.103.1 directly |
| Group membership with levels | p2panda-auth | Four levels `Pull < Read < Write < Manage`, an operation DAG, a "strong removal" resolver | Used through p2panda-spaces |
| Group encryption | p2panda-encryption | Works: key rotation on removal, a late member gets the history | Used through p2panda-spaces |
| Both together | p2panda-spaces | See next list | The bulk of the work-arounds |
| High-level node | `p2panda` umbrella | Its own docs: encryption and access control state "soon"; decryption "in the future" | Not used |

p2panda-spaces 0.7.1, with the evidence:

- **No role change.** `Group` offers `add` and `remove` only; the encryption mapping has `_ =>
  unimplemented!()` for any other auth action (`encryption/message.rs:161`, `space.rs:455`), and
  `SpacesArgs::SpaceUpdate` is `unimplemented!()` in `Manager::process` (`manager.rs:274`). *Read,
  not run.* The binding changes a role by removing the member and adding it back (two auth
  operations, two key rotations) and refuses any operation that carries a promote, a demote or a
  `SpaceUpdate`.
- **No author on decrypted data.** `Event::Application { space_id, data }` (`event.rs:54`) has no
  operation id and no author, and the encryption orderer queues a message received before the receiver
  was welcomed (`orderer.rs`, `next_ready_message`), so its plaintext comes out later as a by-product
  of processing a different message (*read, not run*). The binding makes every entry carry its own
  Ed25519 signature inside the encryption and takes the author from that (`signed.rs`).
- **No authorisation of writes.** Any member holding the group key can encrypt anything. The reader
  rule in `auth_check.rs` is the binding's: an entry counts only if its author had at least the
  required level in the group state at the causal point the entry claims (the auth operations it
  lists as `heads`), and no removal or demotion of the author exists outside the causal past of
  those heads.
- **State is the caller's job.** The `*_persisted` helpers exist only under `test_utils`
  (`manager.rs:536`); the binding persists the returned states itself.
- **One auth graph per manager.** `Group::create` depends on the heads of the manager's *global*
  auth graph (`group.rs:88`, the fixed id `global-groups-context`), so a second document in the same
  manager would depend on operations of the first. *Read, not run.* One manager and one database
  per document.
- **The whole history is in the state.** The encryption orderer keeps every message
  (`encryption/orderer.rs:44`, "TODO: We keep all messages in memory currently which is bad"), and
  the manager reads and rewrites that state for every message. Measured below.
- **A key bundle first.** Adding a member needs its key bundle (`Member` has no constructor and is not
  serialisable; the bundle travels as a signed `KeyBundle` operation, the "contact card"). Without it
  the add fails: `Invalid("missing key bundle for member …")` (measured).
- **Ordering is the caller's job.** Messages must be applied in dependency order; the binding keeps
  a pending set and applies an operation when all `dependencies()` are applied.

p2panda-auth can express three of the four DCP roles (viewer, editor, owner). It has no comment
level; its `Access<C>` carries a generic `Conditions` value that could tag a commenter, which was
not tried. It was used only through p2panda-spaces, not standalone.

## Results

### Conformance (spec section 12)

Job `test`, debug build, three nodes per fixture, 30 s settle:

```
DCP conformance, binding p2p-iroh-p2panda:
  PASS    convergence_on_one_head (spec test 1)
  PASS    second_head_shown_not_hidden (spec test 2)
  PASS    corrupt_file_rejected (spec test 3)
  PASS    truncated_upload_never_published (spec test 3)
  PASS    lease_exclusion_and_readback (spec test 4)
  PASS    lease_expiry_releases (spec test 5)
  PASS    roles_enforced_at_store (spec test 6)
  PASS    forged_version_from_a_viewer_is_not_accepted (spec test 6)
  PASS    revocation_hides_new_content (spec test 7)
  PASS    unknown_algorithm_falls_back_to_versions (spec test 8)
  PASS    update_stream_gap_recovery (spec test 9)
  11 passed, 0 failed, 0 skipped
```

`Capabilities` are all true, `racy_lease` included, so `assert_skipped_exactly(&[])` holds. Two of
the true flags need a sentence: `upload_faults` truncates the writer's import into its own blob store
(there is no remote upload in a peer-to-peer binding), and `forgery` makes the viewer sign and
encrypt a version itself, around every role check; there is no server to refuse it, so the readers'
rule is the only gate. With that rule off (`authorize = false`) the forged-version journey fails
with "a viewer's forged version was accepted by a reader", as it must.

The first CI run of the journeys failed one of eleven (`roles_enforced_at_store`: a demoted viewer's
`role()` answered "not a member" between the removal and the re-addition that make up a role change).
The fix is that a role change is made visible to sessions atomically (the removal and the addition
are created under one exclusive lock and a receiver stores a batch in one transaction), and that
sessions take the membership gate and the log under one lock (see "Revocation"). It passed in the
next runs.

Two more races showed up while the tests were being repeated, both mine: the membership lock above
made two nodes that published at the same time wait for each other (a distributed deadlock: each held
the sessions' read lock across a network wait while its own receiver waited for the write lock; journey 2
timed out once in 13 repetitions). The receive path no longer takes that lock, a membership change waits
for sessions at most 5 s, and a session is abandoned after 90 s. The last CI run (run 40139997921) passed 11 of 11
journeys, 14 of 14 p2p tests, and ten repetitions of the four revocation tests plus three repetitions of
the journeys, 0 failures. Thirteen repetitions is evidence, not proof, that no race is left.

### Offline and catch-up

`a_peer_offline_during_publishes_catches_up_on_reconnect`: the viewer goes offline (the node refuses
and closes every connection), the editor publishes five versions in a chain, nothing reaches the
viewer in the 1.5 s that follow (asserted), the viewer comes back and lists the head: **693 ms in
the debug build, 52 ms in release** after reconnecting. No always-on peer was involved.

### Concurrent publishes and merge

`concurrent_publishes_from_two_peers_give_two_heads_and_a_merge_heals_it`: owner and editor are both
offline, each publishes a child of the same version, both return. **Two heads are visible at all
three nodes 372 ms (debug) / 55 ms (release) after reconnecting**, none is hidden, the
owner merges, and all three show one head.

### A tampered operation

`a_tampered_operation_is_dropped_and_the_node_keeps_working`: an endpoint with a fresh key speaks the
sync protocol (the membership gate is off in this test only, so a stranger can reach a session) and
sends a real header with one bit flipped. `Header::decode` refuses it (signature), the receiving
session fails (`sync_failures` 0 to 1), the document's operation counts are unchanged, and a normal
publish reaches the editor afterwards. One bad header aborts the *session* in p2panda-sync, so a
hostile peer can end a session by sending one early; the binding keeps what the session had received
before the failure, and each such operation was signature-checked when it was decoded. A forged
author inside a validly signed, validly encrypted entry is a different layer and is covered by the
forged-version journey and the unit-level signature check (`signed::verify`).

### Revocation

Three tests, each with a different gate:

1. **Gate off, removed member's node served everything**
   (`a_revoked_member_receives_ciphertext_but_cannot_decrypt_later_entries`): the removed viewer's
   node received the later operations (1 in debug, 3 in release received), decrypted none of them (entry count unchanged; one
   application message counted as refused), could not list or fetch the version, and a blob fetched by
   hash was `nonce || ciphertext || tag` (plaintext length + 40 bytes), did not contain the
   plaintext, and hashed (BLAKE3) to the iroh-blobs hash. This is the encryption holding alone.
2. **Gates on** (`the_blob_gate_refuses_a_removed_member_and_a_non_member`): the removed viewer's sync
   session is refused, its blob fetch by hash fails (`io: connection lost`), as does a never-member's;
   a member's succeeds.
3. **The limit** (`content_from_a_member_that_has_not_seen_the_removal_is_readable_by_the_removed_member`):
   the editor is offline when the owner removes the viewer; `revoke()` returns after 3 (release) / 49 (debug) ms (it waits
   up to 5 s for each reachable member to acknowledge); the editor, unaware, publishes to the old group
   key; the owner goes away; the editor returns and still serves its member list; **the removed viewer
   listed and read the version** (release: yes). In two debug runs of the same test the viewer never listed the version. The cause I found and
   fixed: a session that was queued before the removal could start after it, with the initiator not
   deciding again whether its peer was still a member, and a session could serve the removal before the
   gate knew it. Sessions now take the log and the gate under one lock and an initiator decides again
   under it. This is a diagnosis from reading and from the test passing afterwards, not from a trace.

Removal also rotates the key only for those who apply it: p2panda-encryption's guarantee is "a member
will not learn about newly created data after it is removed", and the removal reaches a member only
when that member syncs. `revoke()` pushes the removal to every remaining member it can reach and waits
for each to say it has applied it (a "barrier" question on the sync ALPN), so that content the
revoking owner or any acknowledging member publishes next is encrypted to the rotated key.

A removed *editor* that keeps writing is refused by the readers: `a_removed_editor_cannot_publish_with_a_stale_view`
(gate off) shows the owner decrypting the entry (owner entries 2 to 3: it still holds the old key) and
then not listing it, because the removal is outside the causal past of the heads the entry claims.

### Roles

`roles_enforced_at_store` passes, including a promotion and a demotion of the viewer (each a removal
plus an addition). The commenter role is refused with `Invalid`
(`granting_a_role_needs_the_grantees_contact_card_first`); the same test shows `grant` failing until
the grantee's contact card is registered. A role change is two operations and two key rotations; a
member whose session breaks between them would see itself as a non-member until the next session.

### Files and hashes

`the_file_is_addressed_by_blake3_and_verified_by_sha256`: the locator's blob hash is the BLAKE3 hash
of the *sealed* blob (iroh-blobs' content address, verified while streaming); the version's `sha256`
and `size` describe the *plaintext* and are checked by the reader pipeline (`integrity::verify`) after
decrypting. The two never coincide. The draft-1 locator table has no entry for a content-addressed
blob; the binding writes `https://iroh-blobs.invalid/<blake3>` as a `url` locator that is never
fetched.

A version is listed only when its entry has been verified *and* its blob is present locally; a
version whose file no peer can currently provide is not shown (a reader that listed it would fail
`fetch_file`).

### Leases and the clock

There is no clock in a peer-to-peer binding, so every part of lease rule 1 ("the binding supplies the
time") has to be redefined. What the binding does:

- A lease is an entry. A lease write carries a Lamport value, one more than the highest lease value
  the writer has seen. The current lease is the entry with the highest (Lamport, entry id) pair, so
  every replica that has the same entries picks the same record. Two writes that did not see each
  other tie on the Lamport value and the entry id decides.
- `Stamped.ts_ms` of a lease is **the time this replica first saw the entry, on this replica's own
  clock** (own writes: when written). `now_ms()` is the same clock. A lease is active while stamp +
  `duration_ms` is later than `now_ms()`, all on one clock, so two devices whose clocks disagree never
  compare a timestamp across devices. The cost: a replica that was offline sees an old lease as new
  and treats it as active for one full duration after it first sees it, never longer.
- Entries that order *versions* (the heads' "newest first") use the author's own `ts_us`. It is not
  authenticated time (the author chose it); it only orders a display.

Results:

- **Clocks 60 s apart** (`leases_work_with_clocks_a_minute_apart`, owner +60 s, editor -60 s):
  `lease_exclusion_and_readback` and `lease_expiry_releases` both pass.
- **Negative control** (`author_stamped_leases_break_under_clock_skew`): stamping with the author's
  clock, with the editor +60 s, makes `lease_expiry_releases` fail ("the lease did not expire after its
  duration"). So the first-seen stamp is what makes the skew harmless, and the test can fail.
- **Propagation** (`lease_propagation_latency`, release, 60 writes, polled every 2 ms): a lease write
  was visible at the other node in min 9.6 ms, median 13.8 ms, p95 16.0 ms, max 17.3 ms. The settle
  interval of lease rule 3 has to cover this plus a lost round; 500 ms (the Matrix value) covers it
  by a factor of more than 30 on loopback; on a WAN nothing was measured.
- **Partition** (`partitioned_sessions_both_acquire_the_lease_and_the_merge_picks_one`): both nodes
  offline, each ran `lease::acquire` (two read-backs, 500 ms settle) and both returned `Acquired`;
  both `holds()` were true. After they reconnected exactly one still held it **254 ms (debug) /
  52 ms (release)** later and both named the same record.

So a peer-to-peer binding can offer L1 leases only as advisory: a lease that stops two sessions
that can see each other from both holding it, and not one that survives a partition. Version rule 5
(two heads, never silently merged) is what makes this safe, which is the draft's own position
("it does not forbid a fork").

### Peers that are never online at the same time

`members_never_online_together_need_a_third_member_that_overlaps_both`: *phase 1*, the viewer is away,
the editor publishes and leaves, the viewer returns: the viewer got the version through the owner,
which had been online with both (51 ms in release). *Phase 2*, the owner and the viewer are away while the
editor publishes and leaves: the viewer and the owner come back and **neither has the version** for
the 3 s asserted; when the editor returns both get it (102 ms in release). So the binding needs no
always-on server, but data is only as available as the members that hold it are online. An always-on
member (a peer that is a `Pull` member, which p2panda-spaces does not encrypt for, would hold ciphertext
and no key) is the way to remove the dependency; **that was not built**.

### Latency and throughput

Release build, loopback, one process, two nodes (job `measure`):

| What | Result |
|---|---|
| QUIC connect + first sync session, direct loopback address, no relay | median **27.3 ms** (min 23.8, max 31.6, 7 fresh pairs) |
| a sync session over a connection that is already open | median 6.6 ms |
| the same session with relay-only addresses vs direct addresses, same debug build and process | 20.9 ms vs 20.5 ms (not a WAN; see below) |
| 1 MiB file: writer (seal, store, read back, verify, entry) / iroh-blobs fetch | 6 ms / 4 ms = 279 MiB/s |
| 32 MiB file: writer / iroh-blobs fetch | 176 ms / 59 ms = 538 MiB/s; reader decrypt and read 52 ms |
| inline update, 34,000 bytes (the DCP maximum): publish / visible at the other node | see below |
| bytes per operation for a 100-byte inline update | **~2,170** on the wire (marginal, 100 to 1,000 entries) |

**Convergence of N small entries** (inline updates of 100 bytes from one writer, one reader; the reader
processes while the writer publishes):

| N | writer publish | reader complete, after the last publish | persisted p2panda-spaces state at the reader / writer |
|---|---|---|---|
| 10 | 33 ms (3.3 ms each) | 53 ms | 29.7 kB / 2.5 kB |
| 100 | 326 ms (3.3 ms each) | 592 ms | 220 kB / 6.1 kB |
| 300 | 1.08 s (3.6 ms each) | 6,055 ms | 644 kB / 14.2 kB |
| 1000 | 5.7 s (5.7 ms each) | 81,317 ms | 2.13 MB / 43.6 kB |

The reader's cost per entry grows with the number of entries (5.9 ms, 20 ms and 81 ms per entry at 100,
300 and 1,000), and the persisted state at the reader grows by about the size of one operation per entry.
That is p2panda-spaces' orderer holding every message in the state it reads and rewrites per message
(source above). A late joiner pulling everything in one session: 596 ms for 108 operations (224,510 bytes, about
2,080 bytes each) and 3,942 ms for 308 operations. Inline updates therefore stay usable to a few
hundred entries per document on this library; the spec's 34,000-byte inline limit was exercised and
works (figures in `spike-c-measure/p2p-release.log`; a publish triggers a sync, the 200 ms anti-entropy is a backstop).

**Per-entry overhead.** 2,170 bytes on the wire for 100 bytes of payload. Of that, the JSON body with its
128-hex signature, the 64-hex heads and the nonce, the AEAD tag, and a CBOR encoding in which cbor-core
writes the ciphertext byte string as an array of integers (measured: a 100,000-byte ciphertext became a
200,209-byte CBOR value, a factor of 2.0 for bytes of 24 or more), are all in the figure; the split between them was
not measured.

**Relay.** `nodes_reach_each_other_through_a_relay_and_sync_a_document` runs iroh's in-process relay
server (TLS with a self-signed certificate that the test endpoints accept) and gives each node the
other's address *with the relay URL and no direct address*. It synced and read a document. Whether the
connection stayed on the relay or hole-punched to the loopback address was **not observed**, so no
relay latency is claimed; there was no WAN, no NAT and no n0 relay.

### Flatpak

RFC-0001 asked what a Flatpak app needs. Measured, in a Flatpak 1.14.6 build sandbox
(`org.freedesktop.Platform//24.08`, `flatpak build`) on the CI runner (job `flatpak`, artifact
`spike-c-flatpak`): the test binary, three iroh nodes on `127.0.0.1` with no relay, passed
`a_peer_offline_during_publishes_catches_up_on_reconnect` both with `--unshare=network` (the sandbox
then saw only `lo`) and with `--share=network`. So **nodes in one sandbox reach each other over loopback
without a network permission**, which is what a two-window test needs. What was not run: two devices.
Reaching another device needs a UDP socket to the peer's address (iroh binds one UDP port, chosen by
the OS when the port is 0) and, when peers are not directly reachable, a relay (HTTPS 443 and QUIC address
discovery on the relay; the binding's default configuration uses none). Either needs `--share=network`
in `finish-args`; the repository's current Flatpak manifests grant it only to the build
(`build-args`), not to the app. mDNS discovery is not used by this binding. A portal-based
alternative (RFC-0001) does not exist today and was not tested.

## Go / no-go per claim

| Claim (RFC-0004, spec 8.4, the Spike C brief) | Verdict | Evidence |
|---|---|---|
| Dial-by-key transport needs no server | **Go** | Three nodes on loopback with no relay, journeys pass; a relay-only run synced (path not observed) |
| Signed append-only logs carry versions and updates | **Go** | `Operation` + p2panda-sync `LogSync`; sessions, catch-up and merge measured; tampered header dropped |
| ... with the published p2panda-net driver | **No-go** | Cannot request a peer, cannot gate by membership (source) |
| File bytes through p2panda-blobs | **No-go** | The crate is empty; iroh-blobs works (hash is BLAKE3, `sha256` of the plaintext checked separately) |
| p2panda-auth expresses the four roles | **Partly** | Three levels map; commenter does not; role *change* is unimplemented in p2panda-spaces |
| Revocation with post-compromise security | **Conditional go** | Later entries unreadable (gate off, measured); content from a member that has not applied the removal stays readable (measured) |
| L1 leases | **Conditional** | Advisory only; not exclusive across a partition (measured); Lamport order and first-seen stamps survive 60 s of skew (measured) |
| L2 inline updates | **Go with a limit** | 34,000 bytes works; reader cost grows to 81 s for 1,000 entries |
| The spec's lease rules without a binding clock | **Rewrite rule 1** | See Proposed 8.5 |
| No always-on peer needed | **Qualified** | Needs overlapping members; stuck data measured |
| Flatpak | **Partly measured** | Loopback in the sandbox works without network permission; cross-device not run |

## Spec corrections

Findings to put into the spec when it is next revised (not edited here: the spec is not owned by this
spike):

1. **8.4 "Reserved"**: replace the sentence about iroh and p2panda with the measured text below. State that
   p2panda-blobs is an empty crate and that p2panda-net's sync cannot be gated.
2. **4.2.1 locator table** needs a content-addressed entry. *(Applied in draft 1 update 2: `blake3`, and at integration `dcp-p2p` emits it; the results below were measured with the never-fetched `url`.)* Proposed:
   `locator.type` `blake3` with `hash` (the BLAKE3 hash, 64 hex, of the bytes the provider serves; the
   plaintext `sha256` and `size` stay as they are, so the two hashes differ whenever the served bytes
   are encrypted). The spike used a never-fetched `url` as a stand-in.
3. **Section 3 "Device"** and **section 9**: in the peer-to-peer binding the key is the device. A person
   with two devices is two principals until a link statement (9.2) exists.
4. **Section 5**: `commenter` cannot be expressed with p2panda-auth (it has no comment level). Section 13
   question 2 stands.
5. **Section 4.3 lease rule 1** assumes the binding supplies a time. A binding with no clock must say what
   its stamp is. Proposed: the stamp is the time the reader first saw the record, on the reader's own
   clock, and `now_ms` is the same clock; the 30-second allowance for skew is then moot. The binding also
   needs an order for concurrent writes without a server: (Lamport value, record id).
6. **Section 4.3 and section 10** should say that a lease is not exclusive across a partition in a
   binding without a coordinator: two sessions that cannot see each other can both hold it, and both
   pass the two read-backs (measured).
7. **Lease rule 3's settle interval** is binding specific. Peer-to-peer propagation on loopback was
   13.8 ms median (p95 16.0 ms); the value must also cover a lost round (a session every anti-entropy
   interval) and has not been measured on a WAN.
8. **Section 11 "Revocation has limits"** needs the second case: what a member publishes before it
   applies the removal is encrypted to the old key and readable by the removed member. In a binding with no
   server, "removal" takes effect for content published by members that have acknowledged it.
9. **Section 4.2 rule 2** ("upload, verify the stored bytes"): in a peer-to-peer binding nothing is
   uploaded; the file is first stored locally and fetched by peers later. A reader MUST NOT list a
   version whose file it cannot yet fetch (it would fail rule 2), and a writer that goes offline right
   after publishing leaves a version whose file only it holds.
10. **Section 12** may state that a journey which polls `role()` must tolerate `Forbidden`: a role
    change in this binding is a removal followed by an addition.

### Proposed 8.5

Replaces the second sentence of 8.4. Everything below was built and run except where marked.

**8.5 Peer-to-peer binding on iroh and p2panda (measured once).**

*Principals.* A principal is an Ed25519 public key, 64 lower-case hex characters, which is at once the
iroh endpoint id, the p2panda author and the p2panda-auth actor. `created_by.principal` MUST equal the
key that signed the entry. A device is the installation and holds one such key.

*Documents.* The document id is 32 random bytes (64 hex characters), and it is the only topic. A document
is one p2panda-spaces space. An implementation MUST keep one p2panda-spaces manager (and one database)
per document.

*Contact card.* Before an owner can add a principal, it MUST hold that principal's key bundle for the
document: a `KeyBundle` operation signed by that principal, obtained at the join request or out of
band. `grant` without it MUST fail.

*Entries.* Meta, version, lease and update are carried as entries. An entry's body is
`{author, doc, heads, ts_us, nonce, env}` where `env` is the draft-1 object; the author signs the
exact bytes of the body with its Ed25519 key; the signed body is encrypted to the group (p2panda-spaces
data scheme) and carried as an operation of the author's log. The entry id is the hex BLAKE3 hash of the
body and is the version, lease or update identifier. A reader MUST verify the signature, that `doc`
names the document, and that `created_by.principal` equals `author`. An operation's header MUST NOT
hold a byte string longer than 512 bytes (p2panda-core refuses it; its source applies the same limit to
arrays, maps and text, which was not separately measured); a message larger than that is split
into byte strings of at most 480 bytes, at most 500 of them.

*Roles.* viewer = p2panda-auth `Read`, editor = `Write`, owner = `Manage`; `commenter` is not expressible.
A role change is a removal followed by an addition, made visible to a peer as a unit; an implementation
MUST NOT process p2panda-auth promote or demote until p2panda-spaces supports them.

*Reader authorisation.* A reader lists an entry as a version, lease or update only if its author had at
least `Write` (for meta: `Manage`) in the group state after the auth operations the entry names in
`heads`, and no removal or demotion of the author exists outside the causal past of those operations.
The rule is evaluated when the entry is read, not when it arrives.

*Files.* The file is sealed with a fresh 32-byte key (XChaCha20-Poly1305, 24-byte nonce prefix) and the
blob is stored in iroh-blobs (BLAKE3). The hash and the key are carried in the encrypted entry, never in
the `Version` object. A provider MUST serve a blob only to a current member of a document that
references it, MUST refuse get-many, observe and push requests (iroh-blobs 0.103.1 or later), and a
reader MUST list a version only when its blob is present.

*Sync.* A session is a bidirectional QUIC stream on ALPN `dcp/p2p-sync/1`: a `Hello` naming the document,
a status byte, then the p2panda-sync `LogSync` protocol. A node MUST start and accept sessions only with
a current member of the document (a device that has just joined and knows no member serves only the
creator it joined through), and MUST take the membership check and the read of the log under one lock
so that a removal is never served before the gate knows it.

*Ordering and failures.* Operations are applied in dependency order. An operation that fails to decode
is dropped and ends its session; operations received earlier in the session are kept.

*Revocation.* `revoke` MUST push the removal to every reachable remaining member and wait (bounded) for
each to confirm it applied it. Content published by a member that has not applied the removal is readable
by the removed member.

*Leases.* A lease is an entry carrying a Lamport value (one more than the highest lease value its writer
has seen). The current lease is the entry with the highest (Lamport value, entry id). Its stamp is the
time the reader first saw it on the reader's clock; `now_ms` is that clock. A lease is advisory and is
not exclusive across a partition. A settle interval SHOULD be at least 500 ms.

*Time.* There is no binding clock. `ts_us` in an entry is the author's own reading and orders heads for
display only.

*Availability.* There is no server. A document's data is available while members that hold it are online
with the members that want it; no particular peer is required.

## Not verified

- A **WAN**, NAT traversal, hole punching, the production n0 relays, DNS or mainline address lookup. All
  timings are loopback. The relay test did not establish that traffic stayed on the relay.
- **Two devices** in Flatpak, and the `finish-args` an installed app would need beyond
  `--share=network`. The sandbox run was a `flatpak build` sandbox on a CI runner, not an installed app.
- **Three or more writers** on one document, concurrent membership changes by two owners, nested groups,
  a `Pull` member as an always-on helper, key-bundle expiry and rotation, more than 1,000 entries,
  restart (all state of a store is in memory, and the replica's own bookkeeping is not persisted), and
  the filesystem blob store.
- The **p2panda-spaces `unimplemented!()` panics** and the **one-auth-graph-per-manager** behaviour were
  read in source and not run.
- **Presence (L3)**, gossip, and live co-editing: nothing was built.
- The `commenter` role through p2panda-auth's `Conditions`.
- Interoperability with another implementation of this binding, and with the Matrix and WOPI
  bindings through `dcp-interop`.
- Behaviour with a hostile peer beyond what the tests do: spam of pending operations (the pending set is
  not bounded), a peer withholding blobs, and an operation that is valid but huge (the header ceiling is
  about 120 kB of ciphertext, see `header_limit.rs`).
