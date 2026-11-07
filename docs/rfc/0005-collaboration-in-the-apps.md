# RFC-0005: Collaboration in the apps (Tables first), over the Matrix and Durable Streams bindings

Date: 2026-10-25 · Status: **draft** · Governed by: [RFC-0001](0001-crdt-collaboration.md) (Loro, `collab` feature, file is the record) · Uses: [RFC-0004](0004-interoperable-document-collaboration.md) and the [DCP spec](../DOCUMENT-COLLABORATION-SPEC.md) · Evidence: [this RFC's experiments](#evidence-ci-runs)

> **Decision already taken (not argued here).** The owner asked that "the best two
> implementations should be tried out in the apps". From the RFC-0004 spike results the
> two bindings are **Matrix** (`tools/collab-spike/dcp-matrix`, matrix-sdk, E2EE) and
> **Durable Streams** (`tools/collab-spike/dcp-streams`, client-side XChaCha20-Poly1305
> and Ed25519, TLS verified). The runners-up, iroh/p2panda (the reader's cost grows
> faster than linearly, about 81 s at 1,000 entries) and Nextcloud WebDAV (no L2), are
> out. This RFC says how to get those two into Letters, Tables and Decks, in what
> order, and what each step must prove. It changes no product code.

## Summary

| Question | Answer, with the evidence that backs it |
|---|---|
| What is in the apps today? | Loro documents for Tables and Decks behind `collab`, with in-process replicas and no transport; nothing in any GTK shell; Letters has none on `main` ([section 1](#1-what-exists-today)). |
| How does a binding plug in? | Three layers: a synchronous GTK-free replica in the core crate, a small GTK-free protocol crate in the product workspace, and **a sidecar process** that owns tokio, the `DocumentStore`, TLS and the secrets ([section 2](#2-the-seam)). |
| Promote the DCP crates into the product workspace (A) or run them out of process (B)? | **B.** A does not compile in the product workspace today and moves 17 crates of the default shipped build; numbers in [section 3](#3-dependency-architecture-decided-by-measurement). |
| Network permission and secrets? | Both bindings need outbound HTTPS and so `--share=network`; Flatpak has no runtime request for it. Secrets: `oo7` with the Secret portal, inside the sidecar ([section 4](#4-flatpak-permissions-and-secrets)). |
| Smallest honest UI? | A `Collaborate` section in the main menu, two action dialogs, one `AdwBanner`, one header-bar status button; about 700 lines in five new modules and about 20 lines in `window.rs` ([section 5](#5-the-ui-slice-in-tables)). |
| What proves it? | Store-level journeys stay in `dcp-conformance`; a Tables GUI journey against a real Synapse and a real Durable Streams server, with video, entered in `conformance/capabilities.json` ([section 6](#6-tests-and-evidence)). |
| In what order? | Five abandonable phases, Tables first; the first three pull requests are bounded and none changes shipped behaviour ([section 7](#7-phases-each-abandonable)). |

## 1. What exists today

Everything below was read on `origin/main` at `287657b8`.

### 1.1 On `main`

| Piece | Where | State |
|---|---|---|
| **Tables: the workbook as a Loro document** | `tables-core/src/controller/collab.rs` (737 lines), `collab_tests.rs` (120), `tests/determinism.rs` (136); feature `collab = ["dep:loro", "dep:serde", "dep:serde_json"]` in `tables-core/Cargo.toml:31`, `loro = "1.16"` optional | `Replica` with `new(peer, &state)`, `fork`, `from_updates`, `export_all`, `merge_from`, `record(&state, &ops)`, `rebuild() -> WorkbookState`. Cells are registers keyed `<row id>\|<col id>\|<field>`, rows and columns are stable-id lists, sheets a movable list with delete-wins tombstones. **No transport, no delta export, no remote import other than `merge_from(&Replica)`.** The module's own "known limits": a remote change rebuilds the workbook, so the local undo no longer applies. |
| **Decks: the deck as a Loro movable tree** | `decks-core/src/collab.rs` (463), `collab_tests.rs` (318); feature `collab = ["dep:loro"]` | Same shape and same limits; `ops.rs` addresses by stable ids, so a local op maps onto the tree one to one. |
| **Letters** | nothing on `main` | `letters-core` has the live model (ADR 0010 stage 3c, `letters/src/live.rs`) but no `collab` feature. |
| **Undo** | `suite-common-core/src/ops.rs` (`Op`, `History`, ADR 0011) | Tables is on it; `History::step` clears the **whole** history when one stored step no longer applies (`ops.rs`, the `Err(_) => self.clear()` arm). ADR 0011 says a step that does not fit is dropped, singular; the code drops all of them. |
| **Presence** | `dcp-core::presence::Roster` only | The receiver's 15-second roster. **Neither binding publishes or receives presence** (`grep presence` finds nothing in `dcp-matrix` or `dcp-streams`; the spec says no binding is L3). |
| **CI** | `ci.yml`: `cargo clippy -p tables-core --features collab …` and `cargo test -p tables-core --features collab --lib collab`, the same for `decks-core` | The `collab` tests run in their own steps, **not in the nextest run whose JUnit feeds the capability ledger**, so no `collab` test can be cited as ledger evidence today. |
| **Advisory ignores** | `deny.toml` | Six entries for Loro's `im`, `sized-chunks`, `bitmaps` and `atomic-polyfill` (RUSTSEC-2023-0126, -2026-0248, -0251, -0255, -0247, -2023-0089), each "must be resolved before `collab` ships (#1043)". |
| **The shells** | `tables/`, `decks/`, `letters/` | No `collab` reference in `tables/` or `suite-common/` (grep), no `[features]` in `tables/Cargo.toml`, no network crate in the root `Cargo.lock` (no tokio, hyper, reqwest, rustls, ring, h2, openssl, oo7, zbus: grep of the lockfile finds `loro` only). |

### 1.2 The old branches

The clone used for this review was shallow (18 commits of `main`), so the branches were compared after `git fetch --unshallow`.

| Branch | Ahead of its merge-base | Contains | Reusable? |
|---|---|---|---|
| `origin/tables-collab` | 1 commit (`5e921f3f`), 533 commits behind `main` | The commit that became `tables-core/.../collab.rs`. `main`'s copy differs from it by 23 changed lines in `collab.rs` and 2 in `determinism.rs`. | **No.** `main` already has it, newer. |
| `origin/decks-collab` | 2 commits (`0b915ead`, `c3118aa8`), 511 behind | The commits that became `decks-core/src/collab.rs`; `main`'s copy differs by 36 changed lines in `collab.rs` and 39 in `collab_tests.rs`. | **No.** Same. |
| `origin/letters-collab` | 2 commits (`211a3b79`, `ac8b7a9e`), 486 behind | `letters-core/src/collab.rs` (374 lines) and `collab/tests.rs` (198): the document as one Loro `Text` with Peritext marks, expand rules per `MarkKey`, inline objects as `OBJECT` chars with an `obj` mark, paragraph style as a `p` mark on the `\n`; `Op::SetParagraphs` and table paragraphs refused (`CollabError::Tables`); feature `collab` in `letters-core/Cargo.toml`. **Not on `main`.** | **Yes, at Phase 5**, after rebasing onto the live model; its encoding comment is the design. 486 commits behind, so it will not apply cleanly. |
| `origin/collab-transport` | 1 commit (`4331c0b8`, "wip … parked"), 486 behind | `tables-core/Cargo.toml` adds `iroh 1.2`, `tokio` and `data-encoding` behind `collab`; `Cargo.lock` **+2,354 −26 lines**. No code. | **No**: iroh is the runner-up, and it is a measurement of what an in-lockfile transport costs (compare [section 3](#3-dependency-architecture-decided-by-measurement)). |

Nothing else in the repository mentions collaboration in a shell. The first piece of
shell code is therefore new, and the first piece of product code that is not new is the
Tables `Replica`.

## 2. The seam

### 2.1 Layers and where code runs

```
┌────────────────────────── GTK process (one thread: the main loop) ─────────────────────────┐
│ tables/  collab_ui.rs  collab_dialogs.rs  collab_banner.rs  collab_status.rs   (new)        │
│    │  GAction handlers, adw dialogs, banner; gio::Subprocess + DataInputStream (no threads) │
│    ▼                                                                                        │
│ suite-collab  (new, GTK-free, sync, NO tokio, NO dcp-*): SessionClient, Command, Event,     │
│               JSON-lines codec, the "loro/1 + tables/1" glue                                │
│    │                                                                                        │
│    ▼                                                                                        │
│ tables-core::controller::collab::Replica   (exists; gains delta export / import / events)   │
│ suite-common-core::{ops::History, atomic_save}   (exist; History gains per-step drop)       │
└──────────────┬──────────────────────────────────────────────────────────────────────────────┘
               │  stdin/stdout of a child process: one JSON object per line, payloads base64
┌──────────────▼───────────────────────── collabd (separate binary, own workspace + lock) ────┐
│ tokio multi-thread runtime · dyn DocumentStore = MatrixStore | StreamsStore                  │
│ dcp-core::{view::build, graph, updates::Receiver/StreamWriter, outbox, presence}            │
│ oo7 (Secret portal) · the matrix-sdk store dir · login flows                                │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

* **The GTK process never runs tokio and never links a network stack.** The sidecar is
  launched with `gio::Subprocess` (pipes for stdin and stdout); the shell reads lines with
  `DataInputStream::read_line_async` on the main loop, so no thread and no channel crosses
  into GTK. The product has no precedent for a network thread feeding the main loop (the
  only `glib` async use is `timeout_add_*_local`; a grep for `spawn_future_local` finds
  no non-test use), so this RFC does not invent one.
* **`Replica` stays on the main thread** next to `WorkbookState` (`Rc<RefCell<SheetModel>>`
  is not `Send`). Only `Vec<u8>` crosses the process boundary.
* **`suite-collab`** holds the types both ends share (`Command`, `Event`, the codec) so the
  protocol is one file and a unit-testable one. It depends on `serde` and `serde_json`,
  which are already in the lockfile. The sidecar depends on it too.
* **The spike's `DocumentStore` has no push.** It is request/response (`versions`,
  `updates`, `publish_*`); a binding discovers news by polling a view that its own sync
  loop keeps fresh (`MatrixStore` runs `/sync` in a task; `StreamsStore::sync`). `collabd`
  therefore owns a poll loop per open document (a `watch` tick on Matrix's sync, a
  `long_poll` on Streams) and turns changes into `Event`s. This keeps `DocumentStore`
  as it is; adding a `subscribe` method to the trait is a later optimisation, not a
  prerequisite.

### 2.2 What goes on the wire

`Command` (shell to sidecar) and `Event` (sidecar to shell), sketched, not frozen:

| Command | Meaning |
|---|---|
| `Connect { binding, config }` | Matrix: homeserver URL; Streams: server URL and optional header. Credentials are *not* in the command: the sidecar reads them from the Secret portal. |
| `Login { … }` | Matrix: user id and password, or the SSO/OAuth flow; Streams: none (the identity is generated, see 4.2). |
| `Share { title, mime, first_version: bytes }` | `create_document` with the saved xlsx as version zero; replies `Shared { doc, invite }`. |
| `Join { invite }` | Matrix: accept the invite for a room id, **wait until the sync has delivered the invite and the SDK can name the inviter before joining** (spec 8.1: joining at once read 0 of 1 versions in 6 of 6 trials; joining while the room was listed but its inviter unknown, 13 of 760 runs). Streams: the owner has already granted this principal; the invite carries server URL and document id. |
| `PublishUpdate { doc, bytes }` | `StreamWriter::next(base, bytes)` with `algorithm: "loro/1"`, `doc_model: "tables/1"`; bytes over 34,000 become an `encoding: file` payload. |
| `PublishVersion { doc, bytes, message, resolve }` | The saved file; the sidecar chooses `parents` (2.3 point 5) and calls `publish_checked`. |
| `Leave { doc }`, `Revoke { doc, principal }`, `Grant { doc, principal, role }` | Owner operations. |

| Event | Meaning |
|---|---|
| `State { connecting \| online \| offline { since } \| auth_required \| error }` | The indicator. |
| `Roster { members }` | From the document's meta/roster; **not presence**. |
| `Genesis { bytes, base_version }` | A joiner's first Loro snapshot. |
| `Update { id, author, bytes }` | One update the receiver has ordered and accepted (`updates::Receiver`), waiting for the shell's dry run. |
| `Heads { heads: [{id, author, ts, covers}], conflict }` | `view::build`'s result. |
| `Dropped { id, reason }`, `Gap { stream }` | Spec 7 rules 2 and 3, for the status button's tooltip. |

The shell answers an `Update` with `Applied { id }` or `Rejected { id, why }`. The
dry run of spec 7 rule 3 belongs to the side that owns Loro: the shell imports the bytes
into a fork of the document and checks that what they touch still decodes (a full
`rebuild()` is only affordable for a genesis: about 400 ms at 100,000 cells, section
3.7), and only then imports into the live replica.
`dcp_core::updates::Target::dry_run` in the sidecar is therefore a structural pass-through
and the real check is the shell's; a `Rejected` is reported by the sidecar as a drop. (The
other placement, Loro in the sidecar, would put `loro` in a second lockfile and duplicate
`Replica`.)

### 2.3 Updates and versions

**The Loro document is the live layer; the versions are the saves.** This is RFC-0001
option B (the file is the record, the CRDT is session-scoped) expressed in DCP terms.

1. **Genesis.** Two peers that each open the same `.xlsx` and call `Replica::new` get
   different documents: line ids are `format!("{:x}.{:x}", peer, counter)`
   (`collab.rs`, `fresh_id`), so every row and column has a different identity on each peer
   and (inferred from that scheme, not run) a merge of the two would hold both sets of lines.
   The sharer therefore publishes (a) version zero, the
   saved file (`parents: []`, spec version rule 7), and (b) update `seq 0`, a Loro snapshot
   of the sharer's `Replica`, with `base_version` = version zero. A joiner opens version
   zero, takes the snapshot with `Replica::from_updates`, and **replaces its workbook with
   the rebuilt one**; the first rebuild is checked against the file (the cells and
   formulas of the xlsx must equal the rebuilt workbook, else the join fails visibly).
2. **Deltas.** After every user action (one `History` step) the shell calls
   `Replica::record(&state, &ops)` as today and then a new
   `Replica::export_since(&last_sent_version_vector) -> Vec<u8>`; the bytes are one
   `PublishUpdate`. The two methods the seam needs are small
   (`version_bytes()` and `export_since()`, plus `import_updates()`, `export_snapshot()`);
   the experiment branch has working versions of all four ([section 3.7](#37-what-a-loro-workbook-weighs)).
3. **Batching is not optional on Matrix.** A default Synapse allows 0.2 events/s per user
   with a burst of 10 (measured 0.26 events/s sustained, [Spike A](0004-spike-a-results.md)).
   The shell coalesces deltas (`Replica::export_since` over a window) and `collabd` flushes
   at most every few seconds on Matrix, immediately on Streams (no such limit). Live
   typing latency on a default homeserver is seconds, not milliseconds; the UI says so
   rather than pretending.
4. **Saving.** Ctrl+S keeps its meaning: `persistence::save_engine_to_xlsx` writes the
   user's file with `atomic_write_bytes` (unchanged, the file is the record). The hook is
   the one `locations::commit_save` already is: after the atomic replace succeeds,
   `persistence::save_engine_to_xlsx` calls `collab::after_save(path)`, which reads back the
   file just written (the record itself, carried parts included) and sends
   `PublishVersion`; the sidecar's `publish_checked` opens the package before publishing
   (spec version rule 3). A failed publish **never fails the save**: the
   file is saved, the status button shows "not shared yet", and the sidecar's outbox
   (`dcp_core::outbox`: queue and back off, never drop a version) retries. Autosave
   (`autosave_bytes`, crash-recovery snapshots) is **never** published: it is recovery, not
   a save, and publishing it would make a version of every timer tick.
5. **Parents (spec 4.2 rule 4 and 6.3).** A save is a child of the version the editor
   started from, *unless* the editor's replica already contains another head's content:
   the sidecar lists as `parents` every current head whose `covers.through` update this
   replica has applied (`dcp_core::publish::parents_for_merge` over that set), else only
   the opened version. Without this rule two people saving seconds apart would create two
   heads with equal content and the banner of 2.5 would fire spuriously on every
   collaboration.
6. **The `loro/1` profile does not exist yet** (spec 7: "None exists yet"), and writing it
   is part of Phase 1. It must fix: the genesis rule above; that **a Loro update is
   applicable to any replica that shares its genesis, whatever `base_version` the writer
   had**. `dcp_core::updates::Receiver::handle` skips an update whose `base_version` is not
   the loaded version (`Skip::OtherBase`), which would silently drop a peer's edit made
   just before it saw a newer version; the profile has to say that for Loro the session
   base is version zero for the life of the session, or the receiver has to take a
   profile hook for it. The determinism rule (spec 7 rule 6) is already tested in
   `tables-core/tests/determinism.rs`.
7. **Compaction is out of scope.** Updates are retained; `covers` is written on versions
   but no reader discards anything. The Matrix room and the Streams log grow without bound
   in the meantime (a stated limit, not a fix).

### 2.4 Remote updates and per-user undo

ADR 0011 already decides the behaviour: remote edits "do not enter the local `History`, so
undo only ever takes back your own steps", and an inverse that no longer fits drops **its
step**. Two things stand between the code and that sentence:

* `Replica::rebuild()` replaces the whole `WorkbookState`, which is what the module's
  "known limits" calls out. The plan replaces it on the remote path with **Loro events
  mapped to ops**: `LoroDoc::subscribe` delivers per-container diffs; a change to
  `cells:<key>` with key `<row id>|<col id>|<field>` is resolved through the sheet's
  mirror (`SheetMirror.rows`/`cols` hold the stable ids) to `(row, col)` and becomes an
  `Op::SetCell`/`SetFormat`/… applied by a new `WorkbookController::apply_remote(ops)`
  that mutates the model and broadcasts `Hint`s like `apply_ops` but **does not
  `record`**. This is designed from the code, not measured.
* Stored inverses are **index-addressed** (`Op::SetCell { sheet, row, col, … }`). A remote
  *cell value* leaves them valid (undo restores your old value over theirs: last writer
  wins, as in Google Sheets). A remote *structural* change (row or column insert/delete,
  sheet add/delete/move) makes them wrong, and wrong in the dangerous way: they still
  apply, to the wrong cell. ADR 0011 says "stored inverses are rebased on remote
  events" and calls that Phase 2 work; **this RFC does not do it.** Until it exists, a
  remote structural change drops the steps that touch that sheet (never silently
  retargets), via the per-step drop below, and the status line says "Undo history for
  this sheet was cleared by a change from <name>".
* `History::step` must change from "clear everything on one failure" to "drop that step,
  keep the others", with a property test (undo-all after interleaved remote ops never
  applies a half step and never loses an unaffected step). That is a change to
  `suite-common-core` used by all three apps and is **PR 1** of section 7.

First slice: remote update → rebuild → **clear the history** and say so. It is wrong for a
long session and right for a first one; the events path replaces it in Phase 2's
second half. Both are abandonable.

### 2.5 Two heads

`dcp_core::view::build` returns `View { heads, rejected }` and `is_conflict()`; the sidecar
sends it as `Heads`. What the shell does:

| Situation | Behaviour |
|---|---|
| One head, equals what I have | Nothing. |
| Another head whose `covers.through` update this replica has applied | Subsumed, **not a conflict**: the next save lists it in `parents` (2.3 point 5). No banner. |
| More than one head and at least one is *not* covered (a non-Loro peer saved a file; a user edited offline in LibreOffice and published it; an unknown `algorithm`) | **Banner** (`AdwBanner`, text "Two versions of this workbook exist", button "Resolve…"), a lasting state, as GNOME-GUIDELINES §7 says banners are for. |

"Resolve…" offers three responses in an `AdwAlertDialog`:

* **Keep mine**: `PublishVersion` with `parents = parents_for_merge(all heads)` and my file
  (spec 6.3: the user asked for the merge).
* **Keep theirs**: load that head's xlsx into a scratch `WorkbookState` and feed it through
  `Replica::record(&scratch, &[])`, the same *diff the workbook against the mirror* path
  local edits already use, so peers converge on it as ordinary updates; then publish a
  version with all heads as parents. It is one undoable step (`History::begin/end`).
* **Open both**: open the other head's file, written to a private cache path the way
  `suite_common::locations::open_location` stages a remote file, in a second window
  (`app.open-file`); nothing is merged or published.

The dialog says whose version each is (spec 9 rule 3: show the principal behind every
version) and when.

### 2.6 Presence (L3)

Not cheap, so not in Phases 1 to 3. `DocumentStore` has no presence method, neither binding
implements one, and the spec forbids writing presence to a durable log: the Streams binding
has no ephemeral channel at all and Matrix would need ephemeral events or to-device
messages that nobody has tried. What is cheap and honest is a **roster and a connection
indicator**: the members from the document's metadata and "last update from X, 12 s ago"
derived from update authors. Cursors and the `AdwAvatar` row of DESIGN-UI follow whichever
transport grows an ephemeral channel first.

## 3. Dependency architecture, decided by measurement

The question: promote `dcp-core`, `dcp-conformance`, `dcp-matrix` and `dcp-streams` into the
product workspace behind `collab` (**A**), or keep them in their own workspace and lockfile
as a sidecar binary that the apps talk to over stdio (**B**)? Measured on GitHub Actions
(`ubuntu-24.04`, 4 vCPU, 16 GB, rustc 1.99.1), on two throwaway branches that are not meant
to merge: `plan/collab-in-apps-exp-a` (A) and `plan/collab-in-apps-exp-b` (B). The scripts are
kept in `tools/collab-spike/scripts/rfc0005/` so every number can be reproduced.

**What A was.** The four crates copied to `tools/collab-promoted/` and made members of the
root workspace (they have to be: path dependencies inside the workspace directory become
members whether or not they are listed), the root `[workspace.package]` and
`[workspace.dependencies]` entries they inherit from, one seam crate `collab-session`
(`dcp-core` always; `dcp-matrix` and `dcp-streams` optional behind `matrix` and `streams`;
plus `matrix-sqlite`), and `tables` features `collab`, `collab-matrix`, `collab-streams`,
all off by default. The lockfile was resolved with `cargo metadata` and committed, so
`--locked` is meaningful.

**What B was.** `collab/` with its own `[workspace]` and lockfile (the spike's committed
lock, pruned by cargo to what five members use), the four crates, a `collabd` binary that
links both bindings and reads JSON lines on stdin, and the root workspace's `exclude`
extended by `collab`. Its Flatpak manifest builds `tables` and then
`cd collab && cargo build --locked --release -p collabd`, and installs
`/app/libexec/tuna-collabd`.

### 3.1 Results

| Measurement | `main` | **A** (promoted) | **B** (sidecar) |
|---|---|---|---|
| Packages in the product `Cargo.lock` | 649 | **896** (+247, +38 %; 269 package@version pairs added, 22 replaced) | **649, byte-identical to `main`** |
| Lines in the product `Cargo.lock` | 6,681 | **9,596** (+2,915) | 6,681 |
| The sidecar's own lock | — | — | 460 packages, 4,838 lines; 126 of its package@version pairs are also in the product lock, 334 are not (union 983) |
| Packages the **shipped** build compiles (`cargo build --locked --release --unit-graph -p tables -p letters -p decks`, default features, nothing compiled to find out) | 505 | 505, but **17 of them at a different version than on `main`** (`aes` 0.9.1→0.9.3, `bitflags` 2.13.0→2.13.2, `futures-*` ×7 0.3.32→0.3.34, `hybrid-array`, `indexmap`, `thiserror`+`-impl` 2.0.18→2.0.21, `time`+`-macros`, `zeroize` 1.8.2→1.9.1, `arrayvec`) | 505, **identical set and versions** |
| Packages `--workspace` compiles (what `ci.yml`'s clippy, nextest and doc tests run) | 505 | **680** (+175) | 505 |
| Is a bump forced? (`cargo update <crate> --precise <old>`) | — | `futures-util`, `zeroize`, `time`, `bitflags`, `aes`: **refused**, "required by `matrix-sdk 0.19.1`" (`^0.3.34`, `^1.9.0`, `^0.3.55`, `^2.13.2`) or `matrix-sdk-crypto`/`-base`/`vodozemac` | — |
| `cargo build --locked --release -p <app>` in the Flatpak sandbox, default features | pass (the `flatpak` job) | pass for letters, decks and tables | pass |
| The same with the collaboration code in | — | **tables with `--features collab-matrix,collab-streams`: FAIL** | tables **plus** the sidecar: pass |
| `ci.yml` `test` job (clippy `--workspace`, nextest `--workspace`, doc tests) | pass | **FAIL**, same error as above | not affected |
| `advisories` job (`cargo deny --all-features check advisories`, the repo's `deny.toml`) | pass | **FAIL**: RUSTSEC-2026-0319, `anymap2` unmaintained, via `matrix-sdk` | product: pass; the sidecar's lock fails on the same advisory (it needs its own `deny.toml` ignore) |
| Licences of the new crates | — | all 264 new non-workspace packages pass a permissive allowlist (MIT, Apache-2.0, BSD-3-Clause, ISC, Zlib, MPL-2.0, CC0-1.0, CDLA-Permissive-2.0, …) with `cargo deny check licenses`; new expressions: `Apache-2.0 AND ISC` (`ring`), `ISC AND (Apache-2.0 OR ISC)` and a longer `… AND MIT AND BSD-3-Clause …` (`aws-lc-rs`, `aws-lc-sys`), `MPL-2.0` (`eyeball`, `readlock`, `as_variant`; `imbl`, `imbl-sized-chunks` at `MPL-2.0+`), `CDLA-Permissive-2.0` (`webpki-root-certs`), `MIT-0`, `BSD-1-Clause` alternative (`fiat-crypto`) | same crates, in the sidecar |
| `links = "sqlite3"` | none | one `libsqlite3-sys 0.38.2` (via `rusqlite 0.40.2`, `matrix-sdk-sqlite`); **no `sqlx` in the lock** | same, in the sidecar |
| Crate archives `cargo fetch --locked` downloads | 640 files, 114,708,926 B | **+264 files, +46,185,442 B** (+40 %) | product unchanged; the sidecar's own were not measured separately |
| `cargo-sources.json` from `flatpak-cargo-generator.py Cargo.lock` | 1,281 entries, 354,845 B (640 archives) | **1,765 entries, 489,854 B** (+484, +134,999 B, +38 %; 882 archives) | the product's file is unchanged; the sidecar's is a second file: 911 entries, 253,390 B (455 archives). One generator run takes one lock, so a manifest lists two source files |
| Cold compile, 4 vCPU | `tables-core` release: 322 s (a reference for what the product already costs) | `dcp-core` dev 10 s; `+streams` 13 s more; `+matrix` 88 s more (dev); release `probe` binary 277 s; `matrix-sqlite` release 218 s more | release `collabd` 267 s |
| Release binary, stripped | — | probe: 55,134,296 B | `collabd`: 55,184,648 B |

`collabd` did not run: the sidecar was only built, to link both bindings and measure.

### 3.2 What the numbers say

* **A does not build.** In the product workspace `ruma-common 0.20.2` (matrix-sdk 0.19.1's
  dependency) fails with `error[E0275]: overflow evaluating the requirement
  'Box<_>: Deserialize<'_>'` at `ruma-common-0.20.2/src/room.rs:139:44` (and two more sites,
  required "for `Box<zerovec::varzerovec::slice::VarZeroSlice<_, _>>`"), in the CI `test`
  job and in the Flatpak build of tables with the collaboration features. The same
  crates compile in the spike's workspace and in A's own `-p collab-session` builds
  (A3 and A4 above pass), so the trigger is, by inference, the **feature unification of `zerovec`
  with its `serde` feature** (the type in the error exists only with it; I did not test by
  switching it off), which the product graph turns on through
  `typst → hayagriva → icu_collator → zerovec` and `icu_*` (`cargo tree -e features -i
  zerovec`). I did not test a fix. It is an upstream compile failure that the spike could
  not have seen, because the spike's graph has no Typst.
* **A changes the default build even with `collab` off.** Same crates, same 505 packages,
  but 17 of them at a newer version. Of the five I tried to pin back
  (`futures-util`, `zeroize`, `time`, `bitflags`, `aes`) all five were refused, each
  "required by" `matrix-sdk`, `matrix-sdk-crypto`, `matrix-sdk-base` or `vodozemac`; the
  other twelve were not tried. RFC-0001's decision 1 ("an ordinary build and the Flatpak
  `--locked` build are unaffected until Phase 2 ships") would stop being true on the day A
  merges, and the audit baseline (what the Flatpak compiles and what `cargo deny` reads)
  moves with it.
* **Streams alone is not free either.** Resolving A without `dcp-matrix` (a local `cargo
  metadata`, not a CI run) gives 750 packages (+101 net, 123 package@version pairs added);
  the same 22 existing crates were still replaced by newer versions at first resolution,
  and pinning some of them back with `cargo update --precise` worked (12 of 22 one by one;
  the rest are companions that must move together, which I did not pursue). With Streams
  only the bumps are not shown to be forced.
* **A widens the CI blast radius.** `--workspace` goes from 505 to 680 packages, the dcp
  crates' own tests (which need a Synapse or a Durable Streams server and *fail* without
  one by design) join the nextest run unless every CI command grows `--exclude`, and
  `tests/test_core_shell_split.py::CoreCratesAreGtkFree::test_c3_no_core_crate_needs_gtk_with_default_features`
  failed on A ("cargo tree failed for tools/collab-promoted/dcp-core": the test finds core
  crates by directory and asks `cargo tree` about them by that path). That one is a
  test fix, but it shows the layering test has opinions about where a `*-core` crate lives.
* **B moves the cost, it does not remove it.** The Flatpak gets the same code either way:
  the binary is 55 MB stripped in both, the vendored crates are the union of two locks
  (983 package@version pairs against A's 896; the 87 extra are mostly the same crate at two
  versions, because the sidecar's lock keeps the spike's older versions where A's resolution
  unified them), and the advisory finding exists in both. What B buys is that **none of
  it touches the product lockfile, the default build graph, `cargo clippy --workspace`,
  the core/shell split test or the `advisories` gate** until someone chooses to put them
  there, and that the E0275 failure is not in its path.
* **The sandbox does not get safer with B.** A sidecar inside the same Flatpak shares the
  app's permissions (`--share=network` is per application, not per process), so a sidecar
  is not a smaller attack surface for the network. It is a crash boundary, and it keeps a
  tokio runtime and a TLS stack out of the GTK process.

### 3.3 Recommendation: B

Decided by three numbers, not by taste: **A does not compile in the product workspace
today** (E0275, in CI and in the Flatpak), **A moves 17 crates of the default shipped
build** (RFC-0001 promised it would not), and **A puts a failing advisory and a failing
`test` job on every pull request** until `--exclude` and an ignore are added. B has none of
the three and costs a second lockfile (460 packages, 4,838 lines, with its own `deny.toml`
and Renovate entry), a stdio protocol, and one more binary in each Flatpak that ships
`collab` (55 MB stripped; the product binaries are untouched while the feature is off,
because the sidecar is built by a separate cargo invocation that the manifest only
contains in the collab-enabled variant).

The seam keeps A reachable: the shell talks to a `SessionClient` in `suite-collab`; the
sidecar is one implementation of it. If matrix-sdk and zerovec stop colliding and the owner
prefers one binary, an in-process implementation is a second implementation of that
trait, not a rewrite. The measurements that would change this recommendation: a build of
`tables --features collab-matrix` that passes in the Flatpak sandbox, and a matrix-sdk
release whose minimum versions do not move the shipped graph.

### 3.4 Flatpak: one more binary

How the Flatpaks get their crates today: the CI manifests (`flatpak/*.json`) give the
*build* `--share=network` and run `cargo clean && cargo build --locked --release -p <app>`,
so crates are fetched from crates.io during the build; **there is no generated
`cargo-sources.json` and no vendoring in CI**, and lockfile growth costs bandwidth and
build time there, not manifest size (`flatpak` job: letters, decks and tables each took
10 to 16 minutes in these runs). The Flathub-layout manifests (`flathub/*.json`) use
`cargo --offline` and a `cargo-sources.json` that `flathub/README.md` says to generate by
hand with `flatpak-cargo-generator.py Cargo.lock` and that is **not committed**; there,
and only there, the manifest-size rows of the table above are the cost, and Flathub's
rules require every dependency to be a listed source with no network during the build.

The manifest's `build-commands` are one `cargo` invocation then `install` lines
(`flatpak/org.tunaos.tables.json`); the sidecar adds
`cd collab && cargo build --locked --release -p collabd` and
`install -Dm755 collab/target/release/collabd /app/libexec/tuna-collabd` (and the same in
`flathub/*.json`, where `cargo --offline` needs a `cargo-sources.json` per lock).
`scripts/release_gate.py` requires the substring `cargo build --locked` and a
`type: dir, path: ..` source; both hold. The sidecar variant built in the CI Flatpak
container (the `gnome-50` image, `rust-stable` extension): `aws-lc-sys` and the whole
matrix-sdk tree compiled there. Bundle sizes with and without the sidecar:
the Tables bundle built from the unmodified manifest is **17,613,280 B**; the same manifest
plus the sidecar (`/app/libexec/tuna-collabd`) is **28,214,488 B** (+10,601,208 B, +60 %,
compressed bundle; the stripped binary is 55 MB, section 3.1). Both built green in the
`gnome-50` container, on `plan/collab-in-apps-exp-b` @ `aa5242a1`. Three apps that ship
`collab` ship three copies, one per app.

### 3.5 sqlite

The root lock has no `sqlx` and no `rusqlite`. With `matrix-sqlite` it gains exactly one
`libsqlite3-sys 0.38.2` (`cargo tree -i libsqlite3-sys`: `rusqlite 0.40.2` ← `matrix-sdk-sqlite`
← `matrix-sdk` ← `dcp-matrix`), and the only package with `links = "sqlite3"`. The clash the
spike found is p2p only (`p2panda-store` → `sqlx 0.8` → `libsqlite3-sys ^0.30`);
Matrix and Streams together have none, in either option.

### 3.6 What was not measured here

The cost of the IPC (latency of a delta over a pipe, 100-byte to megabyte frames: not run);
`collabd` as a running process under Flatpak; whether `opt-level = "s"`, LTO or
`panic = "abort"` shrinks the 55 MB (the profile was the default release profile); the
sidecar's crate downloads as bytes; a build of the whole `tables` binary with and without
`collab` (A's feature build fails; the default one was not size-compared).

### 3.7 What a Loro workbook weighs

Not a dependency question but it decides inline versus file payloads (the spec's 34,000-byte
inline limit) and how large a genesis is. `cargo test --release -p tables-core --features
collab` on the A branch, with `Replica::export_since`, `export_snapshot` and
`import_updates` added there (the same four methods PR 1 would add), a workbook of N rows
by 10 columns (text, numbers and a formula every fourth column), 4 vCPU:

| rows × cols | cells | xlsx (`save_sheets_to_xlsx_bytes`) | `Replica::new` | all updates | snapshot | snapshot import + `rebuild` | one-cell edit as an update | 100-cell paste as an update |
|---|---|---|---|---|---|---|---|---|
| 10 × 6 | 60 | 5,889 B | 0 ms | 2,175 B | 3,086 B | 0 ms | 103 B | 251 B |
| 100 × 10 | 1,000 | 11,232 B | 3 ms | 20,536 B | 25,110 B | 4 ms | 105 B | 1,734 B |
| 1,000 × 10 | 10,000 | 65,213 B | 34 ms | 225,512 B | 253,077 B | 38 ms | 106 B | 1,833 B |
| 10,000 × 10 | 100,000 | 634,144 B | 341 ms | 2,536,387 B | 2,857,606 B | 405 ms | 109 B | 1,934 B |

So: an ordinary edit is about 100 bytes and always inline; a **snapshot is 4 to 5 times
the xlsx** and passes the 34,000-byte inline limit between 100 and 1,000 rows, so the
genesis is a file payload (a media upload on Matrix, a file stream on Streams) for any
real workbook; a join costs about 400 ms of import and rebuild at 100,000 cells, which
blocks the main loop unless it is done in a worker or chunked (the `WorkbookState` is not
`Send`, so the rebuild is main-thread work: a real cost, recorded in the risks). The
record overhead measured in Spike B (a 100-byte update is a 747-byte record on Streams)
makes the small-update case 7 times its size on the wire; the batching of 2.3 point 3
helps there too. The measurement used one process, one core and the in-process replicas; it
says nothing about network.

## 4. Flatpak, permissions and secrets

### 4.1 The network permission

Both bindings need outbound HTTPS and nothing else: Matrix talks to the homeserver
(client-server API, `/sync` long polls, media upload and download), Durable Streams talks
to its server (`PUT`, `POST`, `GET` with long poll; `require_tls` refuses `http://`, the
TLS spike's no-downgrade and no-plaintext rows). Neither needs LAN discovery, listening
sockets or a relay, which is the reason RFC-0001 option 2 ("relay-only via an explicitly
configured endpoint") is what these two bindings *are*.

What the repository does today, read from the manifests:

* `flatpak/org.tunaos.{tables,decks,letters}.json` finish-args: `--share=ipc
  --socket=fallback-x11 --socket=wayland --device=dri --filesystem=xdg-documents
  --own-name=org.tunaos.<app>` (Letters adds `--filesystem=/run/user`). **No `--share=network`.**
  `build-options.build-args: ["--share=network"]` is the build sandbox only; it is how
  crates are fetched in CI (`cargo clean && cargo build --locked --release -p tables`, no
  vendoring).
* `flathub/org.tunaos.*.json` are the submission prep (`cargo --offline`, a
  `cargo-sources.json` that is **not committed** and is generated by hand with
  `flatpak-cargo-generator.py Cargo.lock`, per `flathub/README.md`). Nothing has been
  submitted.
* The root `Cargo.lock` contains no network crate, so today **the shipped binaries cannot
  reach the network** and no sentence in the README, metainfo or `docs/` promises otherwise
  except RFC-0001 (lines 282 to 288: collaboration "off by default, with the permission
  requested only when a user starts or joins a session") and its conformance row
  ("manifest review plus a test asserting the default").

**Is there a runtime mechanism?** No. Flatpak permissions are static in the manifest
(`finish-args`) and the only runtime changes are the user's (`flatpak override --user
--share=network <app-id>`, or Flatseal). The portal that would let an app ask is the
"network permission" discussion RFC-0001 cites (xdg-desktop-portal #1166), which was a
discussion and not an implementation there; I searched for a newer one and found only the
`NetworkMonitor` portal, which reports connectivity and grants nothing. *Not verified
beyond that search.*

So RFC-0001's option 1 is not available as written. The options that are:

| | What ships | Consequence |
|---|---|---|
| **N1. Manifest has no network; collaboration works only after the user grants it** | `flatpak/*.json` unchanged. The `Share…` action is present in a build that has `collab`; when the first connection fails with a network error the dialog explains and shows `flatpak override --user --share=network org.tunaos.tables`. | Honours "no new permission until the user acts" literally. Flathub's permission list shows nothing new. A user must run one command or use Flatseal. A sandbox with no network shares only loopback, so a failure is a connect or DNS error and the dialog can say what to do. |
| **N2. Manifest gains `--share=network` in the build that has `collab`** | One line in `finish-args`. | The app's permission list on Flathub gains "Network"; the install prompt in GNOME Software shows it for every user, including the ones who never share anything. Flathub's current requirements say "static permissions must be kept to an absolute minimum" and portals become mandatory "when a suitable XDG portal exists" (none exists for generic outbound networking, as far as I could find). I did not find a rule that forbids `--share=network` (an archived 2023 copy of the wiki, seen only as a search snippet, listed it among permissions that "can be freely used"; I did not re-read the current text for it). |
| **N3. Wait for a portal** | Nothing. | RFC-0001 option 3; no date. |

**Recommendation:** N1 for Phases 2 and 3, and a **development bundle with N2** for the
evidence runs and for testers (a Flatpak bundle built in CI from a manifest copy with
`--features collab` and `--share=network`; it is a different artefact from the shipped
bundle, and the same app id on Flathub could not carry two permission sets). Whether the
shipped Flatpak ever gets N2 is a decision for the owner at the end of Phase 3 and is an
open question below. A test asserts the default (`tests/test_collab_defaults.py`: no
`flatpak/*.json` or `flathub/*.json` finish-args contain `--share=network`, and the root
`Cargo.lock` has no `tokio`, `reqwest`, `rustls`, `hyper`, `ring`; the second half is what
option B buys, see section 3).

Also relevant to Flathub: its requirements say there is **no network access during the
build** and every dependency must be in the manifest as a source. The CI manifests get
away with `build-args` because they are never submitted. A submitted manifest needs
`cargo-sources.json`, so lockfile growth is paid in the Flathub manifest ([section
3](#3-dependency-architecture-decided-by-measurement) has the size). Flathub's requirements
also contain a generative-AI disclosure policy for code and packaging (and say manifests
"must not contain AI-generated or AI-assisted content"); this repository's commit trailers
say how its code is written. That is not a collaboration question and this RFC does not
decide it, but a Flathub review of an app that gains a network stack would read it.

### 4.2 Where secrets live

There is no credential storage anywhere in the product today: a grep for `keyring`, `oo7`,
`libsecret`, `credential`, `token` and `password` in the product crates finds a formula-bar
word, XLSX sheet-protection hashes and one comment ("Never a credential",
`tables-core/src/snapshot.rs:48`). The lockfile has no `oo7`, `keyring` or `zbus`. RFC-0004
says "GNOME Online Accounts has no Matrix provider, so sign-in would be in-app with tokens
in the Secret Service". Settings are GSettings (`flatpak/org.tunaos.tables.gschema.xml`: 14
keys, none network- or collab-related); GSettings is the wrong place for a secret.

| Secret | Binding | Why it matters |
|---|---|---|
| Matrix access token, device id, user id | Matrix | A stolen token is an account takeover. `Credentials` is exactly these three fields (`dcp-matrix/src/store.rs`). |
| The matrix-sdk store (device keys, room keys) and its passphrase | Matrix | Losing it loses E2EE history: "a new device of an existing member reads none of it" (Spike A). `dcp-matrix` has a `sqlite` feature that **was not built or run in Spike A**; a persistent store is untested. |
| The Ed25519 seed (`Identity::from_seed`) | Streams | The key *is* the device (spec 9 rule 4): neither a recovery nor a rotation exists. |
| An optional `Authorization` header | Streams | The spec says a deployment must put access control in front of the server; `Http::with_header` is the hook. |

**Plan: `oo7` in the sidecar.** `oo7`'s documentation says it picks a backend by whether the
application is sandboxed: the host's Secret Service outside Flatpak, and inside it an
encrypted file keyed from a secret obtained through the `org.freedesktop.portal.Secret`
portal, so the secret is not exposed to other apps on the Secret Service bus. That needs
no manifest permission in the way a `--talk-name=org.freedesktop.secrets` would
(*the permission claim is from the documentation and was not tried here*). Putting it in
the sidecar keeps `zbus` and its tree out of the product lockfile, the same reason as
the network stack. The shell never sees a token: `Connect` carries a server URL, and
`Login` sends a password once and forgets it. Matrix store passphrase and Streams seed
are stored under the same keyring; `collabd` refuses to start a session when it can reach
no keyring (it does not fall back to a file in `$XDG_DATA_HOME`).

Streams identity and the owner's first-run flow: the key is generated on first use and
shown as a short fingerprint in the dialog ("Your key: ed25519:…, Copy"). A person on two
machines is two principals until a link statement exists (spec 9 rules 2 and 4), so
RFC-0001's first user need ("one person across two machines") is **not** met by Streams
without exporting the seed by hand; Matrix meets it by construction (one `@user:server`,
two devices) but with the history limit above. That is an argument about phase order,
discussed in section 7.

## 5. The UI slice in Tables

Rules that apply, from `docs/GNOME-GUIDELINES.md` and `docs/DESIGN-UI.md`:

* Every command is a `gio` action, disabled rather than refused, listed in the Ctrl+K
  palette, in Keyboard Shortcuts and in `docs/ACCESSIBILITY.md` (§6). Document-level
  commands go in the main menu in an unlabelled section after Save and Print; an item
  that opens a dialog ends in "…".
* One field is a prompt; two or more fields are an **action dialog** built by
  `suite_common::dialogs::action_dialog` with `form_body` rows (`adw::EntryRow`,
  `adw::ComboRow`, `adw::SwitchRow`); a form never goes inside an alert (§2).
  `tests/test_gui_guidelines.py` fails on `GtkDialog`, `MessageDialog`, and on `adw::Dialog`
  builders outside `dialogs.rs`, and checks header capitalisation of literals.
* Toasts report a finished action and never carry anything the user must act on; **a
  banner (`AdwBanner`) reports a lasting state of the document** (§7). The two-heads
  conflict is a lasting state.
* Colour is never the only signal (§8): the connection indicator needs text or an
  accessible label. Header-bar buttons are icons with a tooltip as the accessible name; the
  title stays centred and nothing wide is packed beside it (§1).
* DESIGN-UI's collaboration section: presence as an `AdwAvatar` row in the header bar,
  cursors as render nodes, and "an `AdwAlertDialog` 'Share' flow shows a join code, off by
  default". Presence is not in this slice (section 2.6); the join code is.

### 5.1 The slice

| Piece | Widget and pattern to reuse | Behaviour |
|---|---|---|
| **Collaborate** menu section | `suite_common::menus::section(&[(label, action)])`, inserted next to `sheet_actions::main_menu_section()` with `menu.insert_section(...)` (`tables/src/window.rs`, where `primary_menu(...)` is extended) | Items: `Share This Workbook…` (`app.collab-share`), `Join a Shared Workbook…` (`app.collab-join`), `Stop Sharing` (`app.collab-leave`, insensitive unless shared). |
| **Share dialog** | `dialogs::action_dialog("Share This Workbook", "_Share", 420, &form_body(...))`. Rows: `ComboRow` "Service" (Matrix, Durable Streams); Streams: `EntryRow` "Server", `EntryRow` "Access header" (optional), `EntryRow` "People (public keys)"; Matrix: `EntryRow` "Homeserver", `EntryRow` "Matrix ID", `PasswordEntryRow` "Password" (first login only), `EntryRow` "Invite (Matrix IDs)". A row that does not apply is insensitive, not hidden (§2). | Sends `Connect` then `Share`; the first version is the current workbook saved to memory with `save_sheets_to_xlsx_bytes`. On success an `AlertDialog::new(Some("Sharing Started"), Some(code))` shows the join code with a Copy response (the pattern at `persistence.rs:119`). |
| **Join dialog** | the same builders. Rows: `ComboRow` Service, `EntryRow` "Join code"; for Streams a read-only `EntryRow` "Your key" with a copy button so the owner can add it before sharing. | Sends `Connect`, `Join`; a joiner who has no workbook open gets a new window (`app.new-document` path) holding the shared workbook; one that has one open is asked (the `save_changes_question` pattern) before it is replaced. |
| **Status** | One icon-only `gtk::MenuButton` in the header bar's end box beside the main menu, accessible name "Sharing: online, 2 people" built from `Event::State` and `Roster`, tooltip the same, popover with the roster, "last update from <name>", `Copy Join Code`, `Stop Sharing`. Icons `network-transmit-receive-symbolic`, `network-offline-symbolic`, `dialog-warning-symbolic`, plus the text. | It is the only always-visible trace of a session. Hidden entirely when no session exists. Tables' status line (`stats_label`, caption, dim-label) is not reused: it shows the selection. |
| **Conflict banner** | `adw::Banner::builder().button_label("_Resolve…").action_name("app.collab-resolve")`, added with `suite_win.add_top_bar(&banner)` (`window.rs`, as the formula bar `fx_bar` is); precedent: `decks/src/master_view.rs:34` (`set_title`, `set_revealed(true)`). | Revealed on `Heads { conflict: true }` with an uncovered head (2.5). `app.collab-resolve` opens the three-way `AlertDialog` of 2.5 (`add_response` / `set_default_response` / `set_close_response` / `.choose(...)`). |
| **Toasts** | `ToastOverlay` already created in the window (`window.rs:466`); `sheet_actions::toast(...)` | "Sharing started", "Joined <title>", "Saved and shared". Failures that need action (cannot reach the server, key missing, role is viewer) are alerts, not toasts. |
| **Read-only for a viewer** | the existing `refuse_locked` pattern in `sheet_actions.rs` | A `viewer` role (spec 5) disables editing actions; **whether `WorkbookController` has a read-only mode was not checked** and is an open question. |

### 5.2 Files to touch

`tables/src/window.rs` has a **2,300-line ceiling** (`scripts/release_gate.py`
`module_ceilings`) and is at **2,164**; `ROADMAP.md:74` records both and
`tests/test_roadmap_figures.py` checks the table against the file. So 136 lines of headroom
and the rule is that new code goes in modules, with at most about twenty lines in the window.

| File | Change | Size (estimate) |
|---|---|---|
| `tables/Cargo.toml` | `[features] collab = ["tables-core/collab", "dep:suite-collab"]`, off by default; `suite-collab` optional | 6 lines |
| `tables/src/main.rs` | `#[cfg(feature = "collab")] mod collab_actions; mod collab_dialogs; mod collab_status; mod collab_banner; mod collab_session;` | 6 lines |
| `tables/src/collab_session.rs` (new) | Owns the `gio::Subprocess`, the `SessionClient`, the `Replica`; the event loop on the main context; calls `controller.apply_remote`, `refresh_sel()`, `sheet_bar::follow`, `grid_area::refresh_grid_a11y`, `queue_draw` (the four calls the undo action already makes) | ~250 |
| `tables/src/collab_actions.rs` (new) | `install(app, &controller, &drawing_area, &toast_overlay, refresh)` in the shape of `sheet_actions::install`; `main_menu_section()` | ~120 |
| `tables/src/collab_dialogs.rs` (new) | Share, Join, join-code and Resolve dialogs on `dialogs::action_dialog`/`form_body` | ~220 |
| `tables/src/collab_status.rs`, `collab_banner.rs` (new) | The status button and the banner | ~90 and ~40 |
| `tables/src/window.rs` | one `#[cfg(feature = "collab")]` block: `collab_actions::install(...)` and the `insert_section` call, next to the `sheet_actions` ones; the close-request guard asks before closing a window with an open session | ≤20 lines, then update `ROADMAP.md`'s row |
| `tables/src/persistence.rs` | `collab::after_save(bytes)` after `locations::commit_save` in `save_engine_to_xlsx` (all three save routes funnel through it, TBL-001) | ~6 lines |
| `tables-core/src/controller/{core,ops}.rs` | `apply_remote(ops)`: like `apply_ops` without `record`; `clear_history_for(sheet)` | ~60 |
| `tables-core/src/controller/collab.rs` | `version_bytes`, `export_since`, `export_snapshot`, `import_updates`, events | ~120 |
| `suite-common-core/src/ops.rs` | `History::step`: drop the failed step only; proptest | ~40 + tests |
| `po/POTFILES` | the new modules with `i18n(...)` strings (`scripts/release_gate.py` fails on a listed file that does not exist and `msgfmt --check` runs on the `.pot`) | 5 lines |
| `docs/ACCESSIBILITY.md`, the shortcuts window | the four actions | few |

Not touched: `suite-common/src/lib.rs` (1,250-line ceiling, shared by three apps) until
Decks needs the same status button, at which point the button moves there as one
function.

## 6. Tests and evidence

### 6.1 What stays below the GUI

`dcp-conformance` (`tools/collab-spike/dcp-conformance/src/journeys.rs`, driven by
`runner.rs`' `JOURNEYS` table) holds spec section 12 written once against `DocumentStore`
and runs against every binding. These stay there, unchanged, in `collab-spike.yml` today and
in the promoted workspace's CI after PR 2:

| Spec 12 test | Journey function | Level it stays at, and why |
|---|---|---|
| 3 version complete | `corrupt_file_rejected`, `truncated_upload_never_published` | store: the app never sees a bad version; `view::build` drops it. |
| 4 lease exclusion, 5 lease expiry | `lease_exclusion_and_readback`, `lease_expiry_releases` | store. **The apps do not use leases:** a Loro session has none (edits merge). Leases return only for an app or mode without a CRDT. |
| 6 roles enforced | `roles_enforced_at_store`, `forged_version_from_a_viewer_is_not_accepted` | store (spec: "at the binding, not only in the UI"); the GUI journey adds only "a viewer cannot edit". |
| 7 removed members lose new content | `revocation_hides_new_content` | store (needs a real second principal and its control). |
| 8 unknown algorithm falls back to versions | `unknown_algorithm_falls_back_to_versions` | store, plus one GUI step: a peer publishes `algorithm: other/1`, Tables shows the banner, not a half result. |
| 9 stream gap recovered | `update_stream_gap_recovery` | store; the shell-visible half (`Gap` event, snapshot reload) is a journey in Phase 2. |
| 10 WOPI | — | not an app journey. |
| 11 schemas | `python3 docs/collab-spec/validate.py` | stays a Python check. |

Those that become **GUI journeys** (the spec's own wording is "two app instances; both show
the same head and content"):

| Spec 12 test | GUI journey (new, `tests/gui/test_collab.py`) |
|---|---|
| 1 convergence | `TablesCollabStreams.test_two_instances_converge`: A shares, B joins, A types in a cell, B's grid shows it (AT-SPI read of the cell, `grid_area::refresh_grid_a11y`); B types a different cell; both grids equal; both status buttons read "online". |
| 2 a second head is shown, not hidden | `…test_second_head_is_shown_and_resolved`: stop the server, edit and save on both sides, restart it; both show the banner; "Keep mine" on A, then B's banner clears and both hold A's content. |
| RFC-0001 table rows | `…test_undo_is_mine` (A types, B types, A undoes: only A's cell reverts); `…test_delete_wins_over_move`; `…test_offline_divergence_merges` (the first RFC-0001 evidence row it can finally meet: disconnect, edit both, reconnect, equal). |
| RFC-0001's last row | `tests/test_collab_defaults.py` (Python, in the `model` layer's `tests/test_` prefix): shipped manifests have no `--share=network`, the root lockfile has no network crate. |

### 6.2 Running two Tables against real servers

**Servers.** `collab-spike.yml` already starts both on the hosted `ubuntu-24.04` runner and
CI-verifies them: Synapse by `scripts/synapse-up.sh relaxed|default` (docker; the image is
`matrixdotorg/synapse:latest`, the `q15` job pins `v1.163.2@sha256:c3c21919…`), Durable
Streams by `scripts/streams-up.sh node` (the Node reference server 0.3.9 from
`scripts/streams-server/package.json`, `npm ci`, port 4437) or `streams-up.sh caddy`
(the Go server release v0.3.0 by pinned checksum, port 4438), and TLS by
`scripts/streams-tls-up.sh`. `gui-tests.yml`'s `smoke` job runs on the host runner (not the
container), so docker and node are there; it is the **required** check
(`conformance/required-checks.json`), so the collaboration journeys go in a **new job**
(`collab-gui.yml`, path-filtered to `tables/src/collab*`, `suite-collab/**`, `collab/**`,
`tests/gui/test_collab.py`), classified `optional` there because a path-filtered check
cannot be required (the spike jobs say the same). Every job a pull request can start must
be classified in that file or `conformance/required_checks.py` fails.

**Two instances of one app.** The harness cannot do it today, and the reasons are in the
code, not guessed:

* `launch_second_app(self, app_name, launch_args=())` (`tests/gui/framework/base.py:1139`)
  takes no environment override, so the second process shares the first's XDG
  directories (its settings, autosave state and, here, collab keyring), and
* its first statement is `_terminate_owned(app_name)`, which kills every live
  harness-launched process of that name: **launching a second `tables` kills the first**;
* the apps are `adw::Application`s with flags `HANDLES_OPEN` only
  (`suite-common/src/lib.rs:88-91`), not `NON_UNIQUE`, so [INFERENCE] a second process on
  the same session bus would forward to the first instead of starting. A separate
  `dbus-run-session` per instance would also give it its own AT-SPI bus, which dogtail
  would not see. *The inference was not tried.*

The plan: (1) `launch_second_app(..., env=None, same_name=False)` that leaves the first
instance alone when asked and passes `XDG_DATA_HOME`/`XDG_STATE_HOME`/`GSETTINGS_BACKEND=memory`
overrides; (2) an `ApplicationFlags::NON_UNIQUE` switch **in test mode only**
(`GTK_OFFICE_TEST_MODE` already exists and is set by `isolate_snapshot`), so two instances
share one bus and one AT-SPI tree and `wait_for_app(name, pid=…)` (which exists) tells them
apart. (3) Until (1) and (2) land, **the second user is a headless peer** (a small binary
in the `collab/` workspace that drives `collabd` and a `Replica` and edits a cell on
command), so Phase 2's journey needs one GTK instance and no harness change. A second GUI
instance is the Phase 2 exit; the headless peer is how it is first made to pass.

**Evidence.** `tests/gui/run_gui_tests.sh` starts a private Xvfb, `matchbox-window-manager`,
`dbus-run-session` with toolkit accessibility on, then `pytest` (dogtail/AT-SPI over
`xdotool`). Video is `GUI_TEST_VIDEO=off|failures|all` with `GUI_TEST_VIDEO_DIR`: ffmpeg
`x11grab`, one `<Class>.<method>.mp4` and a `.json` sidecar with the outcome and
`$GITHUB_SHA`. The smoke gate keeps only failures' videos; a PR labelled `verify-video` runs
`feature-verification.yml` (journeys in the test container, GIFs, `summary.md`,
`evidence.json`, clips pushed to the orphan `ci-evidence` branch and posted on the PR). The
rule is in `AGENTS.md` ("a clip is evidence about a result, an assertion is the result")
and `docs/CI-VIDEO-EVIDENCE.md`. **Nothing in `capabilities.json` or
`validate_capabilities.py` mentions video**: the ledger holds test ids and a revision.

### 6.3 Ledger entries

`conformance/capabilities.json` entries have `id`, `app`, `issue` (int), `title`, `scope`,
`status`, `revision`, `requires` (layers), `evidence` (layer to test ids). The validator
enforces C1 to C9: every required layer has evidence, a cited id exists in the inventory a
lane collected (C3), a skipped or failed test cannot support `verified` (C4), a `deferred`
entry needs a waiver, and `lanes.json` says which id prefixes each layer may cite (a core
unit test filed as `gui` is refused). Entries this plan needs, none to be added before its
journey has passed at a recorded revision (spec 12 says the same):

| Entry id | requires | Evidence it would cite |
|---|---|---|
| `tables.collab.replica-converges` | `model` | `tables-core::controller::collab_tests::two_peers_editing_at_once_converge_and_equal_a_replay` (exists), `…::delta_updates_equal_a_replay` (PR 1), `tables-core::determinism::…` |
| `tables.collab.undo-is-mine` | `model`, `gui` | `suite-common-core::ops::tests::remote_ops_drop_only_the_steps_they_break` (PR 1), `tests/gui/test_collab.py::TablesCollabStreams::test_undo_is_mine` |
| `tables.collab.streams-two-instances` | `bridge`, `gui` | `tables::bin/tables::collab_session::tests::…`, `tests/gui/test_collab.py::TablesCollabStreams::test_two_instances_converge` |
| `tables.collab.two-heads` | `gui` | `…::test_second_head_is_shown_and_resolved` |
| `tables.collab.matrix-two-instances` | `bridge`, `gui` | Phase 3 equivalents on a real Synapse |
| `tables.collab.off-by-default` | `model` | `tests/test_collab_defaults.py::…` |

Two things have to change in the lanes before any of these can be `verified`:

* **The `collab` unit tests are invisible to the ledger.** They run in a separate
  `ci.yml` step with `--features collab`, not in the nextest run whose JUnit
  (`collect_test_inventory.py --junit target/nextest/ci/junit.xml`) is the inventory. PR 1
  adds `cargo nextest run -p tables-core -p decks-core --features collab` to that lane (or
  a second JUnit merged into the inventory) so their ids are collected.
* **A new lane in `lanes.json`** for `tests/gui/test_collab.py`, declaring
  `runs_on_every_revision: false` and, until the capability is release-critical,
  `runs_on_candidate: false`; a release-critical claim may not rest on such a lane
  (SH-001), which is the right answer for an experimental feature.

## 7. Phases, each abandonable

Each phase has an exit that is a recorded, repeatable result, and can be abandoned there
without leaving the tree worse (the `collab` feature is off, and with B the default
lockfile and binaries are untouched throughout). Effort figures are my estimates of
focused work for one person who knows the code, not measurements; risk is where I expect
to be wrong.

| Phase | What | Exit evidence | Effort | Risk |
|---|---|---|---|---|
| **P1: core glue and a headless two-instance test (no GTK)** | `History` per-step drop; `Replica` delta export, snapshot, import and events; `loro/1` + `tables/1` profile text; `suite-collab` (types, codec, `SessionClient`); `collab/` workspace (the four DCP crates promoted, `collabd` binary); `collab-apps.yml` | In CI: two `Replica`s and two `collabd` processes converge **through a real Durable Streams server** (Node 0.3.9, TLS on), including offline divergence and a merge version; the same test with a deliberately wrong `loro/1` base rule fails; `cargo nextest` collects the `collab` tests; default lockfile and `cargo build --locked` of the apps byte-identical to `main` (`cmp`). | 2 to 3 weeks | The profile's base-version rule (2.3 point 6) is a spec change, not code; the events→ops mapping is designed, not measured. |
| **P2: Tables UI slice, Streams binding** | Section 5, Streams only; the dev bundle with `--share=network`; the harness change (or the headless peer) | `tests/gui/test_collab.py` journeys 1, 2 and the undo and offline ones pass on the runner **with video** (`verify-video`), entered in `capabilities.json` at that revision; the `off-by-default` test passes; `window.rs` ≤ 2,300 | 3 to 4 weeks | Applying remote changes through `apply_remote` without redraw glitches; the AT-SPI reads of a custom-drawn grid (`grid_area.rs` already exposes cells); the second-instance harness. |
| **P3: Matrix binding in the same UI** | `Login` (password; SSO/OAuth is a follow-up), `oo7`, the persistent store, invite handling (wait until the SDK can name the inviter before joining), batching for default Synapse limits | The same journeys against a real Synapse (`default` limits, not only `relaxed`) with E2EE; a removed member's next update unreadable (the store journey 7, repeated through the app); token and store in the keyring, none on disk in the clear | 3 to 4 weeks | The `sqlite` store was never run (Spike A); Matrix's 0.2 events/s default makes live typing seconds-slow; login UX. |
| **P4: Decks** | `doc_model: decks/1` over the same sidecar; the same five UI modules in `decks/src` (the status button moves into `suite-common`) | Decks journeys on both bindings: concurrent delete and move leaves the object deleted on both (RFC-0001 Phase 3's own journey); `window.rs` (Decks: 1,800 ceiling) | 3 weeks | Rebuilding a deck gives fresh local ids (decks `collab.rs` known limit), so undo clearing is wider; masters are not replicated. |
| **P5: Letters** | `letters-collab`'s `Text`+marks encoding rebased onto the live model; `doc_model: letters/1` | RFC-0001's Letters journeys: two people typing in one paragraph; a bold at the end of a run expands as the mark rules say (the branch's last commit records Loro's actual behaviour for a concurrent insert at a mark's end); tables are refused with a message | 6+ weeks | The largest: offsets between `GtkTextBuffer` and the live model under remote edits; tables, footnotes, headers are not replicated by the branch. |

### First three pull requests

1. **`core: per-step undo drop and Replica deltas`.** `suite-common-core/src/ops.rs`
   (`History::step` drops the failed step, keeps the rest; a proptest that interleaves
   remote ops), `tables-core/src/controller/collab.rs` (`version_bytes`, `export_since`,
   `export_snapshot`, `import_updates`; tests `delta_updates_equal_a_replay` and a
   snapshot-genesis test: two replicas that join through a snapshot, not through `fork`,
   converge), `ci.yml` (collab tests into the nextest lane). No new dependency, no shell
   change, no behaviour change with the feature off. Exit: nextest's JUnit lists the
   `collab` tests.
2. **`collab: sidecar workspace, protocol crate, loro/1 profile and a headless test over
   a real Durable Streams server`.** `collab/` (own `[workspace]`, own `Cargo.lock`, the
   four crates copied from `tools/collab-spike` with the `include_str!` paths fixed: they
   are four `../` from `src/` today and would be three), `suite-collab/` (product
   workspace member: `serde`, `serde_json` only), `docs/collab-spec/profiles/loro-1.md`,
   `.github/workflows/collab-apps.yml`, a line in `required-checks.json`, `deny.toml`
   for the new lock (an `[advisories]` run of `collab/` with its own ignores: today's
   `anymap2` is one), `tests/test_collab_defaults.py`. Exit: the P1 exit evidence.
3. **`tables: Collaborate menu, Share and Join, status button and banner (Streams,
   behind collab)`.** Section 5.2's files; `tests/gui/test_collab.py` with the headless
   peer; a dev-bundle job that builds the Flatpak manifest copy with the feature and
   `--share=network`. Exit: journeys 1 and 2 with video on a `verify-video` run; no
   ledger entry until that run's revision is the one entered.

## What this changes for the owner's questions

* **"Is the file still the document of record?"** Yes: Ctrl+S is the existing atomic save;
  a shared session adds a published *copy* of what was saved; no format changes.
* **"Does the default build change?"** With B, not at all (`Cargo.lock` unchanged,
  asserted by test); with A, it does (section 3).
* **"What does it cost the user who never shares?"** While `collab` is off in the shipped
  Flatpak: nothing (no binary, no permission). Once it ships: one extra binary in the
  Flatpak (section 3.4), no permission under N1, no process until `Share` or `Join`.

## Not in scope

* **Access control beyond what the bindings give.** Roles are the bindings' (power levels,
  the signed roster); document-level permissions, revocation UX, Keyhive: RFC-0001 keeps
  them out until something stable exists.
* **Presence, cursors, selections, avatars** (L3): no binding has an ephemeral channel
  (section 2.6).
* **Compaction and history sidecars** (RFC-0001 Phase 4), named versions, a timeline:
  `covers` is written, nothing is discarded; the room and the log grow.
* **Leases in the apps.** A Loro session has no lock. Edit locks return only for a mode
  without a CRDT.
* **Comments, suggestions, @mentions.**
* **Matrix federation, OIDC account creation and registration**: the sidecar logs in with
  an existing account; it does not create one (`dcp-matrix/src/synapse.rs`' registration is
  a test helper over Synapse's shared secret).
* **iroh/p2panda, Nextcloud/WebDAV and WOPI** in the apps (the owner's decision).
* **Key backup, per-device keys, rotation, linking two keys into one person** (spec 9).
* **A runtime permission prompt** (no portal; section 4.1).
* **Any web or mobile client.**
* **Moving `tools/collab-spike` anywhere**: PR 2 *copies* the four crates and leaves the
  spike's workspace, lockfile and CI as they are, so the spike results stay reproducible.

## Open questions

1. **The `loro/1` base-version rule** (2.3 point 6). `Receiver` skips an update whose
   `base_version` differs from the loaded one; for Loro that drops valid edits. The fix is
   either profile text ("the session base is version zero") or a hook in `Receiver`; this
   is a spec-draft change that Phase 1 forces and the spec's author should rule on.
2. **Is one person on two machines still the first user need?** RFC-0001 decision 5 says
   so. Matrix meets it by construction; Streams meets it only by copying a seed (spec 9
   rule 4). If yes, Matrix should not wait for Phase 3.
3. **Should the shipped Flatpak ever carry `--share=network`** (N2), or stay N1 forever?
   Owner decision at the end of Phase 3; it has Flathub-review consequences.
4. **Does `WorkbookController` have, or need, a read-only mode** for the `viewer` role?
   Not checked.
5. **Who patches `ruma-common`?** Option B avoids the E0275 failure only because the
   sidecar's graph has no `zerovec` with `serde`; if the sidecar ever has to share a
   graph with the apps (option A later), this is the blocker. I did not test whether a
   newer `ruma-common`, a `[patch.crates-io]` (which `docs/DEPENDENCIES.md` makes a
   documented, pinned exception) or a `recursion_limit` upstream fixes it.
6. **Do the six Loro advisory ignores in `deny.toml` block shipping `collab`?** They say
   "resolve before collab ships (#1043)"; nothing in this RFC changes that, and the gate
   is `pending` on `main` already.
7. **Startup of the sidecar in a Flatpak** (`/app/libexec`, `gio::Subprocess`) and
   whether a second process in the sandbox is acceptable to Flathub review. *Not tried.*
8. **Generative-AI disclosure.** Flathub's requirements ask for it, and the repository's
   code is largely agent-written. Not a collaboration decision, but a Flathub submission
   that adds a network stack makes it unavoidable to decide.

## Not verified

Everything in this list was *not* observed in a run, a build or a test:

* That `Replica`'s Loro events can be mapped to `Op`s as 2.4 describes: designed from
  `collab.rs`, no code written.
* That a shell can drive `gio::Subprocess` + `read_line_async` to a sidecar at the rate
  of edits (nothing was built against GTK).
* Anything about the **GUI** of a collaboration session: no widget was written, no AT-SPI
  journey was run, no video was recorded. Section 5 is a design from the existing widgets.
* The second-instance harness changes of 6.2, including the `NON_UNIQUE` inference.
* `oo7` inside a Flatpak with the Secret portal (documentation only), a persistent
  matrix-sdk store (`dcp-matrix`'s `sqlite` feature was **not run** in Spike A; here it was
  compiled in the A experiment only), Matrix login, SSO/OAuth.
* That `--share=network` is acceptable to Flathub review (no rule found either way) and
  whether the Secret portal needs a permission; the `/.flatpak-info` approach to
  detecting the missing permission.
* Latency and throughput over a **real network**: all spike numbers are loopback, one
  runner. Default-Synapse live typing is derived from the measured 0.26 events/s, not
  run through an app.
* The Streams binding's behaviour behind an access-control gate that follows the roster
  (spec question 12: not built).
* Windows and macOS: not a target; nothing was checked.
* That A's failure is not fixable with a patch: only the unpatched failure was run.

## Evidence: CI runs

Branches (throwaway, never to be merged; they exist so the numbers can be audited and the
runs re-read): `plan/collab-in-apps-exp-a` and `plan/collab-in-apps-exp-b` on
`tuna-os/gtk-office-suite`. The workflows `zz-exp-a.yml` and `zz-exp-b.yml` are on those
branches only; the scripts are in `tools/collab-spike/scripts/rfc0005/`.

| Run | Branch @ commit | What it ran | Conclusion |
|---|---|---|---|
| [40224533624](https://github.com/tuna-os/gtk-office-suite/actions/runs/40224533624) `zz-exp-a` | exp-a @ `4b71a7b0` | `lock`: lockfile, unit graph, `cargo tree`, licences, `cargo deny`, `cargo fetch` size, `flatpak-cargo-generator` | **success** (the numbers of section 3.1) |
| same | same | `compile`: cold builds of A and B on 4 vCPU | every build step `exit=0` (timings in 3.1); the **job is red** only because its artifact upload refused a file name containing `:`; the numbers are read from the job log |
| same | same | `flatpak` x4: letters, decks, tables (default features) and tables with `--features collab-matrix,collab-streams` in the `gnome-50` container | letters **success**, decks **success**, tables **success**; tables + collaboration features **failure**: `E0275` in `ruma-common 0.20.2`, `room.rs:139:44` |
| [40224541589](https://github.com/tuna-os/gtk-office-suite/actions/runs/40224541589) `ci.yml` (dispatched on exp-a) | exp-a @ `4b71a7b0` | the repository's own CI | `advisories` **failure** (RUSTSEC-2026-0319, `anymap2`); `test` **failure** (the same `E0275`, in clippy and in `test --no-run`); `python-checks` **failure** (`test_c3_no_core_crate_needs_gtk_with_default_features`, 1 failed 583 passed); `interop-corpus` success; `flatpak` skipped (it `needs: [test]`) |
| [40224817768](https://github.com/tuna-os/gtk-office-suite/actions/runs/40224817768) `zz-exp-a` | exp-a @ `ed4c71e5` | `size`: Loro workbook sizes, `cargo test --release -p tables-core --features collab` | **success** (1 passed; table in 3.7; the test body took 1,996 s, nearly all of it, I infer, the engine filling 100,000 cells rather than Loro, whose own steps are the millisecond columns) |
| [40225971450](https://github.com/tuna-os/gtk-office-suite/actions/runs/40225971450) `zz-exp-b` | exp-b @ `a05fd6b1` | `lock`: product lock identical to `main`, unit-graph identity, sidecar lock, `cargo deny` on it, `cargo-sources.json`; `flatpak`: tables with the sidecar | **success** both. `cargo deny` itself exited 1 on the sidecar lock (`anymap2`); the script records it without failing the step |
| [40228434561](https://github.com/tuna-os/gtk-office-suite/actions/runs/40228434561) `zz-exp-b` | exp-b @ `aa5242a1` | `flatpak` base and sidecar, bundle sizes | **success** both |

The spike's own results (the Spike A, Spike B, TLS and relay documents) are not re-argued
and not re-run here; this RFC cites them by file.
