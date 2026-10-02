# [P1] Decks: prove authoring, master fidelity, and presenter/export workflows end to end

Audited on `e7e4df6`. Follow-up verification to closed #115–#117, using `docs/DECKS-STYLING-CHECKLIST.md` and `docs/adr/0004-decks-advanced-workflows.md` as contracts, not test results.

## Architecture
One DecksController owns slide/object/master state, stable identities, selection, history and dirty revision. Canvas and inspector apply the same commands. A drag/resize gesture commits one undoable operation. Rendering, thumbnails, presentation and PDF export consume the same model and geometry. Imported package context travels with the document into the loss-budget save boundary.

## Slices and tests
- [ ] Inventory each advertised action and prove it mutates canonical state: select/multi-select, move/resize/rotate, align/arrange/group, duplicate/reorder, style and image fit/crop.
- [ ] Undo/redo mixed object and slide edits, including deletion and selection repair; no detached state or reentrant RefCell panic.
- [ ] PPTX and ODP journeys preserve supported text runs, images, object geometry/style, master decorations/mapping, slide order and speaker notes.
- [~] Compare supported shape/style/rotation/crop fields in our writer → Impress rewrite → our reader, not just text extraction.
    Shape kind, fill, outline colour and width, rotation and geometry
    for every preset we draw (rect, rounded rect with its radius,
    ellipse, triangle, diamond) now go through Impress in both formats:
    `shape_kind_fill_outline_and_rotation_survive_impress_in_both_formats`
    in `decks-core/tests/soffice_oracle.rs`. Gradients and theme
    decorations already had their own oracle tests. Picture crop is the
    field still waiting for its test, which is why the row stays `[~]`.
- [x] Presenter current/next/notes/timer and external-display disconnect fallback meet the admitted ADR.
    Current, next, notes and the running clock: `DecksPresenterDisplaySmoke`.
    Disconnect fallback, ADR 0004's "return to primary and show a visible
    status": `decks_core::presenter::layout_after_monitor_change` moves a
    window whose monitor went away back to the primary one; the show
    watches GDK's monitor list, puts the audience window back fullscreen
    there, and shows "The external display was disconnected…" over the
    slides and as a banner on the presenter display.
    `DecksPresenterDisplayLostSmoke` lays a show out for two monitors
    (test mode only), reports the one real monitor, and sees the banner,
    Dismiss, and the slides back "On display 1". Explicit display
    selection: Preferences ▸ Presentation Display (Automatic or a display
    by name, the `presentation-display` key); `show_layout_on` gives the
    chosen display the slides and the presenter display another, and a
    choice that isn't connected is automatic.
    `DecksPresentationDisplayChoiceSmoke` plants the setting and finds the
    slides "On display 1" (the slide area's accessible description, also
    what tells a screen reader where the show is).
- [ ] PDF/print uses the same slide size/order/master content; export failure is visible and leaves the source document untouched.
- [ ] Missing media and unsupported animation/comment content is preserved or blocked/warned by #374 before save.

Exit: deterministic GUI snapshots plus reopened-file semantics for authoring; display fallback test and exported-PDF evidence for presentation. A documentation checklist alone cannot close this issue. Depends on #354, #374 and the shared P0 persistence work.

