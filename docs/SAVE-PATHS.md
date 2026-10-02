# Save paths: every way a document is read or written

Each app reads and writes documents through a few entry points. This page lists every call to them, what path in the app reaches it, how it treats the format's losses, and the test that exercises it (#1273).

`tests/test_save_paths.py` keeps this page true. It scans the app sources, excluding test modules, for every call to these entry points and fails in three cases:
- a call has no row here, or a row no longer matches a call;
- a row marked `asks` isn't behind the loss question: the call must be inside the save closure handed to the guard (`save_after_asking` in Tables and Decks, `save_asking_about_loss` in Letters), or inside the guard itself;
- a cited test doesn't exist.

The entry points are listed in the checker's `SINKS`. A call is identified by its file, its entry point and its order among that entry point's calls in the file (`#`).

## The boundary column

| Boundary | Meaning |
|---|---|
| `asks` | Writes the user's file in a format chosen by its name. If that format can't hold something the source has, the user is asked first. Cancel writes nothing, and Save Anyway writes without it (#1206, #1272, #1264). |
| `helper` | The one function the `asks` paths call to do the write, or a delegation inside the writer. It is never called elsewhere. |
| `snapshot` | A crash-recovery snapshot in the app's state directory, never the user's file (`suite_common::autosave`). It is in the document's own format, written only to be read back by recovery. |
| `export` | Writes another format (PDF, PNG) that doesn't stand in for the document, so there is nothing to ask about. |
| `read` | Opens a document. What the model can't hold isn't reported at open. The loss question reports it at the save that would drop it, because it re-reads the source then. That way a document that is only read never warns. |

Building the trace found one path writing around the boundary: **the Decks close guard's Save** wrote straight over the file, dropping content the format can't keep without the question every other save asks. It now asks (`DecksUnsupportedContentSmoke.test_the_close_guards_save_asks_too`, which fails without the fix). It also found that **the Open dialog and drag-and-drop had no journeys** in any app, and that a drop replaced a document with unsaved changes without asking. Letters loaded it over the active tab, which kept its path, so the next Ctrl+S wrote the dropped document over the original. A drop now takes the file manager's path, `GApplication::open` (`suite_common::open_files_on_drop`), so it asks first in Tables and Decks and opens a new tab in Letters, and every Open dialog and drop has a journey (#1316).

## Call sites

| App | Path | File | Entry point | # | Boundary | Test |
|---|---|---|---|---|---|---|
| letters | Export as PDF: the page view hands its typeset to the writer | `letters/src/page_view.rs` | `write_pdf` | 1 | helper | `letters-core/src/layout/pango.rs::the_pdf_has_the_trees_pages_at_their_size` |
| letters | Export as PDF (Print Layout's pages) | `letters/src/printing.rs` | `write_pdf` | 1 | export | `letters-core/src/layout/pango.rs::the_pdf_has_the_trees_pages_at_their_size` |
| letters | Open from the command line, a file manager or a drop (`GApplication::open`, a new tab) | `letters/src/window.rs` | `load_file_to_buffer` | 1 | read | `tests/gui/test_smoke.py::test_a_drop_opens_a_new_tab_and_leaves_the_edited_one_alone` |
| letters | Open… dialog | `letters/src/window.rs` | `load_file_to_buffer` | 2 | read | `tests/gui/test_smoke.py::LettersOpenPathsSmoke` |
| letters | Autosave snapshot | `letters/src/window.rs` | `autosave_slot.write` | 1 | snapshot | `tests/gui/test_smoke.py::LettersAutosaveSmoke` |
| letters | Export as PDF with Typst | `letters/src/window.rs` | `engine::export_pdf` | 1 | export | `letters/src/engine.rs::test_export_pdf_success_writes_pdf_and_cleans_temp_source` |
| letters | Render lab capture (test mode only) | `letters/src/window.rs` | `write_pdf` | 1 | export | `tools/render-lab/capture.py::tier_a` |
| letters | Headless `--export-pdf` (test mode only) | `letters/src/window.rs` | `write_pdf` | 2 | export | `tools/render-lab/export_render.py::export_one` |
| letters | Save, Save As and the close guard's Save, when nothing would be lost | `letters/src/window/saving.rs` | `save_page_to_path` | 1 | asks | `tests/gui/test_smoke.py::LettersSaveFormatSmoke` |
| letters | The same, after Save Anyway | `letters/src/window/saving.rs` | `save_page_to_path` | 2 | asks | `tests/gui/test_smoke.py::LettersLossWarningCancelSmoke` |
| letters | The writer, inside the save transaction | `letters/src/window/saving.rs` | `save_buffer_to_file` | 1 | helper | `tests/gui/test_smoke.py::LettersSaveFormatSmoke` |
| tables | The writer, behind every save | `tables/src/persistence.rs` | `save_sheets_to_xlsx_with_engine` | 1 | helper | `tests/gui/test_smoke.py::TablesLossQuestionSmoke` |
| tables | Export as PDF | `tables/src/window.rs` | `to_pdf_with_setup` | 1 | export | `tables-core/src/export.rs::to_pdf_with_setup` |
| tables | Close guard's Save, file already has a path | `tables/src/window.rs` | `save_engine_to_xlsx` | 1 | asks | `tests/gui/test_smoke.py::TablesCloseGuardSmoke` |
| tables | Close guard's Save, never saved (asks for a name) | `tables/src/window.rs` | `save_engine_to_xlsx` | 2 | asks | `tests/gui/test_smoke.py::TablesCloseGuardSmoke` |
| tables | Open… dialog | `tables/src/window.rs` | `load_workbook` | 1 | read | `tests/gui/test_smoke.py::TablesOpenDialogSmoke` |
| tables | Save As | `tables/src/window.rs` | `save_engine_to_xlsx` | 3 | asks | `tests/gui/test_smoke.py::TablesFormatSafeSaveMixin` |
| tables | Save (Ctrl+S) | `tables/src/window.rs` | `save_engine_to_xlsx` | 4 | asks | `tests/gui/test_smoke.py::TablesLossQuestionSmoke` |
| tables | Autosave now (the action) | `tables/src/window.rs` | `slot.write` | 1 | snapshot | `tests/gui/test_smoke.py::TablesAutosaveSmoke` |
| tables | Autosave timer | `tables/src/window.rs` | `slot.write` | 2 | snapshot | `tests/gui/test_smoke.py::TablesUnattendedAutosaveSmoke` |
| tables | Open from the command line, a file manager or a drop (`GApplication::open`), and recovery | `tables/src/window.rs` | `load_workbook` | 2 | read | `tests/gui/test_smoke.py::TablesDropGuardSmoke` |
| decks | Export as PDF | `decks/src/export_ui.rs` | `export_pdf` | 1 | export | `tests/gui/test_smoke.py::DecksExportSmoke` |
| decks | Export Handouts | `decks/src/export_ui.rs` | `export_pdf` | 2 | export | `tests/gui/test_smoke.py::DecksExportSmoke` |
| decks | Export Slide as PNG | `decks/src/export_ui.rs` | `export_png` | 1 | export | `tests/gui/test_smoke.py::DecksExportSmoke` |
| decks | Headless `--export-pdf` | `decks/src/main.rs` | `export_pdf` | 1 | export | `tools/render-lab/export_render.py::export_one` |
| decks | Close guard's Save, file already has a path | `decks/src/window.rs` | `save_deck` | 1 | asks | `tests/gui/test_smoke.py::test_the_close_guards_save_asks_too` |
| decks | Close guard's Save, never saved (asks for a name) | `decks/src/window.rs` | `save_deck` | 2 | asks | `tests/gui/test_smoke.py::DecksCloseGuardSmoke` |
| decks | Open… dialog | `decks/src/window.rs` | `read_deck` | 1 | read | `tests/gui/test_smoke.py::test_open_dialog_opens_the_chosen_file` |
| decks | Save (Ctrl+S) | `decks/src/window.rs` | `save_deck` | 3 | asks | `tests/gui/test_smoke.py::DecksUnsupportedContentSmoke` |
| decks | Save As | `decks/src/window.rs` | `save_deck` | 4 | asks | none: a Save As journey lands with #1270 |
| decks | Autosave now (the action) | `decks/src/window.rs` | `write_deck` | 1 | snapshot | `tests/gui/test_smoke.py::DecksAutosaveSmoke` |
| decks | Autosave now: the snapshot write | `decks/src/window.rs` | `slot.write` | 1 | snapshot | `tests/gui/test_smoke.py::DecksAutosaveSmoke` |
| decks | Autosave timer | `decks/src/window.rs` | `write_deck` | 2 | snapshot | `tests/gui/test_smoke.py::DecksUnattendedAutosaveSmoke` |
| decks | Autosave timer: the snapshot write | `decks/src/window.rs` | `slot.write` | 2 | snapshot | `tests/gui/test_smoke.py::DecksUnattendedAutosaveSmoke` |
| decks | Open from the command line, a file manager or a drop (`GApplication::open`), and recovery | `decks/src/window.rs` | `read_deck` | 2 | read | `tests/gui/test_smoke.py::test_a_dropped_deck_asks_before_replacing_unsaved_work` |
| decks | The writer, behind every save | `decks/src/window.rs` | `write_deck` | 3 | helper | `tests/gui/test_smoke.py::DecksUnsupportedContentSmoke` |
