# RFC-0004 Spike B: Durable Streams binding results

Date: 2026-10-24 · Tracks: [RFC-0004](0004-interoperable-document-collaboration.md), spec [section 8.4](../DOCUMENT-COLLABORATION-SPEC.md) (reserved) · Code: [`tools/collab-spike/dcp-streams/`](../../tools/collab-spike/dcp-streams/) · Method and bar for honesty: [Spike A](0004-spike-a-results.md), [Spike D](0004-spike-d-results.md)

**Every number and observation here comes from CI runs on GitHub-hosted runners, with the client and the
server on one machine over loopback, against two servers: the Node reference server
`@durable-streams/server` 0.3.9 and the Go server (Caddy plugin) release v0.3.0. They describe those two
builds and one client implementation, not Durable Streams in general, and loopback latency is not network
latency.** Anything not run is listed under [Not verified](#not-verified); a statement marked
*[INFERENCE]* was reasoned, not run.

## Recommendation

**GO, with conditions, for L0, L1 and L2 over a Durable Streams server. NO-GO for "the server enforces
roles" on both servers tested. L3 was not built.**

The deciding results:

- All 11 shared journeys of spec section 12 (tests 1 to 9, the forged-version half of test 6 included)
  **pass with nothing skipped on both servers**, with every capability declared true. The lease is a real
  compare-and-set (`Stream-Seq`), so the lease journeys run as exclusion journeys, not as race-and-read-back
  ones.
- Test 7 was proved with a real second principal that holds every key it ever had: after its removal it
  decrypted **0 of 2** records and **0 of 1** file blob published afterwards; a control that reuses the key
  instead of rotating it decrypted them (so the check can fail).
- A viewer's forgery is accepted by the server (neither server has any access control) and dropped by every
  reader, by signature and roster, not by server trust; with the reader's role check removed the same
  journey fails.

The conditions, each of which the spec does not yet say (see [Spec corrections](#spec-corrections)):

1. **A deployment must put access control in front of the server, or accept that anyone who holds a document
   id can delete, close or poison its streams.** Both servers answer `DELETE` from anyone. Confidentiality
   and authorship survive that, availability does not. A shared credential per role gates a viewer's writes
   (tested) but cannot follow a revocation: the removed editor still holds the `writer` credential.
2. **Every roster change (grant, role change, removal) is a new epoch with a new key**, and revocation takes
   effect when the old epoch's streams are sealed, not when the owner decides. Between the two a removed
   editor can still write into the old epoch, and what it writes there is accepted (tested).
3. **The server has no timestamps.** `now_ms` is built from the HTTP `Date` header (whole seconds); after a
   one-off calibration it was a lower bound within 11 ms of the truth, before it up to a second behind.
4. **The lease stream can be poisoned** by anyone who can append: one junk message with a large `Stream-Seq`
   blocks lease writes for the rest of the epoch (tested; a roster change heals it).
5. **A new member reads all history** (its key opens every earlier epoch's key), and a member removed and
   re-added reads what was published while it was out.
6. **Record overhead is large for small updates**: a 100-byte update is a 747-byte record (7.5x), a 34 000-byte
   update 61 016 bytes (1.8x).

What this does not show: behaviour over a real network, TLS, a server other than the two builds above, or any
failure of the server's disk. See [Not verified](#not-verified).

## What a Durable Streams server is, and what was used

The protocol ([`PROTOCOL.md`](https://github.com/durable-streams/durable-streams/blob/main/PROTOCOL.md), read at
commit `461b402` of 2026-09-10; announcement 2025-12-09) is HTTP over URLs. A stream is a URL holding an
append-only sequence of bytes, or in JSON mode (`Content-Type: application/json`) of JSON messages whose
boundaries the server keeps. `PUT` creates (optionally with a body and closed), `POST` appends, `GET
?offset=` reads from an opaque offset the server handed out earlier (`-1` is the start), with
`Stream-Next-Offset`, `Stream-Up-To-Date` and `Stream-Closed` headers and long-poll or SSE live modes, `HEAD`
gives the tail, `DELETE` deletes. Also in the protocol: `Stream-Seq` (a strictly increasing,
lexicographically compared writer sequence, refused with `409`), idempotent producers (`Producer-*`), forks,
TTL and expiry, retention (`410 Gone`). **Authentication and authorisation are "explicitly out of scope"
(section 12.1).** There is no timestamp on a message, no snapshot, no clock endpoint.

| | Node reference server | Go server |
|---|---|---|
| Version | `@durable-streams/server` 0.3.9 (published 2026-09-10; client 0.2.7), pinned by `scripts/streams-server/package-lock.json`, run by `server.mjs`, file-backed (log files + LMDB), Node v22.23.3 | release v0.3.0 (2026-06-02), `durable-streams-server_0.3.0_linux_amd64.tar.gz`, sha256 `120b03d9…acb9` checked by `scripts/streams-caddy-fetch.sh`, a Caddy v2.10.2 build with the `durable_streams` module, file-backed |
| Access control on streams | none (the `401`s in its source are in the subscription-callback code) | none of its own; Caddy's `basic_auth` can be put in front (tested) |
| `Stream-Seq` scope | per stream (`stream.lastSeq`, string comparison in the source); the spec says a server MUST document its scope, the README does not | per stream (measured the same way, below) |
| Acknowledged append | `fdatasync` before the reply, per the source (`FileHandlePool.fsyncFile`); not tested against power loss | not looked at |
| Bodies | read whole into memory, no limit found in the source | see Limits |
| `Date` header | yes | yes |

## Method

`dcp-streams` is a crate in the standalone `tools/collab-spike` workspace. The run that every number below
comes from is [40137430958](https://github.com/tuna-os/gtk-office-suite/actions/runs/40137430958) at commit
`0c99551` of branch `spike/collab-b`, workflow
[`collab-spike.yml`](../../.github/workflows/collab-spike.yml) (run as the spike's own workflow `collab-spike-b.yml`, merged into it at integration; now the jobs `streams` and `streams-measure`), two jobs, both green:

- `streams` (debug build, 1 m 57 s): `cargo test -p dcp-streams --lib` (6 tests); then the whole `--test
  streams` suite against the Node server (25 passed, 0 failed; 9 `#[ignore]`d measurements not run); then the
  same suite against the Go server with `--skip restart` (22 passed, 0 failed); then the one test that needs
  access control against the Go server behind `basic_auth` (1 passed). Before them the job proves that a
  test fails when no server is configured (`DCP_STREAMS_URL is not set`) and that `DCP_SKIP_STREAMS=1` skips
  and says so.
- `measure` (**release** build, 5 m 42 s, selected by `[measure]` in the head commit message): the 9
  measurements against the Node server, then against the Go server, and the restart tests again. Runner:
  4 vCPU AMD EPYC 9V45, 15 989 MB, Linux 6.17.0-1022-azure, rustc 1.99.1, Node v22.23.3. Timings are in
  release mode because the client's cost is mostly Ed25519 and X25519 (1 000 updates took 10.9 s to catch up in a debug build on this spike's development machine, 63 ms in the release build on CI); the debug `streams` job is for correctness only.

Crates, as resolved on 2026-10-24: chacha20poly1305 0.10.1 (XChaCha20-Poly1305), ed25519-dalek 2.2.0,
x25519-dalek 2.0.1, hkdf 0.12.4, sha2 0.10.9, reqwest 0.12.28 (no TLS stack built), tokio 1.54.1. `Cargo.lock`
is regenerated at integration, not committed.

## What was built

### One stream per document and kind, per epoch

```
<base>/dcp/<doc>/ctl              control: one signed EpochRecord per epoch          (JSON mode, cleartext)
<base>/dcp/<doc>/<prefix>/log     meta and versions of the epoch                     (JSON mode, encrypted)
<base>/dcp/<doc>/<prefix>/upd     updates of the epoch                               (JSON mode, encrypted)
<base>/dcp/<doc>/<prefix>/lease   lease records of the epoch, each with Stream-Seq   (JSON mode, encrypted)
<base>/dcp/<doc>/f/<id>           one file, an encrypted blob, created closed        (octet stream)
```

Why not one stream per document: the lease needs `Stream-Seq`, which is per stream and refuses any value not
greater than the last, so a lease sharing a stream with versions would make every version write carry a
sequence number and make concurrent editors collide; updates arrive at a different rate and a late reader must
be able to skip them; and the three kinds are sealed and read independently. Why per epoch: see below.

### Epochs: a roster and a key that never change

An **epoch** is a roster (principal and role) and a 256-bit key. The control stream `ctl` holds one cleartext
`EpochRecord` per epoch, signed by an owner: epoch 0 by the creator, whose public key the document id commits
to (`doc = base64url(SHA-256("dcp-streams/1" ‖ "doc" ‖ creator ‖ root nonce)[..16])`, so a server cannot
substitute another genesis), epoch *n*+1 by an owner of epoch *n*, naming the hash of epoch *n*. It holds the
epoch key wrapped to every member and the previous key sealed under the new one. The first valid record for
each number wins. **Every roster change makes a new epoch**, because then a record's authority is decided by
its epoch alone and no ordering across streams is needed; a role change that took effect inside an epoch
would let a viewer's earlier forgery turn valid when it was promoted.

A principal is `ed25519:<base64url public key>`. The same key is the recipient key for wrapping (the
Edwards-to-Montgomery map, as libsodium does; reusing one key for signing and Diffie-Hellman is accepted
practice but is a design choice to review).

### Records

A record is one JSON message `{v, k, e, a, n, c, s}`: kind, epoch, author, 24-byte nonce, ciphertext, signature.
The ciphertext is XChaCha20-Poly1305 under the epoch key with the document, kind, epoch and author as
associated data. The author signs (Ed25519, encrypt-then-sign) a length-prefixed string of document, kind,
epoch, author, nonce and ciphertext, so the signature can be checked without the key and a record cannot be
replayed into another document, epoch or stream. The plaintext is `{st, dev?, body, hint?}`: the DCP object
and the author's server-time estimate. A reader applies these rules in order and drops, silently, what fails
(all but "wrong epoch" and "schema" have a test): shape, after a seal, wrong stream kind, wrong epoch, signature, not in the epoch's roster,
role too low for the kind (meta: owner; version, update, lease: editor), authenticated decryption, schema,
`created_by.principal` or `holder.principal` not the signer. The record id is the SHA-256 of its normalised
JSON, so a duplicate (a replayed record, or a retried append) is ignored.

### Revocation: rotation and a seal

`revoke` (and `grant`) by an owner: make the new key and wrap it for the new roster; create the new epoch's
three streams (the log with the owner's metadata record in the same request); append the epoch record to
`ctl`; read `ctl` back and, if another owner's record for that number came first, report `Conflict` (two
owners rotating at once: 8 of 8 rounds, exactly one won and the other was told, on both servers); then **seal**
every stream of the old epoch by appending an owner-signed seal with `Stream-Closed: true` in the same request.
A seal ends a stream for readers (nothing after it counts) and an honest server answers `409` to later
appends.

### Files, lease, time, snapshots

- **Files.** One closed stream per file: `PUT` with the blob and `Stream-Closed: true`, so an upload is atomic.
  An upload cut off on the wire left no stream behind (raw TCP test, both servers; the Node server's log has `Request error: Error: aborted`). Blob: magic, epoch, nonce, ciphertext, 53 bytes of overhead. The writer reads the
  blob back, decrypts and compares before it publishes the version (rule 2). The locator is `url`, the
  `https` form of the stream; a reader uses only the path and its own server (a version must not send a
  client to another host).
- **Lease.** One record per write, `Stream-Seq` = the record's sequence number as 16 hex digits (last valid + 1).
  The server admits one append per number: 20 rounds of 8 concurrent appends of the same number gave exactly
  1 accepted and 7 `409` in every round, and 10 rounds of two editors racing through the store gave 1
  accepted and 1 `Conflict`, on both servers. A write is also refused locally if its `renewal` does not
  follow the record just read (rule 8). Acquire and renew cost 5.5 and 5.1 ms p50 at a settle interval of 0.
- **Time.** See [Clock](#clock).
- **Snapshot.** The server has none. A client writes one as a version that `covers` the updates so far
  (spec 4.4, 7.4) and records, inside the encrypted version record, the offset of the `upd` stream to resume at.
  A reader that starts later reads the versions, finds the newest such version and reads the updates from
  there. The offset is an editor's claim; the reader refuses one beyond the tail (offsets sort
  lexicographically) and reads everything instead, but cannot check one inside the stream.

## Results

### Conformance journeys (spec section 12)

Both servers, run 40137430958, `run_all` with every capability true and `racy_lease` false:

```
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

`assert_all_passed` is the check, so a skip cannot appear unnoticed. The suite can fail on this binding:
`the_forged_version_journey_fails_when_no_reader_checks_roles` runs the forged-version journey with every
reader's role check removed and requires it to fail (it does: the viewer's version becomes a head).
`truncated_upload_never_published` cuts the blob inside the client, after encryption, so it exercises the
writer's read-back and not the network; the TCP-level cut is its own test. Tests 10 (WOPI) and 11 (schemas) are
not store journeys.

### Properties the journeys cannot see

All in `tests/streams/`; "both" means passed against both servers in run 40137430958.

| Claim | Test | Result |
|---|---|---|
| A tampered record is dropped | `a_tampered_record_is_dropped_and_never_shown` | both. Flipped ciphertext, changed author, stranger's signature, a viewer's signature, a malicious editor's re-signed tampering, a record moved to another stream, two non-records, two replays: every reader lists exactly the 2 real versions; the drop reasons are the expected ones. |
| The wrong key fails closed | `the_wrong_key_fails_closed` | both. A stranger gets `Forbidden` from `versions`, `meta`, `updates`, `role` and `fetch_file`; a key of another document, another epoch or a wrong file id opens nothing. |
| Revocation (test 7) with a real second principal | `a_removed_member_with_every_key_it_ever_held_reads_nothing_published_afterwards` | both. The removed viewer holds the keys of epochs 0, 1, 2 and decrypts 0 of the new epoch's records and 0 of its file blob; a member who stayed reads the file (the control); the viewer still reads what it had; the old streams answer it `409`. |
| ... and it can fail | `negative_control_without_rotating_the_key_the_removed_member_reads_everything` | both. With the key reused, the removed member decrypts records and the blob. |
| Roles at the reader | `writes_by_lesser_roles_are_accepted_by_the_server_and_dropped_by_every_reader` | both. A viewer's update, lease and version, and an editor's metadata, are accepted by the server and dropped by owner, editor and viewer. |
| ... and it can fail | `negative_control_a_reader_that_skips_the_role_check_accepts_the_viewers_version` | both |
| Control stream noise | `the_control_stream_ignores_forged_replayed_and_out_of_order_epochs` | both. Replayed epochs, a forged epoch signed by an editor and by the viewer, `[1,2,3]`: no epoch added, the owner can still grant. |
| Duplicates and replay | `replaying_an_offset_is_idempotent_and_a_fresh_reader_agrees_with_an_incremental_one` | both. The same offset read twice gives the same bytes; every record appended a second time changes nothing; a fresh client and an incremental one agree. |
| The window before a seal | `until_the_old_epoch_is_sealed_a_removed_editor_can_still_write_into_it` | both. With the owner's client stopped before sealing, a removed editor's forged version lands in the old epoch and readers accept it; `heal` seals, later writes get `409`; what was accepted stays. |
| Lease compare-and-set | `the_lease_is_a_compare_and_set_on_stream_seq` | both (numbers above) |
| Lease poisoning | `anyone_who_can_append_can_poison_the_lease_stream` | both. One junk append with `Stream-Seq: ffffffffffffffff` makes every lease write `Conflict`; a roster change gives a fresh lease stream. |
| No server access control | `on_the_reference_server_anyone_with_a_url_can_delete_and_close_streams` | both. A client with no identity deleted a version's file stream and an update stream. (In `writes_by_lesser_roles_...`, both servers: a member closed a log with a forged seal; readers drop the seal, writers get `Conflict`, the epoch is wedged until the roster changes.) |
| Rotation race | `two_owners_changing_the_roster_at_once_never_fork_the_document` | both. 8 rounds: one owner won each, the other got `Conflict`; one chain, everyone told "granted" is a member. |
| Snapshot bootstrap | `a_late_joiner_bootstraps_from_a_snapshot_and_reads_only_what_follows` | both. 300 updates, a covering version, 5 more: the late joiner read 5 updates (15 169 bytes against 188 099 without the snapshot) and its state equals the editor's. |
| Snapshot hint | `a_snapshot_hint_cannot_be_checked_by_the_server_so_the_reader_bounds_it` | both. An offset past the tail: **the Node server answers `200` with no items and clamps `Stream-Next-Offset` to the tail, the Go server answers `500`**; a forged hint beyond the tail is ignored by the reader. |
| Concurrent appends | `concurrent_appends_are_all_kept_once_and_every_reader_sees_one_order` | both. 8 writers x 50 appends: 400 kept, two readers see the same order, each writer's order kept. |
| Idempotent producers | `idempotent_producer_headers_deduplicate_a_retried_append_...` | both: first `200`, retry `204` (stored once), a gap `409`. The binding does not use them (records are content-addressed). |
| Truncated upload on the wire | `an_upload_cut_off_on_the_wire_leaves_no_stream_behind` | both |
| Clock | `the_server_clock_estimate_is_a_lower_bound_within_the_poll_interval` | both: Node -2..-1 ms, Go -11..-10 ms |
| A gate in front | `a_server_that_gates_by_credential_refuses_a_viewers_forgery_but_cannot_follow_revocation` | Go server behind `basic_auth` (`reader` GET/HEAD, `writer` anything): the viewer's forged append gets `401` from the server; the removed editor, still holding `writer`, can still append to the new epoch (readers drop it). |
| Restart | `restart::*` | Node, see below |

### Latency (Node / Go), release build, loopback

200 samples each, 60 ms apart, after 30 warm-up appends, one 100-byte update between two clients
(`measure::latency_append_to_a_second_client`); all in ms:

| | Node p50 / p95 / max | Go p50 / p95 / max |
|---|---|---|
| bare `POST` of a 620-byte JSON message (300 samples) | 0.91 / 1.19 / 1.89 | 0.59 / 0.90 / 1.38 |
| `publish_update` returns (read `ctl`, encrypt, sign, `POST`) | 2.67 / 3.21 / 16.9 | 1.40 / 1.64 / 4.30 |
| A starts the append -> B's long poll returns | 2.43 / 2.99 / 15.2 | 1.51 / 1.79 / 4.35 |
| A starts the append -> B's `updates()` has decrypted and verified it | 3.91 / 4.53 / 20.8 | 2.01 / 2.67 / 5.96 |

A publish costs two requests (a read of `ctl` to learn the current epoch, then the append). On a real network
add two round trips to the first row and one to the last two. Lease acquire and renew with a settle interval of
0: 5.5 / 5.1 ms p50 (Node), 2.1 / 1.8 ms (Go); with the spec's 500 ms settle interval the cost is the interval.

### Catch-up, release build

Cold reader, 100-byte updates, median of 5 (`measure::catch_up_*`). "Snapshot" = the covering version and
offset hint above, 10 updates after it.

| Updates in the stream | Node without snapshot | Node with | Go without | Go with | Bytes read without / with |
|---|---|---|---|---|---|
| 100 | 15.4 ms | 10.8 ms | 10.2 ms | 6.0 ms | 120 604 / 50 858 |
| 1 000 | 63.4 ms | 12.1 ms | 51.5 ms | 5.4 ms | 796 515 / 50 869 |
| 5 000 | 265.8 ms | 11.2 ms | 238.1 ms | 6.1 ms | 3 804 515 / 50 869 |

Cost without a snapshot is linear (about 53 µs per update on Node, *[INFERENCE]* mostly the signature check); with one it is constant. The 50 KB floor is the reader's work other than the updates; *[INFERENCE]* most of it is the control stream, because the measuring loop itself added 10 epochs of growing rosters (not itemised). Versions (list only, no files): 50 / 200 / 500 versions in 7.1 / 15.5 / 29.7 ms (Node) and
5.0 / 11.5 / 25.5 ms (Go), 9 requests, 63 728 / 195 278 / 458 378 bytes; opening the head (list, fetch the file, decrypt, verify) takes 33.8 ms for 500 versions. Writing 5 000 updates one by one took 7.1 s (Node), 4.0 s (Go).

**Epochs add a cost the spec does not have:** a cold reader reads every epoch's streams once (sealed ones
are final, so never again): 15 / 30 / 55 epochs cost 33 / 63 / 113 requests, 72 720 / 194 130 / 543 980
bytes and 20.6 / 38.2 / 79.0 ms (Node; 8.6 / 17.2 / 35.7 ms Go) for a document of 20 versions. A roster of 50
members is 50 epochs.

### Record overhead

`measure::record_overhead_in_bytes`, same on both servers. Update payload -> bytes of the JSON message sent
(and bytes the server stored, from `Stream-Next-Offset`; Node +6, Go +4 per message):

| Payload | DCP update object | Record on the wire | Ratio |
|---|---|---|---|
| 0 | 194 | 563 | |
| 100 | 332 | 747 | 7.5x |
| 1 000 | 1 533 | 2 348 | 2.35x |
| 10 000 | 13 534 | 18 350 | 1.84x |
| 34 000 | 45 534 | 61 016 | 1.79x |

The fixed part is about 560 bytes: the author's principal string (51), the nonce (32), the Ed25519 signature
(86), the field names, and the encrypted-and-base64-encoded DCP object. The slope is 1.78 per payload byte
because the payload is base64 inside the update object (4/3) and the object is base64 again inside the record
(4/3). A version record for a one-parent version of a 10 kB file is 880 bytes; its file blob is the file plus
53 bytes. An epoch record is about 440 bytes plus 295 per member (1 914 bytes for 5 members, 6 341 for 20,
15 191 for 50). A roster change costs 12 ms (4 members) to 126 ms (50 members) on Node, 7 to 52 ms on Go: the
streams, the wraps, the control append and the seals.

*[INFERENCE]* a binary record framing instead of JSON mode would drop the second base64 (slope 1.78 to about 1.33) at the price of the server's message boundaries; not built.

### Limits

`measure::the_largest_record_and_file_the_server_accepts`, one JSON message of the size (a string field)
appended, and one closed octet stream of the size created, read back whole. Sizes in MiB on a doubling grid,
so the limits are bracketed, not found. Neither server was down after any request.

| | Largest JSON message accepted | First refusal | Largest closed octet stream | First refusal |
|---|---|---|---|---|
| Node 0.3.9 | 256 MiB (3.0 s to append) | 512 MiB: `500` | 512 MiB (the cap tried) | none |
| Go 0.3.0 | 32 MiB | 64 MiB: `500` | 64 MiB | 128 MiB: `500` |

Neither refusal was a `413`, which the protocol offers; the cause of the `500`s was not investigated. Through
the binding (encrypt, upload, read back, decrypt, compare) a 32 MiB file published in 431 ms (Node) and 257 ms
(Go); a client's own limit here is 256 MiB. The spec's 34 000-byte inline update limit gives a 61 016-byte
record, well inside both servers' limits. A file over what the server takes needs chunking, which is not
defined.

### Restart

`restart::*`, which start their own Node server on a free port with a data directory:

- **SIGTERM and SIGKILL** (`kill -9` straight after the last acknowledged append): the restarted server held
  all 20 updates, 3 versions and the lease the client had seen acknowledged (4 runs, 2 debug and 2 release,
  all passed). The client resumed from the offset it kept: 8 requests and 10 bytes to learn there was nothing
  new, no history re-read. Writing continued, a repeated `Stream-Seq` was still refused after the restart (the
  counter is persistent), and a client with an empty cache read everything. While the server was down the
  client reported a transport error, not an empty document. Process start to `READY`: 61 to 76 ms (release
  job), 190 to 350 ms on the slower development machine.
- **In-memory mode** (`dataDir` omitted, the server's default): after a restart the document is `404`. The
  client reports `NotFound` for a document it knows it had and does not recreate it. A deployment must not run
  this mode for anything that matters.
- The Go server's restart was **not run**. Neither server was tested against power loss or a full disk.

### Clock

`measure::the_clock_before_and_after_calibration`, 40 clients each, server and test on one host, so the error
is the estimate minus the host's clock:

| | min | p50 | max |
|---|---|---|---|
| After one response (the `Date` header, floored to the second) | -476 ms | -253 ms | -39 ms |
| After calibration (`HEAD` every 10 ms until the header ticks over, 0.52 to 0.97 s once per client) | -11 ms | -6 ms | 0 ms |

Go: -472 / -258 / -38 and -11 / -5 / 0. Every estimate was a lower bound, as designed. The uncalibrated error
is bounded by one second, so the spec's rule for a `Date` clock (error taken as 1 s, a lease under about 3 s
not meaningful) holds; after calibration a 1 s lease is meaningful if the clock stays calibrated, which was not
tested beyond tens of seconds (drift of the monotonic clock against the server's). Because no record carries a
server timestamp, each author puts its own estimate in the encrypted record and a reader clamps it to its own
estimate plus the 30 s skew of lease rule 1: a writer can claim a past time (shortening its own lease) or a
future one up to 30 s (extending it by that much), so **lease expiry is only as trustworthy as the lease's
holder**, which is what "advisory" already says.

## What the server must be trusted for

| The server can | Effect | Evidence |
|---|---|---|
| Refuse, delete, close, slow or lose streams, **and so can anyone who has the document id** | Availability and history; the ciphertext is never exposed by it | `on_the_reference_server_anyone_with_a_url_can_delete_and_close_streams` |
| Acknowledge and then lose an append | A lost version or update; the writer's read-back catches a file, nothing catches a record | not tested; for files, `truncated_upload_never_published` |
| Show a reader a stale prefix (withhold the newest records, **including the epoch record of a revocation**) or reorder | A frozen view; a removed member and a colluding server could keep a reader in the old epoch. Nothing forges or alters a record | *[INFERENCE]*: by construction, no signed tail or gossip exists. Not run |
| Choose which of two competing valid epoch records is first | Which rotation wins | by construction; the rotation-race test has an honest server |
| Lie about time | Lease timing and head ordering, within the 30 s clamp | the clock section |
| See who writes what, when | Paths (document id, number of epochs, a random prefix per epoch), the kind of each stream, the author principal of every record, record and file sizes, timing, the roster with roles (cleartext), IP or credential | the wire format above |
| Not see | Titles, content, the version graph (parents are inside the ciphertext), file names, MIME types | the wire format above |

It is **not** trusted for confidentiality, record integrity, authorship, the roster (a signed chain from the
document id) or which role may write: those hold if the server is hostile, except that it can withhold.

## What a deployment must put in front

Not built beyond one test, and the verdict on it is "partial".

- The **minimum**: a gate in front so that only members' clients can write and only holders of the document id
  can read, as Caddy's `basic_auth` did (viewer: `401` on the forged append). The server then also stops the
  deleting and closing by outsiders, which the tests show it otherwise allows.
- A gate by **role** cannot be a static credential per role: a viewer promoted to editor and a removed editor
  both need the gate to change at once. Because `ctl` is cleartext and signed, a gate (Caddy `forward_auth`
  to a service) can derive the current roster itself, without keys, by verifying the chain from the document
  id, and authorise each request by a signature of the requester over the request. *[INFERENCE]*: this is
  possible and nothing in the binding prevents it; it was not built or measured.
- Even then the gate sees whom it admits. It does not make the server trusted for confidentiality.

## Go / no-go per claim

| Claim (spec, RFC-0004, the assignment) | Verdict | Why |
|---|---|---|
| L0, L1: documents, versions, roles over an append-only HTTP log | **GO** | 11 of 11 journeys, both servers |
| One stream per document, or per document and kind | **per kind and epoch** | lease needs its own stream; epochs make authority exact |
| Offset-based catch-up | **GO** | offsets resume exactly (8 requests, 10 bytes after a restart) |
| Server-side snapshots or bootstrap | **NO-GO** | none exists; client-side covering version plus offset hint **GO**, constant 11 ms against 266 ms at 5 000 updates |
| End-to-end encryption of every record and file, server sees ciphertext | **GO** | but the server sees principals, roles, sizes, timing (table above) |
| Authorship from signatures, not from the server | **GO** | every forgery and tampering test, negative controls fail |
| A viewer cannot publish what a reader accepts | **GO** | at the reader. **At the server: NO-GO**, neither server has a gate; a deployment can add one |
| Write capability enforced at the server | **NO-GO** as offered; partial with a gate in front | basic_auth test |
| Revocation by key rotation (test 7) | **GO** for content published into a new epoch, real second principal, control fails | not for what the member already has; not for a member who is wedged-in before the seal; integrity window until the seal |
| Lease with append compare-and-set | **GO** | `Stream-Seq`, exact on both servers; **poisonable**, healed by a roster change |
| `now_ms` from a server timestamp | **NO-GO**: none exists. **GO** for a `Date`-header estimate | -11..0 ms calibrated, up to -1 s not |
| L2 updates | **GO** | gap journey, unknown-algorithm journey; 7.5x overhead at 100 B |
| L3 presence | **not built; NO-GO as specified** | spec 4.5 forbids a durable log; a TTL stream was not tried |
| Restart | **GO** file-backed (Node); **NO-GO** in-memory | tests above; Go not run |
| Large files | **GO to 32 MiB** through the binding on both servers; larger not portable | limits |
| New device of an existing member | **not built** | the principal is a key; moving it moves the secret. No key backup or per-device key exists here |

## Spec corrections

1. **Proposed 8.4** (below).
2. **4.2.1, locator.** *(Applied in draft 1 update 2: the type is `durable-stream`, and at integration `dcp-streams` emits it and refuses `url`; the results below were measured with `url`.)* The four types do not fit a stream whose bytes are ciphertext: a `url` reader fetches
   ciphertext and fails the `sha256`. This spike used `url` with the `https` form of the stream and reads
   by path. Either add a type (`{"type": "durable-stream", "href": "https://..."}` with the rule that the
   reader resolves it against its own server, so a version cannot make it fetch from another host) or state
   under `url` that the bytes are what the binding defines. Exact schema change: add `durable-stream` to the
   `enum`, with the `href` rule of `url`.
3. **4.3 lease rule 3 and 7.** A binding with an append compare-and-set acquires the lease when its append is
   accepted; the second read-back and the settle interval are then redundant for acquisition (20 x 8 raw and 10
   store races: exactly one winner each; journeys 4 and 5 pass with the 500 ms interval, and the lease
   protocol costs 5.5 ms without it). The rule should say "a binding with atomic compare-and-set MAY use a
   settle interval of 0". `MATRIX_SETTLE_MS` in `dcp-core::lease` is Matrix-specific naming for what every
   binding passes.
4. **4.3 lease, a compare-and-set that can be poisoned.** State that a binding whose compare-and-set is a
   server-side sequence refuses all writers once anyone writes a high one, and that the owner's remedy is a
   new lease stream (here: a new epoch).
5. **4.3 rule 1, time.** "The binding supplies the time" has no source in this binding but the `Date` header;
   say how a client estimates it and that an author's own estimate in a record is a claim, clamped by the
   reader (section 8.4).
6. **4.4, 7.4 compaction.** "A reader that starts later loads the version and applies only updates after it"
   needs a way to find where in the update log "after" is. In a stream binding that is an offset, recorded by
   the writer of the covering version, who can lie; say that it is a claim of an editor and that a reader MAY
   verify what it can (here: not beyond the tail). Neither server can validate an offset: Node returns an
   empty `200`, Go a `500`.
7. **11, revocation.** Add for epoch bindings: removal takes effect at the seal, an editor removed
   before the seal can still write into the old epoch and be accepted (tested); a member added later reads all
   earlier epochs (history is shared, as Matrix `shared`) and a member removed and re-added reads what was
   published meanwhile; the signed `prev_key` chain is what shares it.
8. **11, server availability.** A binding whose server has no access control lets anyone with the document
   id delete, close or poison streams. This is not in the section's list.
9. **12, test 7, stronger evidence.** The journey checks the store's answers. For an encrypted binding the
   spike's test (decrypt every record and blob of the new epoch with every key the removed principal ever held,
   plus a control that reuses the key) is stronger and cannot be written against the `DocumentStore` trait
   because it needs the raw bytes. It should be a binding-specific requirement of the section, which this one
   meets.
10. **12, tests and the trait.** `Fixture::standard` makes 3 epochs here (create and two grants), harmless.
    `Faults::truncate_next_upload` is satisfied by a client-side cut; a network-level cut is a separate test. No
    trait or journey change is needed.
11. **Draft 1's reserved text for 8.4** says Durable Streams is "an append-only HTTP log with offset resume".
    That is accurate; what the section also needs to say is that it is **bytes with no principals**, which is
    what every other row of this document follows from.

### Proposed 8.4

> ### 8.4 Durable Streams binding (measured once)
>
> **Server.** Any server of the Durable Streams protocol that supports JSON-mode streams, `PUT` with a body
> and `Stream-Closed`, `POST` with `Stream-Closed` and `Stream-Seq`, `HEAD` and offset reads, with the `Date`
> header. It stores bytes. A document MUST NOT rely on it for confidentiality, authorship, roles or time. A
> deployment MUST put access control in front of it that admits only the document's members to write and
> SHOULD admit only them to read; without it any holder of a document id can delete, close or poison the
> document's streams.
>
> **Principals.** `ed25519:` and the unpadded base64url of a 32-byte Ed25519 public key. The same key is the
> recipient of key wraps (X25519 by the Edwards-to-Montgomery map).
>
> **Document id.** The first 16 bytes of SHA-256 of the concatenation of `dcp-streams/1`, `doc`, the creator's
> principal and a 128-bit random nonce, in unpadded base64url. Epoch 0 carries the nonce.
>
> **Streams.** Under `<base>/dcp/<doc>/`: `ctl` (JSON mode), and for each epoch `<prefix>/log`, `<prefix>/upd`,
> `<prefix>/lease` (JSON mode), and per file `f/<id>` (octet stream, created closed). `<prefix>` is `e<n>-`
> and 12 random hex digits chosen by the owner who creates the epoch; `<id>` is 96 random bits in base64url.
>
> **Epochs.** `ctl` holds one JSON `EpochRecord` per epoch (fields `v`, `doc`, `n`, `prefix`, `prev`, `root`,
> `roster`, `wraps`, `prev_key`, `a`, `s`). Every change of roster, including a role change, is a new epoch with
> a new random 256-bit key. A reader accepts the first record for each number `n` = 0, 1, 2, ... that has a
> valid Ed25519 signature by `a` over its fields; whose `roster` has at least one `owner` and no duplicate;
> whose `wraps` name roster members only; and, for `n` = 0, whose `root` hashes with `a` to the document id
> and whose creator is an owner, or, for `n` > 0, whose `prev` is the lower-case hex SHA-256 of the previous record's JSON (fields in the order above), which has `prev_key`, and whose `a` is an owner of epoch `n - 1`. A wrap is the epoch key encrypted to a
> member by ephemeral X25519, HKDF-SHA256 over the shared secret (salt: ephemeral and recipient public keys;
> info: `dcp-streams/1/wrap`, the document, the recipient, the epoch) and XChaCha20-Poly1305. `prev_key` is the
> previous epoch's key sealed under the new one, so that a member can read every earlier epoch.
>
> **Records.** A message of `log`, `upd` or `lease` is a JSON object `{v, k, e, a, n, c, s}` (and `x` on a
> seal): `k` is `meta`, `ver`, `upd`, `lease` or `seal`; `e` the epoch; `a` the author's principal; `n` a random
> 192-bit nonce; `c` the XChaCha20-Poly1305 ciphertext of the plaintext `{st, dev?, body, hint?}` under the
> epoch key with the document, `k`, `e` and `a` as associated data; `s` the author's Ed25519 signature over the
> length-prefixed (4-byte big-endian lengths) `dcp-streams/1`, `record`, the document, `k`, `e` (8 bytes), `a`, `x` (8 bytes, all ones when absent), `n` and `c`; a lease's plaintext is `{st, seq, body}`. A `seal` has no `n` or `c`, `x` = `e` + 1, and is
> sent with `Stream-Closed: true`. A reader MUST drop, without showing it: a message after a valid seal of its
> stream; of a kind that does not belong in the stream; whose `e` is not the epoch of the stream; whose
> signature fails; whose author is not in the epoch's roster or below `editor` (`owner` for `meta`; a seal
> MUST be from an owner); that fails authenticated decryption or its schema; or whose `created_by.principal`
> (version) or `holder.principal` (lease) is not `a`. A reader MUST ignore a second copy of a record (its identifier is the lower-case hex SHA-256 of its JSON with the fields in the order `v, k, e, a, n, c, s, x`). The identifier of a version is that identifier.
>
> **Roles.** `owner`, `editor` and `viewer` are the roster's roles. `commenter` is a viewer until the comment
> event is defined.
>
> **Writing.** A writer reads `ctl`, finds the latest epoch, checks its own role, and appends to that epoch's
> streams. On `409` with `Stream-Closed: true` the epoch was sealed: read `ctl` again and write into the next.
> A file is `PUT` closed, then read back, decrypted and compared before the version that names it is sent
> (version rule 2). A file blob is `DCPF`, `0x01`, the epoch (8 bytes), a 24-byte nonce and the ciphertext,
> with the document, a file id and the epoch as associated data; a `url` locator names it and a reader
> resolves only the path against its own server.
>
> **Changing the roster.** An owner makes the new key and `EpochRecord`, creates the new epoch's streams (the
> log with the owner's `meta` record), appends the record to `ctl`, reads `ctl` back (if another owner's record
> for that `n` came first it MUST report a conflict and retry), then seals every stream of the old epoch. The
> change takes effect for readers when the old epoch's streams are sealed; until then a removed member's
> records in the old epoch are valid. The old epoch's content stays readable to those who had its key.
>
> **Lease.** In the epoch's `lease` stream, every write is one `lease` record carrying its sequence number
> (the last valid record's + 1), appended with `Stream-Seq` equal to that number as 16 lower-case hex digits.
> The server's `409` is the compare-and-set: a writer whose append is accepted holds the lease (lease rules 3
> and 7 need no settle interval); a reader takes the valid record with the greatest sequence number. Rule 8's
> `renewal` is checked against the record just read before the append. A new epoch starts with no lease.
>
> **Time.** A client calibrates a clock once by sending `HEAD` every 10 ms until `Date` changes second, and
> advances the last `Date` by its monotonic clock; its error is then within tens of milliseconds, otherwise
> up to one second. A record's `st` is its author's estimate; a reader clamps it to its own estimate plus 30 s.
> A lease SHOULD NOT be shorter than 3 s unless every client calibrates.
>
> **Updates.** `upd` records carry `dev` (the author's device); `seq` is per author and device (4.4). An inline
> payload keeps the 34 000-byte limit (a 61 016-byte record). A version with `covers` MAY carry in its
> plaintext `hint` = `{epoch, offset}`, an offset of that epoch's `upd` stream; a reader that starts later MAY
> read the stream from there, MUST NOT if the offset is beyond the stream's tail, and trusts it no further than
> the editor who wrote it.
>
> **Failure.** `404` on a document the client has known is data loss, not an empty document. `410` on a read
> (retention) was not exercised. A server in memory mode loses everything on restart and MUST NOT be used. A
> file-backed server kept every acknowledged record across `kill -9` in the runs below.
>
> **Measured** (Node 0.3.9 and Go 0.3.0, one run, loopback, release build): append to a second client's verified
> read 3.9 ms p50 and 4.5 ms p95 (Node); 5 000 updates cold in 266 ms, 11 ms from a snapshot; a 100-byte update
> is a 747-byte record; a roster change 12 to 126 ms for 4 to 50 members. The 11 section-12 journeys pass with
> no skips. Not measured: TLS, a real network, any other server, presence (L3).

## Not verified

- **TLS and HTTP/2**: the client is built without a TLS stack and every run is plain `http` on loopback; the
  Go server skipped h2 and h3 (no TLS).
- **A real network**: latency, loss, connection resets beyond the 2 s idle-pool setting, proxies in front of a
  long poll, CDN caching of reads (the protocol designs for it; `ctl` would need not to be cached stale).
- **SSE mode**, and long-poll reconnection after the server's 30 s timeout; only long poll was used, and only
  in the latency test.
- **Retention, expiry and TTL** (`410`, `Stream-TTL`, `Stream-Expires-At`): a server that expires or trims a
  document's streams. The client reports an error; there is no recovery from a lost stream.
- **Forks** (`Stream-Forked-From`), a possible cheap snapshot or epoch mechanism; not tried.
- **The Go server's restart**, **power loss**, **a full disk**, **a second server process**, clustering.
- **A malicious server** that withholds, reorders or rolls back: reasoned, not run (the table above).
- **Anything beyond 50 members** or 55 epochs, more than two concurrent owners, more than 8 concurrent
  appenders, more than 5 000 updates, files over 32 MiB through the binding.
- **Clock drift** over minutes or hours, and a client that stays calibrated; tested over tens of seconds.
- **Multiple devices for one principal, key backup, identity key compromise and rotation, post-quantum
  primitives**: the principal is one key, nothing handles loss or theft of it.
- **A gate that follows the roster** (above): inferred only.
- **Idempotent producers as the retry mechanism**: measured to work, not adopted.
- **Another client implementation**, an independent reading of the bytes: the wire format here is read only by
  this crate. The Rust code is the only specification the signed byte strings have besides the proposal above.
- **The `measure` job's 256 MiB and 512 MiB cases on a smaller machine**: the Node server read a 512 MiB
  body into memory on a 16 GB runner; what a smaller one does was not run.

## Reproducing

```
cd tools/collab-spike
(cd scripts/streams-server && npm ci)                       # the pinned reference server

# Node reference server, file-backed
eval "$(scripts/streams-up.sh node)"
cargo test -p dcp-streams --test streams -- --nocapture --test-threads=2
scripts/streams-down.sh node

# the Go server (downloads v0.3.0 and checks its sha256), and the same suite
eval "$(STREAMS_PORT=4438 scripts/streams-up.sh caddy)"
cargo test -p dcp-streams --test streams -- --nocapture --test-threads=2 --skip restart
STREAMS_PORT=4438 scripts/streams-down.sh caddy

# a gate in front (basic_auth): the one test that needs it
eval "$(STREAMS_PORT=4439 scripts/streams-up.sh caddy-auth)"
DCP_STREAMS_AUTH_URL="$DCP_STREAMS_URL" env -u DCP_STREAMS_URL cargo test -p dcp-streams --test streams auth:: -- --nocapture
STREAMS_PORT=4439 scripts/streams-down.sh caddy-auth

# the measurements (release build, one at a time); DCP_MEASURE_MAX_MB limits the size test
eval "$(scripts/streams-up.sh node)"
DCP_MEASURE_OUT=/tmp/measure.jsonl DCP_MEASURE_MAX_MB=512 \
  cargo test --release -p dcp-streams --test streams measure:: -- --ignored --test-threads=1 --nocapture

# a test fails when no server is configured, and skips only when told to
env -u DCP_STREAMS_URL cargo test -p dcp-streams --test streams                        # fails: DCP_STREAMS_URL is not set
env -u DCP_STREAMS_URL DCP_SKIP_STREAMS=1 DCP_SKIP_STREAMS_AUTH=1 cargo test -p dcp-streams --test streams -- --nocapture   # SKIPPED ... (DCP_SKIP_STREAMS=1)
```

The tests that restart or kill a server start their own Node server (`node` and the installed
`scripts/streams-server` are needed; they fail without them unless `DCP_SKIP_STREAMS=1`). Raw logs and
measurements of run 40137430958 are its `streams-logs` and `streams-measurements` artifacts.
