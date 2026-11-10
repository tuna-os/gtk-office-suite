# Security and privacy

What Mandelbrot protects, what it does not, and where your data goes. To report a vulnerability,
follow [SECURITY.md](../SECURITY.md) — do not open a public issue.

Mandelbrot is a Matrix client. Much of what is and is not private is decided by the Matrix
protocol and by the homeservers involved, not by the app; the
[Matrix specification](https://spec.matrix.org/) and the
[Matrix.org security disclosure policy](https://matrix.org/security-disclosure-policy/) cover those
layers.

## End-to-end encryption

Encryption is provided by the [Matrix Rust SDK](https://github.com/matrix-org/matrix-rust-sdk)
(`matrix-sdk` with its `e2e-encryption` feature).

* In an encrypted room, message content — text, replies, edits, polls, files, and voice messages —
  is encrypted on your device. Attachments are encrypted before upload.
* Each of your sessions is a separate device with its own keys. **Account Settings → Encryption**
  manages your crypto identity (cross-signing) and account recovery (server-side key backup with a
  recovery key or passphrase).
* You can verify your other sessions and other users by comparing emoji or scanning a QR code, so
  that you know the keys you encrypt to belong to the right people.
* Messages sent before you joined a room, or before a new session was set up, are only readable if
  the keys reach that session — through key backup, key forwarding from your other sessions, or a
  QR login that transfers encryption.

What end-to-end encryption does **not** hide:

* Room **state**: names, topics, avatars, membership, power levels, and other state events, are
  not encrypted. This includes call memberships (see [Calls](#calls)).
* Unencrypted rooms, which are readable by every homeserver taking part in them.
* Metadata, covered next.

## Metadata your homeserver sees

Your homeserver (and, for federated rooms, the other homeservers in the room) can see:

* which rooms you are in, who else is in them, and when you send events;
* your IP address and the list of your sessions (devices);
* the type and size of encrypted events and uploads, though not their content;
* read receipts and typing notifications, if you send them.

**Account Settings → Safety → Privacy** has two switches:

* **Send Read Receipts** — when off, other members cannot track which messages you have seen.
* **Send Typing Notifications** — when off, other members do not see when you are typing.

The same page has **Media Previews** (whether media is previewed in all rooms, only private rooms,
or none) and **Show Avatars for Invites**.

## Local storage

For each session, Mandelbrot creates a random passphrase and stores it, together with the session's
homeserver, user ID, and device ID, in your system secret storage:

* inside the Flatpak, through the **Secret portal**;
* outside the Flatpak, through the **D-Bus Secret Service** (for example GNOME Keyring).

The passphrase encrypts the session's local Matrix SDK databases (messages, keys, cached state) and
a `tokens` file holding the session's access tokens. Both live in the session's data directory —
for the Flatpak, `~/.var/app/org.tunaos.mandelbrot/data/fractal/<session>/`. Your account password
is used to log in and is not stored.

The secret storage is only as strong as what unlocks it. As the [README](../README.md#security-best-practices)
says: use a strong password for your keyring or user session, and lock your screen when you step
away.

Logging out deletes the session's secret and local data and tells the homeserver to end the
session.

## Sessions and devices

* **Account Settings → General → Sessions** lists every session of your account and lets you log
  out of the ones you do not recognise.
* **Link New Device…** in the same place shows a QR code for another device to scan (MSC4108);
  you confirm with a two-digit check code, and encryption is transferred to the new session.

## Files, images, and location

* Images are decoded with [glycin](https://gitlab.gnome.org/GNOME/glycin), which runs image loaders
  in a sandbox, so a malicious image file has less to attack. (The `disable-glycin-sandbox` Meson
  option exists for development builds only; release builds keep the sandbox.)
* The Flatpak has no access to your files: opening and saving files goes through the file chooser
  portal, so the app only sees the files you pick.
* Your location is only sent when you choose to share it from the composer, and is read through
  the location portal.

## Calls

Call media is end-to-end encrypted with per-participant keys, sent as Olm-encrypted to-device
messages and rotated when someone leaves. Details and limits are in
[CALLING.md](CALLING.md#security-and-encryption). What stays visible:

* **Call membership is room state**: your `m.call.member` event, including your device ID and the
  LiveKit service URL, is visible to everyone in the room and to the homeservers, even in an
  encrypted room.
* **The LiveKit SFU** sees your IP address, which call you are in, and the volume and timing of
  your (encrypted) media.
* **The `lk-jwt-service`** receives a Matrix OpenID token to prove your Matrix ID before it issues
  an SFU token.

## Flatpak sandbox

`org.tunaos.mandelbrot.json` grants the app:

| Permission | Why |
| --- | --- |
| `--share=network` | Talking to homeservers, identity and media servers, and the call SFU. |
| `--socket=wayland`, `--socket=fallback-x11`, `--share=ipc` | Displaying windows; X11 only when no Wayland session is available. |
| `--socket=pulseaudio` | Playing audio and recording voice messages and call audio. This socket also gives microphone access without a portal prompt. |
| `--device=dri` | GPU-accelerated rendering. |

It does not get home-directory or device access beyond that. The camera, location, files, secrets,
notifications, and desktop settings go through XDG portals, which ask you or your desktop for
permission. `flatpak info --show-permissions org.tunaos.mandelbrot` lists the permissions of your
installed copy, and `flatpak override --user --show org.tunaos.mandelbrot` shows any local
overrides you added.

## Security updates

* Fixes ship by publishing a new build to the TunaOS Flatpak remote (see
  [RELEASING.md](../RELEASING.md)); `flatpak update` picks it up.
* Dependency updates come from Renovate pull requests. `cargo-deny` checks every pull request
  against the RustSec advisory database.
* Upstream Fractal security fixes are merged through the upstream sync process in
  [UPSTREAM.md](UPSTREAM.md).

## Known limitations

* State events, including call memberships, are not end-to-end encrypted (a Matrix protocol
  property).
* Calling is experimental; see [CALLING.md](CALLING.md#known-limitations).
* The `join_call` development example in `matrixrtc/examples/` takes an access token on its command
  line ([#168](https://github.com/tuna-os/mandelbrot/issues/168)). It is a test tool, not part of
  the app; do not use it with a real account's token.

## For contributors

Before opening a pull request that touches security-relevant code:

* Never log access tokens, passphrases, recovery keys, or message content. Debug logs end up in
  public issues.
* Keep secrets in `Zeroizing` types where the surrounding code does (see `src/secret/`), and do
  not write them anywhere except the secret store and the encrypted tokens file.
* Do not add Flatpak `finish-args` when a portal can do the job; explain any new permission in
  the pull request.
* New dependencies must pass `cargo-deny` (`deny.toml`); the `hooks/checks` crate runs it.
* Calling changes must keep the `matrixrtc/` conformance suite green and must not weaken media
  encryption or key rotation.
* If a change fixes a vulnerability, coordinate through a
  [private advisory](https://github.com/tuna-os/mandelbrot/security/advisories/new) before
  opening a public pull request.
