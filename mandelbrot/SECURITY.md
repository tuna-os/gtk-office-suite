# Security policy

## Reporting a vulnerability

**Do not open a public issue, pull request, or discussion for a security vulnerability.**

Report it privately through GitHub's private vulnerability reporting:

1. Open the [Security tab](https://github.com/tuna-os/gtk-office-suite/security) of tuna-os/gtk-office-suite, where Mandelbrot is maintained.
2. Click **Report a vulnerability**, or go straight to
   [the new advisory form](https://github.com/tuna-os/gtk-office-suite/security/advisories/new).
3. Describe the affected component (app, `matrixrtc/` crate, Flatpak packaging, CI workflows),
   the version or commit, the impact, reproduction steps, and a suggested fix if you have one.

Only the reporter and the repository maintainers can see the report. We aim to acknowledge it
within **5 business days**, in line with the
[TunaOS organization security policy](https://github.com/tuna-os/.github/blob/main/SECURITY.md),
keep you updated in the advisory while we work on a fix, and credit you in the published advisory
unless you prefer otherwise. Please give us a reasonable window to ship the fix before disclosing
details publicly.

## Supported versions

Fixes are made on `main` and shipped as a new build on the TunaOS Flatpak remote
(`org.tunaos.mandelbrot`; see [RELEASING.md](RELEASING.md)). Only the latest published build is
supported: run `flatpak update org.tunaos.mandelbrot` to get it. Older builds and other packagings
do not receive fixes.

## Scope

In scope:

* the Mandelbrot application code in this repository, including code inherited from Fractal as
  shipped in Mandelbrot;
* the `mandelbrot-matrixrtc` crate (`matrixrtc/`) and the native calling integration;
* the Flatpak manifests and their sandbox permissions;
* this repository's GitHub Actions workflows and release pipeline.

Report to the upstream project instead when the issue is in:

* **upstream Fractal** and also affects Fractal itself — report it to the Fractal maintainers as a
  confidential issue on [GNOME GitLab](https://gitlab.gnome.org/World/fractal/-/issues), and tell
  us so we can ship the fix;
* the **Matrix Rust SDK**, the Matrix protocol, or a Matrix.org service — see the
  [Matrix.org security disclosure policy](https://matrix.org/security-disclosure-policy/);
* **LiveKit**, GTK, GStreamer, the GNOME runtime, or Flatpak — report to their maintainers;
* a **homeserver** or bridge implementation — report to its maintainers.

If you are unsure, report it here privately and we will route it.

## Automation

This organization runs automated agents that open issues and pull requests. Do not share
vulnerability details with a bot in a public thread; use the private channel above.

For what Mandelbrot does and does not protect, see
[docs/PRIVACY.md](docs/PRIVACY.md).
