# Document Collaboration Protocol (DCP), draft 1

**Status**: specification draft, **not accepted** · The Matrix binding and the WOPI adapter have each been measured once; the OCM, Durable Streams and iroh bindings have not · **Rationale**: [RFC-0004](rfc/0004-interoperable-document-collaboration.md) · **Machine-readable parts**: [`collab-spec/`](collab-spec/)

> **Read this first.** Draft 1 is the first text that has been measured, and only once each way.
> The **Matrix binding** (8.1) was run against **Synapse 1.163.2** with matrix-sdk 0.19.1, and the
> **WOPI host adapter** (8.2) against **Collabora Online 26.04.5.2**, each in one CI run on loopback
> ([Spike A](rfc/0004-spike-a-results.md), [Spike D](rfc/0004-spike-d-results.md)); their limits and
> what was not run are stated in those sections. The **OCM and WebDAV** (8.3), **Durable Streams**
> and **iroh** (8.4) bindings, L3 presence and any update algorithm profile are **unmeasured**
> outline, and nothing in them has been built. The namespace `org.tunaos.dcp` is a placeholder and
> MUST be replaced by a registered one before any external implementer relies on it. Do not ship
> collaboration on this text.

The key words MUST, MUST NOT, SHOULD, SHOULD NOT and MAY are to be read as in RFC 2119.

## 1. Scope

DCP defines how several programs that edit the same office document agree on:

1. what the document is and who may do what to it (**identity and permissions**);
2. what its saved states are, and how they relate (**versions**);
3. who is allowed to save right now (**lease**);
4. optionally, how programs that share an algorithm exchange live edits (**update profile**);
5. optionally, who is looking at what (**presence**).

DCP does not define a document model, a CRDT, a rendering protocol, or a file format. The document
of record is an ordinary file (ODF or OOXML). A suite that cannot merge another suite's live edits
still interoperates through versions.

DCP separates these semantics from the system that carries them. A **binding** (section 8) maps the
semantics onto a sharing and sync plane such as Matrix, OCM with WebDAV, or WOPI.

### Non-goals

- Live co-editing between suites with different document models. This is a model-mapping problem,
  not a transport problem, and DCP does not attempt it.
- Defining encryption or access control primitives. DCP uses those of the binding.
- Server-side search, thumbnails or conversion.

## 2. Conformance levels

An implementation declares the highest level it supports in the document metadata (section 4.1).
Levels are cumulative from L1; L0 is implied.

| Level | Requirement | Sections |
|---|---|---|
| **L0** | Document identity and role mapping | 4.1, 5 |
| **L1** | Versions and lease | 4.2, 4.3, 6 |
| **L2** | Update profile for at least one algorithm | 4.4, 7 |
| **L3** | Presence | 4.5 |

An implementation MUST state which binding it implements. Conformance is per binding. A WOPI adapter
(8.2) is L1 with stated conditions and is never L2 or L3.

## 3. Terms

- **Principal**: an identifier native to the binding (for Matrix, a user ID such as
  `@alice:example.org`; for OCM, a federated address). Principals from different bindings are
  distinct unless explicitly linked (section 9.2).
- **Device**: one installation of an editor used by a principal. A device identifier is stable for
  the life of that installation.
- **Session**: one editing run on one device. A session identifier is unique per run.
- **Document**: one logical office file, identified by a binding-assigned identifier.
- **Version**: an immutable saved state of the file.
- **Head**: a version that no other version lists as a parent.
- **Editor service**: a server-side program that opens the file on behalf of a principal
  (for example a WOPI client such as Collabora Online). It sees plaintext.

## 4. Data model

All objects carry `"dcp": 1`. Schemas are in [`collab-spec/schemas/`](collab-spec/schemas/) and
examples in [`collab-spec/examples/`](collab-spec/examples/). `python3 docs/collab-spec/validate.py`
checks the examples against the schemas and checks that malformed variants are rejected, including
every draft 0 object (`"dcp": 0`). The number changes whenever an on-wire shape changes, and a
receiver MUST reject an object whose `dcp` it does not implement.
Receivers MUST ignore unknown fields only inside a binding's own envelope; DCP objects use
`additionalProperties: false`, so an object with an unknown field MUST be rejected.

### 4.1 Document metadata

Schema: [`meta.schema.json`](collab-spec/schemas/meta.schema.json). Example:
[`meta.json`](collab-spec/examples/meta.json).

| Field | Meaning |
|---|---|
| `title` | Display name, 1 to 255 characters. It carries no file extension; an adapter that needs a file name derives one (8.2). |
| `mime` | MIME type of the file, for example `application/vnd.oasis.opendocument.spreadsheet` |
| `levels` | Supported levels, a set drawn from `L0`..`L3`. MUST contain `L1`. |
| `algorithms` | Update-profile algorithm ids, such as `loro/1`. REQUIRED, and non-empty, if `levels` contains `L2`. |
| `editor_services` | Principals of editor services that are allowed to act on the document |

Algorithm ids have the form `name/version` with lowercase `name` and integer `version`.

### 4.2 Version

Schema: [`version.schema.json`](collab-spec/schemas/version.schema.json). Example:
[`version.json`](collab-spec/examples/version.json).

| Field | Meaning |
|---|---|
| `parents` | Version identifiers this version was saved from. Empty for the first version. At most 8. |
| `file` | A file reference (section 4.2.1) to the complete file |
| `created_by` | `principal` and `device`. On Matrix `principal` MUST equal the event's sender (8.1). |
| `message` | Optional note, at most 1000 characters |
| `covers` | Optional. Marks a version that includes all updates of one algorithm up to and including `through` (section 7.4) |

The version identifier is assigned by the binding.

#### 4.2.1 File reference

Schema: [`file-ref.schema.json`](collab-spec/schemas/file-ref.schema.json).

`mime`, `size` and `sha256` describe the **plaintext** file. `locator` says where to fetch it:

| `locator.type` | Required fields |
|---|---|
| `matrix-encrypted-media` | `file`: a Matrix encrypted-attachment object (`url`, `key`, `iv`, `hashes`, `v`) |
| `matrix-media` | `url` beginning `mxc://` |
| `webdav` | `href` beginning `https://` |
| `url` | `href` beginning `https://` |

#### Version rules

1. A version is **immutable**. Corrections are new versions.
2. A writer MUST upload the whole file, verify that the stored bytes hash to `sha256`, and only then
   publish the version. A reader MUST fetch the file, recompute `sha256` and `size`, and MUST
   discard the version if either differs.
3. A writer MUST NOT publish a version whose file it has not opened successfully with the format's
   own reader (for an OOXML or ODF file, a ZIP archive whose package structure parses).
4. A save creates a **child** of the version the editor started from. It MUST NOT overwrite or
   delete earlier versions.
5. If two versions share a parent, the document has more than one head. Programs MUST show that as
   a conflict, MUST NOT silently choose one, and MUST NOT merge heads unless the user asks or an
   algorithm profile defines the merge (section 7).
6. A reader MUST reject a version that lists itself, a descendant, or an unknown identifier among
   its parents, except that identifiers older than the retained history MAY be unknown.
7. The first version has `parents: []`. A document with no version has no content.

### 4.3 Lease

Schema: [`lease.schema.json`](collab-spec/schemas/lease.schema.json). Examples:
[`lease-held.json`](collab-spec/examples/lease-held.json),
[`lease-released.json`](collab-spec/examples/lease-released.json).

A lease says one session intends to save. It is **advisory**: it prevents accidental overwrite and
carries the WOPI lock token. It is not an access control mechanism (section 11).

| Field | Meaning |
|---|---|
| `state` | `held` or `released` |
| `holder` | `principal` and `session`. Required when held. |
| `token` | Opaque string, up to 256 characters. Required when held. It is the WOPI lock string when a WOPI client is in use. |
| `base_version` | The version the holder started from. Required when held, except that it MUST be omitted (never replaced by a placeholder such as `"none"`) when the document has no version yet. |
| `renewal` | Integer, 0 or more. Required when held. Makes every write over a held record a different record (rule 8). |
| `duration_ms` | How long the lease lasts from the time the binding stamped it, from 1 second to 1 hour. Required when held. A WOPI lease is always 1 800 000 (8.2). |

#### Lease rules

1. The binding supplies the time. Implementations MUST use the binding's own timestamp on the lease
   record and MUST NOT compare it with the local clock without allowing 30 seconds of skew. A
   binding whose clock is only available through written events or is coarse MUST state its error
   (8.1, Time), and a lease shorter than that error is not meaningful.
2. A lease is **active** while `state` is `held` and the binding timestamp plus `duration_ms` is
   later than the current binding time.
3. A session MAY acquire a lease only if none is active, or if it is the holder. After writing, it
   MUST read the lease back, wait a **settle interval**, read it back again, and treat itself as
   holder only if both records name its session and carry the `renewal` it wrote. A binding without
   atomic compare-and-set lets two sessions write at once (rule 7); the read-backs are what resolve
   it. A single read-back is not enough: on Matrix it left both sessions believing they held the
   lease in 7 of 40 forced races, although the final state named one holder in all 40. The settle
   interval MUST be at least as long as the gap between competing writes the binding must tolerate;
   section 8.1 gives the Matrix value. The loser learns of its loss at the second read-back and MUST
   NOT save as the holder.
4. The holder MUST renew at or before half of `duration_ms` has elapsed while it keeps editing, and
   MUST release the lease when it closes the document normally. A crash is handled by expiry. A
   renewal is a write under rule 8.
5. Another session MUST NOT save while a lease it does not hold is active, except that a principal
   with the `owner` role MAY break a lease. Breaking a lease MUST be visible to the former holder
   and recorded as a lease whose `state` is `held` by the breaker.
6. A save by a session without the active lease is still accepted as a **version** (rule 5 of the
   version rules makes it a branch if it forks). The lease prevents the fork in the normal case; it
   does not forbid it. Over WOPI an adapter refuses a save whose lease is no longer active (8.2
   rule 5).
7. **Concurrent writers.** Where the binding has no compare-and-set, two writers can both succeed;
   neither gets an error and the later write is the record. A session MUST NOT infer from a
   successful write that it holds the lease.
8. **Renewal counter.** A lease written over a record that is not `held` (released, or none) MUST
   have `renewal` 0. A lease written over a `held` record (a renewal by the holder, a takeover after
   expiry, a break) MUST have `renewal` equal to the replaced record's `renewal` plus 1. A store may
   discard a write whose content equals the sender's current record (Synapse does, per sender: it
   returns the old event ID and timestamp), so without a changed field a renewal extends nothing. A
   session treats a renewal as done only when the record it reads back carries the new `renewal`.

### 4.4 Update

Schema: [`update.schema.json`](collab-spec/schemas/update.schema.json). Example:
[`update.json`](collab-spec/examples/update.json). Defined only at L2.

| Field | Meaning |
|---|---|
| `algorithm` | The algorithm id that produced `payload`, such as `loro/1` |
| `doc_model` | The application's document-model id, such as `tables/1` |
| `base_version` | The version the update applies to |
| `seq` | Counter, starting at 0, increasing by 1 for each update from one `(principal, device)` stream |
| `payload` | Either `{encoding: "base64", data, size}` with `size` at most 34000 bytes, or `{encoding: "file", file}` where `file` is a file reference |

### 4.5 Presence

Presence is ephemeral: cursors, selections, the active page. Presence MUST NOT be written to a
durable log. A binding that has no ephemeral channel does not offer L3. The payload is defined by
the algorithm profile; DCP only requires that it carries the principal and session and that
receivers drop it after 15 seconds without refresh.

## 5. Roles

| Role | May |
|---|---|
| `viewer` | Read versions and metadata |
| `commenter` | Everything a viewer may, and add comments where the document model supports them |
| `editor` | Everything a commenter may, and publish versions, updates and leases |
| `owner` | Everything an editor may, change roles, and break a lease |

A binding MUST state how each role maps onto its permission model (sections 8 and 9). A program
MUST NOT assume a role from anything but the binding's permission data.

## 6. Version graph behaviour

1. **Head selection.** To open a document, a program computes the heads. With one head, it opens
   that version. With more than one, it shows the conflict and offers each head, newest first by
   binding timestamp.
2. **Divergent offline edits.** A program that was offline and then publishes a version based on an
   older head creates a second head. The user chooses.
3. **Merging heads.** When the user merges, the program publishes a version whose `parents` lists
   every merged head. How the merge is computed is up to the program. A tracked-changes comparison
   is one option; DCP does not define it.
4. **Retention.** A binding MAY drop versions that are older than a documented limit, but MUST
   keep every head and every version that a retained version lists as a parent, or mark it
   unknown (rule 6, version rules).

## 7. Update profile (L2)

1. **Same profile only.** A receiver applies an update only if it supports `algorithm` and
   `doc_model`. Otherwise it MUST ignore the update and use versions. It MUST NOT display a
   partial result.
2. **Order.** For one `(principal, device)` stream, receivers apply updates in `seq` order. A gap
   in `seq` means a lost update: the receiver MUST stop applying that stream and recover from the
   latest version.
3. **Validation.** A receiver MUST validate the result of applying an update (a dry run) before
   changing its live state, because updates carry instructions rather than data. An update that
   fails validation MUST be dropped and reported.
4. **Compaction.** A writer MAY publish a version whose `covers` names the algorithm and the last
   update included. After that version exists, receivers MAY discard older updates for that
   algorithm. A reader that starts later loads the version and applies only updates after it.
5. **Size.** A payload over 34000 bytes MUST use `encoding: "file"`. A binding MAY set a lower limit
   (8.1 shows why a Matrix deployment with long identifiers might).
6. **Determinism.** Two programs that apply the same updates in different orders MUST compute
   byte-identical stored values. This follows RFC-0001.

An algorithm profile for a given id (for example `loro/1`) is a separate document that fixes
encoding, document model mapping and the determinism test. None exists yet.

## 8. Bindings

### 8.1 Matrix binding (measured once)

Informed by NeoBoard's architecture decision records, ADR002 and ADR005, and by the 2022 Matrix
collaborative-documents draft. **Measured once**: Synapse 1.163.2 (`matrixdotorg/synapse:latest`,
SQLite, no federation) with matrix-sdk 0.19.1, in one CI run on a GitHub-hosted 2-core runner over
loopback, debug builds. Results: [Spike A results](rfc/0004-spike-a-results.md). The figures below
are what that binding did on those workloads, not a benchmark of Matrix; network latency would add
to every latency figure. Verdict: **go** for L0, L1 and the version-carrying half of L2 with the
rules below.

**Rooms.** One document is one room. The room's `m.room.create` content has
`"type": "org.tunaos.dcp.document"` (Synapse accepts it). Spaces (`m.space.child`) provide folders.
The room name is the title.

**Encryption.** The room MUST be end-to-end encrypted unless it is public by the owner's choice:
`m.room.encryption` (megolm) and `history_visibility: shared` go in the room's `initial_state`.
`shared` lets a late member **fetch the server's ciphertext only**; it does not hand over keys
(Element's own default for new rooms no longer exposes history to later joiners, so this is an
explicit choice). With matrix-sdk 0.19.1 a member invited later could read every earlier version
and update, files verified, only because of MSC4268 key-bundle sharing, which needs both:

- the inviter has set up cross-signing (a DCP client that creates or invites into rooms MUST
  bootstrap it); and
- the invitee's own sync has delivered the invite **before** it joins. A client MUST wait for the
  invite before joining by room ID: joining early read 0 of 1 versions in 6 of 6 trials, and the wait
  fixed it (the explanation, that the SDK had not yet recorded who invited it, is an inference).

**A new device of an existing member reads none of the room's history** (every event undecryptable
in rooms of 11, 101, 301 and 1001 events). The spike tried neither key backup nor device
verification; an implementation MUST NOT promise a new device access to history without one of them.

**Events.**

| Event type | Kind | `state_key` | Carries |
|---|---|---|---|
| `org.tunaos.dcp.meta` | state | `""` | Document metadata (4.1) |
| `org.tunaos.dcp.lease` | state | `""` | Lease (4.3) |
| `org.tunaos.dcp.version` | timeline | | Version (4.2). The event ID is the version ID. |
| `org.tunaos.dcp.update` | timeline | | Update (4.4) |

The event `content` is the DCP object. A client MUST ignore an event whose content does not
validate against the schema. **`created_by.principal` MUST equal the event's `sender`**: a writer
sets its own user ID and a reader MUST discard a version event where they differ, because the
server's signature on the event is the only authentication of the author (section 11). (Implemented
in the measured binding; not separately tested.)

**Roles to power levels.** The owner MUST set these in `m.room.power_levels`:

| Setting | Level |
|---|---|
| `users_default` | 0 (viewer) |
| `events["m.room.encrypted"]` | 50 |
| `events["org.tunaos.dcp.version"]` | 50 |
| `events["org.tunaos.dcp.update"]` | 50 |
| `events["org.tunaos.dcp.lease"]` | 50 |
| `events["org.tunaos.dcp.meta"]` | 100 |
| `state_default` | 100 |
| `invite`, `kick`, `ban` | 100 |
| users with role `editor` | 50 |
| users with role `owner` | 100 |

**In an encrypted room the server sees every DCP timeline event as `m.room.encrypted`, so
`events["m.room.encrypted"]` is the only timeline gate it can apply.** The entries for the DCP
timeline types only take effect in an unencrypted room and are kept for that case. With the
literal draft-0 table (no `m.room.encrypted` entry) a viewer's forged version was accepted by the
server (reproduced); with the entry it was refused `403`. The state-event levels (`lease` 50, `meta`
100) are enforced as written (`403` tested). `invite` is set because Synapse's default of 0 lets
anyone invite. A reader MUST check the room's power-levels event on joining, and when an encrypted
room's table lacks the `m.room.encrypted` entry SHOULD drop DCP timeline events from senders below
the editor level (50) whatever the server accepted.

A level of 50 on `m.room.encrypted` also stops any member below 50 from sending any encrypted event,
so a Matrix `commenter` cannot send comments either. `commenter` stays reserved until the comment
event is defined, and section 13 question 2 is harder than it was.

**Files.** A version's file is an encrypted attachment uploaded to the content repository, with
`locator.type` `matrix-encrypted-media` in an encrypted room. `locator.file` is the Matrix
`EncryptedFile` object as the SDK serialises it (`url`, `key`, `iv`, `hashes`, `v`). The plaintext
`sha256` is recorded in the version. The writer MUST download and decrypt what it just uploaded and
check length and `sha256` before publishing (version rule 2); a truncated upload was refused this
way and no version was written. AES-CTR adds nothing to the length. **Matrix media is not
access-controlled per room**: the key in the version event is the capability, and a removed member
who kept it can still download that file (section 11).

**Ordering and rate.** A client MUST queue DCP events per room and send them in the order created.
The send budget is the homeserver's, not DCP's: it is per **user**, shared by every event that user
sends in any room, and room creation, state events (lease, meta, power levels) and the first publish
spend the same tokens. Synapse's default `rc_message` is 0.2 events per second with a burst of 10;
against it the binding sustained 0.26 events/s, received 23 `M_LIMIT_EXCEEDED` in 30 events, and was
first refused after 7. A client therefore MUST NOT assume any rate and MUST take its pacing from the
server: on `M_LIMIT_EXCEEDED` it MUST wait the returned `retry_after_ms`, MUST keep the same
transaction ID on every retry, and MUST NOT drop or reorder an event while waiting (the measured
queue honoured all 23 without loss or reorder). A client SHOULD NOT send more than 3 timeline events
per second to one room (NeoBoard's budget). That is a **client-side ceiling**, never a rate it can
count on: on a homeserver with the limits lifted the binding sustained 25 to 35 events/s unthrottled
and 3.1/s at that ceiling, and on a default homeserver the ceiling changed nothing because the
server's limit was the binding one. A deployment that wants inline updates (L2) as live co-editing
MUST configure its homeserver for it: raise `rc_message` (`per_second` and `burst_count`) for the
DCP users to at least the ceiling its clients use (derived, not run at 3/s). On an unmodified Synapse
inline updates arrive about 4 to 5 seconds apart once the burst is spent, so a client SHOULD then
use versions only, or file-encoding payloads at version cadence. The limits `rc_joins`,
`rc_invites` and `rc_login` have their own defaults and were not pushed (the binding retries 429s on
room creation, join, invite and kick). For one `(user, device)` stream the room timeline gives
causal order. Events from different streams may interleave arbitrarily.

**Send queue.** matrix-sdk's own send queue is not enough: on a recoverable network error it
disables the room's queue and does not resume by itself (after a 6 to 8 s outage 0 of 3 queued
events were sent in the next 15 s until the queue was enabled by hand, then all 3 within 89 ms).
A binding MUST either resume that queue itself or keep its own. The measured binding keeps its own:
one worker per room, in order, same transaction ID on every retry; after the same outage all 3
events arrived in order with 3 distinct event IDs and no duplicate. A queue that gives up MUST hand
the version back to the application's outbox (section 10: never drop a version).

**Size.** An inline update payload MUST NOT exceed 34000 bytes before encoding (the schema
maximum). Measured: a 34000-byte payload is a 45,514-byte plaintext event stored as 61,510 bytes,
about 6% under Synapse's 65,536-byte cap; the largest payload that worked was **36,109 bytes**
(65,265 stored), and 36,156 was refused `M_TOO_LARGE`. The cost is about 1.78 stored bytes per
payload byte plus about 1.1 KB of envelope. The cap applies to the whole event, including room ID,
event IDs, sender, signatures and hashes, so the ceiling moves with their lengths: 36,109 holds for
38-character user IDs, a 43-character `base_version` and the server name `localhost`, and each
additional byte in those identifiers costs one payload byte. The limit MUST NOT be raised, and an
implementation on a deployment with longer server names or user IDs SHOULD lower it. A client
refused with `M_TOO_LARGE` SHOULD resend as `encoding: "file"`.

**Lease.** The lease is a state event, and Matrix state events have no compare-and-set: two writers
both succeed and the later event is the room's state (lease rule 7). The lease rules apply with
these Matrix specifics:

- *Settle re-read (rule 3).* In 40 forced simultaneous acquisitions the final state named exactly
  one holder every time, but after one read-back both sessions believed they held the lease in 7 of
  40 (their events were 0 to 33 ms apart in `origin_server_ts`). A session MUST therefore read back,
  wait a settle interval, and read back again, and believe itself the holder only if both reads name
  it. The interval SHOULD be at least 500 ms: 500 ms after the writes all 40 rounds read one
  record. The two-read rule itself was not run. Read the lease from `GET /rooms/{room_id}/state`
  (server-authoritative), not from the local sync view.
- *Renewal changes the record (rule 8).* Synapse does not create a new event when a user sends a
  state event with content identical to its own current one: it returns the old event ID and
  timestamp, so a renewal that repeats the record extends nothing (conformance journey 5 failed on
  exactly this). Hence the `renewal` counter.

Whether the lease should be a timeline event with a deterministic winner rule remains open
(section 13, question 3).

**Time.** The binding's time is `origin_server_ts`, but there is no clock endpoint: it exists only
on an event that has already been written. A device obtains the time from one of:

1. the `origin_server_ts` of the newest event this device wrote, advanced by a monotonic clock
   (measured error: mean +25 ms, range 21 to 55 ms; +21 ms after 20 s idle). A device SHOULD use
   this;
2. until it has written anything (a viewer never will), the HTTP `Date` header of any response. It
   has one-second resolution and lagged by a mean of 432 ms (range −889 to +16 ms); its error MUST
   be taken as 1 second;
3. a probe event written for the purpose (exact; one rate-limit slot; editor role; p50 53 ms).

On a `Date` clock a lease shorter than about 3 seconds is not meaningful (an expired lease can look
active for up to a second), so a Matrix lease SHOULD NOT be shorter than 3 seconds. The 30 seconds of
skew allowed against the local clock (lease rule 1) stands; the one local-minus-server difference
measured (+44 ms) says nothing about real clients.

**Member removal (measured).** After a member was removed, a new event was not decryptable by that
member's device in 70 of 70 trials across seven variants (and 14 of 14 on default limits), with a
control that did decrypt, the megolm session changed every time, and the server answered the removed
user's fetch with 404. The condition is that the sender's own member view already shows the
removal; the binding waits for it before it sends. What the removed device had already received
stays readable (section 11).

**Editor services.** An editor service that needs plaintext MUST join the room as a visible member
listed in `editor_services`. A client MUST show the list of editor services to every member and
MUST show when one holds the lease. The room's keys are available to that member. Not built or
tested.

**Not measured.** Real network, TLS, federation or any homeserver other than Synapse; interop with a
real Matrix client (Element); a Matrix v12 room; key backup, secret storage and device verification;
the `sqlite` crypto store; release builds; editor services and presence (L3); server restart;
rooms beyond 1,000 events (reading was linear, about 12 ms per event in debug builds, with no
cache); concurrent writers beyond two; clock skew between the server and clients.

### 8.2 WOPI host adapter (measured once)

A program implementing this adapter acts as a WOPI **host** for any WOPI client (for example
Collabora Online) and stores documents through another binding. It makes a session-hosted editor an
L1 participant without changing the editor.

**What was measured.** One build, one CI run: Collabora Online 26.04.5.2 (`collabora/code:latest`
of 2026-10-23) opened a document served by this adapter, a user typed, the document was saved, and
the store held a new version whose only parent was the version Collabora had opened. The save sent
the lease's lock string; a stale save was refused as a conflict and published nothing; a viewer
got a read-only session that never locked or saved. Results and raw logs:
[Spike D results](rfc/0004-spike-d-results.md). Everything below marked *measured* was observed in
that run and is not a statement about other WOPI clients or other Collabora builds. Collabora's own
SDK documentation was not readable; nothing here is cited to it.

**Verdict.** L1 over WOPI is **GO with conditions** (the numbered rules below). **L2 and L3 are
not offered over WOPI**: WOPI has no update stream, no presence and no role changes, and an adapter
MUST NOT declare `L2` or `L3` for a document it serves to a WOPI client. WOPI also **cannot express
two heads or a merge** (a save has exactly one parent), which is why the L1 it offers is
conditional (rule 6).

| WOPI operation | DCP mapping |
|---|---|
| `CheckFileInfo` | `BaseFileName` from the title (rule 8), `Size` and `Version` from the head version, `LastModifiedTime` (rule 3), `SupportsExtendedLockLength` false (rule 4). `UserCanWrite` true only for `editor` or `owner` and only when the document has exactly one head (rule 6); `UserCanOnlyComment` true for `commenter` |
| `GetFile` | Fetch and verify the head version's file, and record the version returned against the access token (rule 1) |
| `PutFile` | Verify the lock and the timestamp (rules 3 and 5), then publish a version whose parent is the version recorded for the token |
| `Lock`, `GetLock`, `RefreshLock`, `Unlock`, `UnlockAndRelock` | Acquire, read, renew, release the lease; `token` holds the WOPI lock string. Any other `X-WOPI-Override` MUST answer 501 |

**Call order (measured).** For one editing session Collabora sent: `CheckFileInfo`, `GetFile`,
`LOCK`, `PutFile` (once per save), `UNLOCK` when the last editing session disconnected. The lock is
taken **after** the download. Headers on those requests:

| Request | Headers Collabora set |
|---|---|
| `LOCK` | `X-WOPI-Override: LOCK`, `X-WOPI-Lock: cool-lock<8 hex digits>`; the same string for the whole session. Refresh was the same `LOCK` again with the identical string, not `REFRESH_LOCK` |
| `PutFile` | `X-WOPI-Override: PUT`, `X-WOPI-Lock`, `X-COOL-WOPI-Timestamp` (an ISO 8601 string), `X-COOL-WOPI-IsModifiedByUser`, `X-COOL-WOPI-IsAutosave`, `X-COOL-WOPI-IsExitSave` (each `true` or `false`) |
| `UNLOCK` | `X-WOPI-Override: UNLOCK`, `X-WOPI-Lock` |

The access token travelled in the query string, and `X-WOPI-Editors` was not sent. Collabora sent
no `GET_LOCK`, `REFRESH_LOCK` or `UNLOCK_AND_RELOCK` in one session; the adapter MUST still answer
them for other clients.

**Adapter rules.**

1. **Base version per token.** WOPI gives a `PutFile` no way to name the version it was edited
   from. The adapter MUST record, per access token, the version last returned by `GetFile` (or
   written by that token's `PutFile`) and MUST use it as the parent of the version a `PutFile`
   publishes. It SHOULD refuse a `PutFile` from a token that has not read a version. The lease's
   `base_version` MUST be that recorded version, not the head at the time of `LOCK` (the lock comes
   after the read), and MUST be omitted, not filled with a placeholder, when the document has no
   version yet (section 4.3).
2. **The lease maps onto the lock.** `LOCK` on an unlocked file writes a held lease whose `token` is
   the lock string and reads it back (lease rule 3); `LOCK` with the same string is a refresh (a
   new record with `renewal` increased by one, lease rule 8);
   `LOCK` with another string is `409` with the current string in `X-WOPI-Lock` and changes nothing;
   `GET_LOCK` returns the current string, or the empty string when there is no active lease;
   `UNLOCK` with the matching string releases it, otherwise `409` with the current (or empty)
   string; `LOCK` with `X-WOPI-OldLock` is `UNLOCK_AND_RELOCK`. An empty or missing lock string is
   `400`. An expired lease is an unlocked file.
3. **Conflict is the timestamp, not the lock (measured).** The lease does not stop a native DCP
   writer (lease rule 6), so only the timestamp protects the editor's user from overwriting that
   writer's version. The adapter MUST give `CheckFileInfo` a `LastModifiedTime` string and the
   `PutFile` response a new one, MUST compare `X-COOL-WOPI-Timestamp` (or the legacy
   `X-LOOL-WOPI-Timestamp`, which it MUST also accept) with the string it last gave, and on a
   difference MUST publish nothing and answer **`409` with the JSON body
   `{"COOLStatusCode":1010,"Message":"document changed in storage"}` and no `X-WOPI-Lock` header**.
   The absence of `X-WOPI-Lock` is required: Collabora's source reads a `409` that carries
   `X-WOPI-Lock` and no `COOLStatusCode` as a lock mismatch ("locked by another editor"), not a
   changed document. Collabora then reported "Conflict: Document changed in storage" to its client
   and sent `UNLOCK` (measured). A save without the header (Collabora's forced save) is accepted as
   a child of the token's base version and so makes a second head; this was run against the simulator
   only, not against Collabora.
4. **Fixed lock lifetime and token length.** A WOPI lock lasts 30 minutes and a refresh resets it to
   30 minutes. The adapter MUST write `duration_ms` 1 800 000 for a WOPI lease; DCP's range (1 second
   to 1 hour) is wider than WOPI can use. Collabora refreshed at its configured period (default
   900 s, `storage.wopi.locking.refresh`), exactly half the lifetime, which is why lease rule 4 says
   "at or before half". A lock string is at most 256 characters: the adapter MUST report
   `SupportsExtendedLockLength` false and MUST answer a longer string with `400` (WOPI itself
   allows 1024; Collabora's strings are 17 characters).
5. **A lock is not owned, and no "save anyway".** A WOPI lock is authorised by the string alone: a
   different token can refresh or release it, and an owner's lease break (lease rule 5) has no WOPI
   call; breaking is done at the store and the WOPI client finds out from the `409` on its next
   `PutFile`. A `PutFile` carrying a lock string on a file whose lease is not active is `409` with an
   empty `X-WOPI-Lock`: a WOPI session cannot save after its lease expired, it must re-lock first.
   This is an exception to section 10's "save as a version anyway" row; the adapter MUST NOT publish
   in that case. Collabora's reaction to that `409` was not run.
6. **Two heads and merges.** A WOPI client cannot show a conflict and a `PutFile` has one parent.
   When the document has more than one head the adapter MUST NOT offer a writable session
   (`UserCanWrite` false) and SHOULD list the heads in an extension field (this spike's host uses
   `DcpHeads`); the user merges in a DCP-native client (version rule 5, section 6). This is why L1
   over WOPI is conditional: a WOPI editor forks a document only by saving without the timestamp
   header (rule 3), and can neither show nor merge two heads.
7. **Roles.** `viewer` maps to `UserCanWrite` false and `UserCanOnlyComment` false; Collabora opened
   it `perm: readonly`, sent no `LOCK` or `PutFile`, and typing and `save` changed nothing
   (measured). `commenter` maps to `UserCanOnlyComment` true (not run against Collabora).
   `editor` and `owner` are the same in the standard fields (`UserCanWrite` true).
8. **File name.** Collabora rejects a `BaseFileName` containing `/` and picks the application from
   the extension. `title` (4.1) has neither rule, so the adapter MUST NOT pass a `/` and MUST derive
   an extension from `mime` when the title has none.
9. **What is lost.** A WOPI save carries no `message` (the adapter writes
   `WOPI PutFile (autosave=…, exit_save=…)`), and `created_by.principal` is the principal behind the
   token because `X-WOPI-Editors` was not sent. There is no version listing, so a DCP-aware client
   of this adapter needs extensions (this spike's host offers `DcpVersions` behind `?dcp_history=1`
   and `GetFile?dcp_version=<id>`); a stock WOPI client, Collabora included, never uses them.
10. **Tokens and file ids.** The adapter mints access tokens (conformance test 10 uses the host's
    registry `token -> (document, store handle, principal)`). Collabora's document broker is keyed
    by the `WOPISrc` without the token, so a file id MUST NOT be reused while a session on it is
    closing (a new session was refused `docunloading`), and `UNLOCK` arrives only when the last
    editing session disconnects, not on `closedocument` alone. A non-browser driver must send an
    `Origin` header on the WebSocket upgrade (Collabora 26.04 answers `403` otherwise).

**Not measured.** Two Collabora sessions on one document; autosave and exit-save `PutFile` and how
many versions they create (version churn against a rate-limited binding is open); a forced save
after a conflict; a lock mismatch against Collabora; lease expiry during an edit; `commenter`
through Collabora; HTTPS and WOPI proof keys (`X-WOPI-Proof`: the reference host checks the access
token only and MUST NOT be deployed as it is); other WOPI clients (ONLYOFFICE, Microsoft 365) and
other Collabora builds; large files; the adapter over the Matrix binding (every run used an
in-memory store).

**Conformance over WOPI.** Of the journeys in section 12, tests 1, 3 and 6 can be expressed through
WOPI and passed against the simulator (4 journeys passed counting the two halves of test 3, 0
failed), and test 10 passed against Collabora; 2, 4, 5, 7, 8 and 9 (6 journeys) were skipped with
the missing capability stated, never reported as passed. Lease exclusion and expiry (4 and 5) are
tested on the WOPI wire in the host's own tests.

### 8.3 OCM and WebDAV binding (outline, unmeasured)

Informative until Spike E, which has not run. Open Cloud Mesh already defines discovery, invites, share creation and
notification, WebDAV access and a web-app access mode. DCP does not redefine any of it.

- A document is an OCM share of a file. A DCP principal is an OCM address.
- A version is a file state at an ETag. Publishing a version is a conditional `PUT`
  (`If-Match`). A `412` response means another version was published first; the program MUST
  treat that as a second head, not retry blindly.
- A lease SHOULD use a WebDAV `LOCK` with a timeout where the server advertises locking. Whether
  Nextcloud and ownCloud do is not verified.
- L2 and L3 need a channel OCM does not define.

### 8.4 Reserved

Durable Streams (an append-only HTTP log with offset resume) and iroh with p2panda are reserved for
bindings after RFC-0004's Spikes B and C. **Neither has been built or measured**; no text here is
evidence about them.

## 9. Identity

1. A principal is whatever the binding says it is.
2. Principals from different bindings MUST NOT be treated as the same person unless both have
   signed a link statement. The format of that statement is not defined in draft 1.
3. A program MUST show the principal behind every version, lease and update it displays.

## 10. Failure behaviour

| Situation | Required behaviour |
|---|---|
| Network lost while editing | Keep editing locally. On reconnect, publish a version based on the last known head. If the head moved, this creates a second head. |
| Lease expired while editing | Continue locally. Before saving, try to reacquire. If another session holds it, save as a version anyway (lease rule 6) and show the resulting conflict. **Not over WOPI**: a `PutFile` with a lock string on an unlocked file is a `409`, so a WOPI session must re-lock first (8.2 rule 5). |
| File hash mismatch | Discard the version and report which version. |
| Update fails validation | Drop it, report it, keep the last valid state. |
| Update stream gap | Stop applying that stream and reload from the latest version. |
| Unknown algorithm | Use versions only. |
| Binding rate limit | Back off and queue, honouring the server's `retry_after_ms`. Never drop a version. A queue that gives up hands the version back to the application's outbox; a network error that disables the binding's own queue (matrix-sdk's does) MUST be resumed (8.1). |

## 11. Security considerations

- **A lease is not a permission.** Any member with the editor role can publish a version whether or
  not it holds the lease. Role mapping is what enforces who may write.
- **Revocation has limits.** Removing a member stops them from receiving new content in an
  encrypted binding: measured on Matrix, a removed member's device decrypted none of the next
  events (0 of 70, with a control), on the condition that the sender's own member view already shows
  the removal. Content they already received stays readable to them, including a file whose key they
  hold. DCP does not claim otherwise, and an implementation MUST NOT either, unless a journey proves
  a stronger property.
- **Editor services see plaintext.** Joining one to a document gives its operator access to that
  document. This MUST be shown to users (section 8.1).
- **Untrusted files.** A version's file comes from another principal. Programs MUST open it with the
  same hardening as any untrusted download: bound decompression, reject malformed packages and
  limit sizes. DCP does not make a file trustworthy because a member published it.
- **Untrusted updates.** Section 7 rule 3.
- **Hashes.** `sha256` detects corruption and mix-ups. It does not authenticate the author;
  authorship comes from the binding's signatures (for Matrix, the event signature), which is why a
  Matrix reader checks that `created_by.principal` equals the event's `sender` (section 8.1).
- **Media keys are capabilities.** Matrix media is not access-controlled per room: the key in a
  version event is what protects the file. A removed member who kept that key can still download that
  file, and a removal does not revoke it.
- **Roles on an encrypted Matrix room.** A power-level table without `events["m.room.encrypted"]`
  lets a viewer's forged version through the server (reproduced); see section 8.1.
- **A WOPI host in production.** The measured adapter validates the access token only. A deployment
  MUST use HTTPS and SHOULD validate WOPI proof keys (`X-WOPI-Proof`); neither was exercised.

## 12. Conformance tests

A DCP implementation is demonstrated, as in this repository's definition of done, by journeys and
not by source inspection. The following MUST pass for each binding an implementation claims.

| # | Claim | Evidence |
|---|---|---|
| 1 | Two instances converge on the same head | Two app instances; both show the same head and content |
| 2 | A second head is shown, not hidden | Two offline edits from one version produce a conflict listing both heads |
| 3 | A version is complete and valid | A truncated upload is never published; a hash mismatch is rejected by the reader |
| 4 | Lease: a second session cannot take an active lease | Journey with two sessions; read-back resolves a simultaneous write |
| 5 | Lease expiry releases the document | Kill the holder; another session acquires after expiry |
| 6 | Roles are enforced | A viewer cannot publish; an editor can; checked at the binding, not only in the UI |
| 7 | Removed members lose new content | Remove a member, publish, assert that device cannot read the new version (encrypted bindings) |
| 8 | An unknown algorithm falls back to versions | An L2 update from an unsupported algorithm is ignored and the document still opens from versions |
| 9 | A stream gap is recovered | Drop one update; the receiver reloads from the latest version |
| 10 | A WOPI client round-trips (8.2) | Collabora Online opens, edits and saves; the new version has the correct parent and the lease token is Collabora's lock string. The access token is minted by the adapter (8.2 rule 10). Passed once on Collabora Online 26.04.5.2 |
| 11 | Schemas hold | `python3 docs/collab-spec/validate.py` passes |

Each test is entered in `conformance/capabilities.json` only with the revision on which it passed.
A binding that cannot express a journey reports it skipped with the missing capability stated, never
as passed (over WOPI: 1, 3 and 6 passed through the simulator, 10 against Collabora, and 2, 4, 5, 7, 8
and 9 skipped). A test that needs Synapse or Collabora MUST fail when its environment is missing
unless `DCP_SKIP_SYNAPSE=1` or `DCP_SKIP_COLLABORA=1` is set, and then MUST print that it skipped.
This draft adds no entry.

## 13. Open questions

1. The registered namespace to replace `org.tunaos.dcp`.
2. The `commenter` role: its event, and its power level. Harder than in draft 0: on Matrix the
   `m.room.encrypted` gate at 50 (8.1) stops a member below 50 from sending any encrypted event,
   comments included.
3. Whether the Matrix lease should be a state event at all, given the missing compare-and-set. The
   settle re-read and the `renewal` counter make it work in the measured runs, but a timeline event
   with a deterministic winner rule may fit better; the two-read rule has not been run.
4. Whether unstable MatrixRTC and sticky events should ever carry L3.
5. The link statement format (section 9.2).
6. Whether OCM and WebDAV can express leases and heads, or only versions.
7. Whether L2 needs a homeserver configured for it. A default Synapse allows 0.2 inline events per
   second per user (8.1), so live co-editing needs raised `rc_message` limits or file-encoding
   payloads at version cadence. The 34000 byte limit survived measurement with a 6% margin.
8. A first algorithm profile for `loro/1`, and whether a `yjs/13` profile is needed for
   interoperability with LibreOffice's Yjs-based prototype.
9. How a hosted editor in an encrypted room is shown to users. Editor services were not built or
   tested in Spike A.
10. How a WOPI session's autosaves and exit-saves (one version each?) fit a rate-limited binding.
    Not exercised in Spike D.

## Changelog

- Draft 1 (2026-10-24): the Matrix binding (8.1) and the WOPI host adapter (8.2) were each measured
  once (Synapse 1.163.2 with matrix-sdk 0.19.1; Collabora Online 26.04.5.2); see
  [Spike A](rfc/0004-spike-a-results.md) and [Spike D](rfc/0004-spike-d-results.md). Rewritten
  from those results: 8.1 (power levels, rate, size, time, lease, encryption, send queue, media,
  `created_by`), 8.2 (call order, headers, lock lifetime, conflict form, per-token base version,
  two heads), sections 4.3, 10, 11, 12 and 13. **On-wire changes from draft 0**, which is why `dcp`
  is now `1`:
  - the lease gains `renewal`, an integer of 0 or more, REQUIRED when `state` is `held`;
  - `base_version` is no longer required on a held lease (omitted when the document has no
    version; no placeholder);
  - the Matrix power-level table gains `events["m.room.encrypted"]` 50 and `state_default`,
    `invite`, `kick`, `ban` 100 (a room created from the draft-0 table does not protect an
    encrypted room);
  - a Matrix version event's `created_by.principal` MUST equal the event's `sender`;
  - a WOPI lease has `duration_ms` 1 800 000 and an adapter answers a stale timestamp with `409`
    and `COOLStatusCode` 1010.

  Draft 0 objects (`"dcp": 0`) are rejected by the draft 1 schemas. Unchanged and unmeasured: the OCM
  and WebDAV outline (8.3), Durable Streams and iroh (8.4), L3 presence, and any `loro/1` profile.
- Draft 0 (2026-10-24): first text. Schemas and examples validate; no implementation.
