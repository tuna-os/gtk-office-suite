# ADR 0011: One op and history shape for every app

Date: 2026-09-25 · Status: **accepted** (the owner delegated the call;
the orchestrator made it) · Relates to: [RFC-0001](../rfc/0001-crdt-collaboration.md),
[ADR 0010](0010-letters-page-layout-engine.md) stage 3c

## Context

The three apps were about to have three undo designs:

- **Letters:** `letters_core::edit` ops, where every op returns its exact
  inverse, and an `edit::History` of inverses. It coalesces typed words and
  is property-tested (ADR 0010, stage 3c-3).
- **Tables:** its own edit ops, in progress (#1027).
- **Decks:** deck edits as ops with exact inverses and tombstoned deletes,
  in progress (#1032).
- **Old code:** both Tables and Decks still use
  `suite_common_core::undo::UndoManager`, where each command object
  implements its own `apply` and `undo`.

Three designs mean three sets of undo bugs. RFC-0001 also needs one answer
to "what is a change?" before Loro arrives behind the `collab` feature.

## Decision

`suite_common_core::ops` is the one shape. Each app defines its own op type
and implements one trait:

```rust
pub trait Op: Sized + Clone {
    type Doc;
    type Error;
    /// Apply to `doc`; return the ops that restore it exactly. On failure,
    /// `doc` is unchanged.
    fn apply(&self, doc: &mut Self::Doc) -> Result<Vec<Self>, Self::Error>;
    /// Fold the one-op inverse of the next typed step into this one.
    fn coalesce(&mut self, _next: &Self) -> bool { false }
}
```

`History<O: Op>` provides undo and redo for every app, with Letters'
semantics:

- One user action is one step, however many ops it took. A step is either
  what is recorded between `begin` and `end`, or a single `record`.
- Typing coalesces a word at a time. The app marks a typed word character
  with `set_merge(true)`, and the op's `coalesce` says how two inverses
  combine; for Letters, consecutive one-char inserts have contiguous
  `Delete` inverses. A space, Enter or anything else unmarked starts a new
  step.
- A new step clears redo.
- If a stored step no longer applies, the whole history is dropped rather
  than applying half an undo.

`ops::apply_all` applies a group of ops, rolls back on failure, and returns
the group's inverse.

Rules each app's op type must follow:

1. **The op is the only way the document changes.** No view edits the
   document behind the history's back. Letters' Draft buffer edits are
   diffed into ops (`edit::diff`).
2. **The inverse is exact** and is property-tested per app: after any
   sequence of ops, undoing them all restores the document byte for byte.
   The shared history has its own proptest (undo everything, then redo
   everything).
3. **Ops carry what they need to replay.** For example, an insert carries
   its styled runs, and a delete in Decks sets a tombstone. Replaying an op
   never consults editor state such as expand rules or the selection.

### Migration

- Letters (`letters-core`) is on it now: `edit::History` is
  `ops::History<edit::Op>`, and `edit::apply_all` delegates to the shared
  one.
- Tables and Decks move their own crates onto it, owned by their streams.
  Their existing command objects become op types.
- `UndoManager` remains until the last user moves off it, then goes.

### How it maps onto Loro (RFC-0001)

Ops are the **replay and inverse layer on top of the CRDT**, not a
replacement for it.

- **Local edits** apply an op to the app's model as today. With `collab`
  on, the same op is also translated into Loro container calls. Examples:
  a text insert or delete, a mark, a map set on a paragraph or cell, or a
  tree move for a slide object. Rule 3 makes this a pure function of the op.
- **Remote edits** arrive as Loro events. They are applied to the model as
  the ops the events describe. The local `History` does not record them,
  so undo only ever takes back your own steps, as Google Docs does.
- **Undo** applies the recorded inverse ops. That inverse is itself an
  ordinary local op, so it is sent to peers like any other edit. It is not
  a CRDT rewind, which would erase other people's concurrent work. When an
  inverse no longer fits (a peer deleted the text it restores), the op's
  `apply` returns an error and the step is dropped. This is the
  "no half undo" rule, now for concurrency.
- **Offsets:** a local op addresses the model by offset, as it does today.
  Translating to Loro converts offsets to Loro's own positions at the
  moment the op is applied, and Loro's cursors keep remote ops correct.
  Stored inverses are rebased on remote events. That rebase is Phase 2
  work, and the same place as Letters' `ReviewState::rebase_after_edit`.

## Consequences

- One undo implementation and one set of undo tests for the suite. A bug
  found in one app is fixed for all three.
- Each app's op type must be total and exactly invertible. That is more
  design up front than a command object that "does its best" to undo, and
  is what makes collaboration possible at all.
- `suite-common-core` gains a proptest dev-dependency; it was already in
  the lockfile.
