# Lifafa integration

Lifafa ([tuna-os/lifafa](https://github.com/tuna-os/lifafa)) is the suite's
email client. It lives in its own repository, a soft fork of Hylki, and is
not built from this workspace (see `README.md`). This page records how the
suite and Lifafa meet.

## Send by Email (this repository)

Letters, Tables and Decks each have **Send by Email…** in the main menu
(`app.send-by-email`; Letters also lists it in the command palette). It
writes the document in the app's own format (`.docx`, `.xlsx`, `.pptx`) to a
temporary file and opens a new message with that file attached and the
document's name as the subject.

The apps never name Lifafa. They hand the draft to the desktop's default mail
client, so a person who chose another client gets that client:

1. **XDG Email portal** (`org.freedesktop.portal.Email.ComposeEmail`), which
   carries attachments as file descriptors. This is the path in Flatpak and
   needs no extra permission.
2. **`mailto:` URI**, where no portal answers. A `mailto:` URI cannot carry a
   file, so the body asks the sender to attach it.

The logic is in two places: `suite_common_core::mail` (the draft, its wording,
the RFC 6068 encoding; GTK-free and unit tested) and `suite_common::mail` (the
portal call and the fallback). Each app's `mail_ui.rs` only exports and calls
`compose`.

For a test, `SUITE_MAIL_SPOOL=<dir>` makes `compose` write `draft.txt` and
the attachment into `<dir>` instead of sending. The GUI journeys
(`LettersSendByEmailSmoke`, `TablesSendByEmailSmoke`, `DecksSendByEmailSmoke`)
use it.

## What Lifafa already does, and what is left

Lifafa's desktop file already registers it for `x-scheme-handler/mailto`,
`mid:`, `.eml` and `.msg`, so on a suite install it is the default mail client
the Email portal reaches. What is still open is whether a compose request from
the portal carries its attachments through to the composer; that is checked in
the first GUI journey that drives a real Lifafa from an office app
(INTEGRATION-PLAN.md, A1 and A2).

## Source in this repository (`lifafa/`)

Lifafa's source was imported from `tuna-os/lifafa` at
`a7352f0c4e4fb7b90621c8459b78a4505884d2ee` (Hylki 1.47.1). It keeps its own
licence (AGPL-3.0-or-later, `lifafa/LICENSE`); the rest of the suite stays
GPL-3.0-or-later. The two native SDK submodules (`lifafa/native/*-unofficial-sdk`)
are declared in the root `.gitmodules` at the commits Lifafa pinned.

Where it stands:

- **Not yet a workspace member.** Lifafa builds against gtk4 0.9, libadwaita
  0.7 and relm4 0.9; the suite uses gtk4 0.11 and libadwaita 0.9, and
  `gtk4-sys` declares `links = "gtk-4"`, so two versions cannot share a
  lockfile. Until it is bumped, `lifafa/` is listed in the root `exclude`
  like `collab/`, with its own `Cargo.lock`.
- **Upstream sync has ended.** `fork/MAINTAINING.md`'s daily merge from
  hyprlab/hylki does not apply to a vendored copy; the fork workflows were not
  imported. Changes to Lifafa are made here.
- **Next:** bump the toolkit stack to the suite's, make `lifafa` a workspace
  member, then replace relm4 (236 files, about 96,000 lines, 41 components)
  with plain gtk4/libadwaita as the other apps use, moving logic that needs no
  widget into a GTK-free `lifafa-core` crate (AGENTS.md's architecture rule).
