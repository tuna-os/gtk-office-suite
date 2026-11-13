# Cutover: Lifafa and Mandelbrot into the suite repository

Lifafa (`lifafa/`, imported in #1957) and Mandelbrot (`mandelbrot/`, #1961)
now live in this repository, but their standalone repositories,
[tuna-os/lifafa](https://github.com/tuna-os/lifafa) and
[tuna-os/mandelbrot](https://github.com/tuna-os/mandelbrot), still publish
the apps, host their issue trackers and track upstream. This is the list of
what has to move before either repository can be archived, in the order it
has to happen. Archive a repository only when every box above its own
"Archive" step is ticked.

What the two apps do today, for reference:

| | Lifafa | Mandelbrot |
|---|---|---|
| Upstream | [hyprlab/hylki](https://github.com/hyprlab/hylki), merged continuously (`lifafa/fork/MAINTAINING.md`) | Fractal (GNOME), no remote (`mandelbrot/RELEASING.md`) |
| App ID shipped | `org.tunaos.lifafa`, rewritten from `co.hyprlab.Hylki` at build time by `lifafa/fork/rebrand.sh` | `org.tunaos.mandelbrot` |
| How users get updates | A signed OSTree repository on GitHub Pages: `https://tuna-os.github.io/lifafa/org.tunaos.lifafa.flatpakref` (`lifafa/fork/flatpak-repo.sh`, key `lifafa/fork/flatpak-repo.asc`) | OCI images on `ghcr.io/tuna-os/mandelbrot`, indexed in the TunaOS remote `oci+https://tunaos.org/flatpak` |
| Versions | Upstream's plus a suffix, e.g. `1.47.1-tuna6` | Its own, e.g. `v14.1.1` |
| Licence | AGPL-3.0-or-later | GPL-3.0-or-later |
| Workflows in its own repo, not imported | `upstream-sync`, `fork-release`, `tuta-helper`, `proton-helper` | `publish-flatpak` (the org's reusable workflow), CI |
| What this repository runs | `.github/workflows/lifafa.yml`: build, tests, RustSec and licences | `.github/workflows/mandelbrot.yml`: `matrixrtc` tests and the Flatpak build |
| Separate repositories it needs that stay | [tuta-unofficial-sdk](https://github.com/tuna-os/tuta-unofficial-sdk), [proton-unofficial-sdk](https://github.com/tuna-os/proton-unofficial-sdk) (submodules) | none |

The suite's own apps publish the way Mandelbrot does: `publish-flatpak.yml`
calls `tuna-os/.github`'s reusable workflow per app, to GHCR and the TunaOS
remote, behind the evidence and soak gates.

## 0. Decisions

Taken on 2026-11-13; change them here if they turn out wrong.

- **Code:** this repository is where both apps are maintained from now on.
  The standalone repositories take no new work; anything merged there before
  they are frozen is carried here.
- **Lifafa's channel:** the TunaOS OCI remote, with the Pages OSTree repository
  still published until two releases have gone out on both.
- **Tags:** `lifafa-v<version>` and `mandelbrot-v<version>`, each app keeping its
  own version (Lifafa's follows upstream Hylki's).
- **Upstream:** Lifafa keeps tracking hyprlab/hylki; Mandelbrot records its
  Fractal base and pulls in Fractal changes by hand.
- **Gates:** their releases wait on their CI (`lifafa.yml`, `mandelbrot.yml`)
  until they have journeys of their own (INTEGRATION-PLAN B12), then on those.

The questions those answer:


- [ ] **Where the code is maintained.** This repository, from a named date.
  Until then the standalone repository stays authoritative and changes made
  here are carried back to it, or the two copies drift.
  `lifafa/fork/MAINTAINING.md` says "the code stays here" about tuna-os/lifafa,
  and `mandelbrot/AGENTS.md` is titled for tuna-os/mandelbrot; both are rewritten
  at the switch (step 4).
- [ ] **Lifafa's update channel.** Either move it to the TunaOS OCI remote with
  the other four apps (one remote for the whole suite), or keep publishing the
  Pages OSTree repository. Every existing Lifafa install follows the Pages
  repository, so moving means publishing to both for a transition and telling
  users to switch (the way `lifafa/fork/CHANGELOG.md` handled the move off
  bundles). Recommended: move, with both published until two releases have
  gone out on each.
- [ ] **Versions and tags.** The suite tags `vX.Y.Z` for Letters, Tables and
  Decks. Either the two apps keep their own versions under prefixed tags
  (`lifafa-v1.48.0-tuna1`, `mandelbrot-v14.2.0`), or they take the suite's.
  Recommended: their own versions under prefixed tags, because Lifafa's
  follows upstream Hylki's.
- [ ] **Whether Lifafa keeps tracking upstream Hylki.** If it does, the
  upstream merge needs a home here (step 5); if not, say so in its README
  and stop.
- [ ] **What gates their releases.** The suite's soak runs only Letters,
  Tables and Decks journeys. Decide whether Lifafa and Mandelbrot releases
  wait on journeys of their own (none exist yet: INTEGRATION-PLAN B12) or
  on their CI alone for now.

## 1. CI parity: everything the standalone repositories check, checked here

- [ ] List each standalone repository's workflows and required checks, and
  map each to a job here or to "dropped, because …".
- [ ] Lifafa: `lifafa.yml` builds and tests it; add whatever `fork-release`
  checks before a release that `lifafa.yml` does not (the Flatpak build of the
  rebranded manifest, `fork/check-touchpoints.sh`).
- [ ] Lifafa: decide where `tuta-helper` and `proton-helper` (the helpers built
  from the two SDK submodules) are built, and add them.
- [ ] Mandelbrot: `mandelbrot.yml` builds the Flatpak; compare with its own CI
  (tests, `cargo deny`, lint) and add what is missing.
- [ ] Every new job classified in the required-checks table
  (`tests/test_required_checks.py`) and its actions locked
  (`.github/workflows/actions.lock`).
- [x] Renovate: `mandelbrot/renovate.json` is not read in a subdirectory, so
  its rules are folded into the root `renovate.json`, scoped to
  `mandelbrot/**`, and the file is removed.

## 2. Publishing from this repository

- [ ] Mandelbrot: add it to `publish-flatpak.yml` (manifest
  `mandelbrot/org.tunaos.mandelbrot.json`, `repo-name: tuna-os/mandelbrot` so
  the image path stays `ghcr.io/tuna-os/mandelbrot`), with its own tag
  pattern, and the GitHub Release notes it makes today
  (`build-aux/release-notes.py`, image digests).
- [ ] Lifafa: a publish job that runs `fork/rebrand.sh`, builds the manifest,
  and publishes to the channel decided in step 0 (both, during the
  transition). The Pages OSTree repository needs the signing key as a secret
  here and a `gh-pages` checkout of tuna-os/lifafa, or a new Pages site with
  a redirect.
- [ ] One dry run of each from a branch, publishing nothing, then one real
  release of each from here, installed and updated on a clean machine with
  `flatpak update`.
- [ ] The standalone repositories' publish workflows disabled after that
  release, so nothing publishes from two places.

## 3. Secrets and access

- [ ] Inventory the secrets each standalone repository's workflows use (at
  least Lifafa's Flatpak signing key, `GPG_HOMEDIR` in `flatpak-repo.sh`) and
  add them here, with the narrowest scope that works.
- [ ] GHCR package permissions: this repository's workflows can push to
  `ghcr.io/tuna-os/mandelbrot` (and to Lifafa's package if it moves to OCI).
- [ ] The two SDK submodules stay where they are; check that this
  repository's checkouts can fetch them (they are public today).

## 4. Identity, links and documents

- [ ] Lifafa's metainfo (`lifafa/data/co.hyprlab.Hylki.metainfo.xml`) still
  names Hylki's homepage, bug tracker and source; `fork/rebrand.sh` rewrites
  some of these at build time. Check what the built app shows (About, the
  welcome page, "Report a problem") and point it at this repository's issues
  with a `lifafa` label, or keep tuna-os/lifafa's tracker open (step 6).
- [x] Mandelbrot's metainfo, About and `SECURITY.md` (advisories at
  tuna-os/mandelbrot) pointed at this repository. Before this they named
  upstream Fractal's homepage and tracker, so About's "Report an Issue" sent
  Mandelbrot's bugs to Fractal. The homepage is also the OAuth client URI
  homeservers show at sign-in.
- [ ] `lifafa/fork/MAINTAINING.md`, `lifafa/README.md`, `mandelbrot/README.md`,
  `mandelbrot/RELEASING.md` and `mandelbrot/AGENTS.md` rewritten for this
  repository (where the code lives, how a release is cut, where to report).
- [x] The stale note at the top of `.github/workflows/lifafa.yml` and the root
  `Cargo.toml` (Lifafa "gtk4 0.9") corrected: Lifafa is on gtk4 0.11 now, and
  `renovate.json`'s Lifafa toolkit rule says why it still moves only by hand.
  Whether it can then join the root workspace is a separate question.
- [ ] The suite README and `docs/INTEGRATION-PLAN.md` say where each app is
  released from.

## 5. Upstream tracking

- [x] Lifafa: a workflow here that brings upstream into `lifafa/` and opens a
  pull request, replacing `upstream-sync`: `.github/workflows/lifafa-upstream-sync.yml`
  runs `lifafa/fork/sync-upstream.sh`. Lifafa was imported squashed, so
  `git subtree` has no common history to merge from; the script applies
  upstream's own diff since `lifafa/fork/UPSTREAM_BASE` (hyprlab/hylki
  `b5650f58`, the upstream commit the import was built on) three-way, and
  leaves conflict markers where the fork changed the same lines.
- [ ] One upstream merge done this way end to end before the switch. A dry
  run on 2026-11-13 (98 upstream commits, to `a86e1282`) changed 296 files
  and left 36 conflicting hunks in 23, most where this repository reworked
  Lifafa (the in-tree relm, gtk4 0.11, A2); resolving them and the build is
  its own pull request. tuna-os/lifafa also has three fork fixes since the
  import to carry (`66f86c48`, `dd0ddadd`, `76d5bc62`).
- [ ] Lifafa: sending fixes upstream (MAINTAINING.md's first rule) still
  works from a monorepo: a branch of hyprlab/hylki with the change applied,
  not a branch of this repository. Write the steps down.
- [ ] Mandelbrot: record which Fractal release it is based on, and whether
  Fractal changes are pulled in, and how.

## 6. Issues, pull requests and security reports

- [ ] Labels here for `lifafa` and `mandelbrot`.
- [ ] Open issues transferred here (GitHub's issue transfer keeps comments
  and links), or closed with a pointer. Open pull requests landed, closed,
  or re-opened here.
- [ ] Private vulnerability reporting turned on here, and open advisories in
  either repository dealt with or moved.
- [x] Issue templates for the two apps added to `.github/ISSUE_TEMPLATE`: the
  bug report and feature request forms list Lifafa and Mandelbrot.

## 7. The switch

- [ ] Announce the date in both repositories' READMEs and pinned issues.
- [ ] Last release from each standalone repository, its notes saying where
  the next one comes from.
- [ ] A notice at the top of each standalone README: "Development moved to
  tuna-os/gtk-office-suite (`lifafa/`, `mandelbrot/`)", with the issue and
  release links.

## 8. Archive

- [ ] tuna-os/mandelbrot archived, once its next release has gone out from
  here and installs update from it.
- [ ] tuna-os/lifafa archived, once its next release has gone out from here
  and existing installs update. If its GitHub Pages site still serves the
  update repository, archiving keeps the site up but nothing can publish to
  it: move the channel (step 0) first, or keep the repository unarchived
  and read-only by branch protection until the transition ends.
- [ ] Not archived: tuna-os/tuta-unofficial-sdk and
  tuna-os/proton-unofficial-sdk, which Lifafa still builds from.

## Afterwards

- [ ] Remove the fork-only notes that only made sense in a separate
  repository, and fold `lifafa/fork/` into the suite's own release
  documentation where it overlaps (`docs/RELEASE.md`).
- [ ] Revisit whether Lifafa and Mandelbrot can share the root workspace and
  `suite-common` (INTEGRATION-PLAN C1), now that they are maintained here.
