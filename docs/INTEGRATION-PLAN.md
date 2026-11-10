# Integration plan: the suite with itself and with GNOME

Five apps: Letters, Tables, Decks (this workspace), Lifafa (mail, `lifafa/`) and
Mandelbrot (Matrix chat and calls, `mandelbrot/`). [SUITE-INTEGRATION.md](SUITE-INTEGRATION.md)
says what they are; this page lists the features that make them one product and
one citizen of the desktop, in the order to build them.

## Rules for every feature

- **Process boundaries only.** Lifafa is AGPL and the office apps are GPL, so
  apps talk through portals, D-Bus and `GApplication` actions, never by linking.
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

## Open decisions

- **Where do shared services run?** A single user-level D-Bus service the apps
  call, or a library each app links (not possible across the AGPL boundary)?
  A service means a new process to ship, update and keep private.
- **Matrix sign-in**: the OIDC flow Mandelbrot already has, shared with
  `collab/`, versus a Matrix entry in GNOME Online Accounts (none exists today).
- **Search index ownership**: each app indexes its own data and answers
  queries, or one indexer (Tracker/LocalSearch) holds everything. The first is
  private by default and simpler to ship; the second finds more.
- **What "fully featured" means per app**, against which reference, and where
  it is tracked. The office apps use the LibreOffice parity tables and the
  ledger; Lifafa and Mandelbrot need the equivalents.
