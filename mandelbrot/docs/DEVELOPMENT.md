# Development setup

How to build, run, test, and debug Mandelbrot from a checkout. Contribution rules (commits, pull
requests, code style) are in [CONTRIBUTING.md](../CONTRIBUTING.md); a map of the code is in
[ARCHITECTURE.md](ARCHITECTURE.md).

## Get the source

```sh
git clone https://github.com/tuna-os/mandelbrot.git
cd mandelbrot
```

The repository has no submodules.

## Choose a build path

| Path | Manifest / command | Use it for |
| --- | --- | --- |
| Flatpak, development profile | `build-aux/org.tunaos.mandelbrot.Devel.json` | Day-to-day work in GNOME Builder or with `flatpak-builder`. Builds against the GNOME nightly SDK (`runtime-version: master`). |
| Flatpak, release profile | `org.tunaos.mandelbrot.json` | Reproducing what CI and the release pipeline build (GNOME `50` runtime). |
| Host build | `meson setup` + `ninja` | Working without Flatpak. You install the system dependencies yourself. |

Both manifests build with `-Dcalls-media=true`, so Flatpak builds always include native call
media. The development manifest also sets `RUST_LOG=fractal=debug,warn` and `RUST_BACKTRACE=1`
inside the sandbox and uses the `org.tunaos.mandelbrot.Devel` app ID, so it installs next to the
published app.

## Prerequisites

### Rust

Install Rust with rustup and the nightly toolchain used for formatting; the step-by-step
instructions per distribution are in
[CONTRIBUTING.md → Rust toolchain](../CONTRIBUTING.md#rust-toolchain). The minimum supported Rust
version is `package.rust-version` in `Cargo.toml`.

### Flatpak builds

The Flatpak SDKs, the Rust SDK extension, and `flatpak-builder` are listed in
[CONTRIBUTING.md → Flatpak](../CONTRIBUTING.md#flatpak-prerequisites).

### Host builds

Everything `meson.build` checks for, with the minimum versions it requires:

* Meson ≥ 1.4 and Ninja
* `cargo` and `rustc`
* `blueprint-compiler`, `glib-compile-resources` and `glib-compile-schemas`
* a SASS compiler: `grass` (preferred) or `sass`
* development files for `glib-2.0` / `gio-2.0` ≥ 2.82, `gtk4` ≥ 4.20.2, `libadwaita-1` ≥ 1.8.0,
  `gstreamer-1.0`, `gstreamer-app-1.0`, `gstreamer-base-1.0`, `gstreamer-pbutils-1.0`,
  `gstreamer-play-1.0` and `gstreamer-video-1.0` ≥ 1.20, `gtksourceview-5`, `glycin-2` and
  `glycin-gtk4-2` ≥ 2.0.0, `libwebp`, `shumate-1.0` ≥ 1.1.0, `sqlite3` ≥ 3.24.0
* optionally `desktop-file-validate` and `appstreamcli`, which enable the `meson test`
  validations, and `cargo-nextest` for the `cargo-test` target

Package names differ between distributions; your distribution's package search for the
pkg-config names above (for example `dnf provides 'pkgconfig(gtk4)'` on Fedora) finds them.

## Build and run

### GNOME Builder

Open the clone in [GNOME Builder](https://apps.gnome.org/Builder/) and select
`build-aux/org.tunaos.mandelbrot.Devel.json` as the build configuration. Builder builds and runs the
app in the sandbox; its build terminal opens in the `_build` directory, where the Meson commands
below work.

### flatpak-builder

Build and install the development Flatpak, then run it:

```sh
flatpak-builder --user --install --force-clean app build-aux/org.tunaos.mandelbrot.Devel.json
flatpak run org.tunaos.mandelbrot.Devel
```

To build what CI builds, use `org.tunaos.mandelbrot.json` instead (app ID `org.tunaos.mandelbrot`).

CI runs Clippy inside the Flatpak build environment by stopping the build before the app module
and opening a build shell in it:

```sh
flatpak-builder --keep-build-dirs --user --disable-rofiles-fuse \
  --stop-at=fractal flatpak_app org.tunaos.mandelbrot.json
echo "cargo clippy --workspace --all-targets -- -D warnings" | \
  flatpak-builder --disable-rofiles-fuse --build-shell=fractal \
  flatpak_app org.tunaos.mandelbrot.json
```

### Host build

The app loads its compiled resources from the install prefix, so a host build has to be installed
before it runs:

```sh
meson setup --prefix=/usr/local -Dprofile=development _build
ninja -C _build
sudo ninja -C _build install
fractal
```

The binary is still called `fractal`, like the Meson project and the Cargo package.

### Build options and profiles

`meson.options` defines:

| Option | Values | Effect |
| --- | --- | --- |
| `profile` | `default` | Release build, app ID `org.tunaos.mandelbrot`. |
| | `beta` | Release build with the Beta profile. |
| | `development` | Debug build, app ID `org.tunaos.mandelbrot.Devel`, version suffixed with the Git commit, and the pre-commit hook installed into `.git/hooks/`. |
| | `hack` | Like `development`, with app ID `org.tunaos.mandelbrot.Hack`. |
| | `build-env` | Sets up the build environment only; nothing is compiled. |
| `calls-media` | `false` (default), `true` | Enables the `calls-media` Cargo feature (see [Native call media](#native-call-media)). |
| `disable-glycin-sandbox` | `false` (default), `true` | Disables the glycin image-loader sandbox. Only useful during development. |

Change an option on an existing build directory with
`meson configure _build -Dcalls-media=true`.

### Native call media

The calling engine (`matrixrtc/`) always builds, but the media layer — LiveKit's Rust SDK with a
statically linked libwebrtc, GStreamer microphone and camera capture — is behind the
`calls-media` Cargo feature, which maps to the `livekit` feature of the `mandelbrot-matrixrtc`
crate. It is off by default to keep host builds light; without it the app shows calls but makes no
media connection.

Building with it needs a C/C++ toolchain and pkg-config files for `glib-2.0` and `gio-2.0`. The
libwebrtc headers require Clang 21 or newer, which is why `.github/workflows/matrixrtc-e2e.yml`
installs Clang 21 on `ubuntu-latest`. The first build downloads and compiles a lot; expect it to
take several minutes.

## `cargo` directly

`src/config.rs` is generated by `meson setup` (from `src/config.rs.in`) and is not committed, so
run `meson setup` once before using `cargo` on the app crate. After that, the usual commands work
from the repository root:

```sh
cargo build
cargo test
cargo clippy --all-targets
cargo build --features calls-media
```

The `matrixrtc/` crate is independent of Meson and has its own `Cargo.lock`:

```sh
cargo test --manifest-path matrixrtc/Cargo.toml
cargo clippy --manifest-path matrixrtc/Cargo.toml --all-targets -- -D warnings
```

## Checks before a pull request

The `hooks/checks` crate runs every repository check: rustfmt (nightly), Clippy, typos,
cargo-machete, cargo-deny, `po/POTFILES.in`, `src/ui-blueprint-resources.in`, the data
GResource lists, cargo-sort, and rumdl for Markdown. A `development` Meson setup installs it as a
Git pre-commit hook. Run it by hand with the nightly that CI pins:

```sh
RUSTFMT_TOOLCHAIN=nightly-2026-08-28 cargo run --manifest-path hooks/checks/Cargo.toml
```

The value must match `RUSTFMT_TOOLCHAIN` in `.github/workflows/ci.yml`: the unstable options in
`.rustfmt.toml` format differently between nightlies. Missing tools are offered for installation
when the hook runs.

## Tests

| What | Command | Where CI runs it |
| --- | --- | --- |
| App unit tests | `cargo test --features calls-media` (after `meson setup`), or `meson compile -C _build src/cargo-test` with `cargo-nextest` installed | `Clippy (flatpak sandbox)` |
| Desktop file, AppStream metadata, and GSettings schema validation | `meson test -C _build` | `Flatpak build + tests` (`run-tests` in the manifest) |
| MatrixRTC conformance suite (`matrixrtc/tests/`, ported from matrix-js-sdk, including the Element Call scenarios) | `cargo test --manifest-path matrixrtc/Cargo.toml` | `MatrixRTC conformance tests` |
| Launch smoke test of the built bundle | `tests/gui/run-flatpak-smoke.sh Mandelbrot.flatpak` | `Flatpak launch smoke test` |
| MatrixRTC live-stack harness (Synapse + LiveKit + `lk-jwt-service`) | see [tests/e2e/README.md](../tests/e2e/README.md) | `MatrixRTC e2e harness` |

## How we know an update works

A dependency or runtime update (a Renovate PR or a manual bump) has to get past these checks:

| Check (job name) | What it proves | Runs on |
| --- | --- | --- |
| `Code checks (fmt, typos, deny, i18n)` | formatting, licenses/advisories (`cargo-deny`), unused deps (`cargo-machete`), resource lists | every PR, `main` |
| `MatrixRTC conformance tests` | the `matrixrtc` crate's clippy, tests and a 90 % line-coverage floor | every PR, `main` |
| `Clippy (flatpak sandbox)` | the app builds, lints clean and passes its unit tests in the GNOME SDK, **with `calls-media`**, as the bundle is built | every PR, `main` |
| `Flatpak build + tests` | the real bundle builds from the manifest and its Meson tests pass; uploads `Mandelbrot-Linux-CI` | every PR, `main` |
| `Flatpak launch smoke test` | that bundle installs with its runtime, starts on a headless Wayland display (mutter, as upstream Fractal's UI tests), restores sessions through the Secret portal, stays up 20 s, quits cleanly on `app.quit`, and logs no panic, `CRITICAL` or app `ERROR` | every PR, `main` (after the build) |
| `MatrixRTC e2e harness` | real calls between `join_call` clients over Synapse + LiveKit + `lk-jwt-service`: interop, reconnect, restricted SFU, a five-way huddle, a two-homeserver federated call | PRs touching `Cargo.toml`, `Cargo.lock`, `matrixrtc/**`, `src/session/call/**`, `src/session_view/call/**`, `tests/e2e/**`, the manifest or the workflow; nightly; manual dispatch |

Renovate (`renovate.json`) groups crates that must move together (matrix-sdk + ruma, gtk-rs and
the GNOME bindings, the LiveKit crates) and waits 3 days after a crate release. Because the e2e
workflow is path-filtered it cannot be a required check, so updates it covers (`matrixrtc/**`,
`tests/e2e/**`) are merged by Renovate itself only after every check, the e2e harness included,
is green. The app's own matrix-sdk/ruma/LiveKit bumps and action bumps in `publish-flatpak.yml`
never automerge: no check runs a call inside the app, and nothing on a PR runs the publish
workflow.

Locally:

```sh
# The smoke test, against a bundle (e.g. the Mandelbrot-Linux-CI artifact of a PR's CI run).
# Needs flatpak, mutter, dbus, gnome-keyring, libsecret-tools, xdg-desktop-portal(-gtk).
gh run download <run-id> -n Mandelbrot-Linux-CI
tests/gui/run-flatpak-smoke.sh Mandelbrot-Linux-CI.flatpak

# The e2e harness (needs docker or podman compose; see tests/e2e/README.md)
cargo build --manifest-path matrixrtc/Cargo.toml --features livekit --example join_call
cd tests/e2e && JOIN_CALL_BIN=../../target/debug/examples/join_call ./run-interop.sh
```

The smoke test isolates HOME, the XDG directories, the Flatpak installation and the D-Bus session
in a temporary directory, so it does not touch your own Mandelbrot data or keyring.

## A local homeserver

`tests/e2e/compose.yml` is a small Synapse + LiveKit + `lk-jwt-service` stack with open
registration, built for the MatrixRTC harness. It also works as a throw-away homeserver for the
app. With Podman (or replace `podman compose` with `docker compose`):

```sh
cd tests/e2e
podman compose -f compose.yml up -d

# Create a user (the harness does the same in harness-lib.sh's register())
curl -sf -X POST http://127.0.0.1:8008/_matrix/client/v3/register \
  -H 'Content-Type: application/json' \
  -d '{"username":"alice","password":"testpassword","auth":{"type":"m.login.dummy"}}'
```

In Mandelbrot, click **Advanced…** on the homeserver screen, turn off **Auto-Discovery**, enter
`http://127.0.0.1:8008`, and log in with the password. Stop the stack with
`podman compose -f compose.yml down -v`.

The stack's server name is `synapse.m.localhost`, and the app looks up LiveKit foci at
`https://<server name>/.well-known/matrix/client`, which this stack does not publish on the host.
App-level calls against it therefore find no LiveKit focus; the harness instead drives the
`join_call` example in `matrixrtc/examples/`, which is pointed at the stack directly. Test app
calls against a homeserver that advertises a focus (see [CALLING.md](CALLING.md#requirements)).

[runbooks/matrixrtc-operations.md](../runbooks/matrixrtc-operations.md) covers health checks and
failure modes of the stack.

## Editing the UI

* Widgets are defined in Blueprint (`.blp`) files next to their Rust code. Edit the `.blp`, never a
  generated `.ui` file.
* Every Blueprint file must be listed in `src/ui-blueprint-resources.in`, in alphabetical order (the
  checks enforce both). Meson compiles them with `build-aux/compile-blueprints.sh` and bundles them
  into `ui-resources.gresource`.
* Icons, the SCSS stylesheet (`data/resources/stylesheet/`), and other resources are listed in
  `data/resources/resources.gresource.xml` and compiled into `resources.gresource`.
* A new file with translatable strings must be added to `po/POTFILES.in`. The `po/*.po`
  translations themselves come from GNOME's translation platform; do not edit them by hand.

## Debugging

* **Logs**: the app uses `tracing`; `RUST_LOG` sets the filter (default `fractal=info,warn`, the
  development manifest uses `fractal=debug,warn`). Calling-engine logs use the
  `mandelbrot_matrixrtc` target. For a Flatpak build, pass it with
  `flatpak run --env=RUST_LOG=… org.tunaos.mandelbrot.Devel`.
* **Debugger**: `flatpak run --devel --command=sh org.tunaos.mandelbrot.Devel` gives a shell in the
  sandbox with the SDK's tools such as `gdb`.
* **Image loading**: if an image fails to load during development and you suspect the glycin
  sandbox, rebuild with `-Ddisable-glycin-sandbox=true`.

## Troubleshooting builds

| Problem | Fix |
| --- | --- |
| `cargo` fails with "file not found for module `config`" | Run `meson setup` first; it generates `src/config.rs`. |
| The code style check reformats files you did not touch | Your nightly differs from CI's; use `RUSTFMT_TOOLCHAIN` as shown above. |
| `meson setup` fails on a dependency version | The host libraries are older than `meson.build` requires; use the Flatpak build instead. |
| `Could not load gresource file` when starting a host build | The app was not installed to the prefix it was configured with; run `ninja -C _build install`. |
| `calls-media` build fails compiling `webrtc-sys` | Install Clang 21 or newer and the `glib-2.0` development files (see [Native call media](#native-call-media)). |
