# Releasing Mandelbrot

Mandelbrot is released from the suite's repository,
[tuna-os/gtk-office-suite](https://github.com/tuna-os/gtk-office-suite), where
its code lives in `mandelbrot/`. This differs from upstream Fractal: there is
no GitLab remote, and publishing does not go through a Flathub pull request.
Releases are OCI Flatpaks pushed to GHCR when a `mandelbrot-v*` tag lands,
indexed for the TunaOS Flatpak remote, and announced as a GitHub Release.
Mandelbrot keeps its own version; the suite's plain `v*` tags release Letters,
Tables and Decks and do not start this. Releases up to v14.x were cut in
tuna-os/mandelbrot with plain `v*` tags, and their GitHub Releases stay there.

## How a build reaches users

The suite's `.github/workflows/publish-mandelbrot.yml` runs on a
`mandelbrot-v*` tag push or on manual dispatch. Its `publish` job calls the
org's reusable `tuna-os/.github/.github/workflows/publish-flatpak.yml`, the same
one that publishes Letters, Tables and Decks, which:

1. builds `mandelbrot/org.tunaos.mandelbrot.json` with `flatpak-builder` once per architecture
   (`x86_64` and `aarch64`, the reusable workflow's default `archs`) and exports each build as an
   OCI image;
2. pushes the images to `ghcr.io/tuna-os/mandelbrot` as `latest-<arch>` and `latest`, plus
   `<tag>-<arch>` (for example `mandelbrot-v14.3.0-x86_64`) when the run is for a tag;
3. updates the central TunaOS Flatpak index with the `latest` images. The `tuna-os` remote that
   `README.md` tells users to add (`oci+https://tunaos.org/flatpak`) serves that index, so this is
   the step that makes `flatpak update` deliver the build;
4. attaches a `.flatpak` bundle per architecture to the GitHub Release (`attach-bundles`).

Then, for tag runs only, the `github-release` job writes the GitHub Release for the tag:

* the title is `Mandelbrot <version>`;
* the notes are the matching `<release>` entry of
  `mandelbrot/data/org.tunaos.mandelbrot.metainfo.xml.in.in`, converted to Markdown by
  `mandelbrot/build-aux/release-notes.py`, followed by install instructions;
* the notes list, per architecture, the `<tag>-<arch>` image and its immutable digest
  (`ghcr.io/tuna-os/mandelbrot@sha256:…`), read back from GHCR with `skopeo inspect`;
* releases whose metainfo `type` is not `stable` are marked as pre-releases.

A manual dispatch builds both architectures and pushes nothing unless `publish` is ticked. With
`publish` ticked it publishes `latest`, and therefore updates what users get, but creates no
version tag and no GitHub Release.

Mandelbrot's release waits on its own CI (`mandelbrot.yml`), not on the suite's soak, which has no
Mandelbrot journeys yet.

## Before making a new release

* Update dependencies (crates or system libraries) and migrate from deprecated APIs.
* Make sure `mandelbrot/org.tunaos.mandelbrot.json` targets the runtime version this release
  should ship against (see `runtime-version` in that file; `publish-mandelbrot.yml` builds it in
  the matching `flatpak-builder-image`).
* Check that `main` is green in the suite's
  [Mandelbrot workflow](https://github.com/tuna-os/gtk-office-suite/actions/workflows/mandelbrot.yml):
  the MatrixRTC engine's Clippy and tests (the `matrixrtc/` suite ported from matrix-js-sdk,
  including the Element Call scenarios) and the Flatpak build. A build-only manual run of
  `publish-mandelbrot.yml` (with `publish` off) checks the release build of both architectures.
* For releases that change calling, also run the live-stack MatrixRTC harness locally as
  described in `tests/e2e/README.md`. Its workflow was not imported into the suite; its state was
  tracked in [tuna-os/mandelbrot#66](https://github.com/tuna-os/mandelbrot/issues/66). Record in
  the release pull request whether it ran.
* Build and install the release commit's Flatpak (`docs/DEVELOPMENT.md`) and smoke-test login,
  a room, and (if calling changed) a call.

## Making a release

1. Bump the version:
   * `mandelbrot/meson.build`: update `version` in `project()` and `major_version` (and
     `pre_release_version` for a beta/pre-release; leave it empty for stable).
   * `mandelbrot/Cargo.toml`: update `version` to match, using semver.
2. Update `mandelbrot/data/org.tunaos.mandelbrot.metainfo.xml.in.in`: add a new `release` entry
   at the top of `releases`, with `version` equal to the tag without its `mandelbrot-v` (tag
   `mandelbrot-v14.3.0` → `14.3.0`),
   today's `date`, `type="stable"` (or `development` for a pre-release), and a user-facing
   `description`. This text becomes both the app's release notes and the GitHub Release notes;
   the `github-release` job fails if the entry is missing. Preview the notes with:

   ```sh
   python3 mandelbrot/build-aux/release-notes.py notes 14.3.0 mandelbrot-v14.3.0 x86_64=sha256:0 aarch64=sha256:0
   ```

3. Commit these version-bump changes and get them merged to the suite's `main` through a pull
   request.
4. Tag the merge commit and push the tag:

   ```sh
   git tag mandelbrot-vMAJOR.MINOR.PATCH
   git push origin mandelbrot-vMAJOR.MINOR.PATCH
   ```

   The tag must start with `mandelbrot-v`: that is what `publish-mandelbrot.yml` triggers on. A
   plain `v*` tag would release the office apps instead.
5. Watch the **Publish Mandelbrot** run for the tag. When it finishes, the suite's
   [Releases page](https://github.com/tuna-os/gtk-office-suite/releases) shows the new release
   with the image digests, and `flatpak update` on a machine with the `tuna-os` remote picks up
   the build.

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
  skopeo inspect --format '{{.Digest}}' docker://ghcr.io/tuna-os/mandelbrot:mandelbrot-vX.Y.Z-x86_64
  skopeo inspect --format '{{.Digest}}' docker://ghcr.io/tuna-os/mandelbrot:mandelbrot-vX.Y.Z-aarch64
  python3 mandelbrot/build-aux/release-notes.py notes X.Y.Z mandelbrot-vX.Y.Z \
    x86_64=sha256:… aarch64=sha256:… > notes.md
  gh release create mandelbrot-vX.Y.Z --verify-tag --title "Mandelbrot X.Y.Z" --notes-file notes.md
  ```

  Do **not** re-run the publish workflow on an old tag just to get a release: that rebuilds the
  tag and publishes it as `latest`, which rolls users back.
* **A release is broken and users must go back**: publishing is what moves `latest`, so roll back
  by publishing a known-good build — run **Actions → Publish Mandelbrot → Run workflow** on the
  last good tag, with `publish` ticked. That rebuilds the tag and points `latest` at it. Then mark the broken
  GitHub Release as a pre-release (or delete it) and fix forward with a new tag. Never move or
  re-push an existing tag.

## Translations

Mandelbrot inherits Fractal's translations from the GNOME translation team on
[Damned Lies](https://l10n.gnome.org/module/fractal/) — see `po/*.po`. This fork does not run its
own translation-branch sync; `README.md`'s Contributing → Translations section has the current
pointer to where translators should work.
