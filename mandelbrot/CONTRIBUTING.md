# Contributing to Mandelbrot

[Mandelbrot](https://github.com/tuna-os/mandelbrot) is a downstream fork of
[GNOME Fractal](https://gitlab.gnome.org/World/fractal). This guide covers
Mandelbrot development and its GitHub contribution workflow.

<!-- hive-contribute-plea: donated-compute appeal, keep in sync across repos -->
## Contribute compute — no code needed

**[Donate time with your AI CLI](https://hub.tunaos.org/#contribute).**

Your machine runs one task at a time using your signed-in AI coding CLI.
The relay alternates between eligible **School** (OS) and **Reef** (apps)
hives after each task. If one hive has no work, the other can continue.

The guide covers prerequisites, verified `just` setup, credentials, provider
costs, and how to stop with Ctrl+C. Your subscription quota or API charges apply.
Never post account credentials or API keys in issues or pull requests.

## Getting Started

These resources can help with Rust and GTK development:

* [Learn Rust](https://www.rust-lang.org/learn)
* [GUI development with Rust and GTK 4](https://gtk-rs.org/gtk4-rs/stable/latest/book)
* [gtk-rs website](https://gtk-rs.org/)
* [GNOME Development Center](https://developer.gnome.org/)

Read the [feature overview](docs/FEATURES.md) and
[upstream relationship](docs/UPSTREAM.md) before choosing where to report a
problem. Mandelbrot-specific issues belong in the
[Mandelbrot issue tracker](https://github.com/tuna-os/mandelbrot/issues).
Problems that also reproduce in Fractal belong in the
[Fractal issue tracker](https://gitlab.gnome.org/World/fractal/-/issues).

For questions, join the
[TunaOS Matrix room](https://matrix.to/#/%23tunaos:reilly.asia). The
[“Rust ❤️ GNOME” room](https://matrix.to/#/#rust:gnome.org) can also help
with general Rust and GNOME development.

## Build Instructions

[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) is the full development setup guide (build options,
tests, a local homeserver, debugging), and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) maps the
code base. This section covers the essentials.

### Prerequisites

#### Rust toolchain

Mandelbrot is written in Rust. You need Cargo and a Rust compiler at least as new as
`package.rust-version` in `Cargo.toml`, plus a nightly toolchain for the formatting check in the
[pre-commit hook](#pre-commit). Install both through [rustup](https://rustup.rs/), which manages
several toolchains side by side.

1. Check whether rustup is already installed:

   ```sh
   rustup --version
   ```

   If it is, update the toolchains you have and go to step 3:

   ```sh
   rustup update
   ```

2. Otherwise, install rustup. Distribution packages:

   ```sh
   # Fedora
   sudo dnf install rustup
   rustup-init

   # Debian 13 or later, Ubuntu 24.04 or later
   sudo apt install rustup
   rustup default stable

   # Arch Linux
   sudo pacman -S rustup
   rustup default stable
   ```

   On other systems, use the installer from [rustup.rs](https://rustup.rs/) or one of the
   [other installation methods](https://rust-lang.github.io/rustup/installation/other.html). Open a
   new terminal afterwards so that `~/.cargo/bin` is on your `PATH`.

3. Install the dated nightly that CI uses for formatting. CI pins it, instead of using whatever
   `nightly` is current, because `.rustfmt.toml` enables unstable options whose output changes
   between nightlies — a new one can re-wrap comments across the tree and fail the gate on files
   nobody touched. The pin is the `RUSTFMT_TOOLCHAIN` value in `.github/workflows/ci.yml`:

   ```sh
   rustup toolchain install nightly-2026-08-28 --component rustfmt
   ```

4. Verify the setup:

   ```sh
   rustc --version
   cargo --version
   rustup toolchain list
   ```

   `rustc --version` must be at least `package.rust-version`, and the list must include
   `nightly-2026-08-28`.

Run the hook against the pinned toolchain:

```sh
RUSTFMT_TOOLCHAIN=nightly-2026-08-28 cargo run --manifest-path hooks/checks/Cargo.toml
```

#### Flatpak prerequisites

If you are building Mandelbrot with Flatpak (via GNOME Builder or the command line), you will need
to add the necessary remotes and install the Rust freedesktop.org extension:

```sh
# Add Flathub and the gnome-nightly repo
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak remote-add --user --if-not-exists gnome-nightly https://nightly.gnome.org/gnome-nightly.flatpakrepo

# Install the gnome-nightly Sdk and Platform runtime
flatpak install --user gnome-nightly org.gnome.Sdk//master org.gnome.Platform//master

# Install the required rust-stable and llvm22 extensions from Flathub
flatpak install --user flathub org.freedesktop.Sdk.Extension.rust-stable//26.08 org.freedesktop.Sdk.Extension.llvm22//26.08
```

These match the development manifest, `build-aux/org.tunaos.mandelbrot.Devel.json`. The release
manifest, `org.tunaos.mandelbrot.json`, uses `org.gnome.Sdk//51` and `org.gnome.Platform//51` from
Flathub instead.

If you are building the flatpak manually you will also need flatpak-builder on your system, or the
`org.flatpak.Builder` flatpak from Flathub.

### GNOME Builder

Using [GNOME Builder](https://apps.gnome.org/Builder/) with [Flatpak](https://flatpak.org/) is
the recommended way to build and install Mandelbrot. Clone
`https://github.com/tuna-os/mandelbrot.git` and select
`build-aux/org.tunaos.mandelbrot.Devel.json` as the Flatpak manifest.

Development builds use the `org.tunaos.mandelbrot.Devel` application ID.
Published builds use `org.tunaos.mandelbrot`.

Builder's documentation explains how to
[clone and build a project](https://builder.readthedocs.io/).

To open a build terminal to run commands like [Clippy](#pre-commit), you can use the “+” button at
the left of the header bar of the editor and select “New build terminal”, or use its keyboard
shortcut <kbd>Shift</kbd>+<kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>T</kbd>. The terminal should open in
the `_build` directory.

### Install the flatpak

Some features that interact with the system require the app to be installed to test them (i.e.
notifications, command line arguments, etc.).

GNOME Builder can export a flatpak of the app after it has been successfully built.

Mandelbrot can then be installed with:

```sh
flatpak install --user --bundle path/to/org.tunaos.mandelbrot.Devel.flatpak
```

Alternatively, it can be built and installed with flatpak-builder:

```sh
flatpak-builder --user --install app build-aux/org.tunaos.mandelbrot.Devel.json
```

_Note that the `flatpak-builder` command can be replaced with `flatpak run org.flatpak.Builder`._

It can then be entirely removed from your system with:

```sh
flatpak remove --delete-data org.tunaos.mandelbrot.Devel
```

### GNU/Linux

If you decide to ignore our recommendation and build on your host system, outside of Flatpak, you
will need Meson and Ninja, and the system libraries listed in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md#host-builds).

```sh
meson setup --prefix=/usr/local _build
ninja -C _build
sudo ninja -C _build install
```

## Pre-commit

We expect all code contributions to be correctly formatted. The build process
installs a pre-commit hook that runs the `hooks/checks` crate. Run it before
opening a pull request:

```sh
RUSTFMT_TOOLCHAIN=nightly-2026-08-28 cargo run --manifest-path hooks/checks/Cargo.toml
```

Run [Clippy](https://doc.rust-lang.org/stable/clippy/index.html) in the
Flatpak build environment as well:

```sh
meson compile -C {BUILD_DIRECTORY} src/cargo-clippy
```

The `-C {BUILD_DIRECTORY}` option can be omitted when the command is run from
the build directory. CI runs formatting, lint, tests, and the Flatpak build
for every pull request.

## Commits

Follow the
[GNOME commit message guidelines](https://handbook.gnome.org/development/commit-messages.html).
Prefix the summary with the area of the app being changed. Sign off every
commit for the Developer Certificate of Origin:

```sh
git commit -s
```

## Pull Requests

Create a branch in your fork, push it to GitHub, and open a
[pull request](https://github.com/tuna-os/mandelbrot/compare) against the
`main` branch. Use the commit title as the pull request title for a single
commit; otherwise summarize the whole change.

Describe what changed and why. For UI or UX changes, include screenshots in
both light and dark mode, a screencast, or both. Link each issue the pull
request addresses. GitHub closes linked issues after merge when the
description contains `Closes #XXXX` or `Fixes #XXXX`.

Keep commits reviewable and the history clean. Amend or fix up review changes
instead of adding unrelated correction commits. Separate independent changes
into separate pull requests.

## LLM Contributions

> This section records **GNOME Fractal's upstream policy**. TunaOS's
> fork-specific maintenance rules are in [AGENTS.md](AGENTS.md).

Contributions must not include content generated by large language models or other probabilistic
tools like ChatGPT, Claude, and Copilot.

This policy exists due to

* ethical concerns about the data gathering for training these models
* the disproportionate use of electricity and water of building / running them
* the potential negative influence of LLM-generated content on quality
* potential copyright violations

This ban of LLM-generated content applies to all parts of the projects, including, but not limited
to, code, documentation, issues, and artworks. Translating texts for issues and comments to
English can be achieved with machine translation tools, without the use of generative AI (LLM),
such as DeepL.

### Project-related use of LLMs

We heavily discourage the use of LLM chat bots as a replacement for reading Fractal's documentation
and API reference.

Support requests referencing misleading or false LLM output relating to the project may be ignored,
since it is a waste of time for us to "debug" where things went wrong based on this output before
human support was sought.
