# Tuta and Proton Mail accounts

What Lifafa adds to Hylki: two kinds of account upstream does not have. This
page is the fork's part of the user documentation; everything else is in
Hylki's own [documentation](../docs/DOCUMENTATION.md), and how the helpers
are built and licensed is in [HELPERS.md](HELPERS.md).

- **Tuta:** a Tuta mailbox through a bundled helper built on Tuta's own SDK,
  signed in with the password and an authenticator code where the account
  has one. Hylki keeps the session, never the password. See [Tuta](#tuta).
- **Proton Mail without Bridge:** a Proton mailbox through a bundled helper
  built on Proton's own libraries, signed in with the password, an
  authenticator code and a mailbox password where the account has them,
  and Proton's CAPTCHA when it asks for one. Mail to Proton addresses goes
  end-to-end encrypted. Hylki keeps the session, never a password. Proton
  Mail Bridge stays available as its own provider. See
  [Proton Mail](#proton-mail).

## Tuta

Tuta has no IMAP, SMTP or JMAP, so a Tuta account goes through
`hylki-tuta`, a small helper built on Tuta's own SDK that ships with
Hylki. Hylki starts it for each operation and talks to it over a private
pipe; it opens no port and keeps no files. It is looked for next to the
`hylki` executable first, then on `PATH`.

Choose **Tuta** in the Provider list, in the welcome wizard or under
Settings, and enter the address and the account password. There are no
servers to fill in. **Test & Add** in the wizard, and **Sign In** or
**Save** in Settings, sign in, and Hylki keeps the session that opens, in
the keyring under `session:` and the address. The password is never
stored: it is cleared from the form once it has signed in, and is asked
for again only when the session ends, which Tuta does when it is revoked
from another client.

An account with two-factor authentication through an authenticator app
(TOTP) is asked for its code after the password: the **Authenticator
code** row appears under the password, and signing in again sends both.
Security keys (U2F and WebAuthn) are not supported, so an account whose
only second factor is a key cannot be added.

What works: the folders and the mail in them, read and unread, moving
between folders, Junk and Trash included, and sending. Tuta files the copy
of sent mail in Sent itself. A message that Tuta does not confirm as sent
is not sent again on its own; check Sent before sending it once more.

A reply or a forward joins the conversation of the message it answers in
Tuta's own app, which then shows that message as replied to or forwarded.
This works for mail between Tuta accounts too: such mail has no headers
of its own, so Hylki uses the Message-ID Tuta keeps for it.

Folders are made, renamed, moved and deleted in the sidebar as for any
account. Deleting a folder works as in Tuta's own app: an empty folder, or
one in the Trash or Spam, goes for good, and one holding mail goes to the
Trash with it. Tuta's own folders (Inbox, Sent and the rest) cannot be
changed.

Mail is deleted for good from the Trash and Spam, which is the only place
Tuta erases it: **Delete Permanently** anywhere else moves the mail to the
Trash and erases it there, and **Empty** on the Trash or Junk folder works.
Drafts are kept on Tuta's servers, so a draft saved here opens in Tuta's
own app and the other way round, and is updated in place as you write.
Templates are drafts in a **Templates** folder, which Hylki makes the
first time one is saved. Tags are Tuta labels: tagging a message puts on
it the label named like the tag, made in the tag's color the first time,
and a label set in Tuta's own app shows as the tag of that name. **Find
Tags…** lists the account's labels.

Snoozing works as on other accounts, with a **Snoozed** folder made the
first time, and so does **Undo** after a move: a Tuta message keeps its id
wherever it goes, so it is found again whether or not it was opened.

Mail between Tuta accounts is end-to-end encrypted, as in Tuta's own app:
a message whose recipients are all on Tuta is encrypted for each of them
with their current Tuta key, without asking. A Tuta address that does not
exist stops the whole message rather than leaving anyone out.

### Confidential mail and passwords

Writing from a Tuta account, the composer's header has a **Confidential**
toggle, a lock that is closed while it is on. It starts on, unless **Send
confidential by default** is off in the account's settings (Settings →
Accounts, in the **Tuta** group). Hylki asks Tuta which recipients are
outside Tuta as you type them.

- **Confidential on:** the whole message is end-to-end encrypted. A
  recipient outside Tuta gets an email with a link to Tuta's web app, where
  they enter a password to read the message and answer it. Each of their
  address pills shows whether it has a password, a weak one or none, and
  the bar above the message opens **Passwords**, which lists them with a
  strength bar scored as Tuta scores it. A password your Tuta contacts
  already keep is filled in. Tell each recipient their password another
  way, in person or on the phone, never by email. Sending with a missing
  password stops with a message; a weak one (below Tuta's mark of 64)
  asks first. Once sent, each password is kept with the recipient in your
  Tuta contacts, where Tuta's own apps find it too.
- **Confidential off:** recipients outside Tuta get ordinary email, which
  their provider can read, and so does everyone else on the message. Send
  asks once per message before it goes that way.

A message sent later, or one waiting in the Outbox, keeps whether it is
confidential but never its passwords: they are saved to your Tuta contacts
when it is scheduled and read from there again when it goes. If one is
missing by then, the message stays in the Outbox and opens in the composer
to enter it. A draft keeps whether it is confidential, and no password.
Confidential mail outside Tuta cannot be sent from a shared mailbox, and a
confidential message cannot be mail merged.

New mail is pushed: while Hylki runs, the helper keeps Tuta's own event
channel open and every change it reports syncs the folder it names. While
the channel is down, or **Instant new mail (push)** is off under
**Settings → General**, new mail is looked for at the automatic fetch
interval, at least once a minute, or every two minutes with automatic
fetching off; while it is up, the Inbox is still looked at every quarter
of an hour.

The limits, all of them Tuta's or the helper's:

- **No stars on the server.** Tuta keeps none, so a star stays on this
  computer.
- **A password recipient stays on Tuta's servers.** The first confidential
  message to an address outside Tuta creates a Tuta external user for it,
  which cannot be removed, and the email Tuta sends them with the link is
  always in English.
- **Keys are not verified by hand.** Each recipient's key is taken from
  Tuta as it is, as Tuta's own app does for every contact you have not
  verified there.
- **No searching on the server.** Tuta's servers hold only ciphertext, so
  search finds what is on this computer.
- **Removing the account** ends its session on Tuta's servers and deletes
  it from the keyring.

Mail can be moved or copied into a Tuta account from another account, and
copied between a Tuta account's own folders. The helper imports the whole
message the way Tuta's own app imports a mail file, encrypted for the
account like the rest of its mail: subject, body, attachments and headers
are kept, an unread message stays unread, and a star stays on this
computer. Tuta allows importing only on its Legend and Unlimited plans, as
its own app does; on any other plan the message stays where it was and
Hylki says why. A message larger than 25 MiB is refused too. In
`accounts.toml` the account has `protocol = "tuta"` and no server
settings.

## Proton Mail

The Provider list offers Proton Mail two ways:

- **Proton Mail** signs in to Proton directly through `hylki-proton`, a
  small helper built on Proton's own Go libraries (go-proton-api and
  gopenpgp) that ships with Hylki. Nothing else has to be installed or
  running.
- **Proton Mail (Bridge)** is an ordinary IMAP and SMTP account on Proton
  Mail Bridge, Proton's own program, running on this computer. Take the
  ports and the password Bridge generates from its window.

Use **Proton Mail** unless you already run Bridge. It needs no other
program, and it decrypts mail on this computer just as Bridge does. Use
Bridge when other mail apps share it, or when you want what an IMAP
account has and the native account does not: searching on the server,
drafts and templates kept on the server, snoozing, and making or renaming
folders. Proton offers Bridge on paid plans only. Both can be set up
side by side for different addresses.

The helper is started for each operation and talked to over a private
pipe; it opens no port and writes no files. It is looked for next to the
`hylki` executable first, then on `PATH`, and it tells Proton it is a
third-party client.

To add the account:

1. Choose **Proton Mail** in the Provider list, in the welcome wizard or
   under **Settings → Accounts**, and enter the address and the account
   password. There are no servers to fill in.
2. Press **Test & Add** in the wizard, or **Sign In** or **Save** in
   Settings.
3. An account with two-factor authentication through an authenticator app
   is asked for its code next: the **Authenticator code** row appears under
   the password. Enter the code and sign in again.
4. An account in two-password mode is asked for its mailbox password the
   same way, in the **Mailbox password** row.
5. Proton sometimes wants to confirm that a person is signing in, most
   often from a new network. A **Human Verification** window then opens on
   Proton's own page from `verify.proton.me`, usually a CAPTCHA. Solve it
   there, and the window closes and the sign-in carries on by itself,
   with the authenticator code when the row shows one. Closing the window
   cancels; the **Verify…** button on the **Human Verification** row opens
   it again. If Proton turns the answer down, Hylki signs in once more to
   get a fresh challenge.

The verification page is the only web page Hylki runs scripts for: it is
Proton's, not mail, and it does not work without them. It runs in a
private session that keeps no cookies or storage once the window closes,
it may load pages only from Proton's own servers, and a link to anywhere
else, Proton's website included, opens in the browser.

Hylki keeps the session that opens, in the keyring under `session:` and
the address, and saves it again each time Proton renews it. Neither
password is stored: both are cleared from the form once the session is
open, and are asked for again only when the session ends, for example
when it is revoked from Proton's website.

What works: the folders and labels and the mail in them, read and unread,
stars, moving between Inbox, Archive, Spam, Trash and your own folders,
and sending. A message to a Proton address is end-to-end encrypted; one
to an outside address whose key Proton knows, published or pinned in a
contact, is sent with PGP; anything else goes as ordinary mail over TLS.
Proton files the copy of sent mail in Sent itself, and a reply or a
forward joins the conversation it answers in Proton's own apps. A message
that Proton does not confirm as sent is not sent again on its own; check
Sent before sending it once more.

The limits, all of them Proton's or the helper's:

- **Polling only.** The helper reads Proton's change log rather than
  holding a connection open, so new mail and changes made elsewhere are
  looked for at the automatic fetch interval, at least once a minute, or
  every two minutes with automatic fetching off.
- **Labels are for browsing.** They are listed beside the folders, but
  Proton does not move mail into a label.
- **Less is kept on the server.** Search runs on this computer only,
  drafts and templates stay here, folders cannot be made, renamed or
  removed from Hylki, and deleting outside Trash moves the mail to Trash.
  Mail in Trash cannot be deleted for good from Hylki.
- **Proton does the encryption.** Hylki's own OpenPGP signing and
  encryption are not used for a Proton account, and a message sent with
  them turned on is refused.
- **Authenticator apps only.** Security keys (U2F and WebAuthn) are not
  supported, so an account whose only second factor is a key cannot be
  added.
- **Removing the account** ends its session on Proton's side and deletes
  it from the keyring.

Snoozing is not offered for a Proton account, and mail cannot be moved
into one from another account. In `accounts.toml` the account has
`protocol = "proton"` and no server settings.

in a notice at the foot of the window. Undo brings it back to the composer
<kbd>Ctrl+Alt+3</kbd> do the same. Below or off, the list shows a message on a single line, whatever
the layout below says, the window's close button moves to the list's header,
and replies, forwards and new messages open in a window of their own. Below,
when it is narrower. On one line a row does not swipe, as sideways is how

## Writing the account by hand

An account is normally added from Settings or the welcome wizard. In
`accounts.toml` (see upstream's `accounts.toml.example`), a Tuta or Proton
stanza only names the account: no password is ever stored, here or in the
keyring, so it still has to be signed in from Settings.

```toml
[[accounts]]
name = "Tuta"
email = "you@tuta.com"
protocol = "tuta"
imap_host = ""           # no server: the helper knows Tuta's
username = "you@tuta.com"

[[accounts]]
name = "Proton"
email = "you@proton.me"
protocol = "proton"
imap_host = ""           # no server: the helper knows Proton's
username = "you@proton.me"
```

Proton Mail can still be reached through Proton Bridge as an ordinary IMAP
account, as upstream's example describes.

## Artwork

`data/brands/tuta.png`, the Tuta provider's mark (Tutao GmbH), is the Tuta
glyph from Simple Icons (`data/logos/simple/tuta.svg`), filled with Tuta's
brand red #850122, rendered with
`rsvg-convert -w 128 -h 128 data/brands/src/tuta.svg -o data/brands/tuta.png`.
The trademark notes in [data/brands/README.md](../data/brands/README.md)
apply to it as to the others there.
