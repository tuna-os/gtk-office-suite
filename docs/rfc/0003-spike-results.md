# RFC-0003 spike 1: cloud files through GIO/GVfs — results

Date: 2026-09-25 · Spike for [RFC-0003](0003-cloud-documents.md), section A.
Code: `suite-common/src/locations.rs`, the Tables call sites, and
`tools/cloud-spike/run.sh` (a reproducible run against local WebDAV servers).

## Verdict

- **The path-only bug is fixed for the shared helpers and Tables.** Every
  open and save path in Tables accepts any GIO location. A remote location
  is staged to a private local copy and uploaded with `replace_contents`.
  A location that can't be read or written is reported in a dialog. It is
  no longer ignored without a word. Letters and Decks follow in their own
  small PRs.
- **WebDAV (Nextcloud's protocol) works end to end through GVfs:** open,
  save, save again, Save As to a new name, and a refused save when someone
  else changed the file.
- **Conflict detection needs our own etag check.** GVfs passes the etag on
  as `If-Match`, but a server that ignores it (rclone) takes the stale
  save. We compare the etag ourselves before uploading as well.
- **Nothing was tested against real Google Drive, Nextcloud or OneDrive
  accounts, or inside the Flatpak.** See [What was not tested](#what-was-not-tested).

## How it was run

`tools/cloud-spike/run.sh` runs in the render-lab container (Ubuntu 24.04).
It sets up the following:
- GVfs 1.54 with its backends and the FUSE bridge;
- two WebDAV servers: `rclone serve webdav` and WsgiDAV;
- `gio mount` for each server.

It then records what each backend reports and runs
`suite-common/tests/remote_locations.rs` against both. That test covers:
open, then save, then save again; a save refused after someone else's
change; and Save As to a new remote name. Without `TUNAOS_REMOTE_DIR` the
test skips, so the ordinary test runs are unaffected.

The unit tests in `locations.rs` run everywhere and need no daemon:
- a local file keeps its own path;
- a `resource://` location, which has no local path just like a GVfs one
  without FUSE, is read through GIO under its own name;
- saving back to that read-only location reports an error;
- an unreachable `dav://` location reports an error when opened.

## Findings

| | rclone WebDAV | WsgiDAV (checks `If-Match`, as Nextcloud's sabre/dav does) |
|---|---|---|
| Open and save through GIO | works | works |
| Etag reported by GVfs (`etag::value`) | the content's MD5, quoted: `"b1946ac9…"` | `inode-mtime-size`: `12845192-1790359056-6` |
| Etag changes when the file changes | yes | yes |
| GVfs sends `If-Match` when GIO is given an etag | yes (WsgiDAV saw it) | yes |
| Server refuses a stale `If-Match` | **no**: `PUT` with a wrong `If-Match` gets 201 | yes: 412, and GIO reports "The file was externally modified" |
| Our save refuses to overwrite a newer version | yes, through our own etag comparison | yes (both the comparison and the server check) |
| Rename over an existing file (`gio move`) | works | not measured |
| Atomic save (temp + rename) through the FUSE path | works; the etag changes as expected | not measured |

Other observations:

1. **GIO reports a FUSE path for a `dav://` location** when `gvfsd-fuse`
   runs, for example `/run/user/<uid>/gvfs/dav:host=…/file`. `path()` is
   `Some` even when the FUSE bridge has died. In that case reading it
   fails with "Transport endpoint is not connected", and we saw exactly
   that. Inside a Flatpak, the path points into the host's
   `/run/user/<uid>/gvfs`, which the sandbox can't see without an extra
   filesystem permission. So `locations` uses a path only for **native**
   (local) files, and sends every remote location through GIO. That route
   is also the only one where the etag check can happen.
2. **A FUSE path handed over as a plain file URI** (for example a file
   dragged from `/run/user/…/gvfs/…`) is native, and it keeps the ordinary
   path route. Atomic temp+rename over the FUSE bridge worked on WebDAV.
3. **This GVfs has `google`, `onedrive` and `dav` mount types.** GVfs 1.54
   ships a OneDrive backend (Microsoft Graph, through GOA). So "if a
   backend exists" is answered: it exists. It is **untested** here.
4. **Staged copies** live in `~/.cache/tunaos-office/remote/<hash of the
   URI>/<name>`, under the document's own name. Extension dispatch, titles
   and the Save As name suggestion therefore work unchanged.

## Permissions

- **Outside the Flatpak:** nothing new. GIO talks to the session's gvfsd.
- **Inside the Flatpak (not measured, see below):** for a remote location
  to open or save, the app has to reach GVfs over D-Bus, which means
  `--talk-name=org.gtk.vfs.*`. The other option is for the file chooser
  portal to hand the app a local path. Today's manifests grant neither.
  The GNOME runtime ships GVfs's client-side GIO module, so the
  `talk-name` is expected to be the whole change. That is the claim the
  next spike step must check in a real session.

## What was not tested

These were not run. They need a real desktop session or accounts that this
machine doesn't have.

- **Google Drive through GOA.** Nothing was measured: etags, rename
  behaviour, and how Google-native Sheets appear (size, content type, and
  whether reading them exports anything or fails). The RFC's question on
  native Sheets stays open.
- **A real Nextcloud server.** WsgiDAV stands in for its If-Match
  behaviour. Nextcloud's own etags and its chunked uploads for large
  files were not tested.
- **OneDrive.** The backend exists; its behaviour is unknown.
- **The Flatpak.** No sandboxed run was done:
  - whether `--talk-name=org.gtk.vfs.*` is enough;
  - what the file chooser portal returns for a remote file (a document
    portal FUSE path, or a URI);
  - whether atomic save works through the document portal, which exports
    single files, so a sibling temp file may not be allowed.

## Next steps

1. Letters and Decks call sites: one small PR each (agreed with the
   orchestrator).
2. A conflict dialog in place of the error message: Save as copy,
   Overwrite, or Reload. `CHANGED_ELSEWHERE` is the hook for it.
3. A session run with real accounts and the Flatpak, to fill in the
   table's missing rows. `tools/cloud-spike/run.sh` shows what to record.
4. Reads and uploads are synchronous today. Large files on slow links need
   the async GIO calls and a progress indicator before this ships.
