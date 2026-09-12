# RFC-0001: CRDT collaboration for Letters, Tables and Decks

Date: 2026-09-12 · Status: **draft, not accepted** · Tracks: [#544](https://github.com/tuna-os/gtk-office-suite/issues/544)

## Summary

This proposes a design for offline-first collaborative editing across the
three apps, and argues that the first work item is **not** networking, a CRDT
library, or a sync protocol. It is giving Letters a live GTK-free document
model. Everything else is blocked behind that, and the blocking is visible in
the code today.

It also argues for starting with **Tables**, not Letters — the opposite of the
order a Google-Docs-shaped roadmap would suggest — because Tables already has
the model a CRDT needs and Letters does not.

## Why this is a draft and not a plan

The current execution plan is
[Roadmap to dependable daily use](../readiness-2026-09/README.md) ([#443](https://github.com/tuna-os/gtk-office-suite/issues/443)),
which says plainly:

> collaboration, plugin marketplaces and enterprise policy systems do not
> block dependable everyday editing.

Nothing here should be read as competing with that. An RFC is cheap; shipping
a sync protocol on top of an editor that still has open crash and save-safety
work would be expensive in the way that is hard to undo. What this document is
for is making sure that when collaboration *is* picked up, it is not designed
around a model that cannot carry it — and that the prerequisite work is
recognised as prerequisite rather than discovered late.

## Constraints this design has to satisfy

These come from the repository, not from the literature.

1. **Core crates are GTK-free.** `letters-core`, `tables-core`, `decks-core`
   and `suite-common-core` must stay buildable and testable without GTK
   headers (ADR-0001). Replication state belongs in a core crate; only the
   transport wiring and presence UI belong in the shells.
2. **The file is the document of record.** The suite's entire premise is
   measured, ratcheted parity with ODF/OOXML, verified against headless
   LibreOffice (ADR-0001, `docs/PARITY.md`). A design in which the real
   document is a CRDT and `.odt` is an export would discard that premise.
3. **Saving is atomic and whole-buffer.** `suite-common-core::atomic_save`
   takes a complete in-memory buffer and writes it atomically
   ([#437](https://github.com/tuna-os/gtk-office-suite/issues/437)).
   Collaboration must not introduce a partially-written document.
4. **Undo is per-user and intention-preserving.**
   `suite-common-core/src/undo.rs` is a Command-pattern stack
   (`apply`/`undo`/`description`). Commands are not commutative and the stack
   is not an operation graph; a naive "undo = apply inverse op to the CRDT"
   would let one user's undo revert another's work.
5. **Flatpak is the ship vehicle**, with the sandbox constraints in
   [Transport](#transport-and-the-flatpak-problem) below.
6. **Done means journeys.** In this repository a feature is done when
   current-revision tests prove the behaviour — for GUI behaviour, AT-SPI
   journeys recorded on video, entered in `conformance/capabilities.json`.
   A CRDT merge that no journey exercises is not evidence.
7. **New dependencies are a release-gate concern.** Builds run
   `cargo build --locked`, reproducibility is a release goal, and patched or
   forked dependencies must be pinned and documented (`docs/DEPENDENCIES.md`).

## The blocking finding: Letters has no live GTK-free model

`letters-core/src/model.rs` defines a proper GTK-free `Document` —
paragraphs of styled runs — and its offsets deliberately match
GtkTextBuffer's:

> Offsets are global character offsets; each paragraph break counts as one
> character, matching GtkTextBuffer's offset model exactly.

But that model is a **serialization** form, not the live editing state. The
bridge is a whole-document round trip:

```rust
pub fn capture_from_buffer(buf: &gtk::TextBuffer) -> Document
pub fn render_to_buffer(doc: &Document, buf: &gtk::TextBuffer)
```

The model appears in the app only at boundaries — save, snapshot, and crash
recovery, where `letters/src/window.rs` deserializes a snapshotted `Document`,
renders it into a fresh buffer and drops it. Between those boundaries the
`GtkTextBuffer` is the editing state; no live `Document` is held.
`letters-core/src/session.rs` says so in as many words, and names the gap:

> Letters' actual document *content* model is a GtkTextBuffer (rich text +
> formatting tags), which has no GTK-free representation today — that's a
> larger design question (mirror buffer state into a GTK-free AST kept in
> sync, or accept GtkTextBuffer itself as the content boundary).

A text CRDT needs per-edit operations against a stable sequence. A
whole-document capture per keystroke is not that: it is O(document) per
character, and worse, it destroys the information a CRDT exists to use —
*which* characters a user inserted where, as opposed to what the document
looked like afterwards. Two users typing in different paragraphs would produce
two full documents whose merge is a guess.

So: **Letters cannot be the first app to get collaboration**, and the
prerequisite is the design question `session.rs` defers. That is a substantial
piece of work in its own right (mirror the buffer into a live AST with change
signals, or move the editing surface onto a model-backed widget) and it should
be scoped and justified on its own merits — better editing, better
accessibility, better testability — rather than smuggled in as sync plumbing.

The offset decision above is a real asset when that work happens: a text CRDT
indexed on the same offset model maps to buffer positions without translation.

### Tables and Decks are in a different position

| app | live GTK-free model | natural CRDT shape |
|---|---|---|
| **Tables** | `tables-core/src/sheet.rs` + `controller/`, with `snapshot.rs` | map keyed by `(sheet, row, col)` → cell; last-writer-wins per cell is already the semantics users expect of a spreadsheet |
| **Decks** | `decks-core/src/controller.rs`, `snapshot.rs`, slide objects with geometry and z-order | movable tree (slides → objects), plus per-object LWW registers for geometry |
| **Letters** | serialization only (above) | sequence CRDT with formatting marks — blocked |

This is why the recommended order is Tables → Decks → Letters. Tables is also
where the semantics are least contentious: a cell is a small independent unit,
and "last writer wins per cell" is both implementable and explicable.

Tables does add its own hard problem: the formula engine. Two users editing
different cells can each leave the sheet consistent while the *merged* sheet
needs recalculation, and recalculation has to be deterministic across peers or
they will disagree about displayed values while agreeing about content. The
engine is already GTK-free and already has a recalculation entry point, so the
design question is where recalculation is triggered after a merge and whether
computed values are replicated at all (proposal: no — replicate inputs,
recompute locally, and treat a divergence in computed values as a bug in the
engine's determinism, which is testable without any networking).

## Where replication state lives

Three options for the relationship between the CRDT and the file.

**A. CRDT is the document of record; ODF/OOXML becomes export.**
Rejected. It contradicts constraint 2 and would make every parity measurement
in `docs/PARITY.md` a statement about an export path rather than about the
document users keep.

**B. File is the record; the CRDT is session-scoped.** A session starts from
the file, peers exchange operations while connected, and the result is written
back through the existing atomic save. The CRDT is discarded when the session
ends. Simple, no new durable state, no garbage collection — and no offline
divergence: two people who both edit the same document while disconnected
still have a conflict the system cannot merge.

**C. File is the record; CRDT history is a sidecar.** The operation history
lives beside the document, keyed to it, the way crash-recovery snapshots
already do (`suite-common-core::autosave`, per-app `snapshot.rs`). Offline
divergence merges. Costs: history growth and compaction, and a real question
about what happens when another application edits the file.

**Recommendation: B first, C as the target**, with the sidecar introduced only
once a journey demonstrates the offline-divergence case it exists for.

The sidecar's sharpest open question is one the local-first literature mostly
does not have to answer, because research systems generally own their format:
**what happens when LibreOffice edits the file behind our back?** The sidecar
is then a history of a document that no longer exists on disk. The proposal is
to store the file's content hash with the sidecar, treat a mismatch as "the
file is authoritative and the history is stale", and surface that to the user
rather than silently discarding either side — the same posture the suite
already takes toward unsupported content and lossy saves
(`CompatibilityReport`).

## Library choice: measure, don't pick from a table

All three serious candidates are Rust-native or Rust-cored, which matters here
because the cores must stay GTK-free and the shells are Rust.

**Automerge** — JSON-like CRDT with a Rust-first implementation and a `marks`
API built for rich text, descended from the lab's own
[Peritext](https://www.inkandswitch.com/peritext/) work on preserving
formatting intent across concurrent edits. Marks carry expand-on-insert
semantics per mark type (bold expands at a boundary, a hyperlink does not),
which is exactly the distinction Letters would need. One sharp caveat for a
Rust-only product: `automerge-repo-rs`'s on-disk layout and WebSocket sync
protocol are **not** compatible with the JavaScript `automerge-repo`, with a
compatible Rust implementation still experimental (`alexjg/samod`). If we only
ever talk to ourselves that is irrelevant; if interoperating with a web or
mobile client is ever wanted, it is a commitment.

**Loro** — Rust implementation built on a replayable event graph, with
`Text` (Peritext-style rich text), `MovableList`, `LWW Map` and a
**`MovableTree`** that neither Automerge nor Yjs exposes natively. It
implements Fugue, which is designed to minimise interleaving when concurrent
insertions meet. The movable tree is a direct fit for Decks' slide/object
hierarchy, and the published encoding claims are favourable.

**Yjs / `yrs`** — the most widely deployed of the three and very fast, but its
gravity is in the JavaScript ecosystem; for a native Rust desktop suite that
is a weaker argument than it looks.

This RFC deliberately does **not** pick one. The repository's standard is
measurement over assumption, and the decision should be made by a spike with
stated criteria, run against our own data rather than someone's benchmark:

1. replay a recorded Tables editing session (the stress harness can produce
   one) through each candidate and measure document size and merge time;
2. the same for a Decks deck with a deep object tree, specifically testing
   object moves and z-order, which is where the tree CRDTs differ;
3. for Letters (later), whether the marks model can express our `RunStyle`
   without losing the distinction between styles that should and should not
   extend when text is typed at their boundary;
4. build-cost and dependency-surface impact, since this lands in crates that
   must keep building with `--locked` in the Flatpak sandbox.

A spike that cannot be completed is itself a result worth having.

## Transport, and the Flatpak problem

[**iroh**](https://docs.iroh.computer/what-is-iroh) reached 1.0 on 2026-06-15
("dial keys, not IPs"): QUIC connections addressed by public key, direct where
hole-punching succeeds and relayed where it does not, mutually authenticated
and end-to-end encrypted because the key *is* the address. On top of it,
`iroh-gossip` (HyParView + PlumTree epidemic broadcast) suits presence and
operation fan-out, and `iroh-docs` offers an eventually-consistent key-value
store. For a Rust desktop suite that wants peer-to-peer without running
infrastructure, this is the strongest current option, and the relay fallback
means it degrades rather than fails behind NAT.

The problem is not the protocol. It is the sandbox.

- **mDNS does not work inside Flatpak.** A portal for local device discovery
  has been
  [proposed since June 2020](https://github.com/flatpak/xdg-desktop-portal/discussions/1365)
  and is still unimplemented, with no work scheduled as of the most recent
  discussion. `.local` resolution simply fails for sandboxed apps.
- **A "local-first" portal is planned but does not exist.** In the
  [network permission portal discussion](https://github.com/flatpak/xdg-desktop-portal/discussions/1166),
  AdrianVovk wrote on 2024-10-03: *"Some in GNOME are very interested in
  local-first networking… I've spent a while planning a 'local first' portal
  with @adzialocha from @p2panda"* — letting apps reach other instances of
  themselves without a general network permission. The thread is still open;
  no implementation has landed, though Foundation funding for a prototype was
  reportedly discussed.

The consequence is concrete and worth stating before anyone designs around it:
**on Flatpak today, peer discovery requires `--share=network`**, which is the
broad permission the portal work exists to avoid, and which affects how the
apps are rated on Flathub. A suite whose pitch includes privacy should not
quietly take a blanket network permission to deliver a feature most users have
not asked for. Options, in preference order:

1. ship collaboration **off by default**, with the permission requested only
   when a user starts or joins a session (the runtime-permission model that
   discussion #1166 is about, which does not exist yet either);
2. relay-only via an explicitly configured endpoint, so the app needs no local
   discovery and the user chooses who they trust;
3. wait for the local-first portal and help build it — the GNOME-facing
   contribution this project is well placed to make, and the one that would
   benefit every other local-first app on the platform.

Option 3 is slow but it is the one that matches the project's GNOME-native
positioning. It is also an argument for why this RFC should not be rushed
into implementation: the platform piece it depends on is still being designed,
and we would be better as a participant in that design than a workaround for
its absence.

## Identity, authorisation and encryption

Do not invent this. [**Keyhive**](https://www.inkandswitch.com/keyhive/notebook/01/)
is Ink & Switch's local-first access-control work — capabilities plus
end-to-end encryption, aiming at production readiness and general enough for
most local-first applications, and part of the ARIA Safeguarded AI programme.
It is the reference to track; adopting it should wait until it is stable, and
until this project has something to protect.

Until then, the honest scope is: sessions are between peers who already have
each other's public keys, shared out of band, and there is no revocation story
beyond ending the session. Anything more — document-level permissions, removing
a participant's future access without rotating everything — is a research
problem with an active project attached, and claiming otherwise in a changelog
would be the kind of unverified claim the capability ledger exists to prevent.

## What would have to be proven

Matching this repository's definition of done, not a feature list:

| claim | evidence |
|---|---|
| Two peers editing different cells converge | AT-SPI journey launching two app instances (the harness already drives two processes — see the cross-app clipboard journeys and `launch_second_app`), asserting both grids agree |
| Concurrent edits to the *same* cell converge to one value, deterministically | core test on the CRDT layer, plus a journey asserting both UIs show the same value |
| Merged sheet recalculates identically on both peers | core test over the engine; divergence is an engine-determinism bug, findable without networking |
| Offline divergence merges (the sidecar's reason to exist) | journey: disconnect, edit on both sides, reconnect, assert convergence |
| A merge never produces a partial or blended file | the existing atomic-save property tests extended to the merge path |
| A merged document still round-trips ODT/DOCX with a truthful loss report | existing oracle corpora, run on post-merge documents |
| The file being edited by another application is detected, not silently lost | journey: edit in LibreOffice between sessions, assert the stale-history path and the message |
| Collaboration off by default; no new permission without user action | manifest review plus a test asserting the default |

## Phases

Each phase must be abandonable without leaving the codebase worse.

- **Phase 0 — prerequisite, independently justified.** A live GTK-free
  document model for Letters (the `session.rs` question). Scope and justify on
  editing, accessibility and testability grounds; collaboration is a
  beneficiary, not the reason.
- **Phase 1 — spike, no product change.** The measurement above. Output is a
  recommendation with numbers, or a documented dead end.
- **Phase 2 — Tables, session-scoped (option B), LAN or explicit relay, off by
  default.** Exit: the first four journeys above.
- **Phase 3 — Decks**, exercising the movable tree against object moves and
  z-order.
- **Phase 4 — sidecar history (option C)** with compaction and the
  file-changed-underneath path. Exit: the offline-divergence and
  stale-history journeys.
- **Phase 5 — Letters**, once Phase 0 exists.

Access control stays out of scope until Keyhive or an equivalent is stable.

## Open questions

1. Does Phase 0 stand up on its own merits? If it does not, this RFC's
   recommendation is to do Tables and Decks and stop, not to force it.
2. Replicate computed cell values, or only inputs? This RFC argues inputs
   only, but the failure mode of a non-deterministic engine is two peers
   disagreeing about what they see, which is worse than a slow recalculation.
3. Is any cross-platform or web client ever wanted? That question, not
   benchmarks, is what would settle Automerge vs Loro.
4. What is the actual user need? None of this is justified by
   [#544](https://github.com/tuna-os/gtk-office-suite/issues/544)'s premise
   that "modern enterprise desktop productivity suites require" it. Real
   candidate needs: one person across two machines; two people passing a
   document back and forth without email. The first is much easier than the
   second and may be most of the value.
5. Should the project contribute to the local-first portal instead of working
   around its absence? An honest cost comparison is missing and would change
   the sequencing.

## References

**Foundations**
- [Local-first software: You own your data, in spite of the cloud](https://www.inkandswitch.com/local-first-software/) — Ink & Switch, 2019
- [Local-first software (overview)](https://en.wikipedia.org/wiki/Local-first_software)

**Rich text and CRDT algorithms**
- [Peritext: A CRDT for Rich-Text Collaboration](https://www.inkandswitch.com/peritext/) — Ink & Switch; [CSCW paper](https://www.inkandswitch.com/peritext/static/cscw-publication.pdf)
- [Automerge rich text / marks](https://automerge.org/docs/reference/documents/rich-text/) and [`automerge::marks`](https://automerge.org/automerge/automerge/marks/struct.Mark.html)
- [Loro](https://www.loro.dev/) — Peritext + Fugue, `MovableTree`; [`loro` crate](https://docs.rs/loro/)
- [crdt-richtext](https://github.com/loro-dev/crdt-richtext) — Peritext and Fugue in Rust
- [crdt.tech implementations index](https://crdt.tech/implementations)

**Sync and transport**
- [iroh](https://docs.iroh.computer/what-is-iroh) — QUIC, dial-by-public-key; 1.0 on 2026-06-15 ([crate](https://docs.rs/iroh))
- [`iroh-gossip`](https://docs.rs/iroh-gossip) — HyParView + PlumTree broadcast
- [Automerge Repo](https://automerge.org/blog/automerge-repo/) and [`automerge-repo-rs`](https://github.com/automerge/automerge-repo-rs) — note the JS-incompatible disk and wire formats

**Access control**
- [Keyhive](https://www.inkandswitch.com/keyhive/notebook/01/) — local-first capabilities and end-to-end encryption; [syncing Keyhive](https://www.inkandswitch.com/keyhive/notebook/05/)

**Platform (GNOME / Flatpak)**
- [Network permission portal discussion](https://github.com/flatpak/xdg-desktop-portal/discussions/1166) — includes the planned "local first" portal with p2panda (2024-10-03)
- [mDNS local device discovery portal discussion](https://github.com/flatpak/xdg-desktop-portal/discussions/1365) — open since 2020, unimplemented
- [Flatpak sandbox permissions](https://github.com/flatpak/flatpak/wiki/Sandbox)

**Community**
- [Local-First Conf 2026](https://www.localfirstconf.com/), Berlin, 12–14 July 2026 — [schedule](https://app-2026.localfirstconf.com/schedule); directly relevant talks include Scott Jenson, *How the Desktop UX needs to evolve to keep up with Local First*, and Martin Kleppmann, *Local-first in an unstable world*
- [Patchwork](https://www.inkandswitch.com/patchwork/notebook/tasks-02/) — version control and branching for local-first documents
