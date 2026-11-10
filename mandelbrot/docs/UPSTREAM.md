# Upstream Synchronization and Tracking

Mandelbrot is a downstream fork of [GNOME Fractal](https://gitlab.gnome.org/World/fractal).
This document tracks the upstream baseline, describes divergence boundaries, and outlines the
process for synchronizing upstream changes and security fixes.

## Baseline

* **Upstream Repository**: `https://gitlab.gnome.org/World/fractal.git`
* **Upstream License**: GPL-3.0-or-later
* **Initial Fork Baseline Commit**: `a262c7d656cc4c3a87656370e10cd279cf6e081a`
* **Initial Fork Baseline Tag**: Fractal 14.1 (released July 19, 2026)
* **First Mandelbrot Tag**: v14.1.1 (released July 24, 2026)
* **Current Sync Point**: upstream `fractal-14` @ `561b073e` (Fractal 14.3), merged
  2026-11-07, plus the `main` cherry-picks listed in [Sync History](#sync-history)

## Divergence Boundaries

Mandelbrot maintains full feature parity with Fractal while developing downstream extensions:

1. **Native MatrixRTC Calling (`matrixrtc/` crate, `src/session/call/` & `src/session_view/call/`)**:
   * MatrixRTC session state machine and MSC4140 delayed event handling.
   * LiveKit Rust SDK media pipeline and GStreamer camera/microphone capture.
   * Native GTK4/Adwaita calling UI shell and freedesktop notification portal v2 integration.

2. **Simplified Sliding Sync (MSC4186)**:
   * Uses `matrix-sdk-ui` `SyncService` and `RoomListService` with fallback to classic sync.

3. **QR Code Login (MSC4108)**:
   * Bi-directional OAuth QR scanning and grantor device link flows.

4. **Timeline Enhancements**:
   * MSC3440 thread support with adaptive side pane.
   * MSC3381 poll creation and voting.
   * MSC3245 voice message recording.
   * Spaces hierarchy browsing and spaces navigation (space switcher rail, per-space room list).

5. **Packaging and Infrastructure**:
   * GHCR OCI Flatpak packaging (`org.tunaos.mandelbrot.json`).
   * Upstream sync automation script (`build-aux/sync-upstream.sh`).

## Synchronization Process

### Which upstream branch to take

Upstream has two kinds of branches:

* **Stable branches (`fractal-N`)** only carry Fractal releases (e.g. `fractal-14` → 14.1, 14.2,
  14.3). They build against released `matrix-sdk`/`ruma` crates. These are **merged** with a real
  merge commit, so git records exactly which upstream release we contain and later merges only
  bring in what is new.
* **`main`** is Fractal's development branch. It regularly pins `matrix-sdk` and `ruma` to
  unreleased git revisions (as of November 2026 it does), and carries large refactors. Mandelbrot
  only uses released crates, so `main` is **not merged**: bug fixes and translations from it are
  taken with `git cherry-pick -x <sha>`, which records the upstream commit in a
  `(cherry picked from commit …)` trailer. Skip dependency bumps (our own dependency updates
  supersede them) and anything that needs unreleased SDK APIs.

Merge sync PRs with GitHub's "Create a merge commit" method. Squash or rebase merging flattens the
upstream merge commit, and the next sync would then re-merge the same upstream history.

### Synchronization Script

The sync script is located at `build-aux/sync-upstream.sh`.

To run upstream synchronization:

```sh
./build-aux/sync-upstream.sh [upstream_branch]
```

It performs the following steps:

1. Fetches latest branches and tags from `https://gitlab.gnome.org/World/fractal.git`.
2. Picks the upstream branch: the argument if given, otherwise the newest stable `fractal-N`
   branch. Passing `main` is possible but warns about the git dependencies it would bring in.
3. Computes the upstream commits not yet merged (from the merge base with `HEAD`).
4. If there are any, creates a sync branch (`sync/upstream-YYYYMMDD`) from the current `HEAD` and
   runs `git merge --no-commit --no-ff`, reporting success or conflicts.
5. Lists the `upstream/main` commits whose change is not in `HEAD` yet, as cherry-pick candidates.

### Conflict Resolution Strategy

When resolving conflicts during an upstream sync:

* **Dependencies**: Keep ours for `Cargo.toml`, `Cargo.lock`, `deny.toml`,
  `hooks/checks/Cargo.toml` and the Flatpak manifests. This fork moves to the latest released
  crates on its own schedule, which supersedes upstream's version bumps.
* **Upstream-only infrastructure**: Files this fork deleted or renamed (`.gitlab-ci.yml`,
  `.gitlab-ci/`, `org.gnome.Fractal*` manifests) stay deleted; `.github/` is ours.
* **Branding and releases**: Keep ours for `README.md`, the Mandelbrot app ID, metainfo URLs and
  `<releases>`, and the `meson.build` version; Mandelbrot has its own release numbering. Take
  upstream's other metainfo changes (descriptions, screenshots, content rating).
* **Translations**: Take upstream's `po/*.po` and `po/LINGUAS` as-is.
* **Core Matrix Features**: Prefer upstream fixes for common matrix-sdk interactions and protocol
  parsers unless they conflict with Mandelbrot's sliding sync / MatrixRTC integrations.
* **MatrixRTC / Calls**: Mandelbrot's `matrixrtc/` crate and calling components are downstream
  additions; ensure changes to `src/session/` preserve call manager hooks.
* **Crate Naming**: Keep `Cargo.toml` and `meson.build` crate metadata aligned with upstream
  tracking until a scheduled tree-wide rename.
* **Repository Governance Files**: `AGENTS.md` states this fork's own contributor-automation
  contract and must not be replaced wholesale by an upstream merge. If a sync brings in an
  upstream change to that file (or to `CONTRIBUTING.md`'s policy sections), reconcile it —
  keep upstream's substantive policy changes, but do not let a merge silently reinstate
  content this fork has deliberately overridden. This is the gap that let
  upstream's "Instructions for LLM agents" file (a satirical policy-against-AI statement,
  see `CONTRIBUTING.md`'s "LLM Contributions" section for the real policy it stood in for)
  sit unreconciled in `AGENTS.md` for weeks after this fork started running the App-authored
  maintenance the old file's own text told every agent not to do.

## Sync History

### 2026-11-07

* **Merged** `upstream/fractal-14` @ `561b073e` (Fractal 14.3, including 14.2). Its source changes
  were already in the 14.1 baseline except a clippy allow in `src/secret/linux.rs` and the
  `Cow`-based fix hint in `hooks/checks`. Upstream's dependency/security bumps (rustls,
  rustls-webpki, h2, chacha20, ruma, matrix-sdk, gettext-rs) were not taken: the move to the
  latest released crates and the GNOME 51 runtime (#236) supersedes them.
* **Cherry-picked** from `upstream/main` (no git dependencies needed):
  * `7fe6ef26` explore: Use AdwStatusPage for empty search results
  * `b42c2043` Disable automatic text selection of room topic — plus a downstream fix: upstream
    calls `grab_focus()` inside `debug_assert!()`, which does not run in release builds
  * `83551215` Unify margins for room dialogs
  * `969b6cd7` calls: fix text wrapping
  * `1e5c1cab` call-row: fix text wrapping mode
  * `7971693c` map: shorten attribution string
  * `6da6b3e9` map: scale the width of the widget for small screens
  * `6b124990` pill: reduce maximum name length to 20 chars
  * Translations: `13fdf845` Kazakh, `da2bb6c2` Serbian, `5fe03f8b` + `e63ad3a5` Chinese (China),
    `c537082a` Turkish, `e210ba3b` Basque, `a628e368` Danish, `9fbc7283` Belarusian (new
    language)
* **Taken via #236** (dependency update): `e97ffc53` and `4f0545cf` (test assertions moved off
  `assert_matches!`), adapted to released crates.
* **Not taken** from `upstream/main`: `f6ff0e42` (fixes a typo in README text this fork no longer
  has), dependency bumps, the login and secret refactors, `cfg_if!` → `cfg_select!`, REUSE
  licensing, GitLab CI changes, and `840d7519`/`4087fc21` (Freedesktop 26.08 build docs: this
  fork's rewritten `CONTRIBUTING.md` already names the 26.08 SDK extensions since #236).
