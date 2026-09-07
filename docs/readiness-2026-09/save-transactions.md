# [P0] Letters: preserve unsaved work on failed Save, Save As, and close-after-save

Audited on `e7e4df6` (2026-09-07). Follow-up to closed #99 and #103.

## Confirmed problem
`letters/src/window.rs::save_page` logs a bridge write error, then clears the buffer's modified flag, tab attention and autosave slot, and returns success. `close_all_dirty_pages` can therefore close after a failed save. Save As has separate writers and ignored Results; non-DOCX Save As reconstructs a plain-text Document, bypassing the rich bridge.

## Architecture
Introduce an explicit save outcome (Saved, NeedsPath, Cancelled, Failed) at the document-session boundary. One serialization/commit path serves Save, Save As, tab-close, window-close and recovery. Update identity, recent files, clean state and recovery cleanup only after a successful commit. GTK owns the chooser/error presentation; core/session code owns the transition policy. Capture the target tab before awaiting a chooser so switching tabs cannot redirect the save.

## Work
- [ ] Add failing regressions before changing each path.
- [ ] Route all saves through the canonical rich document serializer.
- [ ] Keep failed/cancelled documents dirty and open; retain recovery bytes and original identity.
- [ ] Show a localized actionable error; do not treat failure as NeedsPath.
- [ ] Guard close-all: stop at the failed/cancelled document; do not discard remaining tabs.
- [ ] Preserve formatting in ODT/DOCX/Markdown Save As; define explicit TXT/HTML dispatch and loss behavior.

## Acceptance
Real GUI: edit a named file, replace its parent with an unwritable or missing destination, Save and close-with-Save; verify the window remains open, edited state/recovery survive, original content is intact, and an error is visible. Repeat for Save As failure/cancel and multi-tab close. A successful save then permits close and reopens with matching styled Unicode content. Headless tests exercise the same transition policy.

Depends on shared atomic-save hardening for durability; coordinate with #322 (recovery) and #354 (GUI harness).

