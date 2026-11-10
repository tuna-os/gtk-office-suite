<div align="center">

<img src="data/icons/org.tunaos.mandelbrot.svg" alt="" />

# Mandelbrot

</div>

Mandelbrot is a Matrix messaging app for GNOME written in Rust, forked from
[Fractal](https://gitlab.gnome.org/World/fractal/) to push further into the modern Matrix feature
set: native MatrixRTC voice/video calling, simplified sliding sync, QR login, threads, and more.
Its interface is optimized for collaboration in large groups, such as free software projects, and
will fit all screens, big or small.

Highlights:

* Find rooms to discuss your favorite topics, or talk privately to people, securely thanks to
  end-to-end encryption
* Send rich formatted messages, files, voice messages, polls, or your current location
* Reply to specific messages — in the room or in threads — react with emoji, edit or remove messages
* View images, and play audio and video directly in the conversation
* See who has read messages, and who is typing
* Log into multiple accounts at once, with Single-Sign On, OAuth 2.0, and QR code login

New in Mandelbrot (see [docs/FEATURES.md](docs/FEATURES.md) for details):

* **Native voice & video calls** (experimental) — MatrixRTC/LiveKit built in, end-to-end
  encrypted, with GNOME-native call UI and call notifications; no embedded browser
* **Simplified sliding sync** (MSC4186) with automatic classic-sync fallback
* **QR code login** (MSC4108) — sign in by scanning, or link a new device
* **Threads** (MSC3440) with an adaptive thread panel
* **Polls** (MSC3381) — create, vote, end
* **Voice messages** (MSC3245) — record and send

<!-- hive-contribute-plea: donated-compute appeal, keep in sync across repos -->
## Contribute compute — no code needed

**[Donate time with your AI CLI](https://hub.tunaos.org/#contribute).**

Your machine runs one task at a time using your signed-in AI coding CLI.
The relay alternates between eligible **School** (OS) and **Reef** (apps)
hives after each task. If one hive has no work, the other can continue.

The guide covers prerequisites, verified `just` setup, credentials, provider
costs, and how to stop with Ctrl+C. Your subscription quota or API charges apply.
Never post account credentials or API keys in issues or pull requests.

## Installation

Install from the TunaOS flatpak remote:

```sh
flatpak remote-add --user --if-not-exists tuna-os https://tunaos.org/flatpak/tuna-os.flatpakrepo
flatpak install --user tuna-os org.tunaos.mandelbrot
```

The `--user` flag matters: without it flatpak tries a system-wide install, which
needs root and fails with "ConfigureRemote not allowed for user". Drop `--user`
from both commands (and run them with `sudo`/polkit) only if you deliberately
want a system install.

Mandelbrot runs on the GNOME 51 runtime, so your system also needs the
[Flathub remote](https://flathub.org/setup) (or another source for
`org.gnome.Platform//51`) to pull it in.

Builds are published for **x86_64** and **aarch64** (64-bit ARM).

To update later, run `flatpak update --user org.tunaos.mandelbrot`.

### Installation troubleshooting

**Check the remote.** List your user remotes with their URLs:

```sh
flatpak remotes --user -d
```

The `tuna-os` row must show `oci+https://tunaos.org/flatpak`. If it is missing, run the
`remote-add` command above. If it is there, check that it offers Mandelbrot for your
architecture:

```sh
flatpak remote-ls --user --app tuna-os
```

**"Nothing matches org.tunaos.mandelbrot in remote tuna-os"** (or "No match for
org.tunaos.mandelbrot in remote tuna-os"). The usual cause is a `tuna-os` remote added before
this document changed: `--if-not-exists` keeps your existing entry, which may still point at the
retired `oci+https://tuna-os.github.io/flatpak-index` index. That index never carried
`org.tunaos.mandelbrot`. Point the remote at the current URL with:

```sh
flatpak remote-modify --user tuna-os --url=oci+https://tunaos.org/flatpak
```

**"No remote refs found for 'tuna-os'".** The remote was added to the other installation:
remotes belong either to your user or to the system, so use `--user` for both `remote-add` and
`install`, or for neither.

**The `org.gnome.Platform` runtime is not found.** Add Flathub to the same installation, then
install again:

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
```

**Mandelbrot does not start, or fails on first launch.** Run it from a terminal to see the error:

```sh
flatpak run org.tunaos.mandelbrot
```

Errors about the "Secret Portal" or "Secret Service" mean the app cannot reach your keyring; see
[docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md#could-not-store-session-or-secret-storage-errors).
If you changed the app's sandbox permissions (for example with Flatseal), show and undo your
overrides with:

```sh
flatpak override --user --show org.tunaos.mandelbrot
flatpak override --user --reset org.tunaos.mandelbrot
```

`flatpak info --show-permissions org.tunaos.mandelbrot` lists the permissions the app ships with;
[docs/PRIVACY.md](docs/PRIVACY.md#flatpak-sandbox) explains each one.

## Documentation

* [docs/FEATURES.md](docs/FEATURES.md) — what Mandelbrot adds to Fractal
* [docs/CALLING.md](docs/CALLING.md) — voice and video calls: requirements, usage, limitations
* [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) — logs, login, sync, and other problems
* [docs/PRIVACY.md](docs/PRIVACY.md) — security and privacy: encryption, metadata, sandbox
* [SECURITY.md](SECURITY.md) — how to report a vulnerability
* [CONTRIBUTING.md](CONTRIBUTING.md), [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md),
  [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — contributing, building, and the code base
* [RELEASING.md](RELEASING.md) — how releases are published

## Relationship to Fractal

Mandelbrot is a friendly downstream fork of GNOME Fractal (GPL-3.0-or-later). All Fractal
functionality is retained, and we aim to track upstream releases. See
[docs/UPSTREAM.md](docs/UPSTREAM.md) for upstream tracking and baseline documentation,
[ROADMAP.md](ROADMAP.md) for current priorities, and
[MANDELBROT.md](MANDELBROT.md) for the detailed feature gap analysis. Please report
issues with core messaging that also reproduce in Fractal to the
[Fractal project](https://gitlab.gnome.org/World/fractal/-/issues), and Mandelbrot-specific
issues (calls, sliding sync, QR login…) to this repo.

## Security Best Practices

You should use a strong **password** that is hard to guess to protect the secrets stored on your
device, whether the password is used directly to unlock your secrets (with a password manager for
example) or if it is used to open your user session and your secrets are unlocked automatically
(which is normally the case with a GNOME session).

Furthermore, make sure to lock your system when stepping away from the computer since an unlocked
computer can allow other people to access your private communications and your secrets.

[docs/PRIVACY.md](docs/PRIVACY.md) describes what Mandelbrot encrypts, what it stores, and what
your homeserver can see. To report a vulnerability, follow [SECURITY.md](SECURITY.md).

## Contributing

### Code

Please follow our [contributing guidelines](CONTRIBUTING.md).

### Translations

Fractal is translated by the GNOME translation team on [Damned lies](https://l10n.gnome.org/).

Find your language in the list on [the Fractal module page on Damned lies](https://l10n.gnome.org/module/fractal/).

The names of the emoji displayed during verification come from [the Matrix specification repository](https://github.com/matrix-org/matrix-spec/tree/main/data-definitions).
They are translated on [Element’s translation platform](https://translate.element.io/projects/matrix-doc/sas-emoji-v1).

## Frequently Asked Questions

Does Fractal have encryption support?

: **Yes**, since Fractal 5, encryption is supported using Cross-Signing. See
  <https://gitlab.gnome.org/World/fractal/-/issues/717> for more info on the state of encryption.

Can I run Fractal with the window closed?

: Currently Fractal does not support this. Fractal is a GNOME application, and accordingly adheres to
  the GNOME guidelines and paradigms. This will be revisited [if or when GNOME gets a proper paradigm
  to interact with apps running in the background](https://gitlab.gnome.org/World/fractal/-/issues/228#note_2054826).

## The origin of Fractal

The current version is a complete rewrite of Fractal built on top of the
[matrix-rust-sdk](https://github.com/matrix-org/matrix-rust-sdk) using [GTK4](https://gtk.org/).

The previous version of Fractal was using GTK3 and its own backend to talk to a matrix homeserver,
the code can be found in the [`legacy` branch](https://gitlab.gnome.org/World/fractal/-/tree/legacy).

Initial versions were based on Fest <https://github.com/fest-im/fest>, formerly called ruma-gtk.
In the origins of the project it was called guillotine, based on French revolution, in relation with
the Riot client name, but it's a negative name so we decide to change for a math one.

The name Fractal was proposed by Regina Bíró.

## Code of Conduct

Fractal follows the official [GNOME Code of Conduct](https://conduct.gnome.org/).
