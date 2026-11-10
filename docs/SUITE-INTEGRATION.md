# One GNOME office suite

The goal is a fully featured, fully integrated GNOME office suite: documents,
spreadsheets and presentations (Letters, Tables, Decks), mail (Lifafa) and
chat and calls (Mandelbrot), that look, behave and talk to one another as one
product. This page is the map; [LIFAFA-INTEGRATION.md](LIFAFA-INTEGRATION.md)
has the mail details.

The feature list and the order to build it in is
[INTEGRATION-PLAN.md](INTEGRATION-PLAN.md).

## What each part is

| Part | Source | Licence | Toolkit | State |
|---|---|---|---|---|
| Letters, Tables, Decks | this workspace | GPL-3.0-or-later | gtk4 0.11, libadwaita 0.9 | shipped |
| Lifafa (mail) | `lifafa/`, imported from tuna-os/lifafa | AGPL-3.0-or-later | moving from gtk4 0.9 + relm4 to gtk4 0.11 (PR [#1960](https://github.com/tuna-os/gtk-office-suite/pull/1960)) | imported, not yet compiling on the new stack |
| Mandelbrot (chat, calls) | `mandelbrot/`, imported from tuna-os/mandelbrot | GPL-3.0-or-later | gtk4 0.11, libadwaita 0.9, no relm4 | imported as is |

Lifafa and Mandelbrot are each their own Cargo workspace with their own
lockfile (`exclude` in the root `Cargo.toml`): their dependency trees
(matrix-sdk, LiveKit, tokio, WebKitGTK) stay out of the office apps' lockfile
and out of the Ubuntu-based `test` job. Each has a path-filtered CI workflow
that builds it in the Flatpak SDK it needs.

## The integration, in layers

1. **One look.** Header bars, primary menus, About, preferences, toasts and
   dialogs come from `suite-common` in the office apps. Mandelbrot is already
   on the same libadwaita; Lifafa gets there with the relm4 port. Adopting
   `suite-common`'s chrome in both is the next step after each builds here.
2. **One identity.** Matrix is the common thread. `suite-collab` and the
   `collab/` sidecar are designed around Matrix for live collaboration
   (RFC-0005; the Matrix binding itself is wired in a later step);
   Mandelbrot is a Matrix client, so one account can serve both. A person signs in
   once and the apps share it (secret storage via the Secret portal), instead
   of each asking for credentials.
3. **Hand-offs between apps.** Send by Email (shipped: Letters, Tables and
   Decks open a Lifafa compose with the document attached). Next:
   share a document into a Matrix room from any app, start a call from a
   document's collaborator list, attach from the file chooser's recent
   documents in Lifafa's composer.
4. **One release.** One Flatpak remote, one release cadence, one set of
   evidence (the capability ledger and the GUI journeys) covering all five
   apps.

## What it takes, honestly

- Lifafa's UI is built on relm4 (236 files, about 96,000 lines). Porting it to
  plain gtk4/libadwaita, as the other apps use, is the largest single piece of
  work; it goes component by component with CI as the compiler.
- Both imported apps are large. They keep their own tests; they join the
  suite's evidence only as their journeys are written.
- Lifafa is AGPL and the office apps are GPL; linking them into one binary is
  not on the table, so integration is by process boundary (portals, D-Bus,
  `GApplication` actions), which is also what makes the hand-offs safe.
- "Fully featured" is measured the way the office apps are: against a
  reference (LibreOffice for documents, Thunderbird and Evolution for mail,
  Element and Fractal for chat) and tracked in the capability ledger.
