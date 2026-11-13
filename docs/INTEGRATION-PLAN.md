# Integration plan: the suite with itself and with GNOME

Five apps: Letters, Tables, Decks (this workspace), Lifafa (mail, `lifafa/`) and
Mandelbrot (Matrix chat and calls, `mandelbrot/`). [SUITE-INTEGRATION.md](SUITE-INTEGRATION.md)
says what they are; this page lists the features that make them one product and
one citizen of the desktop, in the order to build them.

## Rules for every feature

- **Process boundaries.** Apps talk through portals, D-Bus and `GApplication`
  actions, not by linking each other. That is a design choice (independent
  updates, a failure in one app does not take down another, a smaller attack
  surface), not a licence requirement: AGPLv3 section 13 and GPLv3 section 13
  allow combining the two licences in one work.
- **Portals first.** Anything that crosses the Flatpak sandbox uses an XDG
  portal, so it works for Flatpak and for a native install, and needs no new
  `finish-args`. A feature that needs a new permission says so and why.
- **No business logic in widgets** (AGENTS.md). Cross-app contracts (a draft, a
  share request, a search result) live in GTK-free crates, with unit tests.
- **Every feature ships with evidence**: a unit test for the contract and a GUI
  journey for the visible result (the harness can already launch two apps,
  `launch_second_app`), and a capability-ledger claim when it is a promise.
- **Opt-in and local.** Nothing leaves the machine until a person asks; the
  search provider and any index respect that.

## What already exists

| Piece | Where |
|---|---|
| Send by Email from Letters, Tables, Decks (portal, `mailto:` fallback) | `suite-common::mail`, each app's `mail_ui.rs` |
| Lifafa is the `mailto:`, `mid:` and `.eml`/`.msg` handler | `lifafa/data/*.desktop` (`MimeType`) |
| Lifafa notifications, launcher badge, tray, background mode, single instance, Nautilus extension, GNOME Online Accounts, Evolution Data Server contacts and calendar | `lifafa/src/desktop/`, `lifafa/src/contacts/`, `lifafa/src/calendar/` |
| Mandelbrot calls with portal call notifications, camera portal, QR login | `mandelbrot/` |
| Recent files list, remote locations staged and uploaded, dark mode, window geometry | `suite-common-core::recent`, `suite-common::locations` |
| Live collaboration designed around Matrix | `suite-collab`, `collab/` (RFC-0005) |

Not there yet, anywhere: a GNOME Shell search provider, document thumbnails in
Files, desktop actions ("New Document"), a shared account, any hand-off between
the office apps and chat, a shared contact picker in the office apps.

## A. Hand-offs between the apps

| # | Feature | How | Needs |
|---|---|---|---|
| A1 | **Send by Email, complete**: send as PDF, send the selection or current slide, several documents at once | extend `MailDraft` (more attachments, format choice); one dialog in `suite-common` | none |
| A2 | **Edit an attachment and put it back**: open a mail attachment in the right app, and on save offer to replace the attachment in the draft or reply with it | Lifafa writes the attachment to a watched temp file, opens it with `GApplication::open`, replaces it when the file changes and the person agrees | Lifafa composer hook |
| A3 | **Share to a Matrix room**: "Share to Room…" in every office app and in Lifafa | Mandelbrot exposes a `share-file` action (path, optional room); it shows its room picker, uploads, posts. The office apps call it through `gapplication` or D-Bus | Mandelbrot action; a GTK-free `ShareRequest` |
| A4 | **Call from a document**: a Call button in the collaborators list that starts or joins a call in the document's room | collab session already names a room; Mandelbrot `start-call(room)` action | A3's plumbing, RFC-0005 Matrix binding |
| A5 | **Present into a call**: Decks presents to a call as a screen share | ScreenCast portal sharing the presenter window into the call | Mandelbrot screen-share track |
| A6 | **Mail merge**: Tables rows into a Letters template, then send each through Lifafa | Letters merge fields; a batch of `MailDraft`s, with a confirm-before-send review | A1, merge model in `letters-core` |
| A7 | **Shared contacts**: one person picker in Letters chips, Decks and Tables, and Lifafa's composer | Evolution Data Server over D-Bus (Lifafa already reads it); a `suite-common` picker | GTK-free contact model |
| A8 | **Links that cross apps**: `mid:` (a message), `matrix:` (a room or event), and document links open in the right app | register URI handlers; Letters chips gain Matrix user and message chips | A3 |
| A9 | **Shared language services**: spell, grammar and translation in every text field | Lifafa's grammar (Harper, LanguageTool) and translate services, and Letters' hunspell, behind one local service | decision on where it lives |
| A10 | **Clipboard and drag between apps**: a Tables range pastes into Letters, Decks and the Lifafa composer as a table, and back | audit the formats each app offers and accepts, then fix the gaps | none |

## B. The suite with GNOME

| # | Feature | How | Needs |
|---|---|---|---|
| B1 | **Shell search**: type in Activities and find documents, mail, rooms | one `org.gnome.Shell.SearchProvider2` per app, each with its own `*.search-provider.ini`; results open the item | per-app index API; none shared |
| B2 | **Thumbnails and file types**: previews in Files for DOCX, XLSX, PPTX, ODT, ODS, ODP | a `.thumbnailer` entry calling an app's headless render (the render lab already draws pages) | headless render flag per app |
| B3 | **Desktop actions**: "New Document", "New Spreadsheet", "New Presentation", "New Message", "New Room" on each app's launcher and in Files' New menu | `Actions=` in desktop files; templates through `xdg-templates` | none |
| B4 | **Notifications and Do Not Disturb**: one behaviour across the suite, honouring DND and per-app settings, with badges | `GNotification` through the portal; Lifafa's `notify.rs` and Mandelbrot's call notifications become the model | shared notification module |
| B5 | **Background and autostart**: mail and chat keep running when asked, the office apps do not | Background portal `RequestBackground`, with the "Run in background" switch in each app's preferences | none |
| B6 | **System settings follow**: colour scheme, accent colour, high contrast, text scale and reduced motion from the Settings portal | `suite-common` observes `org.freedesktop.appearance`; Lifafa already tracks text scale | none |
| B7 | **Accounts**: sign in once. GNOME Online Accounts for mail (Google, Microsoft), the Secret portal for the rest, one first-run | a shared `suite-accounts` GTK-free model; apps read the same entries | A decision on Matrix (OIDC) vs GOA |
| B8 | **Default apps**: the suite sets itself as the default for its file types, `mailto:` and `matrix:`, through the standard prompts; Settings → Default Applications shows it | desktop files and `mimeapps` hints in metainfo; no silent overrides | none |
| B9 | **Files integration**: Send to, Share, "Open in…" for the suite from Files | Lifafa's Nautilus extension generalised: Send by Email, Share to Room, Convert to PDF | A1, A3 |
| B10 | **Sandbox tightening**: drop `--filesystem=/run/user` and `xdg-documents` where a portal is enough, use the Documents portal, check each app's `finish-args` against what it needs | audit then remove one permission at a time with a GUI journey each | none |
| B11 | **Print and PDF**: the Print portal everywhere, the same print dialog, PDF/A where the format allows | `GtkPrintDialog` (Letters and Lifafa already use it) | none |
| B12 | **Accessibility and language**: AT-SPI names for every control, keyboard-only paths, right-to-left, shared translation memory (`po/`) | the keyboard-only and accessibility journeys exist for the office apps; extend to Lifafa and Mandelbrot | none |

## C. One product

| # | Feature | How |
|---|---|---|
| C1 | **One chrome**: header bar, primary menu, About, Preferences, Keyboard Shortcuts, toasts, dialogs from `suite-common` | Lifafa after the relm4 port; Mandelbrot by adopting the shared pieces one at a time |
| C2 | **One schema family**: `org.tunaos.*` GSettings, one shared `org.tunaos.suite` for cross-app preferences (default account, send format, notification policy) | schema in `suite-common`, read by every app |
| C3 | **One release**: one Flatpak remote, one cadence, one changelog, an umbrella `org.tunaos.suite` metainfo listing the five apps | the autorelease workflow gains Lifafa and Mandelbrot jobs |
| C4 | **One evidence base**: the capability ledger gets claims and lanes for the cross-app promises; the GUI journeys cover them | `conformance/capabilities.json`, `lanes.json` |
| C5 | **Onboarding**: first launch of any app offers the others and the shared accounts, once | a `suite-common` welcome that reads installed apps |

## Order

1. **Foundations** (in progress). Lifafa compiles on the shared toolkit (PR #1960);
   Mandelbrot builds in CI (PR #1961); the relm4 port starts. Nothing below
   needs the port to finish except C1 for Lifafa.
2. **Cheap and visible**: B3 desktop actions, B8 defaults, B6 settings follow,
   A1 Send by Email complete, B5 background. Small, contained, each with a
   journey.
3. **Search and files**: B1 search providers (office apps first, then Lifafa),
   B2 thumbnails, B9 Files integration.
4. **Chat meets documents**: A3 Share to Room, A8 links, then A4 call from a
   document once RFC-0005's Matrix binding is wired.
5. **Shared services**: A7 contacts, B7 accounts, A9 language services, A10
   clipboard.
6. **Workflows**: A2 edit-and-return attachments, A6 mail merge, A5 present
   into a call.
7. **One product**: C1 to C5 land as each app reaches the shared chrome; B10
   sandbox tightening runs alongside, one permission per change.

## Status

As of 2026-11-10, checked against the tree (not against what was intended).

| Item | State | Evidence or what is missing |
|---|---|---|
| Foundations | done | Lifafa on the shared toolkit (#1960), Mandelbrot built in CI (#1961); Lifafa builds without the `relm4` crate (#2101-#2110: no macro views left, the runtime is vendored under `lifafa/src/relm`). `lifafa-core` extraction and the shared chrome (C1) are still open (#2025). Moving their publishing, issues and upstream tracking here from tuna-os/lifafa and tuna-os/mandelbrot, before those are archived: [CUTOVER-LIFAFA-MANDELBROT.md](CUTOVER-LIFAFA-MANDELBROT.md). |
| A3, A4 | partial | Mandelbrot has `open-room(target)`, `start-call(target)` (#2111) and `share-file(path, room)` (#2124) actions over a GTK-free, tested `hand_off` contract (`ShareRequest` included). `share-file` selects the named room, or uses the open one, and the composer's own preview asks the person to confirm before anything is sent. Missing: a room picker when no room is named and none is open, rooms in another account, the `gapplication` GUI journey, and the callers ("Share to Room…" and a Call button) in the office apps and Lifafa. |
| A8 | partial | The `matrix:` handler and `x-scheme-handler/mailto` / `mid` registrations exist (`mandelbrot/data/*.desktop.in.in`, `lifafa/data/co.hyprlab.Hylki.desktop`). The office apps register no URI schemes, and Letters has no Matrix chips. |
| B3 | partial | Lifafa has `Actions=new-message`. The office apps and Mandelbrot have no desktop actions, and there are no `xdg-templates`. |
| B4 | partial | `GNotification` in `lifafa/src/desktop/notify.rs` and Mandelbrot's `session/notifications`. No shared notification module, no Do Not Disturb handling across the suite. |
| B5 | partial | Lifafa has a "Run in background" setting (`lifafa/src/ui/preferences`). No Background portal request in any app. |
| B6 | mostly the toolkit | No app forces a scheme until the person toggles dark mode (`suite-common` only sets `ForceDark`/`ForceLight` then, and restores the saved choice at startup), so `adw::StyleManager` follows the desktop's colour scheme, accent and high contrast; GTK follows the desktop's text scale (`gtk-xft-dpi`) and animation setting itself. Lifafa adds its own size override (`lifafa/src/desktop/text_scale.rs`, off at 100%). Not covered by any journey: no test changes the desktop setting and watches an app follow. |
| B8 | partial | The office apps declare their file types, Lifafa `mailto:` and `message/rfc822`, Mandelbrot `x-scheme-handler/matrix`. Nothing offers the default through the standard prompts. |
| B9 | partial | Lifafa has a Nautilus extension (`lifafa/src/desktop/nautilus_ext.rs`); it is not generalised to the suite. |
| B11 | partial | `GtkPrintDialog` in Letters (`letters/src/printing.rs`) and Lifafa's print preview. Tables and Decks do not print through it. |
| B1, B2, B7, B10, B12 | B1 for the three office apps, B2 entry and B7 model; the rest not started | B1: GTK-free matching and ranking (`suite_common_core::search_provider`, #2122) and the D-Bus half (`suite_common::shell_search::export`, #2141, #2142): Letters, Tables and Decks each export `org.gnome.Shell.SearchProvider2` over their `recent-files` list and ship a `*.search-provider.ini` and `--gapplication-service` D-Bus service file (`tests/test_shell_search.py` checks them against each other). CI compiles it; nothing has run inside a real Shell yet, so bus activation and the 20 s service idle timeout are untested. Lifafa searches mail (`lifafa/src/desktop/shell_search.rs`: subject, sender and preview from its mail cache, never bodies; a result opens a `mid:` link) with the same D-Bus interface and its own `.search-provider.ini` and service file, since it does not link the suite's crates and has no recent-files list (it builds only where GTK is installed, so its CI job is what compiles it); Mandelbrot searches joined rooms by name (`mandelbrot/src/search_provider.rs`; a result opens its `matrix:` link) the same way, with its ini built from a meson template (its Flatpak build is what compiles it); the Matrix account decision is made (ADR 0013); B7 has its GTK-free model (`suite_common_core::accounts`: GOA providers to capabilities, the one Matrix account, per-capability defaults, first-run), and reading GOA and the Secret portal is the shell half; the office apps still request `--filesystem=xdg-documents`. B10's audit is written up in [`SANDBOX-AUDIT.md`](SANDBOX-AUDIT.md) (no permission changed yet). B2 has its `.thumbnailer` entry (`flatpak/org.tunaos.suite.thumbnailer`, checked against `suite-convert`'s PNG engine); the Letters Flatpak builds and installs `suite-convert` and the entry (one app only, since Flatpak exports `share/thumbnailers` from whoever ships it; a test enforces that), and presentations are in it too: `suite-convert` draws slides to PDF and PNG as a Typst re-drawing (backgrounds, text, solid shapes, pictures, tables and charts (tables without border rules, charts without gridlines or a value axis; an unreadable picture is named as left out)), so a slide thumbnail is approximate. B12: `tests/test_icon_buttons_are_named.py` fails when a Lifafa icon-only button has no tooltip or accessible name (it found the notification card's Dismiss button, now named); it scans Mandelbrot's Blueprint buttons too (a tooltip, label, accessible label or child, or a tooltip its Rust sets through a template property), which found ActionButton's Success, Warning and Error states unnamed (now labelled, with Loading); right-to-left and the keyboard-only journeys for both apps are still open. |
| A10 | partial | A neutral fragment and a suite clipboard MIME exist and Letters, Tables and Decks copy and paste it (ADR 0012); outside HTML is not read. Audit in [`CLIPBOARD-AUDIT.md`](CLIPBOARD-AUDIT.md). |
| A6 | partial | Lifafa already merges: one message per row of a CSV or contact group, `{{Field}}` filled in, tested apart from GTK (`lifafa/src/mail/merge.rs`, #447). Missing is the cross-app half: Tables rows as the source and a Letters document as the template. Letters has no merge fields today, so that half needs a field model in `letters-core` first. |
| A2, A5 | built; no end-to-end journey yet | A2, in Lifafa over the GTK-free `attachment_edit::EditSession`: a draft's attachment opened for editing offers the saved change back, swapped in as one undoable step (#2200), and a received attachment opened for editing offers to reply with the changed file (#2212); both watch the private staged copy through `ui::edit_watch`, and the person's own file is never written to. A5 (ADR 0016): Decks' "Present in Call…" (`decks/src/call_ui.rs`, shown while Mandelbrot runs, `--talk-name=org.tunaos.mandelbrot`) titles the slide window "<deck> — Slide Show" and triggers Mandelbrot's `app.share-screen` (#2208); Mandelbrot's Share Screen asks the ScreenCast portal for a window, reads it from PipeWire and publishes it as LiveKit's `Screenshare` track beside the camera (#2209). Neither has a GUI journey: A2 needs a second app saving into the portal's copy, A5 a call. |
| A1, A7, A9 | not started or model half only | A1's send-as-PDF is in Letters and Decks ("Send as PDF by Email…", `app.send-as-pdf-by-email`, with a GUI journey each; Tables, the selection or current slide, and several documents at once are not); A1 and A7 have their GTK-free halves (`MailDraft::for_documents`, `people::suggest` and `people::from_vcard` (an address book's vCard to a `Person`, groups and address-less cards left out); Letters' "@" chip popover has the rule (`chips::suggestions_with_contacts`: the address book after the people in the document, one chip per matching address, never a repeat); Letters reads the desktop's address books for it over D-Bus (`suite_common::contacts`, with `--talk-name=org.gnome.evolution.dataserver.*`, off in the GUI harness with `SUITE_NO_CONTACTS`); the pickers of Tables, Decks and Lifafa's composer are not built); A9 has its contract (`suite_common_core::language`, ADR 0014); no adapter implements it yet. |
| C1-C5 | C2 schema and C5 model only | C2: `org.tunaos.suite` (default mail account, send format, notification policy) ships with Letters, Tables and Decks; Lifafa and Mandelbrot don't install it yet, and no app reads the keys. C5: `suite_common_core::onboarding::welcome` decides what the first-run welcome offers (other apps not installed, a mail account if none can send); Letters, Tables and Decks now show it (`suite_common::welcome::install`: an AdwAlertDialog over the first window, naming the other apps with no desktop file, once for the whole suite through `welcome-shown` in the shared schema, off with `SUITE_NO_WELCOME=1`, which the GUI harness sets); it does not offer a mail account yet, which waits on reading the desktop's accounts (B7). C1: Lifafa and Mandelbrot cannot link `suite-common` yet, so `tests/test_chrome_contract.py` checks all five apps' sources for the standard pieces (Ctrl+Q quits, Ctrl+, opens Preferences, a Keyboard Shortcuts item, an About item); Lifafa gained Ctrl+, with it. Letters, Tables and Decks each build their main window as `suite_common::SuiteWindow` (shared header bar, primary menu, narrow breakpoint), which the same test now holds them to; their plain messages go through `suite_common::dialogs` (#2204). Lifafa's and Mandelbrot's own header bars and the shared toasts are still open. Lifafa links `suite-common` itself (#2187), starting with the suite's first-run welcome: it is on the suite's gtk4 0.11 and libadwaita 0.9, and the path dependency resolves its lock with one version of every GTK, glib, libadwaita, cairo and pango crate (checked 2026-11-13; it adds `suite-common-core`'s archive crates to `lifafa/Cargo.lock`). GPL-3.0 `suite-common` may be combined with AGPL-3.0 Lifafa (GPLv3 §13). C3 runs on merges to `main` and cuts tags and draft releases, but no release has yet passed the publish gates; C4 not started. |
| #1975 (licences) | done | Notices, SPDX rule, `deny.toml` for both nested workspaces, About dialogs (#2112). |
| #1973 (one Matrix account) | decided | ADR 0013 and `tests/test_matrix_sdk_pin.py` (#2114); the sign-in plumbing is not built. |

## Open decisions

- **Where do shared services run?** Decided in [ADR 0014](adr/0014-suite-shared-services-and-search.md):
  libraries first; a daemon only for state that must be shared across running
  apps, and then as an optional sidecar.
- **Matrix sign-in**: decided in ADR 0013 (the sidecar signs in as a second
  device of the same account; nothing is shared). Whether GNOME Online Accounts
  ever gets a Matrix entry is still open (none exists today).
- **Search index ownership**: decided in ADR 0014: each app answers from its own data.
- **What "fully featured" means per app**: decided in ADR 0014 (LibreOffice for the
  office apps, Thunderbird and Evolution for Lifafa, Element and upstream Fractal for
  Mandelbrot), each tracked in the ledger (C4).
