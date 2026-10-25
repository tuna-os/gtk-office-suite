# RFC 0004: An interoperable document-collaboration protocol (Matrix, OCM, WOPI and local-first bindings)

Date: 2026-10-24 · Status: **draft, not accepted** (Spikes A to F have each run once; see [Spike results](#spike-results)) · Related: [RFC-0001](0001-crdt-collaboration.md), [RFC-0003](0003-cloud-documents.md)

## Provenance

The owner (2026-10-24) asked three questions in sequence: could Matrix be the collaboration
backend for the suite, as NeoBoard does it; could that become a protocol other office suites
implement (Collabora Office was "just one example"); and what newer local-first technology, like
iroh, should be considered. This RFC writes down the research behind those answers so it can be
argued with before any code exists.

The research below was written before anything was built, and every claim in it is either cited to a
source in [References](#references) or listed under [Not verified](#not-verified). Where a source is a
vendor blog or a conference abstract, the claim is only as strong as that. Spikes A (Matrix), B
(Durable Streams), C (iroh and p2panda), D (WOPI), E (OCM and WebDAV) and F (default rate limits and
verification gaps) have since run once each; see [Spike results](#spike-results), and the claims
they checked have moved out of Not verified.

## Summary

1. **Matrix can carry identity, permissions, storage and versions** for documents, and it fills the
   gaps RFC-0001 deliberately left open (transport, identity, authorisation, revocation). It does
   not replace the CRDT: NeoBoard still needs Yjs on top of Matrix, and this suite already chose
   Loro.
2. **A single wire protocol for live editing across all suites is not realistic**, because suites
   collaborate in two incompatible ways (session-hosted and replicated, below). What *can* be
   standardised is everything around the editing: document identity, permissions, a version graph
   anchored on the real file, a lease, and an optional per-algorithm update profile.
3. **The sharing and access plane already has a standard** for file-sync-and-share hubs: Open Cloud
   Mesh (OCM), an IETF working group used by Nextcloud, ownCloud/OpenCloud and CERNBox. A new
   protocol must not duplicate it. It should define the collaboration semantics once and offer
   **bindings** to Matrix, OCM/WebDAV, WOPI, and local-first transports.
4. **Several relevant technologies matured in 2026** (iroh 1.0, p2panda's permissions and
   encryption crates, Keyhive's alpha, Durable Streams, a CRDT prototype in LibreOffice Writer).
   They change the choice of transport and of access control more than the choice of CRDT.

This RFC proposes the design shape and a set of measuring spikes. It does not propose shipping
anything.

## Constraints

Inherited unchanged from [RFC-0001](0001-crdt-collaboration.md#constraints-this-design-has-to-satisfy):
GTK-free core crates; **the file is the document of record**; atomic whole-buffer saves; per-user
undo; Flatpak as the ship vehicle; "done" means journeys; new dependencies are a release-gate
concern.

Added here:

- **Other suites must be able to implement the shared layer without changing their document
  model.** A design that requires LibreOffice, ONLYOFFICE or Nextcloud Text to adopt a new
  document schema fails this constraint.
- **Collaboration stays off by default** and requests network permission only when a user starts
  or joins a session (RFC-0001, Transport).
- **No unverifiable security claim ships.** In particular, a revocation or encryption property is
  claimed only after a journey proves it.

## Two ways suites collaborate

| Suite | Where the live document lives | How it is saved |
|---|---|---|
| Collabora Online, Nextcloud Office | One server process holds the document; clients get rendered tiles. Collabora says it "shares pixels", not the document, and stores nothing itself. | WOPI: the host stores the file; the editor loads and saves through the host. WOPI covers auth, metadata, load/save/lock and session management. |
| ONLYOFFICE Docs | Document Server, "Fast" and "Strict" co-editing modes | Callback URL to the host (read from its config docs, not its architecture) |
| Nextcloud Text | Tiptap/ProseMirror on Yjs; synced through the Nextcloud server, with websockets if the Files High-Performance Backend is installed | Saved to the file as Markdown |
| Nextcloud Whiteboard | Browser (Excalidraw), IndexedDB cache | A separate Node websocket server relays scene snapshots and updates, with a lossy "volatile" lane for cursors; Nextcloud and the server share a JWT secret |
| CryptPad | Client | The server stores and relays encrypted patches in a parent-linked chain (ChainPad) |
| NeoBoard | Client replica (Yjs) | Matrix room events |
| This suite (RFC-0001) | Client replica (Loro) | The file; replicas sync over a transport |

Two consequences follow.

- A LibreOffice session on a server cannot merge edits with a Loro replica in real time. Their
  document models and operations differ. That is a mapping problem, not a transport problem, and
  [RFC-0003](0003-cloud-documents.md#b-near-live-bridges-to-googles-editors) reached the same
  conclusion for Google's editors.
- Live cross-suite co-editing is therefore **only defined within a profile** (an agreed algorithm
  and document model). Everything else interoperates through versions.

## Prior art

### Matrix

- **NeoBoard** (Nordeck, Apache-2.0) is the working example. It is a Matrix widget and a standalone
  client sharing one board format. Its ADRs record why it is layered:
  - The Matrix event size limit is 64 KiB. It chunks snapshots into 34 KB blocks so that base64
    and encryption stay under the limit (ADR005).
  - Homeservers rate-limit timeline events to under 10 per second. NeoBoard budgets about 3 per
    second, so a 1 MB snapshot takes about 8 seconds to save (ADR005). ([Spike A](0004-spike-a-results.md)
    measured stock Synapse at 0.2 events per second per user, burst 10: 3 per second is a ceiling,
    not a rate.)
  - Synapse latency averages about 500 ms and each event carries about 350 bytes of JSON
    overhead, so cursors and presence go over a real-time channel and are never stored (MC'25 talk).
  - ADR005 says storing snapshots in the Matrix content repository is "preferable"; the Widget
    API could not reach it. A native client can.
  - One CRDT document per unit of collaboration, a single top-level map keyed by data-model
    version, and dry-run validation of remote updates before applying them (ADR004).
  - Its P2P WebRTC layer failed to scale: STUN works for about 75% of devices, TURN is costly to
    self-host, and Chrome caps peers at 500. It moved to MatrixRTC with LiveKit (MC'25 talk).
- **`matrix-org/collaborative-documents`** is an official draft by a Matrix core developer
  (last commit April 2022; stalled). It proposes documents as rooms (power levels as
  read/comment/edit, Spaces as folders), an `m.document` tree with a ProseMirror-style schema,
  algorithm-agnostic `m.document.change` events, and snapshots as state events. Its schema
  approach would force every suite onto a new document model and is the part this RFC drops.
- **Matrix-CRDT** is a Yjs provider that bridges documents to rooms (last push March 2024).
- **MatrixRTC** (MSC4143) and sticky events (MSC4354) are used by NeoBoard under unstable
  `org.matrix.msc*` prefixes. The talk warns about scalability of delayed events (MSC4140) and
  about apps overwriting each other's shared RTC state.
- **Rust SDK.** `matrix-rust-sdk` (Apache-2.0) has modules for media, encryption, send queue,
  room directory search and widgets. Whether they suffice for non-chat event types and encrypted
  attachments was checked by [Spike A](0004-spike-a-results.md); relations are
  [not verified](#not-verified).

### WOPI

Microsoft's REST protocol between a document host and an editor. Collabora publishes SDK examples
for hosts in Node.js, PHP, Python, React and .NET, uses lease-based locks with timestamp conflict
detection, and has a Mattermost plugin that makes Mattermost a WOPI host. A search found no Matrix
WOPI host; that is not proof none exists.

### Open Cloud Mesh (OCM)

An IETF working group protocol (`draft-ietf-ocm-open-cloud-mesh`; the repository is at v1.5.0 with
a reference stub and an interoperability test suite). It defines discovery, invites, share
creation and notification, WebDAV access, a **web-app access mode** (the receiving server embeds
the sender's web app through a token code flow), HTTP message signatures, an `embedded` JSON
resource type, and a draft using MLS (RFC 9420) for federated groups with optional file
encryption. It defines no versions, leases or live-update stream, as far as the repository
documents show.

### Nextcloud and Matrix today

Nextcloud Talk bridges to Matrix only through Matterbridge. The `integration_matrix` app sends
files from Nextcloud Files to an Element conversation. There is an open request that Talk be
built on Matrix. Nothing found makes Matrix a document backend for Nextcloud, whose record is the
file plus WebDAV.

### openDesk

openDesk combines Element (Matrix), Nextcloud and Collabora. Element's announcement (2026-10-13)
describes openDesk moving to Element Server Suite and the Matrix Authentication Service (OIDC).
This is a real deployment target for a Matrix-based document layer.

## Proposed shape

Split the protocol into **collaboration semantics** (transport-neutral) and **bindings** to the
sharing and sync plane.

### Collaboration semantics

| Level | Defines | Who can implement it |
|---|---|---|
| **L0 Document identity** | A document id, MIME type, title, and which sync profiles it supports. Mapping of roles (viewer, commenter, editor) to the binding's permission model. | Any suite |
| **L1 Versions and lease** | Immutable versions pointing at the actual file (ODF or OOXML) with a parent link, so history is a graph and divergent offline edits are branches. A lease with an expiry. | Any suite. A WOPI host adapter makes Collabora Online an L1 client with no Collabora change. |
| **L2 Update profile** | Deltas carrying an `algorithm` id and a base version. Only suites that share the algorithm and document model interoperate live. | Suites on Loro, Yjs or Automerge |
| **L3 Presence** | Cursors and selections, outside any durable log | Optional |

LibreOffice's tracked-changes compare and merge could join two divergent versions. That is an
[inference](#not-verified), not tested.

### Bindings

| Binding | Carries identity, permissions, storage | Fits |
|---|---|---|
| **Matrix** | A room per document; power levels; encrypted media for files; Spaces as folders; send queue offline | Local-first suites; Matrix-native deployments (NeoBoard, Element) |
| **OCM + WebDAV** | Shares, ETags, web-app code flow | Nextcloud, ownCloud/OpenCloud, CERNBox |
| **WOPI (host side)** | `CheckFileInfo`, `GetFile`, `PutFile`, lock endpoints mapped onto versions and lease | Collabora, Nextcloud Office, any WOPI client |
| **Durable Streams** | A URL-addressed append-only log with offset resume; clients encrypt | Self-hosted relay with no Matrix dependency |
| **iroh / p2panda** | Dial-by-key transport, append-only-log sync, group permissions and encryption | Peer-to-peer, no server |

A Matrix room could also carry an OCM share link for a document held in Nextcloud, and a Nextcloud
file could be mirrored as a version history that a Matrix client can read. These are bridges, not
a requirement.

### Matrix binding: decisions the evidence supports

- **One room per document.** Matrix power levels are per room, so per-file sharing needs
  per-file rooms, grouped with Spaces. (NeoBoard puts a board in a state event inside a shared
  room, which suits boards and not per-file sharing.)
- **Snapshots and files go in encrypted media, not events.** Events carry small deltas and
  version pointers. This keeps under the 64 KiB and rate limits NeoBoard measured.
- **Do not start with MatrixRTC.** Its events are unstable, and L3 is optional.
- **A strawman, not normative.** A room of type `org.example.document` with a metadata state event
  (title, MIME, profiles, current version), `...document.version` timeline events (media
  reference with encryption info and hash, parent version, author), a `...document.lease` state
  event (holder, expiry), and `...document.update` events (algorithm id, base version, payload
  inline up to about 30 KB or a media reference). Names and shapes were placeholders until a spike
  proved them; Spike A measured the corrected version of this sketch, which is now
  [spec section 8.1](../DOCUMENT-COLLABORATION-SPEC.md#81-matrix-binding-measured-once).

## Problems

1. **End-to-end encryption versus server-hosted editors.** A session-hosted editor needs
   plaintext. In an encrypted room it would have to join as a member that holds the keys, so its
   operator sees the document. The spec needs an explicit, visible trust model for this. This is
   the largest design cost.
2. **Two federation systems and two identity systems.** A Matrix ID and an OCM address would need
   a mapping. Not checked whether the formats collide.
3. **OCM carries notifications and access, not an update stream.** A replicated profile over OCM
   would need a separate channel (for example a websocket).
4. **Room history only grows.** Redaction removes content, not events. Plan snapshot-plus-delta
   compaction and a homeserver retention policy.
5. **Unstable Matrix pieces** (MatrixRTC, sticky events, delayed events) stay out of v0.
6. **E2EE user experience.** Device verification and key backup are real costs for office users.
7. **Flatpak still needs network permission.** Matrix removes mDNS and relay questions but not
   `--share=network`. GNOME Online Accounts has no Matrix provider, so sign-in would be in-app with
   tokens in the Secret Service.
8. **Dependency weight.** The Matrix SDK is large. It would sit behind the same `collab` cargo
   feature as Loro (RFC-0001 decision 1).
9. **Convergence is not correctness.** Ink & Switch's Livelymerge notes make this point for
   Automerge; RFC-0001's determinism test for computed values already addresses it for
   spreadsheets.

## Newer technology (2026)

| Technology | What it is | Status, from its own pages | Relevance |
|---|---|---|---|
| **iroh** | QUIC by public key, relay fallback | 1.0 on 2026-06-15 (RFC-0001; a third-party write-up gives the same date; `1.0.0-rc.0` post dated 2026-05-11) | Transport for the iroh binding |
| **p2panda** | Rust toolkit over iroh: net, sync (append-only logs), blobs, `p2panda-auth` (group permissions), `p2panda-encryption` (post-compromise security, optional forward secrecy), `p2panda-spaces` | Crate `p2panda` v0.7.1, so pre-1.0 | The identity, permissions and E2EE layer RFC-0001 deferred, in Rust |
| **Modal / Reflection** | A collective building a GObject interface for p2p primitives and a planned system service for network permission, node trust, multi-device sync and address-book identity; Reflection is a GTK text editor on Loro and p2panda | FOSDEM 2026 abstract; GNOME designer among the speakers | A candidate for the missing local-first portal RFC-0001 flagged |
| **Durable Streams** | Open HTTP protocol: URL-addressed append-only streams, offset resume (Electric, 2025-12) | Loro's team built a hosted backend on it and added client snapshots, an ephemeral channel and append-compare-and-set for E2EE | A server-light binding any HTTP host could implement |
| **Keyhive / ARK** | Access control and E2EE for Automerge and other hash-graph CRDTs | ARK has an API guide (Automerge August 2026 update) but is an alpha | Alternative to p2panda-auth |
| **Loro "CRDTs are not enough"** (2026-09-21) | Lists gaps beyond a CRDT library: multi-document directory (their "Flock"), consistent visibility across documents, local storage and search, hosted backend, authentication | Written from building a product on Loro | Gaps RFC-0001's phases do not yet cover |
| **Automerge 3.4.1, `automerge` 0.11, `samod` 0.13** | Rust crates; `samod` is a Rust automerge-repo wire-compatible with the JS one; new Hexane storage engine | Release notes, August 2026 | Mature Rust sync stack for comparison |
| **Yjs 14, `yrs`** | FOSDEM talk on versioning and track changes; LibreOffice Writer prototype of collaborative comments on `yrs` (NLnet-funded) | Prototype, read-only comments mode | A LibreOffice-based suite's CRDT path is Yjs, not Loro. Argues for the `algorithm` id. |
| **Teamtype** | Proof-of-concept "Collaborative Editing Protocol" for text editors, modelled on LSP | Authors say they want to form a group and standardise | Overlaps this RFC's goal from the editor side |
| **Willow, Meadowcap** | Capability-based sync protocols with Rust implementations | Only a FOSDEM listing and a repo move seen | Watch |
| **NextGraph, Jazz, Radicle, ElectricSQL** | Other sync engines and local-first systems | Titles only | Weaker fit for document editing |

## What would have to be proven

Matching this repository's definition of done. The last column is what Spikes A to F showed; "not
run" means no spike has tried it.

| Claim | Evidence | After Spikes A to F |
|---|---|---|
| Two instances converge over each binding | AT-SPI journey launching two app instances, per binding | Store-level journey 1 passed (two store handles, not two app instances) on Matrix, WOPI, Nextcloud (three users, one server and a two-server federation), Durable Streams (both servers) and iroh with p2panda (three nodes). No binding was run with two app instances |
| A removed member cannot read new updates | Journey: remove, edit, assert the removed device cannot decrypt (Matrix; p2panda-auth if chosen) | Matrix: passed, 0 of 70 trials with a control, on the condition that the sender's member view shows the removal. Durable Streams: a real second principal holding every key it ever had decrypted 0 of 2 records and 0 of 1 file published after its removal, and a control that reuses the key decrypted them. p2panda: later entries unreadable, **but content published by a member that had not applied the removal was readable by the removed member** (measured). Nextcloud: access control only (`404`), not encryption |
| Offline gap merges after reconnect | Journey: disconnect, edit both sides, reconnect | p2panda: two offline writers' versions were two heads at all three nodes 55 ms after reconnecting (release) and a merge left one head. Matrix: queued events were delivered in order after a 6 to 8 s outage; a second head is shown, not merged. Durable Streams and Nextcloud: the second-head journey passes (a fork is shown); no merge was timed |
| A version always points at a complete, valid file | Atomic-save property tests extended to version creation | Matrix, Durable Streams, Nextcloud and p2panda: verify-after-upload, and a truncated upload was never published (on Nextcloud a short body is refused `400` and the read-back catches the rest; on Durable Streams an upload cut on the wire left no stream). The property tests were not extended |
| A WOPI client round-trips a version | Collabora Online opens a version, edits, saves, and the new version appears with correct parent | **Passed once**, Collabora Online 26.04.5.2, over an in-memory store (Spike D) and over the Matrix binding (interop results, Spike F churn: one version per `PutFile`) |
| Lease expiry releases the document | Journey: kill the holder, assert another editor can take over after expiry | Passed at store level on Matrix, in memory, Durable Streams, iroh and p2panda (with clocks 60 s apart) and Nextcloud; **a 4 s lease failed across an OCM federation in 7 of 8 runs and passes at 20 s**. On the WOPI wire: host tests only |
| A merged document round-trips ODT/DOCX with a truthful loss report | Existing oracle corpora on post-merge documents | Not run |
| No new permission without user action; collaboration off by default | Manifest review plus a test asserting the default | Not run. Spike C: three nodes in one Flatpak build sandbox reach each other over loopback with and without `--share=network`; reaching another device needs `--share=network` in `finish-args`, which the repository's manifests grant only to the build. Relay spike: the permission cannot be limited to ports; outside Flatpak a client needed outbound TCP 443 to the relay and, to be direct, UDP to its peers, and nothing else (no DNS with an IP-literal relay URL; nothing contacted with iroh's `Minimal` preset, while the `N0` preset publishes the endpoint id and home relay to `dns.iroh.link`). Not run inside an installed Flatpak on two devices |
| Identity mapping between bindings is unambiguous | Test: a Matrix ID and an OCM address never resolve to the same principal unless explicitly linked | Not run |

## Phases and spikes

Each must be abandonable without leaving the codebase worse.

- **Spike A: Matrix.** Two Tables instances converge through a local Synapse. Loro deltas as small
  events, snapshots as encrypted media, an offline gap through the SDK send queue. Measure delta
  latency, events per minute, room growth over an hour, and removal of a member. *Ran, with store
  level journeys instead of two Tables instances and no Loro deltas; room growth was measured to
  1,000 events, not an hour (see [Spike results](#spike-results)).*
- **Spike B: Durable Streams.** The same journeys over a Durable Streams server with client-side
  encryption. *Ran, against two servers; see [Spike results](#spike-results).*
- **Spike C: iroh and p2panda.** The same journeys with `p2panda-auth` and `p2panda-encryption`.
  *Ran, with the p2panda pieces that work and iroh-blobs; see [Spike results](#spike-results).*
- **Spike D: WOPI host adapter.** A small Matrix-backed WOPI host. Collabora Online reads and writes
  versions with no Collabora change. This is the test of L1 for session-hosted editors. *Ran, over
  an in-memory store rather than Matrix (see [Spike results](#spike-results)).*
- **Spike E: OCM interoperability.** Read-only: discover and receive an OCM share from a Nextcloud
  instance and open it. Decides whether an OCM binding is practical. *Ran, and went further than
  read-only: a full WebDAV store over OCS sharing and an OCM federation, which rejected the
  draft-1 outline and replaced it (see [Spike results](#spike-results)).*
- **Spike F: verification gaps.** The interop suites on default rate limits, forged updates and
  demoted editors, the lease race 1 200 times, and Collabora's save churn against Matrix's limits.
  *Ran (see [Spike results](#spike-results)).*
- **Output.** A results file for each, like `0001-spike-results.md`, ending in go or no-go, and a
  recommendation on whether the three transports remain bindings or one wins. *See
  [Open questions](#open-questions): the evidence does not make one win.*

A draft-1 specification exists: [`DOCUMENT-COLLABORATION-SPEC.md`](../DOCUMENT-COLLABORATION-SPEC.md), with schemas and examples in [`docs/collab-spec/`](../collab-spec/). Each of its five bindings (Matrix, WOPI adapter, OCM with WebDAV, Durable Streams, iroh with p2panda) has been measured once and the spec corrected from the results ([Spike results](#spike-results)); L3 presence and any update algorithm profile have not. Nobody outside this repository should implement it before the namespace is registered and a second implementation has read the bytes of a binding.

## Spike results

Spikes A to F have each run **once**, in CI, on one build of each system, and say so. Three further
CI verification spikes (TLS, Q15 and relay, below) closed items the first six left open, each run on
its own branch and workflow. The results documents for the three new spikes live on the branches
`spike/collab-tls`, `spike/collab-q15` and `spike/collab-relay` until they are integrated.

| Spike | Measured against | Verdict | Results |
|---|---|---|---|
| **A: Matrix** | Synapse 1.163.2, matrix-sdk 0.19.1, loopback, one GitHub-hosted 2-core runner | **Go** for L0, L1 and the version-carrying half of L2, with nine corrections to the draft (now in the spec). **No-go as written** for the power-level table (a viewer's forged encrypted version was accepted until `m.room.encrypted` was set to 50) and for "3 events per second" as a default (stock Synapse allows 0.2 per user, burst 10); **thin margin** for the 34000-byte inline payload (largest working payload 36,109 bytes). Lease read-back alone is **partial** (both sessions believed they held it in 7 of 40 races) | [`0004-spike-a-results.md`](0004-spike-a-results.md) |
| **B: Durable Streams** | `@durable-streams/server` 0.3.9 (Node, file-backed) and the Go server (Caddy plugin) v0.3.0; client crates chacha20poly1305 0.10.1, ed25519-dalek 2.2.0, x25519-dalek 2.0.1, reqwest 0.12.28; loopback, plain `http`; CI run 40137430958, commit `0c99551` | **GO, with conditions, for L0, L1 and L2; NO-GO for "the server enforces roles"** (neither server has any access control, so a deployment must put one in front). All 11 journeys pass on both servers with nothing skipped, the lease is a real compare-and-set (`Stream-Seq`), and a removed principal holding every key it had read 0 of 2 records and 0 of 1 file published afterwards, with a control that fails. Conditions: every roster change is a new epoch and key and takes effect when the old epoch is sealed; time is a `Date`-header estimate; the lease stream can be poisoned; a new member reads all history; a 100-byte update is a 747-byte record. L3 not built | [`0004-spike-b-results.md`](0004-spike-b-results.md) |
| **C: iroh and p2panda** | iroh 1.4.1, iroh-blobs 0.103.1, p2panda-core, -store, -sync, -auth, -encryption, -spaces 0.7.1, rustc 1.99.1; three nodes on loopback; CI run 40139997921 | **Conditional go for L0 and the version-carrying half of L1, go with a size limit for L2; no-go for `commenter`, for exclusive leases, and for using p2panda-net, p2panda-blobs (an empty crate) or p2panda-spaces' role change (`unimplemented!()`) as published.** All 11 journeys pass with nothing skipped. Content published by a member that has not applied a removal is readable by the removed member. Leases are advisory (both partitioned sessions acquired; one holder 254 ms, debug, after healing). p2panda-spaces took 81 s to catch up on 1 000 entries. The crate is its own workspace (`libsqlite3-sys` `links` clash with `dcp-matrix`) | [`0004-spike-c-results.md`](0004-spike-c-results.md) |
| **D: WOPI host adapter** | Collabora Online 26.04.5.2 (`collabora/code:latest`, 2026-10-23), one run | **GO with conditions** for L1 (versions and lease) on session-hosted editors. **L2 and L3 are not offered over WOPI**, and WOPI cannot express two heads or a merge. Conditions: a per-token base-version record, conflict detection by Collabora's timestamp header (`409` with `COOLStatusCode` 1010), a fixed 30-minute lock | [`0004-spike-d-results.md`](0004-spike-d-results.md) |
| **E: OCM and WebDAV** | Nextcloud 34.0.5 and 35.0.2 (`nextcloud:*-apache`, by digest), PostgreSQL 16, one server and a two-server OCM federation, plain HTTP; CI run 40139702356, commit `cee909b9` | **The draft-1 outline is rejected as written** (an ETag is not a version: a replaced file kept its ETag in 16 of 20 trials; `If-Match` gave 0 or 1 winner; Nextcloud has no core lock provider and `LOCK` is authorised by its token alone). **A redesign is GO for L0 and L1 for `viewer` and `editor`**: a document is two folders, versions are create-only files with a sidecar, the lease is a create-if-absent log (0 double wins in 280 rounds). L2 and L3 not offered; `commenter` not expressible, and over a federation `owner` is not either; `created_by` is a claim (an editor named the owner). One server: 9 passed, 2 skipped; federation: 8 passed, 1 failed (4 s lease), 2 skipped | [`0004-spike-e-results.md`](0004-spike-e-results.md) |
| **F: verification gaps** | Synapse 1.163.2 (relaxed, default, default-drained) and Collabora Online 26.04.5.2, matrix-sdk 0.19.1, debug builds; CI run 40142775201, commit `513086d`, 14 jobs green | **No binding failure on default rate limits**: the interop suites pass unchanged, because a fresh user's burst of 10 covers a journey. Forged updates and a demoted editor are stopped by the server or dropped by every reader. The two-read lease rule let one session acquire in 1 200 of 1 200 rounds, and fails when a limiter delays a write by seconds. Collabora's churn did not reach the limit (6, 14 and 40 events in 187 s), but on a spent budget it abandons a call after 3003 ms. **Three findings**: `WopiHost` accepted a `PutFile` on a conflicted document (fixed), the Matrix reader rule applied the current power levels to history, and the two-read rule has no bound under delayed writes | [`0004-spike-f-results.md`](0004-spike-f-results.md) |
| **TLS: Durable Streams over TLS 1.3, HTTP/1.1 and HTTP/2** | The same two servers as Spike B (`@durable-streams/server` 0.3.9, Node v22; Go server v0.3.0), each behind Caddy v2.10.2 terminating TLS with a generated test CA; client: rustls 0.23.46 (`ring`), reqwest 0.12.28, hyper 1.12.0, h2 0.4.20; ubuntu-24.04, rustc 1.99.1, loopback. Correctness run [40182677787](https://github.com/tuna-os/gtk-office-suite/actions/runs/40182677787), commit `167aa64` (workflow `collab-spike-tls`): all four server and version jobs and the dependency job green. Measurements run [40178415302](https://github.com/tuna-os/gtk-office-suite/actions/runs/40178415302), commit `63e0bf9`: the four `tls-measure` jobs green (its four correctness jobs failed there on an openssl probe step, since fixed); run [40179818637](https://github.com/tuna-os/gtk-office-suite/actions/runs/40179818637), commit `8e4f228`, has the 32 passed, 0 failed count and the server access-log check | **GO for L0 to L2 over `https`, HTTP/1.1 and HTTP/2, on both servers, with conditions that are now spec 8.4 text.** The Spike B suite behaves the same over TLS; the client fails closed on a wrong CA, a wrong host name, an expired certificate, a server that offers no HTTP/2 when HTTP/2 is required, a plaintext URL and a redirect to `http`; `openssl s_client` agrees. TLS added nothing measurable to a warm connection (4.5 to 6.7 ms p50 Node, 2.6 to 2.8 ms Go); a handshake is 0.3 to 0.6 ms; 50 live readers used 1 HTTP/2 connection against 50 on HTTP/1.1. Findings: the Node server's `Location` names its plaintext origin; a proxy that is silent for 60 s costs a long poll 45 s and an HTTP/2 request 90 s without keep-alive PINGs, and 10 s with them; a server heartbeat longer than the proxy's idle timeout never completes a long poll for a reader that does not resume from its offset. One terminator (Caddy), one run, loopback | [`0004-spike-tls-results.md`](0004-spike-tls-results.md) |
| **Q15: the power level of a sender at an event (Matrix)** | Synapse 1.163.2 (`matrixdotorg/synapse:v1.163.2@sha256:c3c21919a322…`, and once `:latest`, also 1.163.2), SQLite, one homeserver, no federation; matrix-sdk 0.19.1; ubuntu-24.04, rustc 1.99.1, debug builds. Run A [40186028967](https://github.com/tuna-os/gtk-office-suite/actions/runs/40186028967), commit `92c7466`, and run B [40192035297](https://github.com/tuna-os/gtk-office-suite/actions/runs/40192035297), commit `fb2dfa4` (workflow `collab-spike-q15`): 7 of 7 jobs green in both | **Answered on Synapse.** A reader that judges by the power level at the event got every verdict right in all six scenarios (65 reader runs, 0 wrong) under four ways of getting the level (`/context` raw and through matrix-sdk, a linked forward walk, a backward walk through `unsigned.prev_content`) and their hybrid, which is now spec 8.1; draft 1's current-table reader got 24 verdicts wrong in 10 runs and a trusting reader 20 in 8. `/context` was not rate limited (0 of 620 requests per job) and cost 2.7 to 5.6 ms and 4 KB per event on loopback. A plain forward walk fails silently when a demotion is hidden by `history_visibility: joined` (scenario 6, found by the spike), hence the `replaces_state` link check and `/context` for what the walk cannot place. One server, one homeserver: no federation, no other homeserver, no 429 on `/context` | [`0004-spike-q15-results.md`](0004-spike-q15-results.md) |
| **Relay: dcp-p2p over a real `iroh-relay` on shaped links** | iroh 1.4.1, `iroh-relay` 1.4.1 (test CA), noq 1.4.0, iroh-blobs 0.103.1, p2panda 0.7.1; ubuntu-24.04 (kernel 6.17.0-1022-azure), 4 vCPU, Linux network namespaces with `tc netem` (50 and 150 ms RTT, 1 % and 5 % loss, 1 Mbit/s, and all three), relay-only (firewall drops all peer-to-peer packets) and direct modes. Run [40179059127](https://github.com/tuna-os/gtk-office-suite/actions/runs/40179059127), commit `53e2285` (workflow `collab-spike-relay`): all jobs green (`check`, `probe`, `build`, 14 `wan`, `footprint`); run [40192160497](https://github.com/tuna-os/gtk-office-suite/actions/runs/40192160497), commit `f45846a`: in-process relay path data and state-growth counters, green | **Relay and path behaviour measured; several 8.5 sentences corrected.** All 154 journey runs (11 journeys, 2 modes, 7 conditions) passed. Spike C's "relay-only" run had synced over a direct path (its latency comparison compared two direct connections); with peer-to-peer UDP dropped no IP path was ever opened (1,515 connections). An upgraded connection survived the relay being killed (7 of 7); a relay-only one recovered 3.5 to 22.6 s after the relay returned; a silently cut direct path took 15 to 21 s to fail over. **A lease write took 437 to 507 ms median at 150 ms RTT, so the 500 ms settle is too short on a WAN (now at least 2 s).** The reader of p2panda-spaces is CPU-bound without a rate limit and received 438 MB for a 2.1 MB state at 1,000 entries. A client needs TCP 443 to the relay and optionally UDP to peers; iroh's `Minimal` preset contacts nothing, the `N0` preset publishes the endpoint id and home relay. No NAT, no jitter, no real WAN, no n0 relay carrying dcp-p2p traffic | [`0004-spike-relay-results.md`](0004-spike-relay-results.md) |

What the spikes changed in the draft is listed in the [spec's changelog](../DOCUMENT-COLLABORATION-SPEC.md#changelog):
on-wire changes (`dcp` 1: a lease `renewal` counter; two additive locator types), the Matrix
power-level table, the rate and size rules, the time source, lease rules 3 and 9 to 13, the WOPI
call order, headers, conflict form and refusal on a conflicted document, and a replacement of the
OCM and WebDAV binding, plus two new bindings (8.4, 8.5). The three CI verification spikes added
the at-event reader rule (8.1), transport and live-read rules (8.4) and path, settle-interval and
port text (8.5), and changed no object. What the spikes do not show: L3
presence, any update algorithm profile, a network that is not loopback or emulated for any binding
but WebDAV's plain HTTP (Durable Streams was run over TLS on loopback only; iroh through a relay
in network namespaces only, with no NAT or jitter; Matrix, WOPI and OCM over TLS not at all), a
second implementation of any binding, interoperation with real Matrix clients, other WOPI
clients, other OCM servers or ownCloud, and Keyhive.

## Open questions

Each answer below is **evidence, not a decision**; the owner decides.

1. **Which binding first for the suite itself?** The spikes do not make one win. For live editing
   without running a homeserver, Durable Streams (8.4) passed all 11 journeys with nothing skipped
   on two different servers, with a lease that is a real compare-and-set (Matrix also passed all
   11, on one homeserver, with a read-back lease), and an update reached a second client's
   verified read in 3.9 ms p50 on loopback (Node server); its costs are a gate that
   must be put in front, one epoch per roster change, a principal that is a key with no
   multi-device or recovery, and cryptography that this project would own and that only its own
   Rust crate has ever read. Matrix (8.1) passed with a server that already has accounts, access
   control, media and encryption, but needs raised rate limits for inline updates (0.2 events per
   second by default), has no `commenter`, and a new device reads no history. The iroh and p2panda
   binding (8.5) is the weakest today: three of p2panda 0.7.1's pieces cannot be used as published,
   leases cannot be exclusive and need a settle interval of at least 2 s on a WAN, a cut direct path
   takes 15 to 20 s to fail over, and inline updates stop being usable past a few hundred entries.
   OCM with WebDAV (8.3) is L1 only. So the evidence supports Matrix or Durable Streams first and
   the peer-to-peer binding last.
2. **Is a Matrix account an acceptable requirement, or must a Matrix-free binding exist from the
   start?** **A Matrix account is not required by the protocol.** Durable Streams (8.4) and the
   peer-to-peer binding (8.5) each passed all 11 journeys with no Matrix server, and Nextcloud
   (8.3) needs only a Nextcloud account. A Matrix-free binding therefore exists and is measured;
   what it costs is stated in question 1 (a key is the principal, so the link statement of spec
   section 9.2 is needed before one person can use two devices).
3. **Should the permissions layer be p2panda-auth, Keyhive, or Matrix power levels, per binding?**
   There is now evidence for two of three. *Matrix power levels*: server-enforced and tested (a
   forged event was refused `403` once `m.room.encrypted` was gated at 50), with the gap that an
   encrypted room has one timeline gate for every DCP event type. *p2panda-auth*, through
   p2panda-spaces 0.7.1: three of four roles map, **a role change is unimplemented** (the binding
   removes and re-adds), any key holder can encrypt anything so **a reader-side rule is what makes
   a role mean "may publish"**, and revocation reaches only members that have applied it. *A roster
   the binding signs itself* (8.4) enforced roles at every reader and failed when the check was
   removed, at the price of one epoch per change. *Keyhive*: **no spike ran**; it stays an
   unevaluated alpha. Per binding, the measured answer is Matrix power levels for 8.1, a signed
   epoch chain for 8.4, and p2panda-auth plus a reader rule for 8.5.
4. **Who is the first external implementer: Collabora (via WOPI), Nextcloud (via OCM), or a
   Matrix-native client?** Collabora via WOPI is the only one with a measured round trip that needs
   no change on the editor's side (Collabora Online 26.04.5.2), with the conditions of spec 8.2.
   Nextcloud would have to adopt a two-folder layout that its desktop clients would show as two
   sync roots (an inference: not tried), has no encryption and no authenticated authorship, and its
   OCM sender differed from the OCM text in the ten places the spike lists. Whether anyone at those projects
   wants it remains unknown.
5. Should the version graph and lease be proposed to Matrix as MSCs, to OCM as an extension, or
   kept as an independent specification? The Matrix proposal process may require working
   implementations first; the exact requirement was not checked. The spikes add that the lease
   needs a compare-and-set, which Matrix state events lack and Durable Streams and a WebDAV
   create-if-absent log have.
6. How should a hosted editor in an encrypted room be shown to users? No spike built or tested how
   this is shown to users.
7. **New:** how many bindings should the specification keep? Each is feasible, and each adds a
   section that one implementation has read. Holding 8.5 back until p2panda-spaces can change a
   role and scale, or keeping 8.3 as L1 only, would shrink what has to be kept honest.
8. **Closed on one homeserver (spec question 15, [Spike Q15](#spike-results)): how does a Matrix
   reader know a sender's power level at the event?** From the room's own `m.room.power_levels`
   events in the `/messages` pass, trusted only while each names the previous one in
   `unsigned.replaces_state`, or from `GET /rooms/{id}/context/{eventId}?limit=0` for what that
   cannot place; an event whose level cannot be established is dropped, never judged by the
   current table. The measured cost on Synapse 1.163.2 is 0 extra requests in the usual case and
   2.7 to 5.6 ms and 4 KB per event otherwise, with no rate limit. Still open: other homeservers,
   a room that federates across two homeservers, a `429` on `/context`, and incremental `/sync`
   readers.
9. **New: which trust anchor does a Durable Streams client use?** The TLS spike ran with a
   configured CA and no built-in roots; using the platform's roots, and what a product does on
   certificate rotation, is a decision. Real load balancers and CDNs were not run (their idle
   rules were emulated), so the 30 s server heartbeat and the 15 s HTTP/2 PING in spec 8.4 are
   measured against an emulated 60 s proxy only.
10. **New: is the iroh binding acceptable with a 2 s settle and a 15 to 20 s path fail-over?** The
    relay spike measured both on routed namespaces with no NAT; hole-punch success behind real
    NATs and the behaviour on a real WAN are open, and so is whether a host-name relay URL, more
    than one relay or a relay behind an HTTP proxy works.

## Not verified

- Collabora's SDK architecture page was unreachable (anti-bot wall); its internals above rest on
  its blog posts and search snippets.
- ONLYOFFICE and CryptPad internals beyond their documentation snippets.
- Whether `matrix-rust-sdk` is sufficient for **relations**, and whether a Rust MatrixRTC or
  LiveKit path exists. (Checked by Spike A with matrix-sdk 0.19.1: custom state and timeline event
  types and encrypted attachments are sufficient, with its own send queue and cross-signing
  bootstrap; see [Spike results](#spike-results).)
- Homeserver limits for relations and retention, and any homeserver other than Synapse. (Checked by
  Spike A on Synapse 1.163.2: message rate, the 64 KiB event cap and media up to 32 MiB; Spike F
  added the interop suites on default and spent `rc_message` budgets; `rc_joins`, `rc_invites`,
  `rc_login` and `rc_registration` were not pushed.)
- What no spike ran, each results document ending with its own list: any network beyond loopback and
  namespaces with `tc netem` for the Durable Streams and iroh bindings, and any TLS for Matrix, WOPI
  and OCM; a TLS terminator other than Caddy 2.10.2, the platform trust store, mutual TLS,
  revocation checking and HTTP/3 for Durable Streams; a WAN, NAT traversal, jitter and production
  relays for iroh; a second implementation of any binding; Matrix federation, a homeserver other
  than Synapse 1.163.2 and real Matrix clients; a second WOPI client, WOPI proof keys and two real
  Collabora sessions; ownCloud and any OCM server but Nextcloud's sender; L3 presence; and a
  `loro/1` update profile.
- Whether OCM defines anything beyond shares and access that overlaps versions or leases. (Spike E
  found none that it needed; it did not search the OCM text for one.)
- Whether OCM addresses and Matrix IDs could collide.
- Whether Nextcloud editors work with Nextcloud's E2EE folders, and Nextcloud Office or Collabora
  editing a DCP document (Spike E installed the app and configured no Collabora server).
- `p2panda-auth`'s security model and maturity beyond what Spike C used through p2panda-spaces 0.7.1
  (a group CRDT with four levels; read, not audited); how it compares with Keyhive, which no spike ran.
- Willow's status; the status of `spreed#13212`.
- LibreOffice tracked-changes merge as a way to join divergent versions (inference only).
- Whether anyone at Collabora, Element, Nordeck, Nextcloud or openDesk wants this.

## References

Matrix and NeoBoard
- NeoBoard ADRs: https://github.com/nordeck/matrix-neoboard/tree/main/docs/adrs (002, 004, 005, 007)
- NeoBoard MatrixRTC event model: https://github.com/nordeck/matrix-neoboard/blob/main/docs/model/matrix-rtc-events.md
- Scaling Realtime in NeoBoard (MC'25): https://2025.matrix.org/slides/slides_TM3ASG.pdf
- NeoToolSuite: https://nordeck.net/en/neotoolsuite_en/
- Matrix collaborative documents draft: https://github.com/matrix-org/collaborative-documents
- Matrix-CRDT: https://github.com/YousefED/Matrix-CRDT
- matrix-rust-sdk: https://github.com/matrix-org/matrix-rust-sdk
- openDesk and Element: https://element.io/blog/opendesk-now-runs-element-server-suite-direct-from-upstream

Office suites and federation
- Collabora on WOPI: https://www.collaboraonline.com/blog/wopi-is-open-your-office-stack-should-be-too/
- Collabora SDK: https://sdk.collaboraonline.com/docs/How_to_integrate.html
- Collabora Mattermost plugin: https://pkg.go.dev/github.com/CollaboraOnline/collabora-mattermost
- Nextcloud WebSockets and apps: https://nextcloud.com/blog/websockets-in-nextcloud
- Nextcloud Text: https://github.com/nextcloud/text · Whiteboard: https://github.com/nextcloud/whiteboard
- Nextcloud and Matrix: https://github.com/nextcloud/integration_matrix · https://github.com/nextcloud/spreed/issues/13212
- Open Cloud Mesh: https://github.com/cs3org/OCM-API
- CryptPad architecture: https://github.com/cryptpad/cryptpad/blob/main/docs/ARCHITECTURE.md
- ONLYOFFICE co-editing: https://api.onlyoffice.com/docs/docs-api/get-started/how-it-works/co-editing

Newer technology
- iroh 1.0 release candidate: https://www.iroh.computer/blog/iroh-1-0-0-rc-0
- p2panda: https://p2panda.org/
- Modal, Reflection and p2panda (FOSDEM 2026): https://fosdem.org/2026/schedule/event/MCVBNK-p2panda-modal-reflection/
- Writer comments on CRDTs (FOSDEM 2026): https://fosdem.org/2026/schedule/event/B73VPR-writer-comments-crdts-prototype/
- Teamtype CEP (FOSDEM 2026): https://fosdem.org/2026/schedule/event/CQHN8T-teamtype-towards-a-collaborative-editing-protocol/
- FOSDEM 2026 local-first track: https://fosdem.org/2026/schedule/track/local-first
- Durable Streams: https://durablestreams.com/concepts
- Loro, CRDTs are not enough: https://loro.dev/blog/crdt-is-not-enough
- Automerge, August 2026: https://automerge.org/blog/2026-august/
- Keyhive: https://www.inkandswitch.com/project/keyhive

Spike results (B, C, E and F are on the branches `spike/collab-b`, `-c`, `-e` and `-f` until integrated)
- Spike A, Matrix: [`0004-spike-a-results.md`](0004-spike-a-results.md)
- Spike B, Durable Streams: [`0004-spike-b-results.md`](0004-spike-b-results.md)
- Spike C, iroh and p2panda: [`0004-spike-c-results.md`](0004-spike-c-results.md)
- Spike D, WOPI: [`0004-spike-d-results.md`](0004-spike-d-results.md)
- Spike E, OCM and WebDAV: [`0004-spike-e-results.md`](0004-spike-e-results.md)
- Spike F, verification gaps: [`0004-spike-f-results.md`](0004-spike-f-results.md)
- Matrix and WOPI against each other: [`0004-spike-interop-results.md`](0004-spike-interop-results.md)

Sources the new spikes read
- Durable Streams protocol: https://github.com/durable-streams/durable-streams/blob/main/PROTOCOL.md
- iroh-blobs push fix (0.103.0 yanked): https://www.iroh.computer/blog/iroh-blobs-push-fix
- Open Cloud Mesh text and spec: https://github.com/cs3org/OCM-API (`IETF-OCM.md`, `spec.yaml`)
