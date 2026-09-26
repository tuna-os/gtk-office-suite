# Letters review workflow and interoperability budget

This document defines the admitted review scope for issue #111. It is a
contract for the editor, import/export adapters, and LibreOffice-authored
fixtures—not a claim that every office annotation is editable.

## Canonical semantics

### Tracked changes are marks on the text (changed 2026-09-25)

A tracked change is a mark on the text it covers: `RunStyle::revision`
(`letters_core::track`, set and cleared with `MarkKey::Revision`), holding
the kind (insertion or deletion), the author and the date (UTC to the
second, as Word and ODF store it). Deleted text stays in the document,
marked, until the deletion is accepted: a pending deletion is visible
(struck through), not hidden. A deletion of someone else's pending
insertion keeps that insertion (`Revision::under`); rejecting the deletion
brings it back.

This replaces the first design, a sidecar list (`ReviewState::changes`) of
changes addressed by global offsets, with a deletion as a zero-width entry
holding its removed text. The marks design won because:

- **It survives editing without repair.** A change moves with its text
  through any edit, like bold does. The offset list had to be rebased after
  every edit (`rebase_after_edit`), and an edit inside or across a change
  had no correct rebase at all.
- **Every step is an ordinary op with an exact inverse (ADR 0011).**
  Recording a tracked edit, accepting and rejecting are `Insert`, `Delete`
  and `Mark` ops, so they undo like any edit and replicate like any edit
  (the Peritext-style marks RFC-0001 needs). The sidecar was state beside
  the op history, invisible to undo.
- **It is what the file formats say.** Word wraps the runs in `w:ins` and
  `w:del` (deleted text as `w:delText`, kept in place); a mark per run maps
  onto that directly. ODF keeps deleted text in a change region instead,
  which the reader and writer move in and out.

While tracking (`app.track-changes`), typing inserts text marked as the
author's insertion (continuing their own pending insertion as one change),
and deleting marks text deleted; deleting one's own pending insertion
removes it, as Word does. Limits: paragraph breaks and formatting changes
are not tracked (a tracked deletion across a break keeps the break); the
Draft editor is read-only while tracking, because its edits go into the
buffer rather than through the model's tracked ops.

### Comments are marks on the text too (changed 2026-09-26)

A comment thread is anchored to its text by a mark: `RunStyle::comments`
holds the ids of the threads on a run, and each thread is a mark key of its
own (`MarkKey::Comment(id)`), so comments overlap without touching each
other, as Peritext's do. The comments themselves (author, date, text,
resolved, and the comment a reply answers) are `Document::comments`,
changed by `Op::SetComment`, whose inverse is the comment as it was
(`letters_core::comments`). Adding, replying, resolving and deleting are
each one undo step. This replaces `ReviewState::comments` (a sidecar list of
global offsets rebased after every edit), for the reasons tracked changes
moved: an anchor moves with its text through any edit, and every change is
an op with an exact inverse.

Text typed strictly inside a commented stretch is in the comment; text typed
at either edge is not. A thread whose text is all deleted keeps its comments
(an orphan, listed as "The text was deleted") until it is deleted; it saves
as a comment with an empty range at the start of the document, which reopens
as the same orphan. Replies share their thread's anchor and are not marked.

In the window: `app.add-comment` (Ctrl+Alt+M) comments on the selection or
the word at the caret; the sidebar's Comments view (Ctrl+Shift+Alt+A) lists
the threads in document order with a reply field, Resolve (or Reopen) and
Delete; the page tints an open thread's text in its author's colour, with a
mark in the right margin that opens the thread. Print and PDF show neither,
as LibreOffice's do not.

The TOC is regenerated, never incrementally patched: explicit heading levels
win over named styles, named `Title`/`Subtitle` map to levels 1/2, and named
`Heading 1`…`Heading 6` are accepted. Empty headings are omitted. This makes
save/reopen and print output stable.

Paragraph base direction comes from the first strong RTL/LTR character with a
caller-selected fallback for neutral text. The same value must drive caret
movement, selection, alignment, list markers, save/reopen, and print. The
direction helper intentionally does not classify numbers or punctuation.

## DOCX/ODT loss budget

| Feature | Native model | DOCX/ODT status | Required behavior |
| --- | --- | --- | --- |
| Comments | `RunStyle::comments` marks and `Document::comments` (`letters_core::comments`) | mapped: DOCX comments part with `w:commentRangeStart`/`End` and references, replies and resolved in `commentsExtended`; ODT `office:annotation` ranges with `loext:parent-name` and `loext:resolved`; LibreOffice oracle in both directions. LibreOffice 24.2 writes no reply parent to .docx for a document it read from .odt, so there a reply opens as a comment of its own on the same text | Kept through save and reopen; Markdown, HTML and text warn and save the text |
| Tracked insert/delete | `RunStyle::revision` marks (`letters_core::track`) | mapped: DOCX `w:ins`/`w:del` (nested for a deleted insertion), ODT change regions; LibreOffice oracle in both directions | Kept through save and reopen; Markdown, HTML and text warn and save the text |
| TOC | deterministic derived entries | headings are admitted; field refresh is adapter-owned | Rebuild from headings/styles on reopen and print |
| Bidi paragraph direction | shared base-direction helper | warn if a format cannot encode it | Keep text and direction evidence; never infer from alignment alone |

Until the corresponding DOCX/ODT parts are implemented, a native save with
pending review state must emit the existing structured compatibility warning
and retain uninterpreted package members through the opaque pass-through path.
Fixtures authored by LibreOffice must cover each row in both directions and
assert semantic outcomes, not ZIP-byte equality. The fixture manifest and
loss-budget validator in `interop/` are the release gate for promoting a row
from “warn” to “must preserve”.
