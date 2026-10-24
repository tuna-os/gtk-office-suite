# RFC 0004: An interoperable document-collaboration protocol (Matrix, OCM, WOPI and local-first bindings)

Date: 2026-10-24 · Status: **draft, not accepted** (Spikes A and D have run; B, C and E have not) · Related: [RFC-0001](0001-crdt-collaboration.md), [RFC-0003](0003-cloud-documents.md)

## Provenance

The owner (2026-10-24) asked three questions in sequence: could Matrix be the collaboration
backend for the suite, as NeoBoard does it; could that become a protocol other office suites
implement (Collabora Office was "just one example"); and what newer local-first technology, like
iroh, should be considered. This RFC writes down the research behind those answers so it can be
argued with before any code exists.

The research below was written before anything was built, and every claim in it is either cited to a
source in [References](#references) or listed under [Not verified](#not-verified). Where a source is a
vendor blog or a conference abstract, the claim is only as strong as that. Spikes A (Matrix) and D
(WOPI) have since run once each; see [Spike results](#spike-results), and the claims they checked
have moved out of Not verified.

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

Matching this repository's definition of done. The last column is what Spikes A and D showed; "not
run" means no spike has tried it.

| Claim | Evidence | After Spikes A and D |
|---|---|---|
| Two instances converge over each binding | AT-SPI journey launching two app instances, per binding | Matrix and WOPI: store-level journey 1 passed (two store handles, not two app instances). Other bindings: not run |
| A removed member cannot read new updates | Journey: remove, edit, assert the removed device cannot decrypt (Matrix; p2panda-auth if chosen) | Matrix: passed, 0 of 70 trials with a control, on the condition that the sender's member view shows the removal. p2panda-auth: not run |
| Offline gap merges after reconnect | Journey: disconnect, edit both sides, reconnect | Not run as a merge. Matrix: queued events were delivered in order after a 6 to 8 s outage; a second head is shown, not merged |
| A version always points at a complete, valid file | Atomic-save property tests extended to version creation | Matrix: verify-after-upload, and a truncated upload was never published. The property tests were not extended |
| A WOPI client round-trips a version | Collabora Online opens a version, edits, saves, and the new version appears with correct parent | **Passed once**, Collabora Online 26.04.5.2, over an in-memory store |
| Lease expiry releases the document | Journey: kill the holder, assert another editor can take over after expiry | Passed at store level on Matrix and in memory (journey 5), after a fix: a renewal had to change the record. On the WOPI wire: host tests only |
| A merged document round-trips ODT/DOCX with a truthful loss report | Existing oracle corpora on post-merge documents | Not run |
| No new permission without user action; collaboration off by default | Manifest review plus a test asserting the default | Not run |
| Identity mapping between bindings is unambiguous | Test: a Matrix ID and an OCM address never resolve to the same principal unless explicitly linked | Not run |

## Phases and spikes

Each must be abandonable without leaving the codebase worse.

- **Spike A: Matrix.** Two Tables instances converge through a local Synapse. Loro deltas as small
  events, snapshots as encrypted media, an offline gap through the SDK send queue. Measure delta
  latency, events per minute, room growth over an hour, and removal of a member. *Ran, with store
  level journeys instead of two Tables instances and no Loro deltas; room growth was measured to
  1,000 events, not an hour (see [Spike results](#spike-results)).*
- **Spike B: Durable Streams.** The same journeys over a Durable Streams server with client-side
  encryption.
- **Spike C: iroh and p2panda.** The same journeys with `p2panda-auth` and `p2panda-encryption`.
- **Spike D: WOPI host adapter.** A small Matrix-backed WOPI host. Collabora Online reads and writes
  versions with no Collabora change. This is the test of L1 for session-hosted editors. *Ran, over
  an in-memory store rather than Matrix (see [Spike results](#spike-results)).*
- **Spike E: OCM interoperability.** Read-only: discover and receive an OCM share from a Nextcloud
  instance and open it. Decides whether an OCM binding is practical.
- **Output.** A results file for each, like `0001-spike-results.md`, ending in go or no-go, and a
  recommendation on whether the three transports remain bindings or one wins.

A draft-1 specification exists: [`DOCUMENT-COLLABORATION-SPEC.md`](../DOCUMENT-COLLABORATION-SPEC.md), with schemas and examples in [`docs/collab-spec/`](../collab-spec/). Its Matrix binding and WOPI adapter have each been measured once and corrected from the results ([Spike results](#spike-results)); its OCM, Durable Streams and iroh bindings have not been measured. Nobody outside this repository should implement it before the remaining spikes report and the namespace is registered.

## Spike results

Spikes A and D have run; B, C and E have not. Each ran **once**, in CI, on one build, and says so.

| Spike | Measured against | Verdict | Results |
|---|---|---|---|
| **A: Matrix** | Synapse 1.163.2, matrix-sdk 0.19.1, loopback, one GitHub-hosted 2-core runner | **Go** for L0, L1 and the version-carrying half of L2, with nine corrections to the draft (now in the spec). **No-go as written** for the power-level table (a viewer's forged encrypted version was accepted until `m.room.encrypted` was set to 50) and for "3 events per second" as a default (stock Synapse allows 0.2 per user, burst 10); **thin margin** for the 34000-byte inline payload (largest working payload 36,109 bytes). Lease read-back alone is **partial** (both sessions believed they held it in 7 of 40 races) | [`0004-spike-a-results.md`](0004-spike-a-results.md) |
| **D: WOPI host adapter** | Collabora Online 26.04.5.2 (`collabora/code:latest`, 2026-10-23), one run | **GO with conditions** for L1 (versions and lease) on session-hosted editors. **L2 and L3 are not offered over WOPI**, and WOPI cannot express two heads or a merge. Conditions: a per-token base-version record, conflict detection by Collabora's timestamp header (`409` with `COOLStatusCode` 1010), a fixed 30-minute lock | [`0004-spike-d-results.md`](0004-spike-d-results.md) |

What the spikes changed in the draft is listed in the [spec's changelog](../DOCUMENT-COLLABORATION-SPEC.md#changelog):
an on-wire change (`dcp` 1: a lease `renewal` counter), the Matrix power-level table, the rate and
size rules, the time source, lease read-back, and the WOPI call order, headers and conflict form.
What neither spike shows: the OCM, Durable Streams and iroh bindings, L3 presence, any update
algorithm profile, interoperation with real Matrix clients or other WOPI clients, and the WOPI
adapter running over the Matrix binding (Collabora's save frequency against Matrix's limits).

## Open questions

1. Which binding does the owner want first for the suite itself: Matrix, or the lowest-dependency
   one the spikes find?
2. Is a Matrix account an acceptable requirement for users, or must a Matrix-free binding exist
   from the start?
3. Should the permissions layer be p2panda-auth, Keyhive, or Matrix power levels, per binding?
4. Who is the first external implementer: Collabora (via WOPI), Nextcloud (via OCM), or a
   Matrix-native client?
5. Should the version graph and lease be proposed to Matrix as MSCs, to OCM as an extension, or
   kept as an independent specification? The Matrix proposal process may require working
   implementations first; the exact requirement was not checked.
6. How should a hosted editor in an encrypted room be shown to users?

## Not verified

- Collabora's SDK architecture page was unreachable (anti-bot wall); its internals above rest on
  its blog posts and search snippets.
- ONLYOFFICE and CryptPad internals beyond their documentation snippets.
- Whether `matrix-rust-sdk` is sufficient for **relations**, and whether a Rust MatrixRTC or
  LiveKit path exists. (Checked by Spike A with matrix-sdk 0.19.1: custom state and timeline event
  types and encrypted attachments are sufficient, with its own send queue and cross-signing
  bootstrap; see [Spike results](#spike-results).)
- Homeserver limits for relations and retention, and any homeserver other than Synapse. (Checked by
  Spike A on Synapse 1.163.2: message rate, the 64 KiB event cap and media up to 32 MiB; `rc_joins`,
  `rc_invites` and `rc_login` were not pushed.)
- What neither spike ran: each results document ends with its own list, including federation, real
  Matrix clients, a second WOPI client, HTTPS and WOPI proof keys, and the WOPI adapter over the
  Matrix binding.
- Whether OCM defines anything beyond shares and access that overlaps versions or leases.
- Whether OCM addresses and Matrix IDs could collide.
- Whether Nextcloud editors work with Nextcloud's E2EE folders.
- `p2panda-auth`'s security model and maturity; how it compares with Keyhive.
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
