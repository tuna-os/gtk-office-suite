# Document Collaboration Protocol (DCP), draft 1

**Status**: specification draft, **not accepted** · Each of the five bindings (Matrix, WOPI, OCM with WebDAV, Durable Streams, iroh with p2panda) has been measured once, by one implementation; L3 presence and any update algorithm profile have not been measured · **Rationale**: [RFC-0004](rfc/0004-interoperable-document-collaboration.md) · **Machine-readable parts**: [`collab-spec/`](collab-spec/)

> **Read this first.** Draft 1 is text that has been measured, once for each binding, by the one
> implementation that wrote it. The **Matrix binding** (8.1) was run against **Synapse 1.163.2**
> with matrix-sdk 0.19.1 ([Spike A](rfc/0004-spike-a-results.md), and again on default rate limits
> in [Spike F](rfc/0004-spike-f-results.md)); the **WOPI host adapter** (8.2) against **Collabora
> Online 26.04.5.2** ([Spike D](rfc/0004-spike-d-results.md), Spike F); the **OCM and WebDAV
> binding** (8.3) against **Nextcloud 34.0.5 and 35.0.2** ([Spike E](rfc/0004-spike-e-results.md));
> the **Durable Streams binding** (8.4) against **`@durable-streams/server` 0.3.9 and the Go server
> v0.3.0** ([Spike B](rfc/0004-spike-b-results.md)); the **iroh and p2panda binding** (8.5) with
> **iroh 1.4.1 and p2panda 0.7.1** ([Spike C](rfc/0004-spike-c-results.md)). All ran in CI on
> loopback or one docker network, except where a section says so (Durable Streams was also run
> over TLS 1.3 with HTTP/1.1 and HTTP/2, [TLS spike](rfc/0004-spike-tls-results.md); the iroh
> binding was also run through a real `iroh-relay` in network namespaces on shaped links, [relay
> spike](rfc/0004-spike-relay-results.md); the Matrix reader rule was measured in [Spike
> Q15](rfc/0004-spike-q15-results.md)), and each section
> states its limits and what was not run. **L3 presence and any update algorithm profile are
> unmeasured**, nothing in them has been built, and no binding has had a second implementation read
> its bytes. The namespace `org.tunaos.dcp` is a placeholder and MUST be replaced by a registered
> one before any external implementer relies on it. Do not ship collaboration on this text.

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
semantics onto a sharing and sync plane such as Matrix, OCM with WebDAV, WOPI, Durable Streams or a
peer-to-peer network.

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
(8.2) is L1 with stated conditions and is never L2 or L3. The OCM and WebDAV binding (8.3) is L1
for `viewer` and `editor` and never L2 or L3; the Durable Streams binding (8.4) is L2 and has no
L3; the peer-to-peer binding (8.5) is L1 with an advisory lease and L2 within a size limit.

## 3. Terms

- **Principal**: an identifier native to the binding (for Matrix, a user ID such as
  `@alice:example.org`; for OCM, a federated address such as `bob@nc-b`; for Durable Streams and
  the peer-to-peer binding, a public key). Principals from different bindings are distinct unless
  explicitly linked (section 9.2).
- **Device**: one installation of an editor used by a principal. A device identifier is stable for
  the life of that installation. In a binding whose principal is a key (8.4, 8.5) the key is the
  device (section 9).
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
| `durable-stream` | `href` beginning `https://` |
| `blake3` | `hash`: 64 lower-case hexadecimal digits |

`durable-stream` and `blake3` were added in draft 1 update 2 (see the changelog: an additive change,
and a reader that predates it rejects a version that uses them). How each is read:

- **`url`** is fetched by any reader, and the bytes it returns MUST hash to `sha256`. A binding whose
  stored bytes are ciphertext MUST NOT use `url`: a `url` reader would fetch ciphertext and fail the
  hash.
- **`durable-stream`** names a file stream of the document (8.4). The bytes are ciphertext. **A
  reader uses only the path of `href`, resolved against its own server**, so a version cannot make a
  client fetch from another host.
- **`blake3`** names a blob by the BLAKE3 hash of **the bytes the provider serves** (8.5), which are
  encrypted, so it never equals `sha256` and `size`, which stay those of the plaintext. A reader asks
  its peers for that hash, and then verifies and decrypts. The decryption key is never in the DCP
  object; the binding carries it beside the version.
- **`webdav`** in the OCM and WebDAV binding (8.3) is the `https` path of the owner's copy. No other
  principal can fetch it (their path differs), so **a reader of that binding MUST NOT fetch it** and
  resolves the named file in the document's collection through its own mount.

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
8. **Availability.** A reader MUST NOT list a version whose file it cannot fetch and verify (rule
   2). In a binding where nothing is uploaded and files are fetched from peers later (8.5), a
   version appears only once its file is present, and a writer that goes offline right after
   publishing leaves a version whose file only it holds.

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
   (8.1, Time; 8.3, Time; 8.4, Time), and a lease shorter than that error is not meaningful (rule
   12). A binding with no clock of its own states what its stamp is instead (rule 11).
2. A lease is **active** while `state` is `held` and the binding timestamp plus `duration_ms` is
   later than the current binding time.
3. A session MAY acquire a lease only if none is active, or if it is the holder. After writing, it
   MUST read the lease back, wait a **settle interval**, read it back again, and treat itself as
   holder only if both records name its session and carry the `renewal` it wrote. A binding without
   atomic compare-and-set lets two sessions write at once (rule 7); the read-backs are what resolve
   it. A single read-back is not enough: on Matrix it left both sessions believing they held the
   lease in 7 of 40 forced races, 37 of 200 on a relaxed and 69 of 200 on a default-limits Synapse,
   although the final state named one holder in every one. **The two-read rule let exactly one
   session acquire in 200 of 200 rounds on each of those two profiles, and in none of 1 200 rounds
   across three runs did two sessions acquire** (8.1). The settle interval MUST be at least as long
   as the gap between competing writes the binding must tolerate; section 8.1 gives the Matrix
   value and the measured boundary, section 8.5 the value for iroh on a WAN, and rule 13 says what
   a delayed write does to it. The loser
   learns of its loss at the second read-back and MUST NOT save as the holder. A binding with an
   atomic compare-and-set is covered by rule 9.
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
9. **Atomic compare-and-set.** A binding whose write is atomic and admits one of several writers
   that name the same predecessor (a server-side sequence number: 8.4 `Stream-Seq`) makes the
   accepted write the acquisition. A session on such a binding MAY use a settle interval of 0 and
   need not read back to acquire. It MUST still treat a refused write as lost, and MUST NOT infer
   from a refusal that another writer won, because a binding may refuse every writer in a round
   (8.3: both got `423` in some rounds, so a loser retries). Rule 8 still applies and is checked
   against the record just read. Measured: 20 rounds of 8 concurrent appends of one sequence number
   gave exactly one accepted in every round, on both Durable Streams servers; 10 rounds of two
   editors racing through the store gave one accepted and one conflict. **Creating the next record
   only if absent (8.3) is not such a binding.** Nextcloud checks `If-None-Match: *` before it takes
   the file lock, so a second creator that passed the check before the first record was visible
   replaced it: in 12 of 9 216 concurrent rounds (and 6 of 2 240 in earlier CI runs) both writers
   were told their write was accepted (`201` and `204`) and the log kept the second record. A
   session on such a binding follows rule 3 in full, and its read-backs MUST read the newest record
   from the server, not a copy of what the session wrote: with the copy, both sessions held the
   lease in 5 of the 5 such rounds of one run; reading the record, exactly one session held it in
   each of the 19 such rounds of another (9 600 rounds).
10. **Poisoning.** A compare-and-set that is a server-side sequence refuses every later writer once
    anyone who may append has written a high number (8.4: one junk append blocked all lease writes
    for the rest of the epoch). A binding like that MUST give its owner a remedy, which is a new
    lease stream (8.4: a new epoch, which any roster change makes).
11. **A binding with no clock.** A binding with no timestamp of its own MUST say what it uses
    instead. In 8.5 the stamp of a lease record is **the time the reader first saw it, on the
    reader's own clock, and `now_ms` is the same clock**, so a lease is active while stamp plus
    `duration_ms` is later than `now_ms` on one clock and no two devices compare a timestamp; the
    30 seconds of skew in rule 1 is then moot. The cost is that a replica that was offline sees an
    old lease as new and treats it as active for up to one full `duration_ms` after it first sees
    it, never longer. Concurrent writes need an order that does not use a server: 8.5 uses the
    highest (Lamport value, record identifier), where the Lamport value is one more than the highest
    the writer has seen. An author's own time, where a record carries one, orders a display only.
12. **Coarse and claimed clocks.** A lease shorter than the error of the binding's clock is not
    meaningful: about 3 seconds on Matrix when the time comes from `Date` (8.1) and on Durable
    Streams before calibration (8.4), about 10 seconds on one Nextcloud server and 20 seconds over a
    federation (8.3). A binding that estimates a finer clock from a coarse one (the instant `Date`
    ticks over, then a monotonic clock) MUST say so, and where a record carries its author's
    estimate, **that estimate is a claim**: a reader MUST bound it (8.3: accepted only if it agrees
    with the file's own `Last-Modified` within 2 seconds; 8.4: clamped to the reader's own
    estimate plus the 30 seconds of rule 1). Lease expiry is therefore only as trustworthy as the
    lease's holder, which is what "advisory" already says.
13. **Partitions and delayed writes.** Without a coordinator a lease is advisory and **not
    exclusive across a partition**: two sessions that cannot see each other can both acquire and
    both pass two read-backs (8.5: both returned `Acquired`; after they reconnected exactly one
    held it within one sync, 254 ms in a debug and 52 ms in a release build, and both named the same
    record). A deterministic winner rule makes every replica agree afterwards, and version rule 5
    (a fork is shown, never silently merged) is what makes it safe. **Where a rate limiter can delay
    a binding's writes, the gap between competing writes has no bound**, and the two-read rule
    guarantees exclusion only for writes that land within the settle interval: through two WOPI
    hosts in front of one room on spent budgets, both `LOCK`s answered `200` in 3 of 4 rounds, the
    writes landing 4.5 to 5.5 s apart (8.2). A session on such a binding MUST NOT treat "both
    read-backs name me" as exclusion, and MUST judge every save against the lease record at save
    time (rule 5): the WOPI host does, and the losing host's next `PutFile` was refused `409` with
    the holder's string and published nothing (asserted). A stronger rule, having the writer check
    which record its own write replaced (`unsigned.replaces_state` or `prev_content` of the event it
    sent, *[INFERENCE]*: present for state events, not checked), was not tried.

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
MUST NOT assume a role from anything but the binding's permission data. **No binding yet expresses
`commenter`**, and one cannot express every role:

| Binding | `viewer` | `commenter` | `editor` | `owner` |
|---|---|---|---|---|
| Matrix (8.1) | power level 0 | reserved: the gate at 50 stops a member below 50 sending any encrypted event | 50 | 100 |
| WOPI (8.2) | `UserCanWrite` false | `UserCanOnlyComment` true (not run against Collabora) | `UserCanWrite` true | as `editor` in the standard fields |
| OCM and WebDAV (8.3) | share mask 1 | no carrier | create-only mask 5 | local only; across a federation not distinguishable from `editor` |
| Durable Streams (8.4) | roster role, enforced by readers | a `viewer` until a comment event exists | roster role | roster role |
| iroh and p2panda (8.5) | `Read` | not expressible (no comment level) | `Write` | `Manage` |

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
   algorithm. A reader that starts later loads the version and applies only updates after it. In a
   binding where "after" is a position in a log, the writer of the covering version MAY record that
   position (8.4: an offset of the update stream). That position **is a claim of an editor, who can
   lie**: a reader MAY use it, MUST NOT if it lies beyond what the reader can see (beyond the tail),
   and cannot verify one inside the log. A server cannot be relied on to validate it (an offset past
   the tail was an empty `200` on one Durable Streams server and a `500` on the other).
5. **Size.** A payload over 34000 bytes MUST use `encoding: "file"`. A binding MAY set a lower limit
   (8.1 shows why a Matrix deployment with long identifiers might).
6. **Determinism.** Two programs that apply the same updates in different orders MUST compute
   byte-identical stored values. This follows RFC-0001.

An algorithm profile for a given id (for example `loro/1`) is a separate document that fixes
encoding, document model mapping and the determinism test. One exists:
[`loro/1` with `tables/1`](collab-spec/profiles/loro-1.md). It also rules what `base_version` means
for a CRDT: the version the session's replica started from, never the head (profile section 3).

## 8. Bindings

Five bindings follow. What each carries and what it cannot:

| Binding | Measured against | Levels | Lease | Authorship | Confidentiality from the store |
|---|---|---|---|---|---|
| 8.1 Matrix | Synapse 1.163.2 | L0, L1, versions half of L2 | state event, two-read rule | event signature | megolm, room keys |
| 8.2 WOPI adapter | Collabora Online 26.04.5.2 | L1, conditional | the WOPI lock | by access token | that of the backing store |
| 8.3 OCM and WebDAV | Nextcloud 34.0.5, 35.0.2 | L0, L1 for viewer and editor | create-only log | none: a claim | none |
| 8.4 Durable Streams | Node 0.3.9, Go v0.3.0 | L0, L1, L2 | `Stream-Seq` compare-and-set | Ed25519 signature | XChaCha20-Poly1305, one key per epoch |
| 8.5 iroh and p2panda | iroh 1.4.1, p2panda 0.7.1 | L0, L1 advisory, L2 to a few hundred entries | Lamport order, advisory | Ed25519 signature | p2panda-encryption group key |

### 8.1 Matrix binding (measured once)

Informed by NeoBoard's architecture decision records, ADR002 and ADR005, and by the 2022 Matrix
collaborative-documents draft. **Measured once**: Synapse 1.163.2 (`matrixdotorg/synapse:latest`,
SQLite, no federation) with matrix-sdk 0.19.1, in one CI run on a GitHub-hosted 2-core runner over
loopback, debug builds. Results: [Spike A results](rfc/0004-spike-a-results.md). The figures below
are what that binding did on those workloads, not a benchmark of Matrix; network latency would add
to every latency figure. Verdict: **go** for L0, L1 and the version-carrying half of L2 with the
rules below.

**Default limits (Spike F).** [Spike F results](rfc/0004-spike-f-results.md), CI run 40142775201
(commit `513086d`, all 14 jobs green; same Synapse 1.163.2, Collabora Online 26.04.5.2 and
matrix-sdk 0.19.1, debug builds): the interop suites pass on a relaxed Synapse, on a **default**
Synapse (`rc_message` 0.2 events per second, burst 10) and on `default-drained` (every principal's
budget spent first). That is the 11 journeys through `MatrixStore`, 5 passed and 6
skipped-as-declared through the WOPI host over it, and every cross-binding check, with no
assertion changed for the default profile. **On a fresh account nothing is delayed, and that is
itself the finding**: a journey sends 3 to 8 events and a check 1 to 12, and a fresh user's burst
of 10 covers it (0 of 22 journey runs got a `429`), so a default-profile run proves nothing about
the limiter unless the budget is spent first. On a spent budget journeys took 7.5 to 28.4 s against
3.1 to 14.8 s relaxed (median 3.2 times longer, up to 8.6 times), the client waited 140 s
(journeys) and 237 s (checks) for the limiter in total, and all passed, lease expiry (journey 5,
4 s leases) included (*[INFERENCE]* for why: the lease is stamped with the event's
`origin_server_ts`, so a write the limiter delays is stamped late, not early). Only `rc_message`
was exercised.

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
- the invitee's SDK can **name the inviter at the moment it sends the join**. matrix-sdk reads
  `Room::invite_details().inviter` before the join request; with none it records no invite
  acceptance, never fetches the key bundle that arrived before the invite, and does not try again
  (reading `finish_join_room`, 0.19.1), so the history stays undecryptable. A client MUST wait,
  before joining by room ID, until its own sync has delivered the invite **and the SDK has stored
  it**: the sync that carries the invite lists the room as invited a few milliseconds before it has
  stored the invite's state, and a join in that window lost the history. Measured on Synapse
  1.163.2: joining at once read 0 of 1 versions in 6 of 6 trials; waiting only until the room was
  listed as invited failed in 13 of 760 runs of the late-editor journey (8 of 380 on relaxed and 5
  of 380 on default rate limits), and in the four failing trials traced the SDK logged
  `Error fetching invite details for room: UnknownError(EventMissing)` and `inviter: None` at the
  join; waiting until `invite_details().inviter` is `Some` failed in 0 of 760
  ([Spike A](rfc/0004-spike-a-results.md), late joiner).

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
anyone invite. A reader MUST judge a version or an update by its sender's power level **in force
at the event**, never by the room's current table. The level is `users[sender]`, else
`users_default` (a room version 12 creator has unbounded power), from the room's power-levels
state at the event; the event is accepted only if that level is at least 50 and at least what the
same table asks for the event type the server saw (`events[type]`, else `events_default`;
`m.room.encrypted` in an encrypted room). This applies whether or not the table gates
`m.room.encrypted`: with the entry the server has already refused an unauthorised sender at send
time and the rule changes nothing; without it the server accepts the event and only the reader
stands between it and the document. A demotion MUST NOT hide what the editor published before it,
and a promotion MUST NOT validate what the user sent before it. Measured: the draft 1 rule (the
room's current table) dropped the ex-editor's earlier versions and then the owner's save built on
one of them in a draft-0 room, so the document read as its first version (3 of 3 runs, Spike F),
and accepted a viewer's earlier forgery after a promotion; a reader that judges by the level at the
event got every verdict right in all six scenarios, 65 reader runs, 0 wrong, while the current-table
reader got 24 verdicts wrong in 10 runs and a reader that trusts senders 20 in 8
([Spike Q15](rfc/0004-spike-q15-results.md), scenarios 1 to 6).

A reader obtains the power levels at an event in one of two ways, and uses the second for every
event the first cannot place:

1. **From the room's own events.** Page the room's timeline (`/messages`): it carries
   `m.room.create` and every `m.room.power_levels` event in order. Walk them from the oldest;
   judge each event by the table held when the walk reaches it. The walk MUST trust a table only
   while the next power-levels event it meets names it in `unsigned.replaces_state`: a change the
   reader cannot see (history visibility `joined`, a reader who joined after it) would otherwise
   leave it judging by a table that is no longer in force, silently (Spike Q15 scenario 6: the
   unlinked walk accepted the ex-editor's forged event). It MAY read the pass from the newest
   event backwards instead, undoing each power-levels event through its `unsigned.prev_content`.
   This needs no request and works from cached events; it is exact only in a room created with
   `m.federate: false` (all events come from one homeserver, so stream order is state order). The
   owner SHOULD create a document room with `m.federate: false`, as the measured binding does.
2. **From the server.** `GET /_matrix/client/v3/rooms/{roomId}/context/{eventId}?limit=0`, whose
   `state` is the room state at the event; a `filter` of
   `{"types":["m.room.power_levels","m.room.create"]}` keeps the answer to about 4 KB however many
   members the room has. The answer for an event never changes: cache it by event ID. Use it for
   every event of a room that federates. Honour `retry_after_ms` on a `429` as for any request.
   (`GET /rooms/{id}/state?at=` is not a substitute: Synapse ignores `at` and answers the
   current table.)

An event whose level at the event cannot be established MUST be dropped; a reader MUST NOT fall
back to the current table. The measured reader (the hybrid: the linked forward walk in a room
created with `m.federate: false`, `/context` for each event the walk cannot place and for every
event of a room that federates) made 0 extra requests in the usual case and 2 in the scenario
where a late joiner could not see a demotion. **Costs, Synapse 1.163.2 only**: `/context` was not
rate limited (0 of 620 requests per job on the default profile, and a user who made 300 requests
had the same `rc_message` send budget left), and took 2.7 to 5.6 ms and about 4 KB per event on a
loopback server (listing 30 versions: 84 to 173 ms over a reader that checks nothing, in 31
requests, 134 KB; a warm read makes none). **Not tested**: another homeserver (the walks rely on
`unsigned.replaces_state` and `unsigned.prev_content` being present on `/messages`, a server
without them leaves every event to `/context`), a room across two homeservers, a `429` on
`/context`, a real network (31 round trips of 80 ms would be 2.5 s, derived), room version 12
against a server. A forged update is treated like a forged version: refused `403` in a room with
the entry, accepted by the server and dropped by every reader (2 of 2) in a draft-0 room, where a
control that trusts senders fails.

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

Synapse answers `M_LIMIT_EXCEEDED` **before** it looks at the sender's power level, so a `429` says
nothing about authorisation: a check that expects `403` from a user whose budget is spent has to
retry past the `429` (8 of 26 checks in one drained run failed that way, and the test, not the
binding, was changed).

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

- *Settle re-read (rule 3).* After one read-back both sessions believed they held the lease in 7 of
  40 forced races (Spike A; their events were 0 to 33 ms apart in `origin_server_ts`), and in
  Spike F's 200 rounds per profile in 37 of 200 (18.5 %) on a relaxed Synapse and 69 of 200
  (34.5 %) on a default one (two earlier runs: 45 and 41, and 80 and 56, of 200), although the
  final state named one holder every time. **The two-read rule let exactly one session acquire in
  200 of 200 rounds on both profiles, and the final state 1.5 s later named that session in 200 of
  200; across three runs, 1 200 rounds, it never let two sessions acquire.** A session MUST
  therefore read back, wait a settle interval, read back again, and believe itself the holder only
  if both reads name it. The interval SHOULD be at least 500 ms, and it is the boundary: a
  competing write 450 ms or less behind the first never produced two holders (0 of 6 at 100, 300
  and 450 ms on relaxed; 0 of 3 at 100 and 450 ms on default), 550 ms behind produced them in 5 of
  6 (6 of 6 and 2 of 6 in two earlier runs), and 700 ms and later in 6 of 6 (3 of 3 on default).
  A write that the homeserver delays has no such bound (lease rule 13). Read the lease from `GET
  /rooms/{room_id}/state` (server-authoritative), not from the local sync view.
- *Renewal changes the record (rule 8).* Synapse does not create a new event when a user sends a
  state event with content identical to its own current one: it returns the old event ID and
  timestamp, so a renewal that repeats the record extends nothing (conformance journey 5 failed on
  exactly this). Hence the `renewal` counter.

Whether the lease should be a timeline event with a deterministic winner rule remains open
(section 13, question 3); the two-read rule held in every measured race, and what it does not
cover is lease rule 13.

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

**Later runs.** [The interop results](rfc/0004-spike-interop-results.md) ran this adapter over the
Matrix binding (`WopiHost<MatrixStore>`), with a WOPI client simulator and with a real Collabora
Online (run 40093629440), and [Spike F](rfc/0004-spike-f-results.md) (run 40142775201) repeated
that on a Synapse with default rate limits. Figures marked Spike F below come from it.

**Verdict.** L1 over WOPI is **GO with conditions** (the numbered rules below). **L2 and L3 are
not offered over WOPI**: WOPI has no update stream, no presence and no role changes, and an adapter
MUST NOT declare `L2` or `L3` for a document it serves to a WOPI client. WOPI also **cannot express
two heads or a merge** (a save has exactly one parent), which is why the L1 it offers is
conditional (rule 6).

| WOPI operation | DCP mapping |
|---|---|
| `CheckFileInfo` | `BaseFileName` from the title (rule 8), `Size` and `Version` from the head version, `LastModifiedTime` (rule 3), `SupportsExtendedLockLength` false (rule 4). `UserCanWrite` true only for `editor` or `owner` and only when the document has exactly one head (rule 6); `UserCanOnlyComment` true for `commenter` |
| `GetFile` | Fetch and verify the head version's file, and record the version returned against the access token (rule 1) |
| `PutFile` | Verify the lock and the timestamp (rules 3 and 5), refuse with `409` when the document has more than one head (rule 6), then publish a version whose parent is the version recorded for the token |
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
   The adapter MUST also **refuse a `PutFile`** on such a document: `409` with the JSON body
   `{"COOLStatusCode":1010,"Message":"document changed in storage"}` and no `X-WOPI-Lock` header,
   as for a changed document (rule 3), whatever the lock string and with or without the timestamp
   header. Spike F found that without this, a client that ignores `UserCanWrite: false` got `200`
   from the host (observed against the unmodified host over the in-memory store). After the fix
   four attempts (the editor with its lock, with and without Collabora's timestamp; the owner
   without a lock, the same) were all `409`, nothing was published, and after a native merge the
   same session could save, as a child of the merge. Collabora's reaction to that `409` was not run.
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
11. **Time the client allows (Spike F).** On the `UNLOCK` it sends when the last editing session
    disconnects, Collabora gave the host **3003 ms**, retried once about 3 s later, then logged
    `Failed to UNLOCK`, made the session read-only and said the lock stays held until the host
    expires it (two such time-outs in the one spent-budget churn run; the lease record did read
    `released` afterwards, because the host's queue still delivered the write). A host over a
    binding whose writes a rate limiter can delay MUST answer within that time or accept that the
    release lands later. Measured over the wire, without Collabora, in 36 calls (6 cycles of
    `GetFile`, `LOCK`, two `PutFile`, `REFRESH_LOCK`, `UNLOCK`): on a relaxed Synapse `LOCK` took
    580 ms, `PutFile` 205 ms, `REFRESH_LOCK` 587 ms and `UNLOCK` 52 ms (medians; a `LOCK` is at
    least 0.5 s on any profile, the settle interval); on a default Synapse and on a spent budget
    every call that needs an event took about 3.7 to 6.1 s, and **20 of 36 and 30 of 36 calls took
    over 3003 ms**, against 0 of 36 relaxed. It is not shown that the 3003 ms limit applies to
    `LOCK` or `PutFile` (none timed out with calls of 4.5 s). A host that answers before the write
    lands (write-behind) or with `429` or `503` was not tried.
12. **Saves and the rate limit (Spike F; question 10, closed).** Each `PutFile` is one version
    (autosave, idle save and exit-save alike; none is coalesced) and each `LOCK` refresh is one
    lease state event; both draw on the same per-user `rc_message` budget, and each version also
    uploads a file, which is not an event. In a 187 s session (180 s of typing every 5 s) against a
    default Synapse and a fresh editor, the room held **6 events at Collabora's own timers** (1
    `LOCK`, 4 `PutFile`: 3 autosave and 1 exit-save, 1 `UNLOCK`; about 2 a minute, a tenth of the
    sustained 0.2/s), **14** with autosave 30 s, idle save 10 s and refresh 60 s, and **40** with 5
    s, 3 s and 10 s, against a budget of `10 + 0.2 × 187` = 47, with 0 `M_LIMIT_EXCEEDED` in all
    three. The tightest run was 0.21 events/s overall and 13 to 14 a minute in minutes 1 and 2,
    above the sustained limit: the burst alone would last about 10 minutes at that rate (derived,
    not run). The budget is per user, so several open documents add. **The limit is not what breaks
    first; the 3 s of rule 11 is.** **Timers do not predict cadence**: at its own settings
    (`autosave_duration_secs` 300, `idlesave_duration_secs` 30, refresh 900) Collabora saved every
    60 s (at +31, +91 and +151 s) with an exit-save at +181 s; told 30 s and 10 s it saved every 20
    s; told 5 s and 3 s it saved every 10 s, never more often, while the lease refresh obeyed its
    setting (60 s and 10 s). Why is not known from these runs, so an implementer SHOULD NOT derive a
    save cadence, and hence a version rate, from the timers in `coolwsd.xml`. One build, one run
    each.

**Not measured.** Two real Collabora sessions on one document (the multi-session checks use this
repository's own WOPI client, and passed: B's `LOCK` while A holds it is `409` carrying A's string,
and two `LOCK`s at once give one `200`); Collabora's reaction to the `409` 1010 refusal on a
conflicted document, to a lease broken by an owner during an edit, and to a `LOCK` that two hosts
both answered `200` (Spike F: two hosts in front of one room on spent budgets did, in 3 of 4
rounds, with writes 4.5 to 5.5 s apart; the losing host's next `PutFile` was `409` carrying the
holder's string and published nothing); a forced save after a conflict, and a lock mismatch, against
Collabora; lease expiry during an edit; `commenter` through Collabora; churn longer than 3 minutes
and a Collabora cadence that exceeds the limit; why Collabora saves every 60 s at its own timers;
a host that answers before its write lands; HTTPS and WOPI proof keys (`X-WOPI-Proof`: the
reference host checks the access token only and MUST NOT be deployed as it is); other WOPI
clients (ONLYOFFICE, Microsoft 365) and other Collabora builds; large files. Measured since this
section was written: autosave and exit-save `PutFile` (rule 12), the adapter over the Matrix
binding (above), `REFRESH_LOCK` and `UNLOCK_AND_RELOCK` over Matrix (new records with `renewal`
1, 2, 3 and 4, wrong string `409` carrying the current one, a relock `a` to `b` is one record), a
forced save and two sessions against the simulator.

**Conformance over WOPI.** Of the journeys in section 12, tests 1, 3 and 6 can be expressed through
WOPI and passed against the simulator (4 journeys passed counting the two halves of test 3, 0
failed), and test 10 passed against Collabora; 2, 4, 5, 7, 8 and 9 (6 journeys) were skipped with
the missing capability stated, never reported as passed. Lease exclusion and expiry (4 and 5) are
tested on the WOPI wire in the host's own tests.

Through `WopiHost<MatrixStore>` (the host over the Matrix binding, Spike F) the same split holds:
5 journeys passed (tests 1, 3 in both halves, and 6 in both halves) and 6 were skipped with the
capability stated, on a relaxed, a default and a spent-budget Synapse.

### 8.3 OCM and WebDAV binding (measured once)

**Measured once**, on two Nextcloud builds, **34.0.5 and 35.0.2** (the official `nextcloud:*-apache`
images pinned by digest, PostgreSQL 16), over WebDAV and OCS sharing, on one server and across an
OCM federation of two servers, on GitHub-hosted 2-core runners, over plain HTTP on one docker
network, driven by this repository's own tests: CI run 40139702356, commit `cee909b9`.
[Spike E results](rfc/0004-spike-e-results.md). **These are observations of those two builds, not
a statement about Nextcloud, ownCloud or OCM in general**: no other OCM implementation (CERNBox,
OpenCloud, Seafile, the OCM stub) was run, nothing ran over HTTPS, and the OCM *sender* was always
Nextcloud. Verdict: **go** for L0 and L1 for the `viewer` and `editor` roles; L2 and L3 are not
offered; `commenter` is not expressible, and across a federation neither is `owner`.

**Draft 1's outline is withdrawn.** It said a version is a file state at an ETag, that publishing is
a conditional `PUT` and that a lease SHOULD be a WebDAV `LOCK`. Measured:

| Draft 1 outline | Result |
|---|---|
| A version is a file state at an ETag | **Rejected.** A file replaced about 0.2 s after it was created kept its ETag in 16 of 20 trials with different-size content and 16 of 20 with same-size content on 34.0.5 (18 and 18 on 35.0.2). In all 80 trials the ETag moved exactly when the file's whole-second `getlastmodified` moved. A writer holding the old ETag was accepted by `If-Match` in exactly the trials where the ETag had not moved. |
| Publishing is a conditional `PUT` (`If-Match`) | **Rejected for overwrites.** Two writers presenting the same ETag at once: 0 of 20 rounds had two winners, 1 had one and 19 had none (`423` for both). Safe, not live, and the ETag it relies on is the unreliable part. |
| A `412` means a second head | **Rejected.** A `412` or `423` carries no content and nowhere to put the loser's bytes. A second head exists only if the writer has already put its version somewhere. |
| A lease SHOULD be a WebDAV `LOCK` | **Rejected.** Core Nextcloud has no lock provider (`LOCK` is `501` when the bundled `files_lock` app is disabled). With it, a lock is authorised by its token alone (another user unlocked it), changes the file's ETag, is not seen by a federated writer, and `LOCK` through a share token is `500`. |
| A principal is an OCM address | Holds (`bob@nc-b`), after the receiving user accepts the share on the receiving server. Nextcloud writes `alice@http://nc-a`, with a scheme, when the sender is on `http`. |
| L2 and L3 need a channel OCM does not define | Confirmed. WebDAV has none either. Not offered. |

**Document.** A document is two collections in the owner's home: `<id>.v` and `<id>.l`, where `<id>`
is `DCP-` followed by 12 lower-case hexadecimal digits. The identifier is the name, and a share
keeps it on the receiving side. A principal is an OCM address `user@host[:port]`.

**Versions (`<id>.v`).** `meta.json` is the document metadata (4.1), written once. Each version is
two files named by the same version identifier `<vid>`: `<vid>.bin`, the file, and `<vid>.json`,
the version object (4.2) with its `parents`. A writer MUST create each with `If-None-Match: *`,
MUST read `<vid>.bin` back and compare it with what it sent before it creates `<vid>.json` (the
upload's status and `OC-Checksum` do not verify the bytes: Nextcloud stores the checksum as a
client claim, and a body shorter than its `Content-Length` is refused with `400` and leaves
nothing), and MUST NOT overwrite or delete a version. A reader lists the `*.json` files and MUST
ignore a `<vid>.bin` that has no `.json`; an upload that was verified and failed, or a crash,
leaves one, and a create-only editor cannot delete it. A `412` or `423` on a create means the name
is taken; it is not a second head. **Two heads are two version files with a common parent.** An
ETag, Nextcloud's file versions and WebDAV properties MUST NOT be used to carry a version, its
parents or its order: a property belongs to the file and to one user, not to a state of it (bob's
`PROPPATCH` was invisible to alice), and the versions endpoint keeps replaced states by Unix
second (six overwrites in 1.7 to 1.8 s left 2 or 3). The `webdav` locator (4.2.1) of a version is
the owner's `https` path; **a reader MUST NOT fetch it** and resolves `<vid>.bin` in the document's
`.v` collection through its own mount.

**Lease (`<id>.l`).** A lease record is the file `<n>.json`, `<n>` a zero-padded 8-digit decimal,
holding `{by, at_ms, lease}`: the writer, its estimate of the server's time in milliseconds, and
the lease (4.3). The newest record (highest `<n>`) is the lease. A writer that last read `<n>` MUST
create `<n+1>` with `If-None-Match: *`. `412` or `423` is a lost write. **`201` and `204` are only
an accepted write, not the lease**: Nextcloud checks `If-None-Match: *` before it takes the file
lock, so a second creator that passed the check before the first record was visible replaces it and
is answered `204`, while the first keeps its `201`. The writer MUST read back (lease rules 3 and
9), and the read-backs MUST read the newest record from the server, not a copy of what the writer
wrote. Writers MUST NOT change or delete records. A reader that holds `<n>` asks for `<n+1>` and so
on until `404` and then reads `<n>` again: two requests (about 110 ms) when nothing changed. WebDAV
`LOCK` MUST NOT be used as the lease: it is absent unless the `files_lock` app is enabled, is
authorised by its token alone, changes the ETag, and is not honoured across a federation (below).
Measured: two writers creating the next lease record at once, the owner and an editor (a sharee):
a winner in about half of the rounds, `423` for both in most of the others, and in 12 of 9 216
rounds both writes were accepted, `201` for the owner and `204` for the editor, with one file id
and one ETag in both answers and the editor's record in the file (the editor's answer came 30 to
53 ms after the owner's); two clients of one account: 16 of 7 806 rounds; 6 of 2 240 rounds in
earlier CI runs. In 4 of 9 216 rounds (5 of 7 806 for one account) one writer was answered `404`
("File with name … could not be located") while the other's `201` stood: a lost write too, but the
answer a removed member gets as well (below), so a writer that gets it reads the log back before
it decides (the spike's store reports it as `forbidden` and its test counts it as a lost write).
The cause is in Nextcloud, not in the test: delaying its `PUT` handler between the precondition
check and the existence check gave `201` and `204` every time on both builds. The log is **safe
only with the read-backs, and not live**: a loser
retries. One file per write is a growing cost: a holder renewing every 15 minutes writes about 100
files a day per document. Compaction by the owner was not implemented.

**Time.** WebDAV times (`Date`, `Last-Modified`, `getlastmodified`) are whole seconds. A binding
MUST estimate the server's clock to the millisecond (find the instant `Date` ticks over, polling
every 5 ms for about 0.6 s once, then advance a monotonic clock) or accept a lease error of 1 s.
The writer stores its estimate as `at_ms`. A reader uses `at_ms` only if the file's `Last-Modified`
agrees with it within 2 s, and otherwise uses `Last-Modified`. This is a calibrated estimate, not
a reading of the server's clock; drift over a long run was not measured. **A lease shorter than
about 10 s is not meaningful on one server, and 20 s over a federation** (a 4 s lease passed in 8
of 8 runs on one server and in 1 of 8 over a federation; 20 s passed in every run).

**Roles.** `<id>.v` and `<id>.l` are shared with these permission masks, and a role is read back
from the receiving mount's `oc:permissions`:

| DCP role | `.v` mask | `.l` mask | Recipient's `oc:permissions` on `.v` | Measured on `.v` |
|---|---|---|---|---|
| `viewer` | 1 | 1 | `SGDN` | create, overwrite, `MKCOL`: `403`; reshare `403` |
| `editor` | 5 (read, create) | 15 | `SGDNCK` | create `201`, create again `412`, **overwrite, delete and `MOVE`: `403`** |
| `owner` | 31, or the owner of the folders | 31 | `SRGDNVCK` | reshare allowed |
| `commenter` | | | | **no equivalent**: comments are a separate app any reader may use |

An editor MUST NOT be given update or delete on `<id>.v`: **create-only is what enforces version
rules 1 and 4 at the server**, against a hostile client as well as this store; the same editor with
mask 15 overwrote history (`204`). A local mount's role is `R` in `oc:permissions` for `owner`, `C`
for `editor`, else `viewer`. **A federated mount** (its `oc:owner-id` contains `@`) shows the
reshare right `R` for every grant, so a client MUST read `ocm:share-permissions` instead (reading
`R` made an editor an owner and let it break the lease): `write` is `editor`, anything else is
`viewer`, and a create-only grant reads as full write. **Across a federation the receiver's role
is only read or write, and `owner` cannot be told from `editor`.** The sender enforces the real
mask whatever the receiver believes (an overwrite is `403`). The server does not enforce
*owner-only* breaking of a lease: any holder of create rights on `.l` can write the next record, so
the store checks `role()` first. A group share of the same folder worked at the HTTP level; the
binding addresses users only.

**Authorship.** WebDAV does not name the uploader (`oc:owner-id` of a file a recipient created is
the folder's owner). `created_by.principal` is **a claim by the writer**: bob wrote a version
naming alice, the server accepted it, a reader listed it as alice's, and Nextcloud's Activity API
named bob (`file_created`, 56 ms in the final run, 44 to 253 ms over the runs; it is written after
the request, and retention was not measured). A reader that needs authorship authenticated MUST
use the Activity API or another source; the binding provides none (section 11). A viewer cannot
write at all (`403`), so the forged-version journey passes here for a different reason than on
Matrix.

**Revocation.** Unsharing (`DELETE` on the share) makes the removed user's next request `404`; a
store SHOULD report a write by a removed member as forbidden. This is access control, **not** the
encrypted-binding property of conformance test 7: the server holds the plaintext, and content
already downloaded stays (section 11).

**Federation.** A document shared to `user@host` is two OCM shares (one per collection), and the
receiving user MUST accept each on the receiving server (nothing auto-accepts); a DCP client on
the receiving side has to say so. The collections are then mounted by name at the receiver's root,
so the document id is the same on both sides, and the receiving server proxies the receiver's
WebDAV requests to the sender (`If-None-Match`, `If-Match`, ETags behave as on the sender). A
change of permission is **not** notified (the sender enforces it at once; OCM defines
`SHARE_CHANGE_PERMISSION` and Nextcloud did not send it); removal is (`SHARE_UNSHARED`, and the
secret stops working at once). A lock taken by a federated user is not seen by the sender, and a
federated write ignores a Sabre lock a local user holds (`204`) or fails with `500` against a
`files_lock` user lock; the sender's version history is not offered to a receiver. Measured details
of the two builds against the OCM text (a scheme in the cloud id; the `Basic <secret>:` header
works and `Bearer` is `401`; cavage signatures, and RFC 9421 only when the receiver advertises
`http-sig` on 35.0.2; a malformed `https://http://…` URI on 35.0.2; no `webapp` protocol in either
discovery document) are in the results document. A client SHOULD use an app password: verifying
the account password cost about 350 ms per request and an app password 51 ms.

**Cost (final run, 34.0.5; 35.0.2 within 25%).** `publish_version` for a small file 297 ms, a
5 MiB file 414 ms to publish and 65 ms to fetch, a version listing (one `PROPFIND` and one `GET`
per version) 127 ms for 2 versions, 306 ms for 10, 754 ms for 30, a lease read 52 ms and a lease
write 104 ms.

**Conformance (section 12).** On one Nextcloud (34.0.5 and 35.0.2): 9 passed, 0 failed, 2 skipped
(tests 8 and 9: no L2). Across an OCM federation: 9 passed, 0 failed, 2 skipped, with the
federated fixture's lease of 20 s for test 5 (`Fixture::lease_ms` in `dcp-conformance`; the
journey's default is 4 s, at which it failed in 7 of 8 federated runs before the duration was a
parameter, and which still passes on one server). Both Nextcloud versions passed in the
integration run.

**Not offered.** L2, L3, encryption (the server sees everything it stores), notification of a
permission change, `commenter`, and `owner` across a federation.

**Not measured.** ownCloud and any other OCM server or sender; HTTPS and real DNS (Nextcloud ran
with `allow_local_remote_servers`); verification of the HTTP signature by a receiver, the OCM code
flow, invites, and the OCM web-app access mode (absent from both stock images, which is a
statement about those images); Nextcloud Office or Collabora editing a DCP document (the app
installs and no Collabora server was configured); group and circle principals; SQLite, and Redis
or APCu file locking; more than 30 versions, files over 5 MiB, clock drift over hours, lease-log
growth; the desktop and mobile Nextcloud clients syncing the two folders of a document.

### 8.4 Durable Streams binding (measured once)

**Measured once**, against two servers of the Durable Streams protocol, the Node reference server
`@durable-streams/server` **0.3.9** and the Go server (a Caddy plugin) release **v0.3.0**, with one
client implementation, in CI run 40137430958 at commit `0c99551`, client and server on one
GitHub-hosted machine over loopback, plain `http`; and again over TLS 1.3 with HTTP/1.1 and
HTTP/2 behind Caddy v2.10.2 (correctness run 40182677787, commit `167aa64`, all four server and
version jobs green; measurements run 40178415302, commit `63e0bf9`; [TLS spike
results](rfc/0004-spike-tls-results.md)). [Spike B results](rfc/0004-spike-b-results.md).
They describe those two builds and that client, not Durable Streams in general. Verdict: **go,
with conditions, for L0, L1 and L2; no-go for "the server enforces roles"** on both servers; L3
was not built. All 11 journeys of section 12 pass on both servers, with nothing skipped.

**Server.** Any server of the Durable Streams protocol that supports JSON-mode streams, `PUT` with
a body and `Stream-Closed`, `POST` with `Stream-Closed` and `Stream-Seq`, `HEAD` and offset reads,
with the `Date` header. **It stores bytes with no principals**: the protocol puts authentication
and authorisation out of scope, and neither server has any. A document MUST NOT rely on the server
for confidentiality, authorship, roles or time. A deployment MUST put access control in front of it
that admits only the document's members to write and SHOULD admit only them to read; without it,
any holder of a document id can delete, close or poison the document's streams (tested on both).
A static credential per role gates a viewer's writes (tested with Caddy `basic_auth`) but cannot
follow a revocation: the removed editor still holds the `writer` credential. A gate that derives
the current roster from the signed control stream was not built (question 12).

**Principals.** `ed25519:` followed by the unpadded base64url of a 32-byte Ed25519 public key. The
same key is the recipient of key wraps (X25519 by the Edwards-to-Montgomery map; reusing one key
for signing and Diffie-Hellman is accepted practice but a design choice to review).

**Document id.** The first 16 bytes of SHA-256 of the concatenation of `dcp-streams/1`, `doc`, the
creator's principal and a 128-bit random nonce, in unpadded base64url. Epoch 0 carries the nonce,
so a server cannot substitute another genesis.

**Streams.** Under `<base>/dcp/<doc>/`: `ctl` (JSON mode, cleartext, signed), and for each epoch
`<prefix>/log` (meta and versions), `<prefix>/upd` (updates) and `<prefix>/lease` (JSON mode,
encrypted), and per file `f/<id>` (an octet stream, created closed). `<prefix>` is `e<n>-` and 12
random hex digits chosen by the owner who creates the epoch; `<id>` is 96 random bits in base64url.
The lease needs `Stream-Seq`, which is per stream, so it has its own stream.

**Epochs.** `ctl` holds one JSON `EpochRecord` per epoch (fields `v`, `doc`, `n`, `prefix`, `prev`,
`root`, `roster`, `wraps`, `prev_key`, `a`, `s`). **Every change of roster, including a role
change, is a new epoch with a new random 256-bit key**, so that a record's authority is decided by
its epoch alone and no ordering across streams is needed. A reader accepts the first record for
each number `n` = 0, 1, 2, ... that has a valid Ed25519 signature by `a` over its fields; whose
`roster` has at least one `owner` and no duplicate; whose `wraps` name roster members only; and,
for `n` = 0, whose `root` hashes with `a` to the document id and whose creator is an owner, or, for
`n` > 0, whose `prev` is the lower-case hex SHA-256 of the previous record's JSON (fields in the
order above), which has `prev_key`, and whose `a` is an owner of epoch `n - 1`. A wrap is the epoch
key encrypted to a member by ephemeral X25519, HKDF-SHA256 over the shared secret (salt: ephemeral
and recipient public keys; info: `dcp-streams/1/wrap`, the document, the recipient, the epoch) and
XChaCha20-Poly1305. `prev_key` is the previous epoch's key sealed under the new one, so that a
member can read every earlier epoch.

**Records.** A message of `log`, `upd` or `lease` is a JSON object `{v, k, e, a, n, c, s}` (and
`x` on a seal): `k` is `meta`, `ver`, `upd`, `lease` or `seal`; `e` the epoch; `a` the author's
principal; `n` a random 192-bit nonce; `c` the XChaCha20-Poly1305 ciphertext of the plaintext
`{st, dev?, body, hint?}` under the epoch key with the document, `k`, `e` and `a` as associated
data; `s` the author's Ed25519 signature over the length-prefixed (4-byte big-endian lengths)
`dcp-streams/1`, `record`, the document, `k`, `e` (8 bytes), `a`, `x` (8 bytes, all ones when
absent), `n` and `c`. A lease's plaintext is `{st, seq, body}`. A `seal` has no `n` or `c`,
`x` = `e` + 1, and is sent with `Stream-Closed: true`. A reader MUST drop, without showing it: a
message after a valid seal of its stream; of a kind that does not belong in the stream; whose `e`
is not the epoch of the stream; whose signature fails; whose author is not in the epoch's roster or
is below `editor` (`owner` for `meta`; a seal MUST be from an owner); that fails authenticated
decryption or its schema; or whose `created_by.principal` (version) or `holder.principal` (lease)
is not `a`. A reader MUST ignore a second copy of a record (its identifier is the lower-case hex
SHA-256 of its JSON with the fields in the order `v, k, e, a, n, c, s, x`). **The identifier of a
version is that identifier.**

**Roles.** `owner`, `editor` and `viewer` are the roster's roles, enforced **by every reader and not
by the server**: a viewer's update, lease and version, and an editor's metadata, are accepted by
both servers and dropped by every reader (tested; with the role check removed the forged-version
journey fails). `commenter` is a `viewer` until the comment event is defined.

**Writing.** A writer reads `ctl`, finds the latest epoch, checks its own role, and appends to that
epoch's streams (a publish is two requests). On `409` with `Stream-Closed: true` the epoch was
sealed: read `ctl` again and write into the next. A file is `PUT` closed, which makes an upload
atomic (an upload cut off on the wire left no stream behind on either server), and then read back,
decrypted and compared before the version that names it is sent (version rule 2). A file blob is
`DCPF`, `0x01`, the epoch (8 bytes), a 24-byte nonce and the ciphertext, with the document, a file
id and the epoch as associated data (53 bytes of overhead). The version's locator is
`durable-stream` with an `https` `href` of the file's stream (4.2.1); **a reader resolves only the
path against its own server**, so a version cannot send a client to another host.

**Changing the roster.** An owner makes the new key and `EpochRecord`, creates the new epoch's
streams (the log with the owner's `meta` record in the same request), appends the record to `ctl`,
reads `ctl` back (if another owner's record for that `n` came first it MUST report a conflict and
retry), then seals every stream of the old epoch. Two owners rotating at once: exactly one won in
8 of 8 rounds on both servers. **The change takes effect for readers when the old epoch's streams
are sealed, not when the owner decides**; until then a removed member's records in the old epoch are
valid. The old epoch's content stays readable to those who had its key. A new member reads all
history, and a removed and re-added member reads what was published while it was out (section 11).

**Lease.** In the epoch's `lease` stream, every write is one `lease` record carrying its sequence
number (the last valid record's + 1), appended with `Stream-Seq` equal to that number as 16
lower-case hex digits. The server's `409` is the compare-and-set: **a writer whose append is
accepted holds the lease** (lease rule 9: no settle interval is needed); a reader takes the valid
record with the greatest sequence number. Rule 8's `renewal` is checked against the record just
read before the append. A new epoch starts with no lease. Measured: 20 rounds of 8 concurrent
appends of one number gave exactly 1 accepted and 7 `409` in every round, and 10 rounds of two
editors racing through the store gave 1 accepted and 1 conflict, on both servers; acquire and
renew cost 5.5 and 5.1 ms p50 at a settle interval of 0 (Node). **The lease stream can be
poisoned** (lease rule 10): one junk append with `Stream-Seq: ffffffffffffffff` by anyone who can
append made every later lease write a conflict, and a roster change gave a fresh lease stream.

**Time.** The server has no timestamp on a message and no clock endpoint. A client calibrates a
clock once by sending `HEAD` every 10 ms until `Date` changes second (0.52 to 0.97 s), and
advances the last `Date` by its monotonic clock. After calibration the estimate was a lower bound
within 11 ms of the truth (p50 −6 ms); before it, up to a second behind (p50 −253 ms); drift was
tested only over tens of seconds. A record's `st` is its author's estimate, and **a reader clamps it
to its own estimate plus 30 s** (lease rule 12), so a writer can shorten its own lease by claiming
a past time or extend it by up to 30 s. A lease SHOULD NOT be shorter than 3 s unless every client
calibrates.

**Updates.** `upd` records carry `dev` (the author's device); `seq` is per author and device (4.4).
An inline payload keeps the 34 000-byte limit (a 61 016-byte record, 1.8 times; a 100-byte update is
a 747-byte record, 7.5 times, because the payload is base64 inside the update and the update is
base64 again inside the record). A version with `covers` MAY carry in its plaintext `hint` =
`{epoch, offset}`, an offset of that epoch's `upd` stream (section 7 rule 4): a reader that starts
later MAY read the stream from there, MUST NOT if the offset is beyond the stream's tail, and
trusts it no further than the editor who wrote it. Neither server can validate an offset: Node
answers an offset past the tail with an empty `200` and Go with `500`. With a snapshot a cold
reader of 5 000 updates took 11 ms against 266 ms without (Node, release build).

**Failure.** `404` on a document the client has known is data loss, not an empty document. A server
in memory mode (the Node server's default) loses everything on restart and MUST NOT be used. A
file-backed server kept every acknowledged record across `kill -9` (4 runs, Node), and the Go
server across SIGTERM and SIGKILL (behind TLS; power loss and a full disk were not run). `410` on a read (retention) was
not exercised, and there is no recovery from a lost stream.

**Transport.** A client MUST reach a Durable Streams server over `https` and MUST verify the
server certificate chain against a trust anchor configured for that deployment (a CA the
deployment names, or the platform's roots) and MUST verify that the certificate covers the host
name of the URL; a client MUST NOT disable verification, MUST NOT fall back to `http` or to a
lower TLS version (TLS 1.2 is the minimum, 1.3 is expected), and MUST fail closed, reporting a
transport error and never an empty document, when verification fails. `http` MAY be used for a
server on the loopback interface in development. Measured with a client that has no built-in
roots: a wrong CA (`UnknownIssuer`), a wrong name (by IP address, or by another host name), and an
expired certificate were each refused with no response accepted, with the right CA, name and
dates as the control, and `openssl s_client` agreed in each case. Only TLS 1.3 was exercised
(the platform trust store and TLS 1.2-only servers were not).

**No redirects, no `Location`.** A client MUST NOT follow a redirect that leaves the origin or that
goes from `https` to `http`, and SHOULD NOT follow any; it MUST NOT take the address of a stream
from a response header (`Location`): a server behind a proxy may name its internal plaintext
origin there (observed: the Node server's `Location` on a `PUT` through a TLS proxy named the
origin's `http` address). A `307` from `https` to `http` was returned as an error and the plain
listener saw 0 connections; a stock client with default redirects followed it.

**HTTP version.** HTTP/1.1 and HTTP/2 are both valid. A client that requires one MUST offer only
that one in ALPN and MUST fail if the response is of another; a client that requires none takes
what ALPN gives (HTTP/2 when offered) and cannot tell it was downgraded (measured: it silently ran
HTTP/1.1 against a server offering only that). With HTTP/2 a client SHOULD use one connection per
origin (50 live streams used one, against 50 connections on HTTP/1.1), MUST NOT assume more than
100 concurrent streams unless the server's `SETTINGS` say so (both servers measured allow 250),
and SHOULD enable adaptive flow-control windows for large file transfers (but see the
measurement below: in one emulated setup the adaptive window was slower).

**Live reads and idle connections.** A server's long-poll timeout and its SSE keep-alive interval
MUST be shorter than the shortest idle timeout of any proxy on the path; 60 s is common, so a
server SHOULD use 30 s or less (both reference servers default to 30 s for long poll; the Go
server's SSE sends nothing while idle and ends the response after 60 s by default, which equals
a 60 s proxy). A client MUST treat any transport error on a live read as "resume from the last
offset" (never as a fatal error and never as an empty document) and MUST bound consecutive
failures with a back-off: a long poll that a proxy always cuts at 60 s never completes, and the
binding's request retry (3 attempts) gives up after about 3 minutes with a transport error to the
caller, while the record is delivered 3 to 9 ms after its append once the reader resumes from its
offset (both servers, both HTTP versions, FIN and RST). A client SHOULD apply a response timeout
to every live read and, on HTTP/2, SHOULD send keep-alive PING frames at no more than a quarter
of the shortest idle timeout it must survive (15 s for 60 s): pings keep an idle rule from firing
and replace a silently dead connection within about 10 s (measured), whereas without them a
silent loss costs 30 s on HTTP/1.1, 90 s for a request and 120 s for a long poll on HTTP/2, and
an SSE stream never recovers. An idle connection closed by the server or by a proxy with FIN or
RST is not an error: the next request opens a new one (about 3 ms on loopback plus a round trip).
Retrying a request after a transport error includes an append, so a lost reply can store a record
twice; readers drop the second copy (see **Records**). Servers: the Go server 0.3.0 behind
Caddy's `log` directive answers SSE with `500` (long poll is unaffected), and the reference Node
server closes idle keep-alive connections at 5 s, which a proxy with a longer upstream keep-alive
would race (not measured).

**Long-poll wake-up latency is not guaranteed by an open connection.** In the Go server v0.3.0,
concurrent appends can race the registration of a long-poll waiter: an acknowledged append was
present on the server while its poll remained parked until the 30 s server timeout, answered
`204` with the old offset, and a new offset read returned the pending record. This reproduced
with HTTP/1.1 and HTTP/2; no such failure occurred in the Node controls. It is a server-side
lost wake-up, not evidence of TLS record loss or a need to retry successful appends. Readers
MUST resume from the last returned offset after an empty poll too; a deployment MUST NOT treat
the millisecond latency samples below as an upper bound. See the
[CI census, access-log evidence and source-patch A/B](rfc/0004-spike-tls-results.md#long-poll-lost-wake-up-census-2026-10-27).
The tested re-check-after-registration patch eliminated failures in its sample; the pinned
release in the spike remains unpatched and its strict measurement assertion is unchanged.

**What the server can do.** It can refuse, delete, close, slow or lose streams (and so can anyone
with the document id); show a reader a stale prefix, including the epoch record of a revocation
*[INFERENCE]*; choose which of two competing valid epoch records is first; lie about time within the
30 s clamp; and see who writes what and when (document id, number of epochs, stream kinds,
principals, sizes, timing, the roster with its roles in cleartext). It cannot read titles, content,
the version graph (parents are inside the ciphertext), file names or MIME types, and it cannot
forge or alter a record.

**Measured** (Node 0.3.9 and Go 0.3.0, one run, loopback, release build): an update from one
client to a second client's verified read 3.9 ms p50 and 4.5 ms p95 on Node (2.0 and 2.7 on Go);
5 000 updates cold in 266 ms (Node), 11 ms from a snapshot; a cold reader of 15, 30 and 55 epochs
took 33, 63 and 113 requests and 21, 38 and 79 ms (Node), so a roster of 50 members is 50 epochs;
a roster change 12 to 126 ms for 4 to 50 members; the largest JSON message accepted was 256 MiB on
Node and 32 MiB on Go, the largest closed stream at least 512 MiB (the cap tried) and 64 MiB, and a 32 MiB file
published through the binding in 431 ms (Node) and 257 ms (Go); a file over what a server takes
needs chunking, which is not defined. The 11 journeys pass with no skips.

**Measured over TLS** (the same two servers behind Caddy 2.10.2, loopback, one run, release
build, 4 vCPU): the 11 journeys and the other tests of Spike B pass over TLS 1.3 on HTTP/1.1 and
HTTP/2 (32 passed, 0 failed in run 40179818637 at `8e4f228`; the run at `167aa64` is green in all
four jobs), and the server's own access log shows the version under test for
every request. An update to a second client's verified read took 4.5 to 6.7 ms p50 on Node and
2.6 to 2.8 ms on Go, no measurable difference from plain HTTP through the same proxy (a single
sample of 200, so differences under 1 ms are noise); a full TLS 1.3 handshake 0.3 to 0.6 ms
(resumed 0.2 to 0.4 ms) and 1.3 kB on the wire; per request, HTTP framing of 98 bytes on HTTP/2
against 162 on HTTP/1.1 (the repeated response headers fall from about 790 to about 81 bytes);
both servers allow 250 concurrent streams per connection. With 50 live readers on one client no
head-of-line blocking was seen at the HTTP/2 level; a large download under delay was limited by
the fixed 64 KiB HTTP/2 window (8 MiB at 12.6 MiB/s at 100 ms RTT in an emulation that does not
model TCP), and the adaptive window was worse in that setup. The lease `409` compare-and-set
behaved identically over TLS (20 rounds of 8 races). The client's dependency tree grew by 23 crates
(rustls with `ring`; no OpenSSL).

**Not measured.** A TLS terminator other than Caddy 2.10.2 (nginx, Envoy, a cloud load balancer,
a CDN), a server with native TLS, mutual TLS, revocation checking, pinning, a chain with an
intermediate CA, TLS 1.2-only servers, 0-RTT, HTTP/3; the platform trust store (a product needs a
decision) and certificate rotation; a real network (the TLS runs were on loopback, a userspace
forwarder emulated delay, stalls and the proxy idle rules, and nothing lost packets, so TCP
head-of-line blocking was not measured); a real load balancer's idle rules; more than 50 streams on
one connection and servers allowing fewer; large files over a long-RTT HTTP/2 path beyond one
emulated 8 MiB; the Go server with HTTP/2 only and its SSE behind a logging front end (which
fails); CDN caching of `ctl`; retention, expiry, TTL and forks; the Go server's power loss; a
malicious server that withholds, reorders or rolls back (reasoned, not run); more than 50
members, more than two concurrent owners, more than 8 concurrent appenders, more than 5 000
updates; clock drift over minutes; multiple devices for one principal, key backup, identity key
compromise and rotation; a gate that follows the roster; another client implementation (the Rust
code is the only specification of the signed byte strings besides this text); presence (L3).

### 8.5 Peer-to-peer binding on iroh and p2panda (measured once)

**Measured once**: iroh **1.4.1** with iroh-blobs **0.103.1** (0.103.0 is yanked: any peer could
push blobs into a provider's store) and p2panda-core, -store, -sync, -auth, -encryption and -spaces
**0.7.1** (pre-1.0; "APIs are not yet considered stable"), three or fewer nodes on loopback in one
process or one sandbox on a shared 4-vCPU GitHub-hosted runner, CI run 40139997921.
[Spike C results](rfc/0004-spike-c-results.md). The timings in this section are loopback unless a
paragraph names the second run ([relay spike results](rfc/0004-spike-relay-results.md), CI run
40179059127): the peers in separate Linux network namespaces behind `iroh-relay` 1.4.1 with a
test CA, links shaped by `tc netem` to 50 and 150 ms RTT, 1 % and 5 % loss and 1 Mbit/s, where
all 11 journeys passed in both modes in all 7 conditions. There is still no NAT, no jitter, no
real WAN and no n0 relay carrying dcp-p2p traffic. Verdict: a **conditional go for L0 and the
version-carrying half of L1, and a go with a
size limit for L2; a no-go for the `commenter` role and for exclusive leases**, and for using
p2panda-net, p2panda-blobs or p2panda-spaces' role-change API as published. All 11 journeys of
section 12 pass with nothing skipped and with a negative control (reader authorisation off) that
makes the forged-version journey fail for the intended reason.

**What 0.7.1 provides.** Usable: p2panda-core's signed logs, p2panda-sync's `LogSync`
protocol, p2panda-auth's group CRDT, p2panda-encryption's key agreement, and iroh-blobs.
**Not usable as published**: p2panda-blobs 0.7.1 is an empty crate, so files go through iroh-blobs;
p2panda-net's sync cannot be started per peer (`initiate_session` is test-only) or gated by
membership, so a removed member would keep receiving every operation; p2panda-spaces cannot change a
role (`unimplemented!()` for any other auth action, *read, not run*), reports decrypted data with no
author, needs one manager and one database per document, and keeps every message in the state it
rewrites on each message; p2panda-core refuses a header holding a byte string over 512 bytes. The
binding is those pieces plus about 3,300 lines of its own.

**Principals.** A principal is an Ed25519 public key, 64 lower-case hex characters, which is at
once the iroh endpoint id, the p2panda author and the p2panda-auth actor, so the QUIC handshake
authenticates the author. `created_by.principal` MUST equal the key that signed the entry. **The key
is the device** (section 9 rule 4).

**Documents.** The document id is 32 random bytes (64 hex characters), and it is the only topic. A
document is one p2panda-spaces space. An implementation MUST keep one p2panda-spaces manager (and
one database) per document, because a second document in one manager would depend on the first
one's auth operations (*read, not run*).

**Contact card.** Before an owner can add a principal, it MUST hold that principal's key bundle for
the document: a `KeyBundle` operation signed by that principal, obtained at the join request or out
of band. `grant` without it MUST fail (measured: `missing key bundle`).

**Entries.** Meta, version, lease and update are carried as entries. An entry's body is
`{author, doc, heads, ts_us, nonce, env}` where `env` is the draft-1 object, with the binding's own
fields beside it (for a version, `key`; for a lease, `lamport`; for an update, `device`). The author
signs the exact bytes of the body with its Ed25519 key; the signed body is encrypted to the group
(p2panda-spaces data scheme) and carried as an operation of the author's log. The entry id is the
hex BLAKE3 hash of the body and is the version, lease or update identifier. A reader MUST verify
the signature, that `doc` names the document, and that `created_by.principal` equals `author`. An
operation's header MUST NOT hold a byte string longer than 512 bytes (p2panda-core refuses it; its
source applies the same limit to arrays, maps and text, which was not separately measured); a
message larger than that is split into byte strings of at most 480 bytes, at most 500 of them.

**Roles.** `viewer` is p2panda-auth `Read`, `editor` is `Write`, `owner` is `Manage`. **`commenter`
is not expressible**: p2panda-auth has four levels (`Pull`, `Read`, `Write`, `Manage`) and none for
comments; its `Access<C>` carries a generic `Conditions` value that could tag a commenter, which was
not tried, and a binding MUST refuse to grant `commenter` (`Invalid`; question 2). A role change is
a removal followed by an addition (two operations, two key rotations), made visible to a peer as a
unit, because p2panda-spaces 0.7.1 has no promote or demote; an implementation MUST NOT process a
p2panda-auth promote, demote or `SpaceUpdate` until it does. A member whose session breaks between
the two would see itself as a non-member until the next session; a journey that polls `role()` MUST
tolerate `Forbidden` for that reason.

**Reader authorisation.** Any member holding the group key can encrypt anything, so **a reader
decides what counts**: it lists an entry as a version, lease or update only if its author had at
least `Write` (for meta: `Manage`) in the group state after the auth operations the entry names in
`heads`, and no removal or demotion of the author exists outside the causal past of those
operations. The rule is evaluated when the entry is read, not when it arrives. A removed editor
that keeps writing was decrypted by the owner (it still held the old key) and then not listed.

**Files.** The file is sealed with a fresh 32-byte key (XChaCha20-Poly1305, 24-byte nonce prefix)
and the blob is stored in iroh-blobs, addressed by the BLAKE3 hash of the sealed bytes. The
version's locator is `blake3` with that `hash` (4.2.1); `sha256` and `size` describe the
plaintext, so **the two hashes never coincide** and the reader checks `sha256` after decrypting. The
file key travels in the encrypted entry beside the version and never in the DCP object. A provider
MUST serve a blob only to a current member of a document that references it, MUST refuse get-many,
observe and push requests (iroh-blobs 0.103.1 or later), and **a reader MUST list a version only
when its blob is present** (version rule 8). Nothing is uploaded in this binding: a writer stores
the blob locally and peers fetch it later, so a writer that goes offline right after publishing
leaves a version whose file only it holds.

**Sync.** A session is a bidirectional QUIC stream on ALPN `dcp/p2p-sync/1`: a `Hello` naming the
document, a status byte (the acceptor applies the membership rule), then the p2panda-sync `LogSync`
protocol (Have, PreSync, Operation, Done). A node MUST start and accept sessions only with a
current member of the document (a device that has just joined and knows no member serves only the
creator it joined through), and MUST take the membership check and the read of the log under one
lock so that a removal is never served before the gate knows it. Anti-entropy (every 200 ms in the
tests) and every local publish start sessions with the document's other members.

**Paths and relays.** A connection is direct or relayed as iroh's `Connection::paths()` says
(`is_selected`, `is_ip`, `is_relay`); an implementation MUST NOT infer it from the addresses it
was given. (Spike C's relay test gave each node only the other's relay URL and called it
relay-only; iroh's path data shows it synced over a direct IP path with a relay path beside it,
so its comparison of "relay-only" and direct latency compared two direct connections and
nothing may be quoted from it.) With UDP reachable between the peers, a connection given only a
relay URL upgraded to a direct path about three round trips after the handshake (median 156 ms
at 50 ms RTT, 463 ms at 150 ms; 5 of 5 pairs in each of 7 conditions). With peer-to-peer UDP
dropped by a firewall, no IP path was ever opened (1,515 connections, 0 bytes on IP paths; the
same assertion failed in 35 of 35 runs with UDP allowed). An established direct connection
survived the relay being killed (7 of 7 conditions); a relay-only connection delivers nothing
while the relay is down, is closed by QUIC after 30 s (iroh's `RELAY_PATH_MAX_IDLE_TIMEOUT`),
and recovered 3.5 to 22.6 s after the relay returned: **a relay is needed to start and to
recover, not to continue**. A direct path that is cut silently is abandoned after 15 s (iroh's
`PATH_MAX_IDLE_TIMEOUT`) and traffic moves to the relay (15.0 to 19.9 s in the runs; the next
publish was visible after 15.6 to 21.2 s). A binding MUST therefore treat 20 s as the time to
notice a lost path and MUST NOT assume faster fail-over in L3. These are one run per condition
on routed namespaces: NAT traversal was not measured.

**Ordering and failures.** Operations are applied in dependency order, from a pending set. An
operation that fails to decode is dropped and ends its session; operations received earlier in the
session are kept (a tampered header, one bit flipped, was refused on decode by signature, the
document's counts were unchanged and a normal publish arrived afterwards).

**Revocation.** `revoke` MUST push the removal to every reachable remaining member and wait
(bounded, 5 s) for each to confirm it applied it. **Content published by a member that has not
applied the removal is encrypted to the old key and is readable by the removed member** (measured);
with the gate off, the removed member's node received the later operations and decrypted none
of them, and with the gate on its sessions and blob fetches were refused.

**Leases and time: no binding clock.** There is no clock in a peer-to-peer binding, so lease rule 1
is replaced by lease rule 11. A lease is an entry carrying a **Lamport value**, one more than the
highest lease value its writer has seen; the current lease is the entry with the highest
(Lamport value, entry id), so every replica holding the same entries picks the same record, and two
writes that did not see each other tie on the Lamport value and the entry id decides. **The stamp
of a lease is the time the reader first saw it on the reader's own clock, and `now_ms` is that
clock**, so two devices never compare a timestamp. With clocks 60 s apart the lease journeys 4 and 5
passed, and a control that stamped with the author's clock (editor +60 s) failed test 5. A replica
that was offline sees an old lease as new and treats it as active for up to one full `duration_ms`
after it first sees it, never longer. The author's `ts_us` in an entry orders the heads for display
only; it is not authenticated. **The lease is advisory and not exclusive across a partition** (lease
rule 13): both partitioned sessions ran `lease::acquire` with two read-backs and a 500 ms settle and
both returned `Acquired`; after they reconnected exactly one still held it 254 ms (debug) or 52 ms
(release) later and both named the same record. Propagation of a lease write between two nodes
on loopback was 13.8 ms median (min 9.6, p95 16.0, max 17.3). **On a WAN it is not 500 ms**
(relay spike, 30 writes per cell, relayed and direct alike): a median of 110 ms at 50 ms RTT,
437 to 507 ms at 150 ms RTT (maximum 525 ms; 11 to 15 of 30 writes exceeded 500 ms), up to
662 ms at 5 % loss with no delay, and up to 955 ms at 150 ms, 5 % loss and 1 Mbit/s; a write
triggers a session of about two to three round trips, and a session that loses a packet waits
for loss recovery (1.0 s in the runs, with an anti-entropy interval of 200 ms). The settle
interval SHOULD be at least 2 s unless every member is on one low-latency, loss-free network;
500 ms is enough only there. (The conformance journeys pass with 500 ms at 150 ms RTT because
they wait on the fixture's own bound, so a passing lease journey is not evidence for the value.)
Version rule 5 (two heads are shown, never silently merged) is what makes this safe.

**Availability.** There is no server. A document's data is available while members that hold it are
online with the members that want it; no particular peer is required. A version published by a
member who then goes offline reaches another member through any third member that was online with
both (51 ms in release); if its only holder is offline it reaches nobody until the holder returns
(measured: neither of two returning members had it for the 3 s asserted, both got it 102 ms after
the editor returned). A peer that is a `Pull` member would hold ciphertext and no key and would
reconnecting (693 ms in debug), and two offline writers' divergent versions were visible as two
heads at all three nodes 55 ms after reconnecting (372 ms in debug). On shaped links (relay
spike, relayed and direct alike) an offline catch-up of five versions took 53 ms unshaped, 363 ms
at 50 ms RTT, 877 ms at 150 ms and 1.4 to 2.5 s at 1 Mbit/s; two divergent heads were visible
everywhere 54 ms to 2.5 s after reconnecting (1.0 s where one lost round was needed); a 1 MiB
file was fetched in 0.5 s at 50 ms RTT, 1.4 s at 150 ms and 9 to 21 s at 1 Mbit/s.

**Updates (L2) and scale.** The 34 000-byte inline update was exercised and works. **p2panda-spaces
0.7.1 does not scale in entries**: a reader needed 0.59 s for 100 inline entries, 6.1 s for 300 and
81 s for 1 000 after the last publish, because the orderer keeps every message in the state it
rewrites per message (2.1 MB at 1 000). Inline updates therefore stay usable to a few hundred
entries per document on this library; versions are fine. A 100-byte inline update costs about
2 170 bytes on the wire (cbor-core writes a byte string of 24 or more bytes as an array of
integers, a factor of 2.0).
The reader is CPU-bound without a rate limit (at 300 and 1 000 entries its CPU time was 79 to
102 % of the wait, 95 % of it in the store's own thread) and network-bound at 1 Mbit/s (the wait
within 4 % of the transfer time of the bytes received); delay and loss alone did not change that.
At 1 000 entries the reader needed 49 s (150 ms RTT) to 64 s (no shaping) and 60 to 99 s at
1 Mbit/s. On an unshaped link it **received 438 MB for 1 000 entries whose state is 2.1 MB**: a
counters run showed each operation received 171 times (768 sync sessions for 1 000 publishes;
the mechanism, a session re-sending the backlog the receiver has not yet applied, is an inference,
not traced), and absolute seconds differed by CPU model (44 s and 64 s for the same test on two
runners), so only ratios inside one job are comparable.

**Measured** (release build, loopback): connect plus first sync 27.3 ms median, a sync session over
an open connection 6.6 ms; a 32 MiB file written in 176 ms and fetched in 59 ms (reader decrypt and
read 52 ms). **Flatpak**: three nodes on `127.0.0.1` passed in a `flatpak build` sandbox with
`--unshare=network` and with `--share=network`, so a two-window test needs no network permission.
**What a client needs outbound** (captured in the relay spike, outside Flatpak): TCP 443 to the
relay (required to start, and for every peer not reachable directly) and, to be direct, UDP to its
peers' addresses on any port (each endpoint binds one UDP socket; the operating system chooses the
port unless the application sets one). UDP 7842 (address discovery) and TCP 80 to the relay are
optional: with them blocked a node was ready after 3.0 to 3.6 s instead of under 0.5 s, and with
everything else dropped a node did not come online. With a relay URL given as an IP address no DNS
is needed (a host name needs the system resolver; not tested). In Flatpak the permission is
`--share=network` in `finish-args`, which cannot be limited to ports and which the repository's
manifests do not grant to the app (only to the build); a site firewall, not Flatpak, enforces the
port list. **Discovery**: the endpoint MUST be built with iroh's `Minimal` preset and a
configured relay (as dcp-p2p is): it then put 0 packets on the wire in a capture, while the `N0`
preset contacted `dns.iroh.link` and four n0 relays and **published the endpoint id and home relay
URL** there (374 packets). Do not use `presets::N0` in a private deployment.

**Not measured.** NAT traversal (every hole punch in the relay spike crossed a routed network,
not a NAT), jitter, a real WAN, production relays carrying dcp-p2p traffic, host-name relay
URLs, IPv6, more than one relay and fail-over between relays, a fresh process that never knew the
peer's address after the relay died; HTTP-proxy (`proxy_url`) relay access (read, not run); two
devices in Flatpak; three or more writers on one document, concurrent membership changes by two
owners, nested groups, key-bundle expiry, more than 1 000 entries, restart (all state of a store is
in memory) and the filesystem blob store; the `unimplemented!()` panics and the one-auth-graph
behaviour (read in source, not run); presence (L3), gossip and live co-editing; `commenter` through
`Conditions`; another implementation of this binding and interoperation with the Matrix and WOPI
bindings; a hostile peer beyond the tests above (unbounded pending operations, withheld blobs, a
valid but huge operation).

## 9. Identity

1. A principal is whatever the binding says it is.
2. Principals from different bindings MUST NOT be treated as the same person unless both have
   signed a link statement. The format of that statement is not defined in draft 1.
3. A program MUST show the principal behind every version, lease and update it displays.
4. In a binding where the principal is a public key (8.4, 8.5) the key is also the device: moving a
   principal to another device moves its secret, and neither binding has key backup, per-device keys,
   rotation or a recovery for a lost or stolen key. A person with two devices is two principals until
   a link statement (rule 2) exists.

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
| A file is not yet available | Do not list the version until the file can be fetched and verified (version rule 8). In 8.5 it appears when a peer that holds the file is online again. |
| A document the client knew answers `404` | Never treat it as an empty document and never recreate it. On 8.4 it is data loss (an in-memory server lost everything on restart); on 8.3 it is a removed member, and a write is reported as forbidden. |
| Two sessions both hold the lease after a partition | Keep the holder the binding's winner rule names; the other stops saving as holder. A fork that was saved stays a second head (lease rule 13, version rule 5). |

## 11. Security considerations

- **A lease is not a permission.** Any member with the editor role can publish a version whether or
  not it holds the lease. Role mapping is what enforces who may write.
  A lease is also not exclusive where no coordinator exists: two sessions that cannot see each other
  can both hold it (lease rule 13).
- **Revocation has limits.** Removing a member stops them from receiving new content in an
  encrypted binding: measured on Matrix, a removed member's device decrypted none of the next
  events (0 of 70, with a control), on the condition that the sender's own member view already shows
  the removal. Content they already received stays readable to them, including a file whose key they
  hold. DCP does not claim otherwise, and an implementation MUST NOT either, unless a journey proves
  a stronger property.
- **Revocation in an epoch binding (8.4).** Removal takes effect when the old epoch's streams are
  sealed, not when the owner decides: until then a removed editor can still write into the old
  epoch and readers accept it (tested on both servers), and what was accepted stays. A member added
  later reads every earlier epoch (the signed `prev_key` chain shares the history, as Matrix
  `shared` does), and a member removed and re-added reads what was published while it was out. A
  server that withholds the epoch record of a revocation can keep a reader in the old epoch
  *[INFERENCE]*: no signed tail or gossip exists. Not run.
- **Revocation without a server (8.5).** A member that has not yet applied a removal still encrypts
  to the old key, and what it publishes is readable by the removed member (measured). Removal takes
  effect for content published by members that have acknowledged it; `revoke` waits for them, and
  one that is offline is not waited for.
- **Revocation by unsharing (8.3)** is access control, not encryption: the server holds the
  plaintext, a removed member gets `404` for new content, and content already downloaded stays.
- **Editor services see plaintext.** Joining one to a document gives its operator access to that
  document. This MUST be shown to users (section 8.1).
- **Untrusted files.** A version's file comes from another principal. Programs MUST open it with the
  same hardening as any untrusted download: bound decompression, reject malformed packages and
  limit sizes. DCP does not make a file trustworthy because a member published it.
- **Untrusted updates.** Section 7 rule 3.
- **Hashes and authorship.** `sha256` detects corruption and mix-ups. It does not authenticate the
  author; authorship comes from the binding's signatures: the event signature on Matrix (a reader
  checks that `created_by.principal` equals the event's `sender`, section 8.1), the Ed25519
  signature of a record in 8.4 and of an entry in 8.5 (a reader checks that `created_by.principal`
  is the signer). **Not on 8.3**: WebDAV does not name the uploader, `created_by.principal` is a
  claim, and an editor wrote a version naming the owner and the server accepted it (8.3). A reader
  that needs it authenticated must use another source; the binding offers none.
- **Media keys are capabilities.** Matrix media is not access-controlled per room: the key in a
  version event is what protects the file. A removed member who kept that key can still download that
  file, and a removal does not revoke it.
- **Roles on an encrypted Matrix room.** A power-level table without `events["m.room.encrypted"]`
  lets a viewer's forged version through the server (reproduced); see section 8.1.
- **A WOPI host in production.** The measured adapter validates the access token only. A deployment
  MUST use HTTPS and SHOULD validate WOPI proof keys (`X-WOPI-Proof`); neither was exercised.
- **Server availability (8.4).** A Durable Streams server has no access control: anyone who holds a
  document id can delete, close or poison its streams (tested on both servers), and one junk append
  blocks the lease stream for the rest of the epoch. Confidentiality and authorship survive;
  availability does not. A deployment MUST put access control in front of it.
- **A hostile peer (8.5).** One malformed header ends a sync session, so a peer can end sessions by
  sending it early; operations received before it were signature-checked when decoded and are kept.
  Spam of pending operations is not bounded, a peer can withhold blobs, and the only bound on a
  valid but huge operation is the header ceiling, about 120 kB of ciphertext. None of that was
  tested beyond those limits.

## 12. Conformance tests

A DCP implementation is demonstrated, as in this repository's definition of done, by journeys and
not by source inspection. The following MUST pass for each binding an implementation claims.

| # | Claim | Evidence |
|---|---|---|
| 1 | Two instances converge on the same head | Two app instances; both show the same head and content |
| 2 | A second head is shown, not hidden | Two offline edits from one version produce a conflict listing both heads |
| 3 | A version is complete and valid | A truncated upload is never published; a hash mismatch is rejected by the reader |
| 4 | Lease: a second session cannot take an active lease | Journey with two sessions; read-back resolves a simultaneous write. On a binding with an atomic compare-and-set, exactly one write is accepted (lease rule 9) |
| 5 | Lease expiry releases the document | Kill the holder; another session acquires after expiry. The journey's lease duration is the binding's: 4 s by default; a binding whose requests are slower states a longer one (8.3 uses 20 s over a federation, where 4 s failed in 7 of 8 runs) |
| 6 | Roles are enforced | A viewer cannot publish; an editor can; checked at the binding, not only in the UI |
| 7 | Removed members lose new content | Remove a member, publish, assert that device cannot read the new version (encrypted bindings). For an encrypted binding the binding's own test MUST also decrypt every record and file of the new epoch with every key the removed principal ever held, and a control that reuses the key MUST decrypt them (8.4 meets this; the journey cannot, because it needs the raw bytes). An unencrypted binding (8.3) reports it as access control and says that content already fetched stays |
| 8 | An unknown algorithm falls back to versions | An L2 update from an unsupported algorithm is ignored and the document still opens from versions |
| 9 | A stream gap is recovered | Drop one update; the receiver reloads from the latest version |
| 10 | A WOPI client round-trips (8.2) | Collabora Online opens, edits and saves; the new version has the correct parent and the lease token is Collabora's lock string. The access token is minted by the adapter (8.2 rule 10). Passed once on Collabora Online 26.04.5.2 |
| 11 | Schemas hold | `python3 docs/collab-spec/validate.py` passes |

Each test is entered in `conformance/capabilities.json` only with the revision on which it passed.
A binding that cannot express a journey reports it skipped with the missing capability stated, never
as passed (over WOPI: 1, 3 and 6 passed through the simulator, 10 against Collabora, and 2, 4, 5, 7, 8
and 9 skipped). A test that needs Synapse, Collabora, Nextcloud or a Durable Streams server MUST fail
when its environment is missing unless `DCP_SKIP_SYNAPSE=1`, `DCP_SKIP_COLLABORA=1`,
`DCP_SKIP_NEXTCLOUD=1` or `DCP_SKIP_STREAMS=1` is set, and then MUST print that it skipped. This
draft adds no entry to `conformance/capabilities.json`.

Results of the journeys of this section, each from the run its section cites:

| Binding | Passed | Failed | Skipped (capability stated) |
|---|---|---|---|
| Matrix (8.1), relaxed, default and default-drained limits | 11 | 0 | 0 |
| WOPI host over the Matrix binding (8.2) | 5 | 0 | 6 |
| Nextcloud, one server (8.3) | 9 | 0 | 2 (no L2) |
| Nextcloud, two servers over OCM (8.3) | 9 (test 5 with a 20 s lease) | 0 | 2 (no L2) |
| Durable Streams, Node 0.3.9 and Go 0.3.0 (8.4) | 11 | 0 | 0 |
| iroh and p2panda (8.5) | 11 | 0 | 0 |

## 13. Open questions

1. The registered namespace to replace `org.tunaos.dcp`.
2. The `commenter` role: its event, and its power level. **No binding expresses it yet.** On Matrix the
   `m.room.encrypted` gate at 50 (8.1) stops a member below 50 from sending any encrypted event,
   comments included. On Durable Streams it is a `viewer` until a comment event exists (8.4). On
   p2panda-auth there is no comment level; its `Access<C>` carries a generic `Conditions` value that
   could tag one, which was not tried (8.5). On Nextcloud comments are a separate app that any reader
   may use (8.3).
3. Whether the Matrix lease should be a state event at all, given the missing compare-and-set. The
   two-read rule held in 1 200 consecutive races (lease rule 3), so the answer is no longer "not
   run". What stays open is the case lease rule 13 describes, writes that a rate limiter delays by
   seconds. A timeline event with a deterministic winner rule, or having the writer check which
   record its write replaced (`unsigned.replaces_state` or `prev_content`; present for state events
   *[INFERENCE]*, not checked), might close it; neither was tried.
4. Whether unstable MatrixRTC and sticky events should ever carry L3. No binding has L3: Durable
   Streams has no ephemeral channel in the measured text (a TTL stream was not tried, 8.4), and
   gossip is the natural carrier on iroh and was not built (8.5).
5. The link statement format (section 9.2). It is now needed in two more places: in 8.4 and 8.5 the
   principal is a key, so a person with two devices is two principals.
6. **Closed by Spike E (8.3):** OCM and WebDAV can express heads and leases, as files in a
   create-only log, not through ETags or `LOCK`. Open: whether an OCM server other than Nextcloud
   34.0.5 and 35.0.2 behaves the same, and a document being two shares that the receiver must each
   accept.
7. Whether L2 is practical per binding. Matrix needs a homeserver configured for it (a default
   Synapse allows 0.2 inline events per second per user, 8.1); Durable Streams carried a 100-byte
   update to a second client in 3.9 ms p50 (Node, loopback, 8.4); the iroh and p2panda binding is
   usable only to a few hundred entries per document, because p2panda-spaces 0.7.1 took 81 s to
   catch up on 1 000 entries (8.5); WebDAV has no channel (8.3).
8. A first algorithm profile for `loro/1`, and whether a `yjs/13` profile is needed for
   interoperability with LibreOffice's Yjs-based prototype.
9. How a hosted editor in an encrypted room is shown to users. Editor services were not built or
   tested in Spikes A, B or C; in 8.4 and 8.5 one would need the epoch key or group key, which makes
   its operator a member.
10. **Closed by Spike F (8.2 rule 12):** one version per `PutFile`, one lease event per refresh, both
    against the user's `rc_message`; 6, 14 and 40 events in 187 s at three timer settings. Open: how
    a host answers within the 3 seconds Collabora allows (8.2 rule 11) over a binding whose writes a
    limiter delays (write-behind, `429` or `503` were not tried), and Collabora's reaction to the
    `409` 1010 refusal on a conflicted document, to a lease broken by an owner during an edit, and
    to a `LOCK` that two hosts both answered `200`.
11. **Which binding first, and whether a Matrix account is required** are decided in
    [RFC-0004](rfc/0004-interoperable-document-collaboration.md#open-questions), not here.
12. Access control in front of a Durable Streams server (8.4). A shared credential per role cannot
    follow a revocation. A gate that derives the roster from the signed, cleartext control stream
    and authorises each request by a signature is possible by construction and was not built.
13. Authorship on a binding that has no sender authentication (8.3). `created_by.principal` is a
    claim there: an editor wrote a version naming the owner and the server accepted it. Whether DCP
    should define a detached signature for such a binding, or leave authorship to the Activity API
    that names the creator asynchronously, is undecided.
14. A locator that is relative to the document. In 8.3 the `webdav` `href` and in 8.4 the
    `durable-stream` `href` are `https` URLs that no reader fetches (a reader resolves the path
    against its own server or mount). That is why 4.2.1 says so, but a locator type naming a file
    relative to the document would need no unfetched URL. It would be an additive enum value
    like the two added in this revision; it was not added because no spike needs it to run.
15. **Answered by [Spike Q15](rfc/0004-spike-q15-results.md) (8.1); the rest stays open.** A reader
    judges a sender by the level in force at the event, obtained from the room's own
    `m.room.power_levels` events in the `/messages` pass (linked through
    `unsigned.replaces_state`; exact in a room created with `m.federate: false`) or from
    `GET /rooms/{id}/context/{eventId}?limit=0` (any room, cached by event ID); an event whose level
    cannot be established is dropped, never judged by the current table. Neither way costs a
    rate-limit token on Synapse 1.163.2, the only server tried. Open: a room that federates across
    real homeservers, other homeservers (the walks need `unsigned.replaces_state` and
    `unsigned.prev_content` on `/messages`), the path when a server answers `429` to `/context`,
    incremental `/sync` readers and a persistent event cache, room version 12 against a server, and
    the in-memory reference store (`MemoryStore`), which still judges by the current role.
16. Epoch bindings (8.4). Every roster change is a new epoch with a new key; a roster of 50 members
    is 50 epochs, and a cold reader read 55 epochs in 79 ms (Node, 33 to 113 requests for 15 to 55
    epochs). Whether a role change can take effect inside an epoch without letting a viewer's earlier
    forgery turn valid is open; the binding avoids the question by never doing it.
17. Availability and a second implementation. In 8.5 data is available only while members that hold
    it are online with the members that want it; an always-on member that holds ciphertext and no key
    (a p2panda-auth `Pull` member) was not built. Both 8.4's signed byte strings and 8.5's wire are
    specified only by one Rust implementation each; neither has been read by another.
18. The trust anchor of a Durable Streams client (8.4). The TLS spike used a configured CA and no
    built-in roots; whether a product uses the platform's roots (and on Windows and macOS what
    that does to certificate rotation and to a deployment's own CA) is a decision, not a
    measurement. Behind a real load balancer or CDN the idle and age rules of 8.4 were emulated.
19. Settle interval and path loss on iroh (8.5). A settle interval of 2 s is a SHOULD on the
    strength of 30 writes per condition on routed namespaces; whether any value covers a NATed
    WAN, and whether a 15 to 20 s fail-over from a cut direct path is acceptable for lease
    renewal, are not known.

## Changelog

- Draft 1, update 5 (2026-10-27): **8.1 only, no wire change; `dcp` stays `1`.** The rule "wait for
  the invite before joining" is tightened to "until the SDK can name the inviter": a join sent
  while the sync that carries the invite is still storing it lost the history in 13 of 760 runs
  of the late-editor journey (Spike A, late joiner). The Matrix binding waits for
  `invite_details().inviter`; the journey went from 13 failures in 760 runs to 0 in 760.
- Draft 1, update 4 (2026-10-25): the first algorithm profile, [`loro/1` with `tables/1`](collab-spec/profiles/loro-1.md)
  (RFC-0005 open question 1). **`dcp` stays `1`: no object or schema changes.** For a CRDT algorithm,
  `base_version` of an update names the version the session's replica started from (the genesis
  version, or a later version that `covers` the updates before it); it does not follow the head, and a
  receiver accepts an update whose `base_version` is its session base whatever the head is.
  `dcp_core::updates::CrdtSession` is the reference implementation.
- Draft 1, update 3 (2026-10-25): three CI verification spikes, each measured once and each green in
  its own workflow: [TLS](rfc/0004-spike-tls-results.md) (8.4 over TLS 1.3, HTTP/1.1 and HTTP/2),
  [Q15](rfc/0004-spike-q15-results.md) (the power level at an event, 8.1) and
  [relay](rfc/0004-spike-relay-results.md) (8.5 through a real `iroh-relay` on shaped links).
  **`dcp` stays `1`: nothing here changes an object or a schema; no schema, example or validator
  case changed.**
  - 8.1: the reader rule judges a sender by the power level at the event, says how a reader
    obtains it (linked walk of the `/messages` pass, `/context` for what the walk cannot place),
    drops an event whose level cannot be established, and recommends `m.federate: false`; question
    15 is answered and its remainder stays open.
  - 8.4: new text on transport (certificate chain and host name verification, no downgrade, no
    plaintext fallback), redirects and `Location`, the HTTP version, and live reads (server
    heartbeat below the shortest proxy idle timeout, resume from the offset on any transport error,
    HTTP/2 keep-alive PING), with the measured TLS figures; the Go server's restart across SIGTERM
    and SIGKILL is now verified.
  - 8.5: the loopback-only and relay sentences are replaced by the measured **Paths and relays**
    paragraph; Spike C's "relay-only" latency comparison is withdrawn (it compared two direct
    connections); the lease settle interval SHOULD be at least 2 s on a WAN (lease rule 3 points
    to it); WAN availability figures, the state-growth CPU and network note, the port list for
    Flatpak and the discovery presets are added.
  - Behaviour changes that are not wire changes: the TLS verification MUSTs and the live-read rules
    (8.4), the 8.1 reader rule above, and the 8.5 MUST NOT infer a path from the addresses given.
    Lease rule numbers and section numbers are unchanged. Questions 18 and 19 are new.
- Draft 1, update 2 (2026-10-24): four more spikes, each measured once and each green in its own CI
  run: [Spike B](rfc/0004-spike-b-results.md) (Durable Streams), [Spike C](rfc/0004-spike-c-results.md)
  (iroh and p2panda), [Spike E](rfc/0004-spike-e-results.md) (Nextcloud WebDAV and OCM) and
  [Spike F](rfc/0004-spike-f-results.md) (default rate limits, forged updates, churn, lease races).
  **`dcp` stays `1`: every change to an object is additive.**
  - Section 8.3 is **replaced**: the ETag and `LOCK` mapping of draft 1 was rejected by Spike E's
    measurements. No implementation of the old outline existed, so nothing on the wire changes.
  - Section 8.4 (Durable Streams) and section 8.5 (iroh and p2panda) are new, from Spikes B and C.
  - Lease rules 9 to 13 are added and rules 1 to 8 keep their numbers. Version rule 8 is added.
  - Section 8.2 gains rules 11 and 12, and rule 6 now also requires `PutFile` on a document with
    more than one head to be refused.
  - Sections 3, 4.2.1, 5, 7, 9, 10, 11, 12 and 13 gain the corrections the four spikes proposed.
  - **On-wire change, additive:** `locator.type` gains two values. `durable-stream` takes an `https`
    `href` like `url`; `blake3` takes a `hash` of 64 lower-case hexadecimal digits. An object that
    uses either is valid under the updated schema and **invalid under the previous one**, so a
    reader that has not been updated rejects such a version (an object that does not validate is
    ignored: section 4 and 8.1) and does not see it. That is allowed within `dcp` 1 for a draft,
    because nothing outside this repository implements `dcp` 1 and no existing object or binding
    uses either value; a deployed (non-draft) `dcp` 1 would have to bump the number or negotiate
    it. Validator cases: `version-durable-stream.json` and `version-blake3.json`, each with its
    `dcp` 0 case, and 11 more negative cases.
  - **Behaviour changes that are not wire changes:** the 8.1 reader rule judges a sender by the level
    at the event (question 15); a session on a binding with an atomic compare-and-set MAY use a
    settle interval of 0 (lease rule 9); the WOPI adapter MUST refuse `PutFile` on a document with
    more than one head (8.2 rule 6).
  - Unchanged and unmeasured: L3 presence, any `loro/1` profile, and interoperation of two
    independent implementations of any binding.
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

  Draft 0 objects (`"dcp": 0`) are rejected by the draft 1 schemas. At that revision, unchanged and
  unmeasured: the OCM and WebDAV outline (8.3), Durable Streams and iroh (8.4), L3 presence, and any
  `loro/1` profile.
- Draft 0 (2026-10-24): first text. Schemas and examples validate; no implementation.
