# ADR 0005: Shared desktop platform integration

## Status

Accepted for the first stable desktop workflow.

## Decision

All three applications use the same platform contract:

- `GtkFileDialog` is the open/save/export boundary. In a Flatpak build it
  delegates access to the document portal; application code must not request
  host filesystem access just to implement a chooser.
- Opening a document from the file manager, command line, or chooser records
  its URI in the app-local `recent-files` GSettings key. The shared policy
  keeps ten entries, moves duplicates to the front, prunes missing files on
  read, and provides a clear operation for privacy resets. Contents and
  document text are never persisted in this list.
- Desktop files declare the formats each application opens. This covers
  Open With and MIME association discovery; `HANDLES_OPEN` remains the runtime
  dispatch path for multiple files.
- Drag/drop is limited to supported file types. A rejected or missing file is
  reported by the owning app instead of silently changing the document.
- Templates are ordinary new-document fixtures for now. They must be
  packaged as app data and never be added to recent files; a template is only
  recorded after the user saves a new document to a real location.

## Verification journey

The platform test checks the three desktop entries and schemas, then the
GTK-free recent-list tests cover ordering, deduplication, retention, missing
file pruning, and clearing. GUI runners additionally exercise:

1. Open a file through `Open With` and verify the correct app receives it.
2. Open the same file twice and verify one recent entry remains.
3. Delete the file, reopen the recent view, and verify it is pruned.
4. Drop a supported file and an unsupported file, verifying success and an
   actionable error respectively.
5. Use a portal chooser in the Flatpak and verify save/export succeeds without
   host filesystem permission.

Session restore and template galleries remain explicit follow-up work; crash
recovery snapshots are not presented as recent documents.
