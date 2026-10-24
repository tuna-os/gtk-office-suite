# RFC-0004 Spike A: Matrix binding results

Date: 2026-10-24 · Tracks: [RFC-0004](0004-interoperable-document-collaboration.md), [DCP draft 0 §8.1](../DOCUMENT-COLLABORATION-SPEC.md) · Code: [`tools/collab-spike/dcp-matrix/`](../../tools/collab-spike/dcp-matrix/) · Raw data: CI artifact `spike-a-measurements` of [run 40079034176](https://github.com/tuna-os/gtk-office-suite/actions/runs/40079034176)

> **Draft 1 follow-up (2026-10-24).** The measurements below are of the draft-0 binding and
> are not redone. Two behaviours described here changed with draft 1: the lease `renewal`
> counter replaced the 1 ms `duration_ms` alternation (the dedupe workaround under "The
> lease race"), and lease rule 3 is now two read-backs with a settle interval. See
> [the interop results](0004-spike-interop-results.md#draft-1-update).

**These numbers come from one CI run on a shared 2-core GitHub-hosted runner, with
Synapse in a container on the same host, over loopback.** They say what this
binding, matrix-sdk 0.19.1 and Synapse 1.163.2 did on our workloads. They are not
a benchmark of Matrix, and network latency to a real homeserver would be added to
every latency figure below. Where a figure is derived rather than measured the
text says so.

## Recommendation

**Go on the Matrix binding for L0, L1 and the version-carrying half of L2, with
six changes to the draft-0 text** (listed under [Spec corrections](#spec-corrections)).
The load-bearing results:

1. **Versions, roles, removal and files work end to end.** `MatrixStore` passes
   all 10 section-12 journeys of `dcp-conformance` against a real Synapse, under
   both the relaxed and Synapse's default rate limits, and a removed member's
   device really cannot decrypt the next version (0 leaks in 70 trials across
   seven variants, plus a control that proves the check can fail).
2. **The "3 timeline events per second per room" budget is wrong for a stock
   Synapse.** Synapse's default `rc_message` is 0.2 events/s per *user* with a
   burst of 10, shared by every event the user sends in any room, including room
   creation and state events. We measured 0.26 events/s sustained. The binding
   itself sustains 25 to 35 events/s when the server allows it. Inline updates
   (L2) are therefore not usable for live co-editing on a default homeserver;
   versions and leases are.
3. **The spec's power-level table does not protect an encrypted room.** The
   server sees every DCP timeline event as `m.room.encrypted`, so
   `events["org.tunaos.dcp.version"] = 50` is never consulted. Under the literal
   table a viewer's forged version was accepted by the server (reproduced). The
   fix is one more table entry, `m.room.encrypted: 50`.
4. **34000 bytes survives; the margin is 6%.** An inline payload of 34000 bytes
   is stored as a 61,510-byte event. The largest that worked was 36,109 bytes
   (65,265 stored); 36,156 was refused with `M_TOO_LARGE`.
5. **The lease read-back (lease rule 3) does not always resolve a race.** In 7
   of 40 forced simultaneous acquisitions both sessions believed they held the
   lease after their read-back, and in 40 of 40 the *final* state named
   exactly one holder. And renewing a lease with an unchanged record is
   silently ignored by Synapse.
6. **History is readable by a late joiner only through matrix-sdk's key-bundle
   sharing, and only if the joiner has seen the invite before it joins.** A new
   *device* of an existing member reads none of it.

## Method

- **Code.** `dcp-matrix` implements `dcp_core::DocumentStore` as `MatrixStore`
  (matrix-sdk `=0.19.1`, features `e2e-encryption`, `automatic-room-key-forwarding`,
  `rustls-aws-lc-rs`, in-memory state and crypto store; a `sqlite` cargo feature
  exists and was **not** built or run).
- **Server.** `scripts/synapse-up.sh <default|relaxed>` starts
  `docker.io/matrixdotorg/synapse:latest` (resolved to **Synapse/1.163.2**, image id
  `sha256:e0d446e9d9f2e0db07dc093219aee61ae32315a1672f81cdcaa44e21e58325a2`)
  under docker or podman (auto-detected; CI used docker 28.0.4; podman 4.9.3 was
  used locally to check the script and the `relaxed` profile, but no tests were
  run locally). SQLite database, no federation, shared-secret registration.
  The `default` profile adds no rate-limit keys, so Synapse's own defaults apply;
  `relaxed` sets every `rc_*` limit to 10,000/s. Users are created through the
  shared-secret admin endpoint (HMAC-SHA1), which returns an access token and
  device without logging in; it created every user on the default profile
  without a 429, so the default profile can make users.
- **Runner.** GitHub-hosted `ubuntu-24.04` (image 20261021.618), 2 vCPU, rustc
  1.99.1, debug (`test` profile) builds.
- **Commands.** Workflow
  [`.github/workflows/collab-spike.yml`](../../.github/workflows/collab-spike.yml) (job `matrix`;
  at the time of the run it was the separate `collab-spike-matrix.yml`, now merged into it);
  in `tools/collab-spike`:
  `cargo test -p dcp-matrix --no-fail-fast` (unit, journeys, conformance, against
  `relaxed`; again against `default` for journeys and conformance) and
  `cargo test -p dcp-matrix --test measure -- --ignored --test-threads=1 --nocapture`
  (against each profile). No `--locked`: `Cargo.lock` is regenerated later.
- **Code under test:** commit `6ed0c57`; green run
  [40079034176](https://github.com/tuna-os/gtk-office-suite/actions/runs/40079034176).
  Earlier runs on the way: [40063408450](https://github.com/tuna-os/gtk-office-suite/actions/runs/40063408450)
  (compiled; 8/10 conformance journeys, which exposed lease renewal and removal
  bugs, fixed) and [40072921112](https://github.com/tuna-os/gtk-office-suite/actions/runs/40072921112)
  (green; its data showed an editor who had not read v1, the late-joiner failure
  below) and [40076572478](https://github.com/tuna-os/gtk-office-suite/actions/runs/40076572478)
  (diagnostics that isolated it).
- **A test that needs Synapse fails when `DCP_SYNAPSE_URL` is unset** unless
  `DCP_SKIP_SYNAPSE=1`, in which case it prints `SKIPPED`.

### How the binding maps the spec

| Spec 8.1 | Done as |
|---|---|
| Room type | `m.room.create` `type: org.tunaos.dcp.document`, `m.federate: false`, preset `private_chat` |
| Encryption | `m.room.encryption` megolm in `initial_state`; every version/update is a timeline event, so it is encrypted |
| History | `m.room.history_visibility: shared` in `initial_state` |
| Meta, lease | state events `org.tunaos.dcp.meta` (level 100) and `org.tunaos.dcp.lease` (level 50), empty state key |
| Version, update | timeline events; the **event ID is the version ID**; sender and `origin_server_ts` come from the server, the device from the megolm session |
| Roles | owner 100 (`users`), editor 50, viewer 0 (`users_default`); event-type levels as the spec, **plus `m.room.encrypted: 50`**, `state_default` 100, `invite`/`kick`/`ban` 100 |
| Files | `client.upload_encrypted_file`, then the store **downloads and decrypts what it just uploaded and checks length and sha256 before publishing** (version rule 2); locator `matrix-encrypted-media` |
| Send queue | one worker per room, token bucket (default 3/s, burst 3), same transaction ID on every retry, backoff honouring `retry_after_ms`; matrix-sdk's own retry is disabled so every 429 is counted |
| Lease | state event, read back from `GET /rooms/{id}/state` (server-authoritative, no sync lag) |
| `now_ms` | see [Clock](#clock) |
| grant / revoke | invite (+ power-levels event) / kick (+ power-levels cleanup), each returning only after this device's own member list agrees |

## Results

### Conformance (spec section 12)

`cargo test -p dcp-matrix --test conformance`, run 40079034176:

| Journey (spec test) | relaxed | default |
|---|---|---|
| 1 convergence on one head | pass | pass |
| 2 second head shown, not hidden | pass | pass |
| 3 corrupt file rejected; truncated upload never published | pass | pass |
| 4 lease exclusion and read-back | pass | pass |
| 5 lease expiry and renewal | pass | pass |
| 6 roles enforced at the store | pass | pass |
| 7 removed member loses new content | pass | pass |
| 8 unknown algorithm falls back to versions | pass | pass |
| 9 stream gap recovered | pass | pass |

10 passed, 0 failed, 0 skipped on both profiles (the suite's two spec-3
journeys are listed once). In journey 3's truncated upload an HTTP-aware proxy halves the body of the
next media upload (rewriting `Content-Length`), and the publish fails with no
version event written. That the refusal came from the verify-after-upload step
rather than from an HTTP error was not asserted separately.
Test 10 (WOPI) and 11 (schemas) are not this binding's.

Also green: 7 unit tests and 10 Matrix-specific journeys (`tests/journeys.rs`) on
both profiles, including a viewer being refused by the *server* (403), a removed
member's device failing to decrypt the next event, and regressions for the two
bugs this spike found in its own binding (lease renewal; joining before the invite
is seen).

**Journey 7 passes partly for a reason the suite cannot see.** It hands the
removed viewer the new version's *metadata* (including the media key) and asks
whether `fetch_file` fails. Matrix media is not access-controlled per room; the
store refuses because it checks membership, which is a courtesy. The cryptographic
property is shown by the next section, not by that journey.

### Member removal

Per trial: owner, an editor and a viewer who is then removed; a new event is sent;
the owner fetches the ciphertext from the server and hands it to the removed
viewer's device, which tries to decrypt it with the keys it holds. The same
ciphertext is given to a remaining member as a control. 10 trials per variant on
`relaxed`, 2 on `default`:

| Variant | removed device decrypts the new event |
|---|---|
| owner `revoke()` (waits for its own member list), owner publishes | 0 / 10 |
| owner kicks directly, owner publishes (with its preflight reads) | 0 / 10 |
| owner kicks directly, owner sends at once with no preflight | 0 / 10 |
| owner `revoke()`, editor sends at once with no preflight | 0 / 10 |
| owner kicks without waiting, editor sends at once with no preflight | 0 / 10 |
| owner `revoke()`, editor publishes, stale-member guard off | 0 / 10 |
| owner `revoke()`, editor publishes, stale-member guard on | 0 / 10 |

In all 70 (and all 14 on `default`): the megolm session changed between the last
event before removal and the first after; the control decrypted; the removed
device *still* decrypted the event sent before its removal (spec section 11: what
it already received stays readable); the server answered the removed user's fetch
of the new event with 404; the store refused with `Forbidden`.

Not found: any window in which matrix-sdk encrypted for a removed member, even
with the guard off and no preflight. The window is bounded by the time the sender's
`/sync` takes to deliver the leave, which on loopback is a few milliseconds. The
`guard_stale_members` option (refuse to encrypt while the local member list still
shows someone the server says left) is therefore **unproven necessary**; it costs one
state read the publish path makes anyway. A real network would widen the window.

### Late joiner and new device

`history_visibility: shared` is **not enough** in an encrypted room: the server
hands over ciphertext, and the keys must come from somewhere. matrix-sdk 0.19.1
implements MSC4268: on invite the inviter uploads an encrypted bundle of the
room's keys and sends it to the invitee's devices, provided the inviter has set up
cross-signing. `MatrixStore::connect` does that bootstrap.

- A member invited after 2 versions and 1 update published read all of it, files
  verified, 150 ms after being granted (grant itself 209 ms).
- **Joining by room ID before the SDK has seen the invite loses the history.**
  In the diagnostic run 40076572478, a store granted the editor role (invite, then a
  power-levels event) and read at once saw 0 of 1 versions in 6 of 6 trials (3
  with the client limiter, 3 without); the same flow as viewer, or with the
  power-levels event first, or after a 3 s pause, saw it in every trial (3/3 each).
  The SDK log shows the bundle arriving before the invite ("historic room key
  bundle for an unknown room"), and in the failing trials no later attempt to
  accept it. The explanation, that the join came before the SDK had recorded who
  invited it, is an inference; the fix built on it worked. The store now waits for
  its own sync to deliver the invite before joining. After the fix (run
  40079034176): 3/3 in all seven variants on `relaxed`, 1/1 on `default`, and a
  regression journey.
- **A new device of an existing member reads nothing.** A second login of the
  same user, fresh crypto store, sees 11, 101, 301 and 1001 undecryptable events
  of 11, 101, 301, 1001 (every one). matrix-sdk offers key backup or verification
  for that; the spike did neither. A DCP client that reinstalls loses the live
  update history and has only the latest version file to go on, which section 6.4
  of the spec already makes acceptable for a compacted log but not for a lost one.

### Publish-to-visible latency (second user)

The reader is a second user with a single `/sync` long-poll loop; "visible" is the
moment the event is in a sync response decrypted. Loopback, 30 samples
(10 for versions), 300 ms between publishes. `key_arrived_after_event` was 0 in
every run: the room key always arrived before the event.

| Path | p50 | p95 | max |
|---|---|---|---|
| `send_queued` 400-byte update, from call start | 48.6 ms | 53.1 ms | 63.1 ms |
| same, from the moment the PUT returned | 25.6 ms | 30.8 ms | 31.0 ms |
| `publish_update` (role check + PUT + timestamp read-back), from call start | 55.9 ms | 59.6 ms | 60.2 ms |
| `publish_version`, 20 KB file (upload + verify-download + event), from call start | 70.6 ms | 76.8 ms | 76.8 ms |

On `default`, any publish that hits the server's rate limit adds the whole
`retry_after` (4.6 s for the one version in the sample; one of the two updates took 2.6 s).

### Event rate against the homeserver's limits

Each event is `publish_update`: a role check (GET), a queued PUT, a timestamp
read-back (GET); only the PUT is rate limited. A fresh user and room per row.

`relaxed` (rc_message 10,000/s):

| Client limiter | Events | Time | Achieved | 429s |
|---|---|---|---|---|
| none | 300 | 11.9 s | **25.2 /s** | 0 |
| 3/s, burst 3 (the spec) | 60 | 19.6 s | **3.1 /s** (4 in the first second) | 0 |
| 10/s, burst 10 | 100 | 9.1 s | 11.0 /s | 0 |

`default` (Synapse's shipped limits), 30 events each:

| Client limiter | Time | Achieved | 429s | Events before the first 429 |
|---|---|---|---|---|
| none | 115.8 s | **0.26 /s** | 23 | 7 |
| 3/s, burst 3 (the spec) | 115.7 s | 0.26 /s | 23 | 7 |
| 0.2/s, burst 10 (matched to Synapse) | 115.8 s | 0.26 /s | 23 | 7 |

The server answers with `retry_after_ms` of about 4.5 s; the queue honoured it
(103 s of the 115 s was waiting) and never dropped or reordered an event. The
7 is consistent with a 10-event burst of which room creation, the first version
and the warm-up event had already used 3 [INFERENCE from the arithmetic; the
per-call cost was not measured]. Nothing the client does at 3/s helps: the
client limiter changed nothing because the server's was the binding one.

### Inline update payload (E2EE and the 64 KiB event limit)

Payload bytes before base64, as the spec counts them; one update each, read back
and compared by a second user:

| Payload | Plaintext JSON | Stored event | Result |
|---|---|---|---|
| 24,000 | 32,178 | 43,739 | ok |
| 30,000 | 40,178 | 54,406 | ok |
| **34,000** | 45,514 | 61,510 | **ok** |
| 35,500 | 47,514 | 64,177 | ok |
| 35,875 | 48,014 | 64,859 | ok |
| 36,062 | 48,262 | 65,179 | ok |
| **36,109** | 48,327 | 65,265 | **ok (largest)** |
| 36,156 | 48,386 | n/a | `M_TOO_LARGE` (413) |
| 36,250 | 48,514 | n/a | `M_TOO_LARGE` |
| 37,000 | 49,514 | n/a | `M_TOO_LARGE` |
| 40,000 | 53,514 | n/a | `M_TOO_LARGE` |

(The bisection ran on `relaxed`; `default` tried only the first four sizes with the
same results.) The cost is about 1.78 bytes of stored event per payload byte
(base64 inside JSON, then megolm's base64), plus about 1.1 KB of envelope. The
limit is Synapse's 65,536-byte cap on the whole event, which includes the room ID,
event IDs, sender, signatures and hashes, so the ceiling moves with the lengths of
those; 36,109 holds for these identifiers (a 43-character `base_version`, 38-character
user IDs, server name `localhost`). A longer server name or user ID costs payload
bytes one for one.

### Encrypted media round trip

Upload encrypted, then download and decrypt to verify, then publish; a second user
lists and fetches. AES-CTR adds nothing to the length: the stored ciphertext is
exactly the plaintext size, and the version event is 1.6 KB whatever the file size
(the encryption keys and hashes sit in it). `relaxed`:

| File | `publish_version` (upload + verify + event) | second user: list | second user: fetch + decrypt |
|---|---|---|---|
| 1 KiB | 118 ms | 88 ms | 2.6 ms |
| 64 KiB | 99 ms | 41 ms | 15.7 ms |
| 1 MiB | 547 ms | 52 ms | 216 ms |
| 8 MiB | 3.9 s | 63 ms | 1.7 s |
| 32 MiB | 15.3 s | 74 ms | 6.8 s |

The 1 MiB row repeated on `default` (553 ms, 214 ms). Debug builds: decryption is
the cost (roughly 200 ms per MiB), so a release build will be faster. Synapse's
`max_upload_size` was left at 50M; 32 MiB is the largest tried.

### Room growth

Update events with a 1,000-byte payload, one room, `relaxed`. "Read" is a
full pass over the room history by a member that has the keys (11 pages of 100
events at 1,000); "new device" is a second login of the same user.

| Events | Full read, member | Synapse DB (sqlite) | Initial `/sync`, limit 10 | Initial `/sync`, limit 100 |
|---|---|---|---|---|
| 10 | 0.15 s | 5.4 MB | 36 KB | 39 KB |
| 100 | 1.2 s | 6.0 MB | 36 KB | 277 KB |
| 300 | 3.6 s | 7.4 MB | 36 KB | 277 KB |
| 1,000 | 12.2 s | 12.0 MB | 36 KB | 277 KB |

Reading is linear, about 12 ms per event in these debug builds, and the store has
no cache: it reads the whole timeline on every `versions()` and `updates()`. A
real client would sync incrementally; this is the worst case, not a design. The
database grows by about 6.6 KB per 1,000-byte update event (about 6.6 times the
payload, from 5.4 MB to 12.0 MB over 1,000 events). The `/sync` limit caps what a client downloads at first sync; the 277 KB
at limit 100 is 100 events, not the room.

### The lease race

40 rounds, `relaxed`, two editors starting at once (20 rounds) or staggered by
1 to 113 ms (20). Each runs lease rule 3 exactly: check, write, read back, believe
only if the record names me. Between rounds the lease is released and 1.1 s passed.

| Outcome | Rounds |
|---|---|
| Exactly one session believed it held the lease after its read-back | 33 |
| **Both believed it** | **7** |
| Neither | 0 |
| One session saw the other's active lease at the check and did not write | 17 (of the 40) |
| 500 ms later both read the same single record | 40 |
| The record then named one of the two believers | 40 |

In the seven split-brain rounds the two lease events were 0 to 33 ms apart in
`origin_server_ts` (0, 0, 1, 1, 5, 7, 33), which is consistent with the second
writer's PUT landing after the first writer's read-back [INFERENCE; the order of
the reads was not recorded]. So read-back **is** what makes the state converge
(there is always one winner), but it does **not** keep two sessions from each
believing they hold the lease for the next round trip; the loser finds out at its
next renewal or save. On `default` 4 rounds gave 0 splits (too few to say). Lease
rule 3 as written is "after writing, read back and trust it"; it needs "and
again after a settle time of at least the write-to-write gap you want to
tolerate", or a deterministic winner rule. Open question 3 of the spec (timeline
event plus a winner rule) is worth taking up.

**Renewal is silently ignored.** Synapse does not create a new event when a user
sends a state event with content identical to their own current one: it returns
the old event ID and the old timestamp (`state_event_dedup`: same sender twice
returns the same ID; a different sender with the same content creates a new event;
the same sender again after that creates a new event). A DCP renewal is the same
`Lease` object, so renewing never extended anything: journey 5 failed on it in run
40063408450. The binding now alternates `duration_ms` by one millisecond when the
content would be identical (59,999 / 60,000 in the measurement); the new event
and later timestamp are verified. A spec-level fix is a renewal counter in the
lease record.

### Clock

`now_ms` has no clock endpoint to ask. Three sources, 20 samples against an
`origin_server_ts` read from a probe event written for the purpose:

| Source | Error vs `origin_server_ts` | Cost |
|---|---|---|
| `Date` header of `GET /versions` | mean **−432 ms**, range −889 to +16 | 1.9 ms, no rate-limit slot, works for a viewer |
| Calibrated: `origin_server_ts` of the newest event this device wrote, advanced by a monotonic clock | mean +25 ms (the read happens after the reference), range 21 to 55; +21 ms after 20 s idle | 0, but needs a prior write |
| Probe event | exact | p50 53 ms, p95 58 ms, one rate-limit slot, editor role |

The `Date` header has one-second resolution and lags, so with the 2,000 ms leases
the conformance suite uses it would let an expired lease look active for up to a
second; the default is the calibrated clock, falling back to `Date` until the
device has written. The local clock minus the server's was +44 ms here, which says
nothing about real clients; lease rule 1's 30 s allowance stands.

### Offline and back

A TCP proxy between a client and Synapse is cut for 6 to 8 s.

| | This binding's queue | matrix-sdk's send queue |
|---|---|---|
| Call made during the outage | `publish_*` fails in under 1 ms with `Transport` (its preflight state read cannot connect); nothing is queued | local echo accepted at once |
| Events already queued when the network drops | 3 of 3 still pending after 6 s, retried with backoff, 3 retries counted | `SendError` after 1.5 s, flagged recoverable; **the room's queue disables itself** |
| After the network returns | all 3 sent 584 ms later, in order, 3 distinct event IDs, none duplicated (same transaction ID on every retry) | **0 of 3 sent in the next 15 s** until the room queue is enabled by hand; then all 3 within 89 ms |

So matrix-sdk's own send queue cannot be what carries "never drop a version": it
parks itself on a network error and needs a caller. The binding's queue retries
on its own up to `send_deadline` (180 s here); beyond that the version has to live
in the application's outbox, which `dcp-core` provides.

## Go / no-go per spec claim

| Spec 8.1 claim | Verdict | Evidence |
|---|---|---|
| One document is one room of type `org.tunaos.dcp.document` | **Go** | Synapse accepts the type; every journey ran on such rooms; a plain room is refused as `NotFound` |
| Events `meta`, `lease` (state) and `version`, `update` (timeline); event ID is the version ID | **Go** | conformance 10/10; ids, timestamps and senders agree across two readers |
| Power-level table gives `viewer 0 / editor 50 / owner 100` and gates the events | **No-go as written, go with one added entry** | literal table: viewer's forged encrypted version accepted by the server (reproduced); with `m.room.encrypted: 50`: 403. State-event levels (`lease` 50, `meta` 100) are enforced as written (403 tested) |
| `history_visibility: shared` lets a new member read what they are invited to | **Conditional go** | needs MSC4268 bundle sharing, inviter cross-signing, and the invite seen before the join; not for a new device |
| Encrypted media, plaintext sha256 recorded and verified | **Go** | verify-after-upload and a truncated upload refused; ciphertext length equals plaintext length |
| Queue per room, in order, back off on 429 | **Go** | 23 429s honoured, no drop or reorder; order kept across an outage |
| **At most 3 timeline events per second per room** | **No-go** as a default; **go** as a ceiling only | stock Synapse allows 0.2/s per user (burst 10); the client sustained 25 to 35/s unconstrained |
| An inline update payload of 34000 bytes fits | **Go, thin margin** | stores as 61,510 B; the largest that worked was 36,109 B |
| `commenter` reserved | **Open, and harder** | with `m.room.encrypted` gated at 50, a level-below-50 commenter cannot send comments either |
| Lease is a state event; read-back resolves a simultaneous write | **Partial** | one winner always; both believed in 7 of 40 first read-backs; renewal needs a changed record |
| `origin_server_ts` is the time | **Partial** | only obtainable by writing an event; `Date` header is 1 s coarse and lags by ~0.4 s |
| Removed member does not read later content (encrypted bindings) | **Go** | 0/70 plus controls, session rotated every time |
| Editor services join as visible members | **Not tested** | no editor service was run |

## Spec corrections

Replace or add the following in `DOCUMENT-COLLABORATION-SPEC.md` (not edited here).

1. **8.1 "Roles to power levels".** Add `events["m.room.encrypted"]: 50`, say that
   in an encrypted room it is the only timeline gate the server can apply, and
   keep the DCP type entries for unencrypted rooms. Also set `state_default`,
   `invite`, `kick` and `ban` to 100 (Synapse's default `invite` of 0 lets anyone
   invite). A reader SHOULD additionally drop DCP events from senders below the
   editor level when the table lacks the encrypted entry.
2. **8.1 "Ordering and rate".** Replace "SHOULD NOT send more than 3 timeline
   events per second to one room (NeoBoard's measured budget)" with: *the budget is the homeserver's, per user,
   across all rooms; Synapse's default is 0.2 events/s with a burst of 10; a client
   MUST take its pacing from `retry_after_ms` and MUST NOT assume 3/s.* Add that
   room creation, state events and the first publish spend the same tokens (7 of
   10 were left after setup in our runs). Inline updates (L2) over a default
   Synapse are about 5 s apart once the burst is gone; live co-editing needs a
   homeserver configured for it, or the file-encoding payload at version cadence.
3. **8.1 "Size".** The 34000-byte rule stands (stored 61,510 B, 6% under
   the 65,536-byte cap). State that the cap applies to the whole event and the
   ceiling was 36,109 bytes with 38-character user IDs on `localhost`; do not
   raise the limit, and lower it if server names or user IDs are longer.
4. **8.1 "Time" and lease rule 1.** Say that `origin_server_ts` exists only on events
   already written and offer: *the origin_server_ts of the newest event this device
   wrote plus elapsed monotonic time; or, for a device that has not written, the
   HTTP `Date` header with a stated error of one second.* A lease duration under
   about 3 s is not meaningful on a `Date` clock.
5. **4.3 / lease rules.** (a) A renewal MUST differ from the current record (add a
   counter field, e.g. `renewal`, to the lease schema); Synapse discards an identical
   state event from the same sender and answers with the old timestamp. (b) Rule 3
   needs "and read back again after a settle time", or a deterministic winner;
   a single read-back left both sessions believing in 7 of 40 races. (c) Say
   explicitly that two writers both succeed and the later one wins.
6. **8.1 "Encryption".** Add that `history_visibility: shared` only grants the
   server's ciphertext; readable history needs the inviter's cross-signing and the
   invitee to have seen the invite before joining (matrix-sdk), and that a new device of
   an existing member cannot read history without key backup or verification.
7. **11 and the editor-service paragraph.** The statement that removal stops new
   content is confirmed, with the condition that the sender's own member view
   already shows the removal (the binding waits for it). Add that Matrix media is
   not access-controlled per room, so the file key in a version event is the
   capability, and a removed member who kept it can still download that file.
8. **4.2 file reference.** `locator.file` is the Matrix `EncryptedFile` object as
   serialised by the SDK (`url`, `key`, `iv`, `hashes`, `v`); the spec text matches.
   `created_by.principal` SHOULD equal the event sender; the binding rejects events
   where it does not (implemented; not separately tested).
9. **10 "Binding rate limit".** Add the network case: matrix-sdk's send queue
   disables the room's queue on a recoverable error and does not resume by itself;
   a binding MUST either resume it or keep its own queue.

## Limitations of this spike

- Loopback only: no real network, no TLS, no federation (`m.federate: false`), one
  Synapse, SQLite, no workers. Latency figures exclude any network.
- Two or three simulated users on one machine; no Element or other Matrix client
  opened a DCP room, so interop with real clients is unverified.
- The room version was the server's default and was not recorded. The v12
  creator-power logic exists with unit tests but was not exercised against a v12 room.
- Key backup, secret storage and device verification were not tried; a new device
  reads nothing.
- The `sqlite` feature was not built.
- Release builds were not measured; decryption cost in particular will be lower.
- Editor services (`editor_services`, a service joining the room) were not built or
  tested; no presence (L3).
- The 429 behaviour was measured for `rc_message` only. `rc_joins`, `rc_invites`
  and `rc_login` have their own defaults; the binding retries 429s on room
  creation, join, invite and kick, but only message limits were pushed to.
- Per-trial counts are small (3 to 10); percentages are not claimed.
- Not verified: behaviour under a server restart, a very large room (more than
  1,000 events), concurrent writers beyond two, clock skew between the server and
  clients, and a homeserver other than Synapse.
