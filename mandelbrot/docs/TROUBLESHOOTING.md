# Troubleshooting

Help for common problems with Mandelbrot. Installation problems (Flatpak remotes, missing runtime,
"Nothing matches" errors) are covered in the README's
[installation troubleshooting](../README.md#installation-troubleshooting); call problems in
[CALLING.md](CALLING.md#troubleshooting-calls).

If nothing here helps, search the
[open issues](https://github.com/tuna-os/mandelbrot/issues) and file a new one with your
Mandelbrot version (from **About Mandelbrot** in the main menu), your homeserver, and a log. A bug
that also happens in upstream [Fractal](https://gitlab.gnome.org/World/fractal/-/issues) belongs
there instead.

## Logs and crash reports

Mandelbrot logs to standard error. Run it from a terminal to see the log:

```sh
flatpak run org.tunaos.mandelbrot
```

The default log filter is `fractal=info,warn`: informational messages from Mandelbrot itself and
warnings from everything else. (The application crate is still called `fractal`, as upstream.)
Set `RUST_LOG` inside the sandbox to override it:

```sh
# More detail from Mandelbrot
flatpak run --env=RUST_LOG=fractal=debug,warn org.tunaos.mandelbrot

# Also the MatrixRTC calling engine and the Matrix SDK
flatpak run --env=RUST_LOG=fractal=debug,mandelbrot_matrixrtc=debug,matrix_sdk=debug,warn \
  org.tunaos.mandelbrot
```

Debug logs can include room and user IDs. Skim a log before attaching it to a public issue.

If Mandelbrot crashes with a Rust panic, run it again with `--env=RUST_BACKTRACE=1` to include a
backtrace in the log.

## Signing in

### The homeserver is not found

The login screen asks for a domain name and looks up the homeserver through
`/.well-known/matrix/client` ("auto-discovery"). If your server has no well-known file, click
**Advanced…** on the homeserver screen and turn off **Auto-Discovery**: the field then takes the
homeserver URL directly.

### Single sign-on or OAuth does not finish

For SSO and OAuth 2.0 logins, Mandelbrot opens the login page in your web browser and listens on a
local address for the browser to come back. Finish the login in the browser that opened, and allow
it to redirect back to Mandelbrot. If no browser opens, check that a default web browser is set
in your system settings.

### QR code login

Signing in with a QR code (and linking another device) only works on homeservers using OAuth 2.0,
such as matrix.org or servers running matrix-authentication-service. The error messages map to
these causes:

| Message | Cause |
| --- | --- |
| The homeserver does not support signing in with a QR code | The server does not offer MSC4108 QR login. Use another login method. |
| Could not access the camera | The camera portal refused access or no camera was found. Check GNOME Settings → Apps → Mandelbrot. |
| The scanned QR code is not a sign-in QR code | The code came from something other than a "link new device" screen. |
| The QR code has expired, generate a new one on the other device | Start the flow again on the existing device. |
| The sign-in was refused or cancelled on the other device | Confirm the sign-in on the existing device and compare the check code. |
| Could not establish a secure connection to the other device | The secure channel between the two devices failed; retry on a stable network. |

### "Could not store session" or secret storage errors

Mandelbrot stores the access token of each session and the passphrase that encrypts its local
database in your system secret storage. Inside the Flatpak this goes through the Secret portal;
outside it, through the D-Bus Secret Service (for example GNOME Keyring). The error message tells
you which:

* _The Flatpak Secret Portal is not available…_ — install `xdg-desktop-portal` 1.5.0 or newer
  with a backend that implements the Secret portal (such as GNOME Keyring's).
* _The request to the … was cancelled…_ — a prompt to unlock or create the keyring was dismissed;
  restart Mandelbrot and accept it.
* _Could not access the default collection…_ — create a default keyring (for example with
  GNOME's Passwords and Keys app) and try again.
* _The collection or item is locked._ — unlock your keyring.

## Sync

Mandelbrot detects, once per session start, whether your homeserver supports simplified sliding
sync (MSC4186). The log says which mode was picked:

* `Homeserver supports simplified sliding sync (MSC4186), using sliding sync`
* `Homeserver does not support simplified sliding sync (MSC4186), using classic sync`

If the server (or a proxy in front of it) rejects sliding sync later in the session, Mandelbrot
logs `Homeserver rejected simplified sliding sync, falling back to classic sync` and switches to
classic `/sync`. Both modes show the same rooms and messages; classic sync is slower to start on
large accounts.

When the homeserver cannot be reached, Mandelbrot shows an offline banner and resumes syncing
when it is reachable again.

## Local data and disk usage

Mandelbrot keeps, per session, a directory of local state (the Matrix SDK databases, encrypted
with the passphrase from your secret storage) and a separate cache directory. For the Flatpak,
they are under:

* data: `~/.var/app/org.tunaos.mandelbrot/data/fractal/`
* cache: `~/.var/app/org.tunaos.mandelbrot/cache/fractal/`

The cache is meant to be safe to delete; quit Mandelbrot first. Do **not** delete the data
directory to free space: it holds your sessions and their encryption state, and losing it can lose
access to encrypted history unless you have key backup or a recovery key set up (**Account
Settings → Encryption**).

**Account Settings → Safety → Media Previews** controls whether media previews are shown in all
rooms, only in private rooms, or in no rooms.

## Desktop integration

* **Wayland and X11**: the Flatpak talks Wayland directly and only falls back to X11 when no
  Wayland session is available.
* **Notifications**: Mandelbrot uses the desktop notification portal. Message notifications can be
  switched off per account in **Account Settings → Notifications**, and for the whole app in
  GNOME Settings → Notifications. Incoming-call notifications use the `call.incoming` category,
  which some notification servers do not know; those show a normal notification with buttons.
* **Running in the background**: like Fractal, Mandelbrot does not keep running with the window
  closed, so it cannot notify you while closed.
* **Camera and location** go through XDG portals; access is managed in GNOME Settings → Apps →
  Mandelbrot.

## Known issues

* Calling is experimental; see [CALLING.md](CALLING.md#known-limitations) for its known gaps.
* Threaded read receipts (MSC3771) are not implemented, and thread activity does not mark a room as
  unread (see [FEATURES.md](FEATURES.md#threads-msc3440)).
* The full list is on the [issue tracker](https://github.com/tuna-os/mandelbrot/issues).
