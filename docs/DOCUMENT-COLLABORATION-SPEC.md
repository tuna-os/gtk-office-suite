# Document Collaboration Protocol (DCP), draft 0

**Status**: specification draft, **unvalidated, not accepted** · **Rationale**: [RFC-0004](rfc/0004-interoperable-document-collaboration.md) · **Machine-readable parts**: [`collab-spec/`](collab-spec/)

> **Read this first.** Draft 0 turns the design shape in RFC-0004 into testable text. No part of it
> has been implemented or measured. The Matrix event names, the size limits and the lease rules are
> a **strawman** taken from NeoBoard's published measurements and from WOPI. They stand or fall with
> RFC-0004's [Spike A](rfc/0004-interoperable-document-collaboration.md#phases-and-spikes) (Matrix)
> and Spike D (WOPI host adapter). The namespace `org.tunaos.dcp` is a placeholder and MUST be
> replaced by a registered one before any external implementer relies on it. Do not ship
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

An implementation MUST state which binding it implements. Conformance is per binding.

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

All objects carry `"dcp": 0`. Schemas are in [`collab-spec/schemas/`](collab-spec/schemas/) and
examples in [`collab-spec/examples/`](collab-spec/examples/). `python3 docs/collab-spec/validate.py`
checks the examples against the schemas and checks that malformed variants are rejected.
Receivers MUST ignore unknown fields only inside a binding's own envelope; DCP objects use
`additionalProperties: false`, so an object with an unknown field MUST be rejected.

### 4.1 Document metadata

Schema: [`meta.schema.json`](collab-spec/schemas/meta.schema.json). Example:
[`meta.json`](collab-spec/examples/meta.json).

| Field | Meaning |
|---|---|
| `title` | Display name, 1 to 255 characters |
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
| `created_by` | `principal` and `device` |
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
| `base_version` | The version the holder started from. Required when held. |
| `duration_ms` | How long the lease lasts from the time the binding stamped it, from 1 second to 1 hour. Required when held. |

#### Lease rules

1. The binding supplies the time. Implementations MUST use the binding's own timestamp on the lease
   record and MUST NOT compare it with the local clock without allowing 30 seconds of skew.
2. A lease is **active** while `state` is `held` and the binding timestamp plus `duration_ms` is
   later than the current binding time.
3. A session MAY acquire a lease only if none is active, or if it is the holder. After writing, it
   MUST read the lease back and treat itself as holder only if the record names its session. A
   binding without atomic compare-and-set will let two sessions write at once; the read-back is
   what resolves it.
4. The holder MUST renew before half of `duration_ms` has elapsed while it keeps editing, and MUST
   release the lease when it closes the document normally. A crash is handled by expiry.
5. Another session MUST NOT save while a lease it does not hold is active, except that a principal
   with the `owner` role MAY break a lease. Breaking a lease MUST be visible to the former holder
   and recorded as a lease whose `state` is `held` by the breaker.
6. A save by a session without the active lease is still accepted as a **version** (rule 5 of the
   version rules makes it a branch if it forks). The lease prevents the fork in the normal case; it
   does not forbid it.

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
5. **Size.** A payload over 34000 bytes MUST use `encoding: "file"`.
6. **Determinism.** Two programs that apply the same updates in different orders MUST compute
   byte-identical stored values. This follows RFC-0001.

An algorithm profile for a given id (for example `loro/1`) is a separate document that fixes
encoding, document model mapping and the determinism test. None exists yet.

## 8. Bindings

### 8.1 Matrix binding (strawman)

Informed by NeoBoard's architecture decision records, ADR002 and ADR005, and by the 2022 Matrix
collaborative-documents draft. All values below are to be measured in Spike A.

**Rooms.** One document is one room. The room's `m.room.create` content has
`"type": "org.tunaos.dcp.document"`. Spaces (`m.space.child`) provide folders. The room name is the
title.

**Encryption.** The room MUST be end-to-end encrypted unless it is public by the owner's choice.
`history_visibility` SHOULD be `shared` so a new member can read the versions they are invited to.
(Element's own default for new rooms no longer exposes history to later joiners, so this setting
needs an explicit choice.)

**Events.**

| Event type | Kind | `state_key` | Carries |
|---|---|---|---|
| `org.tunaos.dcp.meta` | state | `""` | Document metadata (4.1) |
| `org.tunaos.dcp.lease` | state | `""` | Lease (4.3) |
| `org.tunaos.dcp.version` | timeline | | Version (4.2). The event ID is the version ID. |
| `org.tunaos.dcp.update` | timeline | | Update (4.4) |

The event `content` is the DCP object. A client MUST ignore an event whose content does not
validate against the schema.

**Roles to power levels.** The owner sets these in `m.room.power_levels`:

| Setting | Level |
|---|---|
| `users_default` | 0 (viewer) |
| `events["org.tunaos.dcp.version"]` | 50 |
| `events["org.tunaos.dcp.update"]` | 50 |
| `events["org.tunaos.dcp.lease"]` | 50 |
| `events["org.tunaos.dcp.meta"]` | 100 |
| users with role `editor` | 50 |
| users with role `owner` | 100 |

`commenter` is reserved until the comment event is defined.

**Files.** A version's file is an encrypted attachment uploaded to the content repository, with
`locator.type` `matrix-encrypted-media` in an encrypted room. The plaintext `sha256` is recorded in
the version.

**Ordering and rate.** A client MUST queue DCP events per room and send them in the order created.
It SHOULD NOT send more than 3 timeline events per second to one room (NeoBoard's measured budget),
and it MUST back off when the homeserver rate-limits it. For one `(user, device)` stream the room
timeline gives causal order. Events from different streams may interleave arbitrarily.

**Size.** An inline update payload MUST NOT exceed 34000 bytes before encoding. NeoBoard splits at
34 KB so that base64 and encryption stay under Matrix's 64 KiB event limit. Spike A should confirm
that margin, or lower it.

**Lease.** The lease is a state event, and Matrix state events have no compare-and-set. The
read-back rule (lease rule 3) applies.

**Time.** The time source is the event's `origin_server_ts`.

**Editor services.** An editor service that needs plaintext MUST join the room as a visible member
listed in `editor_services`. A client MUST show the list of editor services to every member and
MUST show when one holds the lease. The room's keys are available to that member.

### 8.2 WOPI host adapter (strawman)

A program implementing this adapter acts as a WOPI **host** for any WOPI client (for example
Collabora Online) and stores documents through another binding. It makes a session-hosted editor an
L1 participant without changing the editor.

| WOPI operation | DCP mapping |
|---|---|
| `CheckFileInfo` | `BaseFileName` from the title, `Size` and `Version` from the head version, `UserCanWrite` true only for `editor` or `owner` |
| `GetFile` | Fetch and verify the head version's file |
| `PutFile` | Verify the lock, then publish a version whose parent is the version the editor opened |
| `Lock`, `GetLock`, `RefreshLock`, `Unlock`, `UnlockAndRelock` | Acquire, read, renew, release the lease; `token` holds the WOPI lock string |

On a lock mismatch the adapter MUST return the current lock string in the way the WOPI
specification requires. Collabora describes timestamp-based conflict detection and auto-expiring
lease locks; the exact headers and status codes are to be confirmed in Spike D.

### 8.3 OCM and WebDAV binding (outline)

Informative until Spike E. Open Cloud Mesh already defines discovery, invites, share creation and
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
bindings after RFC-0004's Spikes B and C.

## 9. Identity

1. A principal is whatever the binding says it is.
2. Principals from different bindings MUST NOT be treated as the same person unless both have
   signed a link statement. The format of that statement is not defined in draft 0.
3. A program MUST show the principal behind every version, lease and update it displays.

## 10. Failure behaviour

| Situation | Required behaviour |
|---|---|
| Network lost while editing | Keep editing locally. On reconnect, publish a version based on the last known head. If the head moved, this creates a second head. |
| Lease expired while editing | Continue locally. Before saving, try to reacquire. If another session holds it, save as a version anyway (lease rule 6) and show the resulting conflict. |
| File hash mismatch | Discard the version and report which version. |
| Update fails validation | Drop it, report it, keep the last valid state. |
| Update stream gap | Stop applying that stream and reload from the latest version. |
| Unknown algorithm | Use versions only. |
| Binding rate limit | Back off and queue. Never drop a version. |

## 11. Security considerations

- **A lease is not a permission.** Any member with the editor role can publish a version whether or
  not it holds the lease. Role mapping is what enforces who may write.
- **Revocation has limits.** Removing a member stops them from receiving new content in an
  encrypted binding. Content they already received stays readable to them. DCP does not claim
  otherwise, and an implementation MUST NOT either, unless a journey proves a stronger property.
- **Editor services see plaintext.** Joining one to a document gives its operator access to that
  document. This MUST be shown to users (section 8.1).
- **Untrusted files.** A version's file comes from another principal. Programs MUST open it with the
  same hardening as any untrusted download: bound decompression, reject malformed packages and
  limit sizes. DCP does not make a file trustworthy because a member published it.
- **Untrusted updates.** Section 7 rule 3.
- **Hashes.** `sha256` detects corruption and mix-ups. It does not authenticate the author;
  authorship comes from the binding's signatures (for Matrix, the event signature).

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
| 10 | A WOPI client round-trips (8.2) | Collabora Online opens, edits and saves; the new version has the correct parent |
| 11 | Schemas hold | `python3 docs/collab-spec/validate.py` passes |

Each test is entered in `conformance/capabilities.json` only with the revision on which it passed.
This draft adds none.

## 13. Open questions

1. The registered namespace to replace `org.tunaos.dcp`.
2. The `commenter` role: its event, and its power level.
3. Whether the Matrix lease should be a state event at all, given the missing compare-and-set; a
   timeline event with a deterministic winner rule may fit better.
4. Whether unstable MatrixRTC and sticky events should ever carry L3.
5. The link statement format (section 9.2).
6. Whether OCM and WebDAV can express leases and heads, or only versions.
7. Whether the 3 events per second and 34000 byte limits survive measurement.
8. A first algorithm profile for `loro/1`, and whether a `yjs/13` profile is needed for
   interoperability with LibreOffice's Yjs-based prototype.
9. How a hosted editor in an encrypted room is shown to users.

## Changelog

- Draft 0 (2026-10-24): first text. Schemas and examples validate; no implementation.
