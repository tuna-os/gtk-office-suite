# [P1] Decks: prove authoring, master fidelity, and presenter/export workflows end to end

Audited on `e7e4df6`. Follow-up verification to closed #115–#117, using `docs/DECKS-STYLING-CHECKLIST.md` and `docs/adr/0004-decks-advanced-workflows.md` as contracts, not test results.

## Architecture
One DecksController owns slide/object/master state, stable identities, selection, history and dirty revision. Canvas and inspector apply the same commands. A drag/resize gesture commits one undoable operation. Rendering, thumbnails, presentation and PDF export consume the same model and geometry. Imported package context travels with the document into the loss-budget save boundary.

## Slices and tests
- [ ] Inventory each advertised action and prove it mutates canonical state: select/multi-select, move/resize/rotate, align/arrange/group, duplicate/reorder, style and image fit/crop.
- [~] Undo/redo mixed object and slide edits, including deletion and selection repair; no detached state or reentrant RefCell panic.
    `decks-core/tests/controller_property.rs` drives DecksController, the
    API the window calls, with random mixes of object edits (add, delete,
    move, resize, rotate, z-order), slide edits (add, delete, duplicate,
    move up/down), undos and redos. A model of the history predicts every
    undo and redo; each edit that changes the deck is exactly one step and
    clears redo; ids stay one per object; undo-all restores the start.
    After every step `DecksController::repair_selection` keeps the current
    slide and selected object pointing at something that exists, and both
    undo paths in the window (`app.undo`/`app.redo`, the canvas keys) now
    apply it: before, undoing an added slide or object left indices past
    the end. Still open: a GUI journey that asserts the repaired selection
    (the test snapshot doesn't carry the selection yet).
- [x] PPTX and ODP journeys preserve supported text runs, images, object geometry/style, master decorations/mapping, slide order and speaker notes.
    `DecksFormatJourneySmoke` opens a hand-built odp with styled runs, a
    filled, outlined and rotated shape, an embedded picture, speaker
    notes, two slides in order and a master with a decoration; adds a
    shape; Save As pptx and reopens; Save As odp from that and reopens.
    Each time every object is what was saved, by the test snapshot's new
    `detail` (the model in full, a picture by a hash of its bytes), and
    so are the notes, slide names and order and the master's decorations.
    The journey found the pptx reader truncating run sizes to the half
    point below (19.84 pt read as 19.5); it rounds now.
- [~] Compare supported shape/style/rotation/crop fields in our writer → Impress rewrite → our reader, not just text extraction.
    Shape kind, fill, outline colour and width, rotation and geometry
    for every preset we draw (rect, rounded rect with its radius,
    ellipse, triangle, diamond) now go through Impress in both formats:
    `shape_kind_fill_outline_and_rotation_survive_impress_in_both_formats`
    in `decks-core/tests/soffice_oracle.rs`. Gradients and theme
    decorations already had their own oracle tests. Picture crop is the
    field still waiting for its test, which is why the row stays `[~]`.
- [ ] Presenter current/next/notes/timer and external-display disconnect fallback meet the admitted ADR.
- [ ] PDF/print uses the same slide size/order/master content; export failure is visible and leaves the source document untouched.
- [ ] Missing media and unsupported animation/comment content is preserved or blocked/warned by #374 before save.

Exit: deterministic GUI snapshots plus reopened-file semantics for authoring; display fallback test and exported-PDF evidence for presentation. A documentation checklist alone cannot close this issue. Depends on #354, #374 and the shared P0 persistence work.

