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

## What belongs in the Lifafa repository

Making Lifafa the *default* handler needs changes there, which this
repository cannot make: a `mailto:` handler (`x-scheme-handler/mailto` in its
desktop file) and an Email portal backend or `xdg-email` compose path that
accepts attachments. Those must respect the fork boundary in
`fork/MAINTAINING.md` in Lifafa's repository.
