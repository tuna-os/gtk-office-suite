# RFC-0006: Suite and desktop integration — target tuna-desktop, stay GNOME-compatible, carry GOA/GVFS gaps downstream

Date: 2026-11-25 · Status: **draft, not accepted** · Related: [RFC-0003](0003-cloud-documents.md), [RFC-0004](0004-interoperable-document-collaboration.md), [RFC-0005](0005-collaboration-in-the-apps.md) · Origin: issue #1999

This RFC changes no product code. It proposes a strategy so the integration items below can be
argued with, and then split into child issues.

## Proposal

Three rules for how Letters, Tables, Decks, Lifafa and Mandelbrot integrate with the desktop:

1. **Target desktop: tuna-desktop.** Integration is designed and tested against tuna-desktop as the
   reference desktop.
2. **GNOME compatibility is the baseline.** Upstream GNOME behaviour, the HIG, portals and standard
   freedesktop interfaces win by default. The suite degrades gracefully on vanilla GNOME. No
   platform behaviour is forked into the apps, and nothing *requires* tuna-desktop.
3. **Platform gaps are fixed in the platform, downstream.** If the suite needs more from GNOME Online
   Accounts (GOA) or GVFS than upstream gives, the improvement is carried in tuna-desktop's
   downstream GOA/GVFS, not worked around per app.

**Non-goals:** a suite-specific sync engine; app-level workarounds for platform gaps (rule 3); any
behaviour that only works on tuna-desktop (rule 2).

## Constraints a decision must satisfy

- Vanilla GNOME stays fully supported: every feature below has a stated fallback.
- Flatpak builds stay narrow. Today Letters carries `--filesystem=xdg-run/gvfs` and
  `--talk-name=org.gnome.evolution.dataserver.*`; Tables and Decks carry neither
  (`flatpak/*.json`). #1996 narrowed Letters from `/run/user` to `xdg-run/gvfs`.
- The file stays the record (RFC-0001). Integration surfaces do not hold document state.
- Each child item is independently abandonable.

## 1. Within the suite

| Item | Builds on | Notes |
|---|---|---|
| Live linked objects: paste a Tables range or chart into Letters/Decks as a link that refreshes from the source | #1675 (rich-text primitives in `suite-common-core`), clipboard DTOs (#1661, closed) | Needs a link identity and a refresh trigger; the trigger depends on §3 change notification. Without it, refresh on open. |
| Mail merge: Letters template + Tables data source, out via `suite-export` (PDF) or Lifafa | all four pieces exist in-repo | Mostly wiring. |
| Collab rooms as the sharing surface | RFC-0004 (#1726), RFC-0005, #1973 | One Matrix account across Mandelbrot and the sidecars. |
| One suite shell: shared start/recent surface, shared `org.tunaos.*` settings, drag-and-drop between windows using the clipboard DTOs | | |
| Send-for-review via Lifafa: document + collab invite; comment replies return to the document | #1956 (Send by Email, merged) | Lifafa as the suite's outbox. |

## 2. With GNOME

| Item | Mechanism | Fallback on vanilla GNOME |
|---|---|---|
| Documents and their text in Activities search | GNOME Shell search provider; Tracker miner | None needed; feature absent if Shell/Tracker is missing. |
| Cloud open/save | GOA + GVFS (Nextcloud, Drive), not a suite sync engine; OCM sharing on top for collab invites | Whatever GOA/GVFS upstream provides. |
| Share, print, background | Portals; collabd as a proper background service | Portal absence disables the action. |
| Shared credentials | libsecret / `oo7` via the Secret portal, shared by Mandelbrot, Lifafa and the sidecar | Per-app prompt. |
| Presence | One appstream bundle for the suite; `org.tunaos.*` D-Bus activation with New Document/Spreadsheet/Presentation actions; accent and dark-style via the settings portal | Settings portal is upstream. |

## 3. GOA/GVFS gap candidates (to validate before filing)

These are candidates, **not verified gaps**. Validation means testing against the GOA and GVFS
versions shipped in tuna-desktop and recording the result.

| Candidate | Why the suite wants it | Evidence that would settle it |
|---|---|---|
| GVFS mounts reachable through the document portal | Flatpak'd apps open remote locations without a filesystem grant (Letters carries `xdg-run/gvfs` today) | Open a GVFS-mounted file from a sandbox with no `xdg-run/gvfs` grant via the file chooser portal; record success or failure. |
| GOA account type for the suite's collab identity (Matrix) | Configure once in Settings → Online Accounts; consumed by Mandelbrot, collabd, Lifafa | Whether GOA already has a usable provider/extension point; cost of a downstream provider. |
| Change notification on GVFS mounts | Live links and collab presence refresh on remote change | Whether GVFS emits file-monitor events for the Nextcloud/WebDAV backends; observed latency. |
| OCM share support in the Nextcloud GOA/GVFS path | Feeds the RFC-0004 sharing flows | Whether Nextcloud OCM can be reached through the existing path or needs new GOA/GVFS code. |

Each validated gap is filed in the tuna-desktop repository. A gap that is not validated is dropped,
and the suite uses the upstream behaviour (rule 2).

## Costs and risks

- Carrying GOA/GVFS patches downstream is a rebase burden on tuna-desktop for every GNOME release.
  The alternative is a per-app workaround, which rule 3 rejects; an upstream contribution is a third
  option and should be tried first for each gap.
- "Tested against tuna-desktop" needs a tuna-desktop image in CI or a documented manual run; neither
  is decided here.
- Shell search and Tracker add background indexing of user documents; privacy and opt-out need
  review before that item starts.

## Open questions for the maintainer

1. Accept the three rules as written, or amend?
2. For each §3 gap, upstream first, or downstream first?
3. Should this graduate to an ADR once accepted?

## Suggested order of child issues

1. #1675, then live linked objects.
2. #1973, then the GOA collab-identity account.
3. §3 validation, filing real gaps in tuna-desktop.
4. §2 items independent of §3 (portals, appstream bundle, D-Bus activation).
