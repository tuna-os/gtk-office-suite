# Flatpak sandbox audit (INTEGRATION-PLAN B10)

What each app's manifest grants, why, and which grants are worth trying to
narrow. This is the first half of B10 ("audit, then remove one permission at a
time with a journey each"). **No permission is changed by this document.**

Read from the manifests on 2026-11-10: `flatpak/org.tunaos.{letters,tables,decks}.json`,
`lifafa/co.hyprlab.Hylki.yml` and `mandelbrot/build-aux/org.tunaos.mandelbrot.Devel.json`
(Mandelbrot has no release manifest in the tree; the development one is what
CI builds, so its list is a floor, not the shipped set).

## The office apps (Letters, Tables, Decks)

All three: `--share=ipc`, `--socket=fallback-x11`, `--socket=wayland`,
`--device=dri`, `--filesystem=xdg-documents`, `--own-name=org.tunaos.<app>`.
Letters adds `--filesystem=xdg-run/gvfs` and `--talk-name=org.gnome.evolution.dataserver.*`; Decks adds `--talk-name=org.tunaos.mandelbrot`. None has `--share=network`.

| Grant | Why it is there | Narrowing |
|---|---|---|
| `--share=ipc`, X11/Wayland, `--device=dri` | A GTK 4 window with GPU rendering; the X11 socket is only the fallback. | Not narrowable; this is the baseline of any GTK app. |
| `--own-name=org.tunaos.<app>` | The app's `GApplication` name on the session bus (single instance, `gapplication action`). | Needed; A3/A4 hand-offs call into these names. |
| `--filesystem=xdg-documents` (read-write) | Documents are saved in place with a temporary file beside the target and a rename (`suite-common-core/src/atomic_save.rs`), and autosave and recovery copies live beside the document. | The one real candidate. Files chosen through the file dialog already arrive through the Documents portal, but a portal path is a FUSE view that does not let an app create a sibling file or rename over the original, which is what the crash-safe save needs. Dropping it means either saving through the portal's own write path (losing the sibling temp file) or accepting a weaker save. That trade is a decision, not a cleanup, and needs the crash-stress suite to say what it costs before anyone removes the grant. |
| `--filesystem=xdg-run/gvfs` (Letters only) | Opening documents from `dav://`, `smb://` and similar locations, which GIO reports as FUSE paths under `/run/user/<uid>/gvfs` (`docs/rfc/0003-spike-results.md`). It was `/run/user` until #1996. | Already narrowed to the one directory it needs. Tables and Decks do not offer it; if they should open network locations, it is a feature request, not a hole. |
| `--talk-name=org.gnome.evolution.dataserver.*` (Letters only) | The desktop's address book for the "@" person chips (INTEGRATION-PLAN A7): `suite-common/src/contacts.rs` lists the enabled address books in the EDS source registry and reads their cards with an address, over D-Bus, read-only. Lifafa has the same grant. | The interface names carry a version that changes between EDS releases (`AddressBook10`, `Sources5`), so the wildcard. Read over D-Bus, it needs none of Lifafa's `xdg-*/evolution:ro` filesystem grants. Tables and Decks get it when they have a person picker. |
| `--talk-name=org.tunaos.mandelbrot` (Decks only) | "Present in Call…" (INTEGRATION-PLAN A5, ADR 0016): `decks/src/call_ui.rs` watches whether Mandelbrot is running, to show the item, and calls its `app.share-screen` action through `org.freedesktop.Application.ActivateAction`. | One application's own name, and only that action is called. Nothing is captured or sent through it: the screen share is Mandelbrot's, through the ScreenCast portal and the desktop's picker. |

Apart from Letters' address book and Decks' call to Mandelbrot, no `--talk-name`, no `--share=network` and no home-directory access are
requested by the office apps; `docs/rfc/0005-collaboration-in-the-apps.md`
keeps the network in the separate collaboration sidecar.

## Lifafa

Network (`--share=network`) and the usual display and audio grants
(`--socket=pulseaudio` for the new-mail sound), then, per its own comments:

| Grant | Why it is there | Narrowing |
|---|---|---|
| `--talk-name=org.freedesktop.secrets` | Keyring: passwords and OAuth tokens. | Could move to the Secret portal (`org.freedesktop.portal.Secret`), which gives one app-private secret rather than the whole keyring service. A real narrowing, but a migration of stored credentials. |
| `--talk-name=org.gnome.OnlineAccounts`, `org.gnome.Settings`, `org.gnome.evolution.dataserver.*`, `org.gnome.Contacts` | Import accounts from GNOME Online Accounts; "Open Online Accounts"; read contacts and calendars; "Open GNOME Contacts". | The EDS and GOA names are the integration (INTEGRATION-PLAN A7, B7). `org.gnome.Settings` and `org.gnome.Contacts` exist only for two buttons and are the cheapest to drop if the buttons go through `xdg-open`/portal launching instead. |
| `--talk-name=org.kde.StatusNotifierWatcher`, `com.canonical.Unity`, `org.gnome.Nautilus`, `com.microsoft.identity.broker1` | Tray icon, dock badge, "Restart Files" after the extension install, Microsoft sign-in on managed devices. | Each serves one optional feature. |
| `--filesystem=~/.gnupg`, `xdg-run/gnupg` | OpenPGP through the runtime's `gpg` and the host's agent (#133). | Needed for the feature as built. |
| `--filesystem=xdg-data/evolution:ro`, `xdg-cache/evolution:ro`, `xdg-config/evolution:ro` | Read the host's address-book caches. | Goes away if contacts are read only over the EDS D-Bus API. |
| `--filesystem=~/.var/app/co.hyprlab.Vireo:ro`, `.../com.getveem.Veem:ro` | One-time import of the earlier names' data (`src/legacy.rs`). | Removable once the import window has passed. |
| `--filesystem=xdg-data/icons/hicolor:create`, `xdg-data/applications:create`, `xdg-data/nautilus-python/extensions:create` | App-icon choice, per-user launcher copy, the Files extension. | Write access outside the sandbox for three optional features. |
| **`--filesystem=home:ro`, `/run/media:ro`, `/media:ro`** | A file manager's "Send by email" hands files over as `attach=` paths inside a `mailto:` URI, which Flatpak does not forward as files, so the app must read them where they are. | **The broadest grant in the suite.** Reading all of `$HOME` is what the Nautilus path needs and nothing else does. The alternative is to receive the files through the Email/Documents portal path (the `mailto:` flow is the fallback the suite's own apps avoid; see `suite-common-core/src/mail.rs`), which is B9's work. |

## Mandelbrot (development manifest)

`--socket=fallback-x11`, `--socket=wayland`, `--socket=pulseaudio`,
`--share=network`, `--share=ipc`, `--device=dri`, plus debug environment
variables. No filesystem access: attachments and uploads go through the file
chooser portal. A call also needs the camera, which it reaches through the
Camera portal (`src/components/camera`), not a device grant. Nothing to narrow
here; the release manifest, when it lands, should be diffed against this one.

## Order of work

1. Lifafa's `--filesystem=home:ro` (broadest; its replacement is B9's portal path).
2. Lifafa's `org.gnome.Settings` / `org.gnome.Contacts` names and the two one-time
   import directories (cheap).
3. Lifafa's keyring: Secret portal, with a credential migration.
4. The office apps' `xdg-documents`, only after the crash-stress suite says what a
   portal-only save costs.

Each step is one permission, one change, and a journey that opens, edits and
saves (or sends) through the narrower path.
