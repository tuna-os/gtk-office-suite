# RFC-0007: p2panda as the suite's main way to chat, in Mandelbrot and Lifafa

Date: 2026-11-26 · Status: **draft, not accepted** · Origin: issue #2499 ("use p2panda for
collaboration as GNOME is going to use it in the future") and the owner's follow-up: p2panda
"would be the main way to chat", so plan how it reaches the chat and email clients.
Related: [RFC-0001](0001-crdt-collaboration.md) (no local-first portal yet),
[RFC-0004](0004-interoperable-document-collaboration.md) (Spike C: iroh and p2panda 0.7.1),
[RFC-0005](0005-collaboration-in-the-apps.md) (sidecar architecture, p2panda set aside for
documents), [RFC-0006](0006-suite-desktop-integration.md), ADR 0013 (one Matrix account),
ADR 0014 (shared services are libraries; an optional sidecar for shared state), ADR 0016.

This RFC changes no product code. It says what "p2panda as the main way to chat" would mean
for Mandelbrot (today a Matrix client) and Lifafa (an email client), where it plugs in, what
p2panda can and cannot carry today, and an order of work in which every step is abandonable.

## Summary

| Question | Proposal |
|---|---|
| Where does p2panda run? | In one **sidecar** per user, `tuna-p2pd`: the p2panda node (iroh endpoint, store, spaces), the identity keys (in `oo7` with the Secret portal) and the network. Mandelbrot and Lifafa talk to it over a JSON-lines protocol on a Unix socket. GTK never links iroh, tokio-for-p2p or p2panda's sqlite (which clashes with matrix-sdk's). When Modal's p2panda system service and portal ship, the sidecar becomes a client of it ([section 3](#3-the-sidecar-tuna-p2pd)). |
| What is a conversation? | A **p2panda space**: a group with roles (p2panda-auth) and group encryption. A 1:1 chat is a two-member space. Messages, edits, reactions and receipts are operations in members' logs; typing and presence are gossip only ([section 4](#4-data-model)). |
| Who is a contact? | One person's p2panda identity (Ed25519), exchanged by invite code, QR or link, stored in the suite address book (EDS) as an `IMPP` entry, so Lifafa, Mandelbrot and the document apps all see it ([section 5](#5-identity-and-contacts)). |
| Mandelbrot | p2p conversations appear **beside** Matrix rooms in the same sidebar and room view. That needs a seam Mandelbrot does not have: today about 60k of its 102k lines touch matrix-sdk types directly. The seam goes in the model layer (`Room`, `Timeline`, `Event` backed by either a Matrix or a p2p source), in steps, behind a `p2p` feature ([section 6](#6-mandelbrot)). |
| Lifafa | Email stays email. p2panda enters in two ways: **(a) email introduces people to p2p**: an invite travels in a mail, and a contact's p2p identity is learnt from it; **(b) later, a `Protocol::P2p` account**: mail-shaped messages (subject, thread, attachments) between suite users over p2panda, through Lifafa's existing out-of-process helper pattern, with SMTP as the fallback for everyone else ([section 7](#7-lifafa)). |
| Matrix | Stays. It is how the suite reaches everyone who is not a p2panda user, and it carries calls. "Main way to chat" means **the default for new conversations between suite users**, not a migration of existing rooms. No bridge exists between p2panda and Matrix, and none is proposed first ([section 8](#8-matrix-calls-and-what-stays)). |
| The hard gap | **Offline delivery.** p2panda delivers only between peers that are online at the same time; store-and-forward ("support nodes", Dash Router) is unreleased. Chat needs it. The plan runs `tuna-p2pd` headless on an always-on machine the user or their organisation controls, as a member of their spaces ([section 9](#9-offline-delivery)). |
| Order | Seven phases, the first three bounded and shipping nothing to users: re-measure 0.8.2 (running), a two-node CLI chat on the sidecar, invites over email, then the Mandelbrot seam, then p2p conversations in Mandelbrot, then the support node, then the Lifafa p2p account ([section 11](#11-phases)). |

## 1. Why now, and what the owner asked

- The owner's direction (issue #2499 and its follow-up): p2panda, because GNOME is heading there,
  and as the main way to chat.
- GNOME's direction, as published: Reflection, a GTK editor on p2panda, aims to make "local-first
  sync a standard feature of the GNOME developer platform" (modal.cx/reflection; Flathub Beta
  only). An NLnet-funded **p2panda system service** (started 2025-11) is to sync in the background
  and expose a sync API through an **XDG Desktop Portal**; its FOSDEM 2026 talk describes it as
  managing network permission, node trust, multi-device and identity through the address book,
  and as framework-agnostic (nlnet.nl/project/p2panda-systemservice;
  archive.fosdem.org/2026/schedule/event/MCVBNK-p2panda-modal-reflection). No release date is
  published.
- p2panda 0.8.2 was released 2026-11-25. Its README still says the APIs are not stable for
  production use before 1.0, which has no date. 0.8.1 changed the wire format (ciphertext and key
  bundle moved into the operation body), which Reflection's own update says broke its old data.

Building the suite's messaging on the same stack means adopting that service and portal when they
exist, instead of maintaining a parallel one. It also means carrying a pre-1.0 dependency whose
format has just changed, so everything below versions its data and keeps p2p behind a feature.

## 2. What exists today

**Mandelbrot** (own cargo workspace; global multi-thread tokio runtime, `src/main.rs:45-47`):

- No backend seam. `Session` holds a `matrix_sdk::Client` (`session/mod.rs:141-143`), `Room` a
  `MatrixRoom` (`session/room/mod.rs:337`), `Timeline` a `matrix_sdk_ui` timeline
  (`session/room/timeline/mod.rs:309`), `Event` an `EventTimelineItem`
  (`session/room/timeline/event/mod.rs:92`). UI code reaches through them: 56 calls of
  `.matrix_room()`, `.matrix_timeline()` or `.client()` in `session_view`, `components` and
  `account_settings`; the composer sends through `matrix_room.send_queue()`
  (`session_view/room_history/message_toolbar/mod.rs:812-820`, attachments `:1041-1060`).
- Files touching `matrix_sdk` or `ruma` total about 60k of 102k lines.
- The one real seam is the calls crate `matrixrtc` (`RtcClientApi`, `KeyTransport`; LiveKit behind
  `calls-media`).
- Cross-app surface: `GApplication` actions `open-room`, `start-call`, `share-screen`, `share-file`
  and the `matrix-account` hint (`src/application.rs`); hand-off targets must be Matrix IDs or
  `matrix:` / matrix.to links (`src/hand_off.rs:22-27`).

**Lifafa** (own workspace; one current-thread tokio runtime plus `LocalSet` per account worker,
`src/worker.rs:141-158`):

- Backends are chosen by `config::Protocol` (`Imap | Pop3 | Graph | Jmap | Local | Tuta | Proton`,
  `config/accounts.rs:80-114`), and every backend speaks one request/event protocol
  (`MailRequest` in, `WorkerEvent` out, `worker/api.rs:11,435`).
- Tuta and Proton run as **out-of-process helpers** speaking JSON over stdin/stdout, with a
  streaming watch mode; the helper module is generic ("a second helper only needs a
  `HelperSpec`", `worker/helper.rs:1-17`). This is the seam a p2p backend needs, already built.
- Contacts are read from EDS (`src/contacts.rs`, `contacts/`); threading is by Message-ID and
  References (`models/message.rs`, `cache/threads.rs`).

**Suite-wide**: `suite-common-core::accounts` has `Capability::{Mail, Calendar, Contacts, Chat}`
and `Source::{OnlineAccounts, SecretPortal}` (`accounts.rs:13-26`); `people.rs` has the contact
picker rule over EDS. Mandelbrot has no address-book integration.

**p2panda in the tree**: only `tools/collab-spike/dcp-p2p` (Spike C, pinned to 0.7.1; being
ported to 0.8.2 for #2499). Its findings that matter here: one Ed25519 key is at once the iroh
endpoint, the p2panda author and the p2panda-auth actor, so **a key is a device**
(`DOCUMENT-COLLABORATION-SPEC.md` §8.5); with 0.7.1, p2panda-spaces' reader took about 81 s and
438 MB to catch up on 1,000 entries; content published by a member who had not yet applied a
removal was readable by the removed member; role change and blobs were unimplemented. The 0.8.2
re-measurement will say which of these still hold. **This RFC's later phases depend on it.**

Nothing in the docs covered p2p chat or p2p email before this RFC.

## 3. The sidecar: `tuna-p2pd`

One process per user session owns everything p2panda:

- the p2panda node: iroh endpoint (discovery on iroh's `Minimal` preset, which contacts no
  service by default, per Spike C), store, sync, spaces, blobs;
- the identity: device keys and key bundles, kept in `oo7` with the Secret portal, as RFC-0005
  keeps `collabd`'s secrets;
- the network permission (Flatpak `--share=network` until the portal exists, as RFC-0001
  records).

Mandelbrot and Lifafa (and, later, `collabd` for documents) are clients. Why a sidecar and not a
library linked into each app:

- **One identity, one node.** Two apps each running a node would be two devices with two keys,
  and every space would need both added. ADR 0014 allows a daemon only for state shared across
  running apps, as an optional sidecar with its own lockfile: this is that case.
- **Dependencies.** p2panda-store pulls `sqlx` and `libsqlite3-sys ^0.30`, which clashes with
  matrix-sdk's sqlite in one lockfile (RFC-0005 §3.5). Out of process, Mandelbrot's lockfile is
  untouched.
- **Runtimes.** Mandelbrot runs a multi-thread tokio runtime, Lifafa a current-thread one per
  account; neither has to host p2panda's.
- **The future service.** When the p2panda system service and its portal ship, `tuna-p2pd`
  becomes a thin client of it (or is replaced by it) without the apps changing their protocol.

Shape:

- A new crate `p2p/p2pd` in its own workspace (as `collab/` is), GPL-3.0-or-later.
- Started on demand by the first app that needs it (`gio::Subprocess`), with a lockfile in
  `$XDG_RUNTIME_DIR/tuna-p2pd/`; later apps connect to its Unix socket in the same directory.
  It exits a while after its last client unless it is running as a support node
  ([section 9](#9-offline-delivery)).
- The wire protocol is JSON lines, versioned (`{"v":1,...}`), in a GTK-free crate `p2p/p2p-proto`
  that both apps depend on. It mirrors Lifafa's helper protocol: requests, replies, and a stream
  of events (`message`, `edited`, `reaction`, `receipt`, `typing`, `membership`, `delivery`).
- Nothing in it is chat-specific below the protocol layer, so `collabd`'s DCP binding (RFC-0004
  §8.5) can later run on the same node and identity.

## 4. Data model

| Chat concept | p2panda |
|---|---|
| Conversation (group or 1:1) | A **space** (p2panda-spaces): members with access levels from p2panda-auth (Pull, Read, Write, Manage). A 1:1 chat is a two-member space; a group chat has managers (Manage) and members (Write). |
| Message | An operation in the author's log for that space: `{"t":"msg","id","body","format":"plain"|"markdown","reply_to"?,"thread"?}`, encrypted for the space. |
| Edit, redaction, reaction | Operations referencing a message id (`edit`, `redact`, `react`). Last-writer-wins by causal order for edits; a redaction is honoured by every client but, as with Matrix, cannot unsend what a member already synced. |
| Read receipt | An operation `receipt {up_to}`, rate-limited (at most one per conversation per few seconds). |
| Typing, presence | **Gossip only**, never stored: ephemeral by design, and p2panda's gossip reaches only online peers. |
| Attachment | A blob (p2panda-blobs), referenced from the message by hash and size, encrypted with a per-file key carried in the message (as `dcp-p2p`'s `blobcrypt.rs` already does for documents). p2panda-blobs is still an empty crate in 0.8.2 (#2502), so until it ships, small files (a limit measured in phase 1) go inline in chunked operations, and larger ones over a direct iroh-blobs transfer between online peers, as the spike already does. |
| Membership change | p2panda-auth operations (add, remove, promote, demote), which 0.8.0 integrated into the Node API. Removal must be checked against the Spike C leak before it is offered as a security property (section 10). |

**Encryption scheme.** p2panda-encryption has a "data" scheme (a shared group key; post-compromise
security by rotation on removal, no forward secrecy) and a "message" scheme (a forward-secure
ratchet similar to Signal's, aimed at group chat, per p2panda's 2025-02-24 design post). Chat
should use the message scheme. **Not confirmed**: whether the high-level spaces / Node API in
0.8.2 uses the message scheme, or only the data scheme. Phase 1 must answer that from the code;
if only the data scheme is exposed, chat ships without forward secrecy, and the UI says so.

**Versioning.** Every operation carries a schema version, and the sidecar refuses (and says so)
operations from a newer major version. A p2panda format change like 0.8.1's is handled by the
sidecar migrating its store, never by the apps.

## 5. Identity and contacts

- **A person is not a key.** Per Spike C a key is a device, so a person's identity is the set of
  their devices' keys, managed as p2panda's multi-device groups do (key bundles, renewed
  automatically since 0.8.0). The suite shows one person with several devices, as Mandelbrot
  already does for Matrix sessions.
- **Exchange.** p2panda 0.8.0 added a compact encoding of authenticated member info "for QR codes,
  etc.". The suite offers three ways to add someone, all carrying that encoding: a QR code (in
  person), an invite link (`tuna-p2p:` URI, opened by Mandelbrot), and an email (section 7a).
- **Address book.** A contact's p2p identity is stored in EDS as a vCard `IMPP` value
  (`IMPP;TYPE=p2panda:tuna-p2p:<encoded member info>`), so one person can have an email address,
  a Matrix ID and a p2p identity on one card, and the existing picker (`people.rs`) offers the
  right one for each app. `accounts.rs` gains `Source::P2p` for the user's own identity with
  `Capability::Chat`.
- **Linking identities.** A Matrix ID and a p2p identity are never merged automatically
  (RFC-0004 §9). They are linked only by the user, or by a signed statement from the person
  themselves (the same open question RFC-0004 leaves; this RFC does not define the format).

## 6. Mandelbrot

**Goal:** p2p conversations in the same sidebar, room view and composer as Matrix rooms, so the
user does not have to care which network a conversation is on.

**The obstacle:** there is no seam. A parallel p2p UI would be faster to build but would never
become "the main way to chat"; refactoring all 60k matrix-touching lines first would stall for
months. The proposal goes between them, in the model layer:

1. **Close the leaks first** (no behaviour change). Move the 56 direct `.matrix_room()`,
   `.matrix_timeline()` and `.client()` uses in `session_view`, `components` and
   `account_settings` behind methods on `Room`, `Timeline` and `Session` that say what the UI
   wants (`send_text`, `send_attachment`, `redact`, `react`, `members`, `mark_read`...). Each is a
   small, reviewable pull request with the existing GUI journeys as its test.
2. **Give the model an inner enum.** `Room`, `Timeline` and `Event` hold
   `enum Source { Matrix(..), P2p(..) }` rather than a matrix-sdk type. The Matrix arm is today's
   code. The p2p arm is a client of `tuna-p2pd` that produces the same GObject signals and list
   models (`VectorDiff`-style updates), so `room_history`, `message_row`, the sidebar and the
   composer render either. Matrix-only features (spaces, polls, threads in Matrix's sense,
   verification, calls) stay matrix-only and are hidden for p2p rooms.
3. **A p2p "session"** in the session list, beside Matrix sessions, with its own account settings
   page (devices, identity QR, support node).

Hand-off (`hand_off.rs`) accepts `tuna-p2p:` targets as well as Matrix ones, and `share-file`
works for p2p conversations. Notifications reuse `session/notifications` with a p2p source.

Calls stay on MatrixRTC (section 8). Everything p2p is behind a cargo feature `p2p`, off by
default until phase 4 is accepted.

## 7. Lifafa

Email is a different medium (asynchronous, addressed, interoperable with everyone), and
Delta Chat already shows chat carried over email; the inverse, email carried over p2p, only
works between suite users. So Lifafa gets p2panda in two steps:

**(a) Email as the way people find each other (phase 2).** This is the cheap, high-value part:
almost everyone a user wants to chat with is already in their mail.

- *Invite by email:* from a contact or a received mail, "Invite to chat" composes a normal email
  with a `tuna-p2p:` link and the same encoding as a QR code, in a `text/plain` part and an
  `X-Tuna-P2p` header. A recipient without the suite sees a readable invitation; with it, Lifafa
  offers "Add to contacts and chat", which hands the link to Mandelbrot (`open-room`).
- *Learn from mail:* a received mail with a valid, signed `X-Tuna-P2p` header from a known
  address offers to add the p2p identity to that contact's card (never silently: the user
  confirms, since a header proves only what the sender's mail server let through).
- The header is the Autocrypt pattern applied to p2p identity; it should follow Autocrypt's rules
  for when it is sent (not to mailing lists, not in replies to strangers by default).

**(b) A `Protocol::P2p` account (phase 6).** Mail-shaped messages (subject, threading by
`thread`/`reply_to`, attachments as blobs) to contacts whose card has a p2p identity, through a
new helper on the existing `worker/helper.rs` pattern, whose helper talks to `tuna-p2pd`. The
composer picks the p2p account automatically when every recipient has a p2p identity and the
user allows it, and falls back to SMTP otherwise. This is the part to argue about: it is only
worth building if suite users write each other mail-shaped messages that should not cross a mail
server; it may be enough that phase 2 moves those conversations to Mandelbrot.

## 8. Matrix, calls and what stays

- **Matrix stays** for everyone who is not a suite p2p user, for existing rooms and communities,
  and for calls. ADR 0013's one-Matrix-identity rule is unchanged.
- **No bridge** first. None exists (p2panda ↔ Matrix), and a bridge is a server, which undoes
  the point. Revisit only if users ask to carry a conversation from one network to the other.
- **Calls** stay on MatrixRTC and LiveKit (`matrixrtc`, ADR 0016). p2panda has no real-time
  media. A later option, not proposed here: signal a call over a p2p conversation and carry media
  over an iroh connection; that is research.
- **Default for new conversations:** when the person picked has a p2p identity, Mandelbrot's
  "new chat" proposes p2p first and Matrix second; otherwise Matrix.

## 9. Offline delivery

Chat without store-and-forward fails the common case (the recipient's laptop is shut). Today:
p2panda's gossip reaches only online peers; its log sync delivers "eventually" only when both
sides are online at some point; "shared nodes" / "support nodes" and the Dash Router
(delay-tolerant routing) are announced directions, not releases (p2panda.org 2025-02-24 and
2026-09-16 posts; Reflection's spring 2026 update calls offline sync its most important missing
feature).

Proposal, without waiting for upstream:

- `tuna-p2pd --support-node` runs headless on an always-on machine the user controls (a home
  server, a NAS, a small VPS) or their organisation provides. It is **added to the user's spaces
  as a device of the user**, with the Pull access level where the space allows it (it stores and
  serves operations but cannot write), so it holds ciphertext it can relay but not read once the
  message scheme is in use. (With only the data scheme, a device of the user can read the group
  key; this must be stated, not hidden.)
- Without a support node, the UI shows each message's delivery state (sent to N of M members)
  so nothing is silently lost, and messages deliver when both sides are next online together.
- Replace this with upstream support nodes when they ship.

## 10. Risks and what must be measured first

| Risk | Consequence | Mitigation |
|---|---|---|
| Pre-1.0 API and format (0.8.1 broke data) | Stored history unreadable after an upgrade | Versioned operations; the sidecar migrates; pin exact versions; p2p behind a feature |
| Spike C removal leak (0.7.1): a member who has not applied a removal can still share new content with the removed member | "Remove from group" is not a security property | **Still present in 0.8.2** (#2502, debug and release); a server-free design cannot close it. The UI says removal stops future invitations and sync, not that it guarantees secrecy |
| Spaces catch-up cost (0.7.1: about 81 s and 438 MB at 1,000 entries) | Long chats unusable on rejoin | **Unchanged in 0.8.2** (#2502: 95.5 and 102.0 s against 100.0 s for 0.7.1 on the same machine; the orderer keeps every message upstream). Segment long conversations into epochs (a new space per N hundred messages, N measured in phase 1) |
| Forward secrecy depends on the message scheme being reachable from spaces | Weaker confidentiality than Signal or Matrix's Megolm | Phase 1 reads the code; the UI states the property it actually has |
| No store-and-forward upstream | Undelivered messages | Support node (section 9) and visible delivery state |
| No security audit read (one was pending in 2025) | Unknown flaws | Do not market p2p chat as end-to-end secure until an audit report is read |
| Flatpak needs `--share=network` until the portal exists | A broad permission for a feature that is optional | Already required by Mandelbrot and Lifafa; the sidecar is the only p2p network user |

## 11. Phases

Each phase ends with something measured or shown, and can be the last one.

| # | Phase | Ships to users? | Done when |
|---|---|---|---|
| 0 | Re-run Spike C on p2panda 0.8.2 | No | **Done, #2502**: role change works; catch-up cost and the removal leak unchanged; blobs still empty; p2panda-net gains sync gating |
| 1 | `p2p/p2pd` and `p2p/p2p-proto`; a CLI with two and three nodes chatting in a space: messages, edits, reactions, receipts, attachment, add and remove member | No | A CI journey on loopback and on the relay spike's shaped links; the encryption scheme in use read from the code and written down |
| 2 | Lifafa: invite by email and learn-from-mail (section 7a); `IMPP` on contacts; `Source::P2p` in `accounts.rs` | Behind a setting | A GUI journey: invite mailed, opened, contact card updated |
| 3 | Mandelbrot: close the 56 leaks behind `Room`/`Timeline`/`Session` methods | No (refactor) | Existing journeys green after each pull request |
| 4 | Mandelbrot: the `Source::P2p` arm and a p2p session, behind `p2p` | Behind the feature | Two Mandelbrot instances chat over two `tuna-p2pd` sidecars in a GUI journey, with video |
| 5 | Support node (`--support-node`) and delivery state | Behind the feature | A journey with the recipient offline during sending |
| 6 | Lifafa `Protocol::P2p` account (section 7b) | Only if accepted after phase 4 | — |
| 7 | Move `tuna-p2pd` onto the p2panda system service and portal when they ship | — | The apps unchanged |

## 12. Questions for the owner

1. Is p2p chat for **suite users talking to each other** (as proposed), or should it replace Matrix
   for everyone, including bridging existing rooms? The second needs a bridge server, which this
   RFC avoids.
2. Is the support node something the suite should offer to run (a Flatpak `--support-node`
   mode, or a container), or should it wait for upstream support nodes?
3. Should phase 6 (a p2p mail account in Lifafa) be built at all, or is "invite by email, chat in
   Mandelbrot" enough?
4. Documents: should `collabd` move its p2p binding onto the same `tuna-p2pd` node once phase 1
   exists, reversing RFC-0005's decision to leave iroh/p2panda out of the apps?

## References

- p2panda: https://p2panda.org ; https://github.com/p2panda/p2panda (README, CHANGELOG 0.7.1 to
  0.8.2) ; https://docs.rs/p2panda-net , -spaces , -encryption , -auth , -sync , -blobs ;
  https://p2panda.org/2025/02/24/group-encryption.html ;
  https://p2panda.org/2026/09/16/dash-router-sync-api.html
- GNOME and Modal: https://modal.cx/reflection ; https://modal.cx/blog/reflection-spring-update ;
  https://nlnet.nl/project/p2panda-systemservice ;
  https://archive.fosdem.org/2026/schedule/event/MCVBNK-p2panda-modal-reflection
- Precedent: Delta Chat (chat over email, chatmail relays) https://delta.chat/en/ ; Autocrypt
  (key exchange in mail headers)
- In this repository: `docs/rfc/0004-spike-c-results.md`, `docs/DOCUMENT-COLLABORATION-SPEC.md`
  §8.5 and §9, `tools/collab-spike/dcp-p2p`, `mandelbrot/src/session/`, `lifafa/src/worker/`,
  `suite-common-core/src/accounts.rs` and `people.rs`.
