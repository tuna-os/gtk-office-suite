# Lifafa changelog

The fork's own changes, newest first. Everything else comes from upstream
through the daily merge and is in Hylki's [CHANGELOG.md](../CHANGELOG.md);
a release's notes list the upstream version merged plus the entries here
since the last tag ([MAINTAINING.md](MAINTAINING.md#releases)).

## Unreleased

- **Added: updates through `flatpak update`.** Lifafa is published as a
  signed Flatpak repository on GitHub Pages; install it with
  `flatpak install --user https://tuna-os.github.io/lifafa/org.tunaos.lifafa.flatpakref`.
  The release bundles now update from that repository too. An install made
  from a v1.47.1-tuna5 or earlier bundle never updates: switch it over as
  [Updates](README.md#updates) shows, keeping its accounts and settings.
- **Changed: the repository is now tuna-os/lifafa.** The About window's
  source link points at [tuna-os/lifafa](https://github.com/tuna-os/lifafa),
  and its bug tracker, the welcome page and the metainfo send problems to
  [its issues](https://github.com/tuna-os/lifafa/issues) instead of the
  office suite's tracker.
- **Added: confidential Tuta mail to anyone.** A Tuta account's composer
  has a **Confidential** toggle, on by default as in Tuta (Settings →
  Accounts → **Send confidential by default**). Confidential mail to an
  address outside Tuta is end-to-end encrypted behind a password, read in
  Tuta's web app: each recipient's pill shows its password state, and
  **Passwords** lists them with Tuta's strength check. Passwords are kept
  in your Tuta contacts, never in the Outbox or a draft. With Confidential
  off, sending outside Tuta asks first. See
  [Confidential mail and passwords](ACCOUNTS.md#confidential-mail-and-passwords).
- **Added: Tuta accounts.** Tuta has no IMAP, so a Tuta account goes through
  `hylki-tuta`, a helper built on Tuta's own SDK. Choose **Tuta** in the
  Provider list and sign in with the address and password; an account with
  an authenticator app is asked for its code next. Hylki keeps the session in
  the keyring, never the password. Folders, reading, read state, moving and
  sending work, and mail between Tuta accounts is end-to-end encrypted as in
  Tuta's own app. Folders are made, renamed, moved and deleted on the server, drafts
  and templates are Tuta drafts, tags are Tuta labels, mail is deleted for
  good from the Trash and Spam, snoozing and undoing a move work, new mail
  is pushed, mail moved or copied in from another account is imported as
  Tuta's own app imports a mail file, and a reply or forward joins the
  conversation it answers in Tuta's own app. Stars stay on this computer, and
  security keys are not supported. See [Tuta](ACCOUNTS.md#tuta).
- **Added: Proton Mail accounts without Bridge.** Choose **Proton Mail** in
  the Provider list and sign in with the address and password; Hylki goes
  through `hylki-proton`, a helper built on Proton's own libraries, so Proton
  Mail Bridge is not needed. An account with an authenticator app is asked
  for its code next, a two-password account for its mailbox password, and
  when Proton wants a CAPTCHA solved its own verification page opens in a
  private window and the sign-in carries on once it is solved. Hylki keeps
  the session in the keyring, never a password. Folders, labels, reading,
  read state, stars, moving and sending work; mail to Proton addresses is
  end-to-end encrypted. New mail is polled for, search runs on this computer,
  and security keys are not supported. The **Proton Mail (Bridge)** provider
  is unchanged. See [Proton Mail](ACCOUNTS.md#proton-mail).
