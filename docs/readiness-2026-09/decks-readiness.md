# [P1] Decks: prove authoring, master fidelity, and presenter/export workflows end to end

Audited on `e7e4df6`. Follow-up verification to closed #115–#117, using `docs/DECKS-STYLING-CHECKLIST.md` and `docs/adr/0004-decks-advanced-workflows.md` as contracts, not test results.

## Architecture
One DecksController owns slide/object/master state, stable identities, selection, history and dirty revision. Canvas and inspector apply the same commands. A drag/resize gesture commits one undoable operation. Rendering, thumbnails, presentation and PDF export consume the same model and geometry. Imported package context travels with the document into the loss-budget save boundary.

## Slices and tests
- [ ] Inventory each advertised action and prove it mutates canonical state: select/multi-select, move/resize/rotate, align/arrange/group, duplicate/reorder, style and image fit/crop.
- [ ] Undo/redo mixed object and slide edits, including deletion and selection repair; no detached state or reentrant RefCell panic.
- [ ] PPTX and ODP journeys preserve supported text runs, images, object geometry/style, master decorations/mapping, slide order and speaker notes.
- [ ] Compare supported shape/style/rotation/crop fields in our writer → Impress rewrite → our reader, not just text extraction.
- [ ] Presenter current/next/notes/timer and external-display disconnect fallback meet the admitted ADR.
- [ ] PDF/print uses the same slide size/order/master content; export failure is visible and leaves the source document untouched.
- [ ] Missing media and unsupported animation/comment content is preserved or blocked/warned by #374 before save.

Exit: deterministic GUI snapshots plus reopened-file semantics for authoring; display fallback test and exported-PDF evidence for presentation. A documentation checklist alone cannot close this issue. Depends on #354, #374 and the shared P0 persistence work.

