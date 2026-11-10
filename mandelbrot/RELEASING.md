# Releasing Mandelbrot

This describes how this fork actually ships, which differs from upstream
Fractal: there is no GitLab remote, and publishing does not go through a
Flathub pull request. Releases are OCI Flatpaks pushed to GHCR by a GitHub
Actions workflow when a `v*` tag lands, indexed for the TunaOS Flatpak remote,
and announced as a GitHub Release.

## How a build reaches users

`.github/workflows/publish-flatpak.yml` runs on a `v*` tag push or on manual dispatch. Its
`publish` job calls this org's reusable `tuna-os/.github/.github/workflows/publish-flatpak.yml`,
which:

1. builds `org.tunaos.mandelbrot.json` with `flatpak-builder` once per architecture (`x86_64` and
   `aarch64`, the reusable workflow's default `archs`) and exports each build as an OCI image;
2. pushes the images to `ghcr.io/tuna-os/mandelbrot` as `latest-<arch>` and `latest`, plus
   `<tag>-<arch>` (for example `v14.1.1-x86_64`) when the run is for a tag;
3. updates the central TunaOS Flatpak index with the `latest` images. The `tuna-os` remote that
   `README.md` tells users to add (`oci+https://tunaos.org/flatpak`) serves that index, so this is
   the step that makes `flatpak update` deliver the build.

Then, for tag runs only, the `github-release` job creates (or, on a re-run, updates) the GitHub
Release for the tag:

* the title is `Mandelbrot <version>`;
* the notes are the matching `<release>` entry of `data/org.tunaos.mandelbrot.metainfo.xml.in.in`,
  converted to Markdown by `build-aux/release-notes.py`, followed by install instructions;
* the notes list, per architecture, the `<tag>-<arch>` image and its immutable digest
  (`ghcr.io/tuna-os/mandelbrot@sha256:…`), read back from GHCR with `skopeo inspect`;
* releases whose metainfo `type` is not `stable` are marked as pre-releases.

The release has **no downloadable files**: the Flatpak only exists as OCI images, and the digests
are their SHA-256 checksums. (The reusable workflow's `attach-bundles` option, which would upload
`.flatpak` bundles, is not enabled.)

A manual dispatch from a branch (for example `main`) also publishes `latest`, and therefore also
updates what users get, but creates no version tag and no GitHub Release.

## Before making a new release

* Update dependencies (crates or system libraries) and migrate from deprecated APIs.
* Make sure `org.tunaos.mandelbrot.json` targets the runtime version this release should ship
  against (currently GNOME `50`, see `runtime-version` in that file).
* Check that `main` is green in [CI](https://github.com/tuna-os/mandelbrot/actions/workflows/ci.yml):
  code checks, the `MatrixRTC conformance tests` job (the `matrixrtc/` suite ported from
  matrix-js-sdk, including the Element Call scenarios), Clippy, and the Flatpak build with its
  metadata validation tests.
* For releases that change calling, also run the live-stack MatrixRTC harness: **Actions →
  MatrixRTC e2e → Run workflow** (`.github/workflows/matrixrtc-e2e.yml`), or locally as described
  in `tests/e2e/README.md`. Its current state is tracked in
  [#66](https://github.com/tuna-os/mandelbrot/issues/66); if the workflow is disabled or red for
  that known reason, record that in the release pull request.
* Install the `Mandelbrot-Linux-CI` artifact from the CI run of the release commit and smoke-test
  login, a room, and (if calling changed) a call.

## Making a release

1. Bump the version:
   * `/meson.build`: update `version` in `project()` and `major_version` (and
     `pre_release_version` for a beta/pre-release; leave it empty for stable).
   * `/Cargo.toml`: update `version` to match, using semver.
2. Update `/data/org.tunaos.mandelbrot.metainfo.xml.in.in`: add a new `release` entry at the top
   of `releases`, with `version` equal to the tag without its `v` (tag `v14.1.2` → `14.1.2`),
   today's `date`, `type="stable"` (or `development` for a pre-release), and a user-facing
   `description`. This text becomes both the app's release notes and the GitHub Release notes;
   the `github-release` job fails if the entry is missing. Preview the notes with:

   ```sh
   python3 build-aux/release-notes.py notes 14.1.2 v14.1.2 x86_64=sha256:0 aarch64=sha256:0
   ```

3. Commit these version-bump changes, get them merged to `main` via the normal PR process
   (`CONTRIBUTING.md`).
4. Tag the merge commit and push the tag:

   ```sh
   git tag vMAJOR.MINOR.PATCH
   git push origin vMAJOR.MINOR.PATCH
   ```

   The tag must start with `v` — that is what `.github/workflows/publish-flatpak.yml` triggers
   on.
5. Watch the **Build and Publish Flatpak OCI** run for the tag. When it finishes, the
   [Releases page](https://github.com/tuna-os/mandelbrot/releases) shows the new release with the
   image digests, and `flatpak update` on a machine with the `tuna-os` remote picks up the build.

## Recovery

* **A build or publish job failed** (for example a transient CI failure): re-run the failed jobs
  of the tag's run from the Actions page. Re-running publishes the same tag again and the
  `github-release` job updates the existing release instead of failing.
* **Only the `github-release` job failed**: re-run just that job. It reads the digests of the
  images already pushed for the tag, so nothing is rebuilt.
* **The release notes are wrong**: fix the metainfo entry on `main` for future builds, and edit
  the text of the existing GitHub Release by hand (the tag cannot change). A later re-run of the
  job for that tag would overwrite manual edits with the metainfo text of the tagged commit.
* **A tag was published without a GitHub Release** (such as tags from before the
  `github-release` job existed): create it by hand from an up-to-date `main` checkout — the
  metainfo keeps the entries of past releases. Read the digests, write the notes, and create the
  release:

  ```sh
  skopeo inspect --format '{{.Digest}}' docker://ghcr.io/tuna-os/mandelbrot:vX.Y.Z-x86_64
  skopeo inspect --format '{{.Digest}}' docker://ghcr.io/tuna-os/mandelbrot:vX.Y.Z-aarch64
  python3 build-aux/release-notes.py notes X.Y.Z vX.Y.Z \
    x86_64=sha256:… aarch64=sha256:… > notes.md
  gh release create vX.Y.Z --verify-tag --title "Mandelbrot X.Y.Z" --notes-file notes.md
  ```

  Do **not** re-run the publish workflow on an old tag just to get a release: that rebuilds the
  tag and publishes it as `latest`, which rolls users back.
* **A release is broken and users must go back**: publishing is what moves `latest`, so roll back
  by publishing a known-good build — run **Actions → Build and Publish Flatpak OCI → Run workflow**
  on the last good tag. That rebuilds the tag and points `latest` at it. Then mark the broken
  GitHub Release as a pre-release (or delete it) and fix forward with a new tag. Never move or
  re-push an existing tag.

## Translations

Mandelbrot inherits Fractal's translations from the GNOME translation team on
[Damned Lies](https://l10n.gnome.org/module/fractal/) — see `po/*.po`. This fork does not run its
own translation-branch sync; `README.md`'s Contributing → Translations section has the current
pointer to where translators should work.
