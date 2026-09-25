# RFC 0003: Cloud documents: Google Drive, Nextcloud and Microsoft 365

Date: 2026-09-25 · Status: **draft; the owner asked for it and for spike 1 to be queued** · Related: [RFC-0001](0001-crdt-collaboration.md)

## The ask

The owner (2026-09-25) asked: could the suite "support opening Google docs directly and live
collaborating and opening office online docs, nextcloud docs etc."

That covers three different things, and they have very different answers:

| | What it means | Verdict |
|---|---|---|
| **A. Cloud files** | Open and save documents stored on Drive, Nextcloud or OneDrive | Feasible. Mostly GNOME plumbing we don't use yet. |
| **B. Near-live with Google's editors** | Our edits reach a Google Sheet or Slides deck within seconds, and theirs reach us | Plausible through the public REST APIs. Needs a spike. |
| **C. True co-editing inside their sessions** | Live cursors and keystroke-level merging with Google Docs, Office Online or Collabora | Not realistic. See [Rejected](#rejected-and-why). |

Real-time collaboration *between users of this suite* is RFC-0001's job, on Loro. It works wherever
the file lives, cloud included. This RFC is about talking to *other* editors' clouds.

## Where the code is today

- **Every open and save path goes through `gio::File::path()`** (`letters/src/main.rs`,
  `tables/src/window.rs`, `decks/src/window.rs`, `suite-common/src/file_dialogs.rs`, and others).
  A location without a local path, such as a non-FUSE GVfs URI, is **silently ignored**: the
  `if let Some(path)` falls through and nothing happens. That is a bug worth fixing whatever else
  this RFC decides.
- **Saving is atomic:** write a temp file, then rename (`suite_common_core::atomic_save`). Over a
  FUSE-mounted remote, rename semantics vary. The Google Drive GVfs backend in particular names
  files by id, so rename may not behave. This needs testing, not assuming.
- **The Flatpak manifests grant `--filesystem=host:ro` and no network.** Every route below needs a
  new permission: the GVfs D-Bus names for A, `--share=network` for B. Each one widens the sandbox
  and has to be justified in the PR that adds it.

## A. Cloud files through GNOME Online Accounts and GVfs

GNOME Online Accounts (GOA) holds the user's accounts. GVfs exposes each account's storage as a GIO
location. Nautilus already uses this for Google Drive and Nextcloud. For us:

1. **Accept any GIO location.** Where there is no local path, read through `gio::File`
   (`load_contents` / `replace_contents`). Keep paths for local files, since the zip readers want
   `Read + Seek`: read the remote file into memory or a temp file first.
2. **Detect conflicts.** Record the etag and modification time when opening. Before saving,
   re-query them. If they changed, offer "Save as copy / Overwrite / Reload" as a libadwaita
   dialog, never a silent overwrite. Nextcloud's WebDAV gives real etags. What Drive and OneDrive
   give through GVfs has to be measured.
3. **Google-native files** (Docs, Sheets, Slides) have no bytes to download. The Drive API exports
   them to docx/xlsx/pptx, and uploading with conversion saves them back. Whether GVfs's Drive
   backend does this for us, and how, has to be verified. Each round trip goes through Google's
   converter, and **the render lab can measure what the round trip loses**: add fixtures that go
   out to Drive and back.
4. **Sandbox:** GVfs needs `--talk-name=org.gtk.vfs.*`. The file chooser portal may hand back FUSE
   paths for remote files instead. Spike 1 decides which route works and is the narrower permission.

Unverified assumptions, which spike 1 must check before anything is built on them:
- which GOA providers expose files in the GNOME version our runtime targets (Google, Nextcloud,
  and whether Microsoft 365/OneDrive has a files backend);
- how each backend handles atomic rename;
- what etags each backend gives.

## B. Near-live bridges to Google's editors

Google's editors don't expose their real-time protocol, but their REST APIs are **edit-shaped**:

| Google API | Its edits | Our ops (ADR 0011) |
|---|---|---|
| Sheets `spreadsheets.batchUpdate` / `values.update` | set cell values and formats, insert and delete rows or columns, merge, add a sheet | tables-core ops (#1027) map almost one to one |
| Slides `presentations.batchUpdate` | create a shape, move or resize it, insert text, delete an object | decks-core ops (#1032) are close |
| Docs `documents.batchUpdate` | insert text or delete a range at an index, style a range | letters-core ops; the positions need mapping, and it's the hardest of the three |

A bridge would:
- turn each local op into API requests and send them in batches;
- learn about remote changes through the Drive changes feed or push notifications, then fetch the
  changed state and turn it back into ops against our model.

The result is **shared editing with seconds of delay and no live cursors**. It is honest near-live,
not Google Docs' own experience.

Risks that decide whether B is worth building:
- **Latency and quota.** The per-minute write quotas bound how "live" it can be.
- **Mapping loss.** Formula functions, formatting and chart kinds that exist on only one side.
  The render lab and the oracle tests are the measuring tools.
- **App verification.** Google classifies OAuth scopes. The `drive.file` scope (files the user
  picked or created) is the narrow, recommended one. The Docs/Sheets/Slides editing scopes are
  "sensitive" and need Google's app review. Full Drive access is "restricted" and needs a paid
  yearly security assessment. **Use `drive.file` plus the narrowest editing scopes, and confirm
  what verification that needs before writing any code.** This is a process cost, not only an
  engineering one.
- **Offline.** Edits made offline must merge when reconnecting. RFC-0001's op model and Loro are
  the natural buffer: ops queue locally and replay.

## Rejected, and why

- **Google's internal real-time protocol.** Undocumented. Reverse-engineering it would breach the
  terms of service and break without notice.
- **Microsoft Office co-authoring (MS-FSSHTTP/FSSHTTPB).** It *is* documented in Microsoft's Open
  Specifications, which is what Office desktop uses against OneDrive and SharePoint. But it is very
  large, LibreOffice never implemented it, and Office co-authoring itself syncs at a coarse
  granularity. Revisit only if Microsoft 365 users become a stated priority. Microsoft 365 *files*
  are covered by A.
- **Joining a Nextcloud Office (Collabora) session.** Collabora runs the document on the server,
  and browsers are its clients. A native app has no seat in that session. Nextcloud *files* are
  covered by A. For live editing among suite users on Nextcloud, see RFC-0001 with the file on
  Nextcloud.

## Spikes

1. **Cloud files (A).** Spike 1 is queued for the Tables agent after its Loro work.
   - Open and save an xlsx on Google Drive, Nextcloud and (if a backend exists) OneDrive, through
     GIO/GVfs, from inside the Flatpak.
   - Report which permission was needed, whether atomic save survives each backend, what etags
     each gives, and how Google-native Sheets appear.
   - Fix the `path()`-only bug on the way, with a test using a non-local GIO location.
   - Output: a results file like `0001-spike-results.md`.
2. **Sheets bridge (B).** Only if spike 1 and the verification check come back clean.
   - Replay a recorded Tables session into a real Sheet through the API, and pull back a
     concurrent edit made in the browser.
   - Measure round-trip latency, quota headroom, and cell values and formats after the round trip.
   - Output: go or no-go for Slides, then Docs.

## Open questions

1. Is Microsoft 365 a priority audience? It is the only thing that would justify FSSHTTP.
2. Should cloud accounts be required to go through GOA, or should there also be in-app sign-in for
   desktops without GOA? GOA is the GNOME-native answer. In-app sign-in means storing tokens and
   duplicating what GOA already does.
3. What does the file indicator show for a cloud document: account, sync state, "edited
   elsewhere"? This belongs in `DESIGN-UI.md` once spike 1 shows what the backends can report.
