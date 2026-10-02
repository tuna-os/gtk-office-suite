# Decks: every advertised action and what proves it changes the deck

docs/readiness-2026-09/decks-readiness.md row 9: "Inventory each advertised action and prove it
mutates canonical state". Every `app.*` action Decks registers is a row
here, with what it changes in the canonical model (`DecksController`'s
slides and masters, which saves, undo and the canvas all read) and the
test that proves it. `tests/test_decks_action_inventory.py` fails when an
action is added without a row, a row names an action that no longer
exists, or a row cites a test that isn't there.

Proof is either a GUI journey (`tests/gui/test_smoke.py`, a class name;
these assert the model through the test snapshot, not the widgets) or a
Rust test (a `fn` name). Actions marked *UI only* open a window and change
no document state.

Not offered, so not here: multi-select, group/ungroup, and align and
distribute across objects. The canvas selects one object at a time, and
`DecksController::align_objects`/`distribute_objects` have no control that
calls them with more than one; they keep their unit tests
(`align_and_distribute_objects_with_undo`).

| Action | Changes | Proved by |
|---|---|---|
| `app.new-document` | replaces the deck with one blank slide | `DecksUndoSelectionSmoke` |
| `app.new-from-template` | replaces the deck with a theme's slides and masters | `DecksTemplateChooserSmoke` |
| `app.open-file` | replaces the deck with the chosen file's | `DecksAdvertisedActionsSmoke` |
| `app.save-file` | writes the deck to its file | `DecksSaveFailureSmoke` |
| `app.save-file-as` | writes the deck to a chosen file, which becomes its file | `DecksFormatJourneySmoke` |
| `app.autosave-now` | writes the recovery checkpoint (not the file) | `DecksAutosaveSmoke` |
| `app.undo` | steps the history back | `DecksUndoSelectionSmoke` |
| `app.redo` | steps the history forward | `DecksUndoSelectionSmoke` |
| `app.add-shape` | adds a shape to the current slide | `DecksAdvertisedActionsSmoke` |
| `app.add-text-box` | adds a text box to the current slide | `DecksAdvertisedActionsSmoke` |
| `app.add-image` | adds the chosen picture to the current slide | `DecksAdvertisedActionsSmoke` |
| `app.insert-shape` | adds a library shape | `DecksInsertBarSmoke` |
| `app.insert-table` | adds a 3×3 table | `DecksInsertBarSmoke` |
| `app.insert-chart` | adds a chart of the chosen kind | `DecksChartSmoke` |
| `app.duplicate-slide` | copies the current slide after itself | `DecksSlideOrderSmoke` |
| `app.move-slide-up` | moves the current slide up | `DecksSlideOrderSmoke` |
| `app.move-slide-down` | moves the current slide down | `move_slide_up_and_down_swap_neighbors`, `mixed_edits_undo_and_redo_one_step_each_and_the_selection_stays_valid` |
| `app.go-to-slide` | the current slide (selection, not the deck) | `DecksLayoutsSmoke` |
| `app.apply-layout` | the current slide's layout and placeholders | `DecksLayoutsSmoke` |
| `app.edit-master` | opens the master view: edits go to the master | `DecksMasterViewSmoke` |
| `app.finish-master` | closes the master view as one undo step | `DecksMasterViewSmoke` |
| `app.focus-slide` | moves focus to the slide canvas; Tab then selects objects | `DecksPictureCropSmoke` |
| `app.focus-notes` | moves focus to the notes; typing edits the slide's notes | `DecksSpeakerNotesSmoke` |
| `app.preview-transition` | none: plays the slide's transition | `DecksMagicMovePreviewSmoke` |
| `app.present` | none: runs a show | `DecksPresenterDisplaySmoke` |
| `app.rehearse` | none: runs a show on the presenter display | `DecksPresenterDisplaySmoke` |
| `app.export-pdf` | none: writes a PDF of the deck | `DecksExportSmoke` |
| `app.export-handouts` | none: writes a handout PDF | `DecksExportSmoke` |
| `app.export-png` | none: writes the current slide as a PNG | `DecksExportSmoke` |
| `app.preferences` | *UI only* | — |
| `app.show-shortcuts` | *UI only* | — |

The canvas and the Format inspector change the deck without an `app.*`
action of their own; each is one undo step:

| Control | Changes | Proved by |
|---|---|---|
| Click on an object | the selection | `DecksSelectionSmoke` |
| Drag (move), handles (resize, rotate) | the object's geometry | `DecksCanvasDragSmoke` |
| Delete / Backspace | removes the selected object | `DecksAdvertisedActionsSmoke` |
| Style ▸ No Fill | the shape's fill, one undo step | `DecksAdvertisedActionsSmoke` |
| Style ▸ Fill and Outline colours, No Outline | the shape's fill and outline | `a_fill_edit_on_a_plain_rectangle_makes_it_a_styled_shape_that_looks_the_same_until_then`, `outline_edits_keep_the_other_half` |
| Text ▸ Bold | the text box's runs, one undo step | `DecksAdvertisedActionsSmoke` |
| Text ▸ Italic, font, size, colour, alignment, list | the text box's runs and paragraphs | `text_edits_style_every_run_of_a_plain_box`, `a_list_gives_every_paragraph_a_marker_on_a_hanging_indent_and_back` |
| Arrange ▸ position, size, rotation fields | the object's geometry | `arrange_edits_move_resize_and_rotate` |
| Arrange ▸ Send to Back, Bring to Front, … | the object order | `DecksFormatInspectorSmoke` |
| Tab / Shift+Tab on the canvas | the selection, object by object | `DecksPictureCropSmoke` |
| Picture ▸ Crop to Fill, Show All | the picture's crop, one undo step | `DecksPictureCropSmoke` |
| Animate ▸ build in/out | the slide's builds | `builds_are_set_undoably_and_follow_deletes_and_reorders` |
| Transition | the slide's transition | `a_slides_transition_is_set_as_one_undo_step` |
