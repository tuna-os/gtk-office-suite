# [P0] Letters: preserve unsaved work on failed Save, Save As, and close-after-save

Audited on `e7e4df6` (2026-09-07). Follow-up to closed #99 and #103.

## Confirmed problem
`letters/src/window.rs::save_page` logs a bridge write error, then clears the buffer's modified flag, tab attention and autosave slot, and returns success. `close_all_dirty_pages` can therefore close after a failed save. Save As has separate writers and ignored Results; non-DOCX Save As reconstructs a plain-text Document, bypassing the rich bridge.

## Architecture
Introduce an explicit save outcome (Saved, NeedsPath, Cancelled, Failed) at the document-session boundary. One serialization/commit path serves Save, Save As, tab-close, window-close and recovery. Update identity, recent files, clean state and recovery cleanup only after a successful commit. GTK owns the chooser/error presentation; core/session code owns the transition policy. Capture the target tab before awaiting a chooser so switching tabs cannot redirect the save.

## Work
- [x] Add failing regressions before changing each path.
- [x] Route all saves through the canonical rich document serializer —
      `letters/src/bridge.rs::save_buffer_to_file` captures the buffer with
      `capture_from_buffer` for every format; no path reconstructs a
      plain-text `Document`.
- [x] Keep failed/cancelled documents dirty and open; retain recovery bytes
      and original identity — the write happens inside
      `DocumentSession::save_to`, so `Failed`/`Cancelled` never reach
      `buf.set_modified(false)` or advance `session.file`
      (`failed_save_keeps_original_identity_and_recovery_checkpoint`).
- [x] Show a localized actionable error; do not treat failure as NeedsPath —
      `SaveOutcome::Failed` carries the message to an `AlertDialog`, and only
      a missing path returns `NeedsPath`.
- [x] Guard close-all: stop at the failed/cancelled document; do not discard
      remaining tabs — `close_all_dirty_pages` recurses only on `Saved`.
- [x] Preserve formatting in ODT/DOCX/Markdown Save As; define explicit
      TXT/HTML dispatch and loss behavior — see below.

## The dispatch defect this closed

The format decision lived in the GTK bridge as a two-arm match with a
Markdown catch-all:

```rust
"docx" => docx::write(..), "odt" => odt::write(..),
_      => markdown bytes
```

So every other extension silently received Markdown. `notes.txt` — a
suffix the Save As filter itself offered — got `# Quarterly report` and
`**bold**`. Worse, the Preferences window offered "HTML" and
"RTF (Rich Text)" as the *default save format* with no writer behind
either: choosing HTML pre-filled `Untitled.html` in Save As, and the save
wrote Markdown under that name. That is the extension/content mismatch the
roadmap rules out, one match arm away from a correct save.

Dispatch now lives in `letters-core/src/save.rs`, GTK-free and total:

- `SaveFormat::ALL` is the single list the Preferences window and the Save
  As filter are both built from, so a format cannot be offered without a
  writer (`every_offered_format_has_a_writer`).
- `.txt` writes plain text, `.html` writes real HTML (headings, lists with
  nesting, tables, code blocks, links, images, footnotes with two-way
  anchors, escaping), `.md`/`.markdown` Markdown, `.odt`/`.docx` the
  packages. Aliases and case are accepted.
- An unrecognised or missing extension is an error that writes nothing,
  instead of a guess (`an_unsupported_extension_is_refused_without_writing_anything`).
- Loss is reported from the document's *actual* contents as a
  `CompatibilityReport`, so an unstyled document saved as `.txt` warns
  about nothing, and no warning names something the writer in fact keeps
  (`html_does_not_warn_about_formatting_it_keeps`).
- RTF is no longer offered: nothing in the suite reads or writes it. A
  stale `default-format` of `rtf` falls back to ODT rather than pre-filling
  a name the save would refuse.

Every format now reaches the byte writer with a `&Path` rather than a
`&str`, so a file name that is not valid UTF-8 saves like any other; it is
simply left out of the UTF-8 recent-files list. Letters used to refuse such
a save outright.

The read side carried the same catch-all, so `.txt` went through the
Markdown parser: a plain text file containing `**stars**` or a leading `#`
opened as bold and a heading. `letters_core::save::read` is deliberately
*permissive* where `write` is strict — an unfamiliar extension still opens,
because refusing would be a regression — but it no longer misreads a format
it recognises.

HTML remains export-only: Letters writes it and opens it through the
Markdown parser, which keeps the markup as raw HTML blocks rather than
re-deriving structure from it. That asymmetry is deliberate and has
precedent in the suite (Tables reads ODS and saves xlsx, #439); a real HTML
reader belongs with #438.

## Acceptance
Real GUI: edit a named file, replace its parent with an unwritable or missing destination, Save and close-with-Save; verify the window remains open, edited state/recovery survive, original content is intact, and an error is visible. Repeat for Save As failure/cancel and multi-tab close. A successful save then permits close and reopens with matching styled Unicode content. Headless tests exercise the same transition policy.

Covered by `LettersSaveFailureSmoke` (three journeys: failed save, failed
Save All, Save As cancel), `LettersCloseGuardSmoke` (cancel/discard and
Save-As-from-close-guard) and `LettersSaveFormatSmoke` (plain text, HTML
and a refused format through the real chooser), all recorded on video by
the feature-verification workflow.

Pre-save confirmation and cancellation for a lossy format belongs to #374
(live interoperability loss budgets); this item establishes the dispatch and
reports the loss after the write.

Depends on shared atomic-save hardening for durability; coordinate with #322 (recovery) and #354 (GUI harness).
