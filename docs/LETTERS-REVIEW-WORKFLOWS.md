# Letters review workflow and interoperability budget

This document defines the admitted review scope for issue #111. It is a
contract for the editor, import/export adapters, and LibreOffice-authored
fixtures—not a claim that every office annotation is editable.

## Canonical semantics

`letters-core::ReviewState` is the GTK-free sidecar. Comments and tracked
insert/delete revisions use monotonic IDs and global character offsets (the
same offset space as `GtkTextBuffer`). Comments sort by `(start, id)` for
keyboard navigation. A pending insertion is visible; a pending deletion is
hidden but retains its original text. Accept/reject is therefore deterministic
and reversible until the review decision is made. Buffer edits rebase all
annotations through `ReviewState::rebase_after_edit`.

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
| Comments | `ReviewState::comments` | warn-until-comment parts are mapped | Never silently drop; preserve as opaque package parts where possible |
| Tracked insert/delete | `ReviewState::changes` | warn-until-revision parts are mapped | Never silently flatten pending changes; require explicit accept/reject |
| TOC | deterministic derived entries | headings are admitted; field refresh is adapter-owned | Rebuild from headings/styles on reopen and print |
| Bidi paragraph direction | shared base-direction helper | warn if a format cannot encode it | Keep text and direction evidence; never infer from alignment alone |

Until the corresponding DOCX/ODT parts are implemented, a native save with
pending review state must emit the existing structured compatibility warning
and retain uninterpreted package members through the opaque pass-through path.
Fixtures authored by LibreOffice must cover each row in both directions and
assert semantic outcomes, not ZIP-byte equality. The fixture manifest and
loss-budget validator in `interop/` are the release gate for promoting a row
from “warn” to “must preserve”.
