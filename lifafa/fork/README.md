# Lifafa

Lifafa is the email client of the TunaOS office suite,
[tuna-os/gtk-office-suite](https://github.com/tuna-os/gtk-office-suite),
alongside Letters, Tables and Decks. Its name is a word for envelope. It is
[Hylki](https://github.com/hyprlab/hylki) by hyprlab with native
[Tuta](https://tuta.com) and [Proton Mail](https://proton.me/mail) accounts,
built from this soft fork in the suite's `lifafa/` directory. Upstream
declined both providers, so the fork carries them and follows upstream for
everything else. It belongs to the suite by name, icon and packaging, and its
code, issues and releases are in the suite's repository; the standalone
[tuna-os/lifafa](https://github.com/tuna-os/lifafa) it came from is being
retired, and keeps only the Pages Flatpak repository (see [Updates](#updates)). It
ships as its own app, `org.tunaos.lifafa`, which installs beside upstream
Hylki and shares none of its state.

Neither Tuta nor Proton endorses third-party clients built this way; use them
knowing that. What belongs in the fork and what goes upstream, and how
touchpoints are kept small, is in [MAINTAINING.md](MAINTAINING.md). Report
problems on [the suite's issues](https://github.com/tuna-os/gtk-office-suite/issues/new/choose),
choosing Lifafa as the app.

## What is in this directory

| File | What it does |
| --- | --- |
| `rebrand.sh` | Turns a copy of the tree into Lifafa before it is built |
| `icons/` | Lifafa's icon in the suite's style, and its 256 and 512 pixel renders |
| `import-from-hylki.sh` | Copies upstream Hylki's accounts, settings and passwords into Lifafa |
| `MAINTAINING.md` | The fork's boundary and upstreaming policy |
| `touchpoints.txt`, `check-touchpoints.sh` | The upstream files the fork modifies, and the check that keeps that list honest |
| `flatpak-repo.sh`, `flatpak-repo.asc` | Adds a release to the signed Flatpak repository, and the key it is signed with |
| `sync-upstream.sh`, `UPSTREAM_BASE` | Applies upstream Hylki's changes to `lifafa/`, and the upstream commit last applied |
| `with-suite-common.py` | Copies the suite's shared crates into a build tree, for the Flatpak build |
| `upstream-candidates.md` | Provider-neutral changes drafted for hyprlab/hylki, and their state |
| `ACCOUNTS.md` | The user documentation for Tuta and Proton Mail accounts |
| `HELPERS.md` | Building, packaging and licences of the two helpers |
| `CHANGELOG.md` | The fork's own changes |

The workflows are in the suite's `.github/workflows/`: `lifafa.yml` (build,
tests, RustSec and licences on every change to `lifafa/`),
`lifafa-flatpak.yml` (the Flatpak, and releases) and
`lifafa-upstream-sync.yml` (following upstream). `lifafa/README.md` is
upstream's README, left as upstream wrote it; the fork's documentation lives
here rather than in upstream's files, so that the upstream sync never
conflicts on it.

## How the rebrand works

The source keeps upstream's `co.hyprlab.Hylki` strings. Renaming them in the
source would touch dozens of upstream lines, and each would conflict on a
later merge. Instead `fork/rebrand.sh <tree>` rewrites a copy of the tree just
before it is built.

Everything the app is called sits in one block of variables at the top of the
script: `APP_ID`, `APP_NAME`, `DATA_DIR_NAME`, `KEYRING_SERVICE`, the desktop
comment, the metainfo summary and developer, the suite's name, the homepage,
bug tracker and source links, and the icon. A rename edits those lines and
nothing else; `lifafa-flatpak.yml` and `import-from-hylki.sh` read them through
`fork/rebrand.sh --identity`. With the current values it rewrites:

- **App ID** `co.hyprlab.Hylki` becomes `org.tunaos.lifafa`, lowercase like
  the suite's `org.tunaos.letters`: the Rust constant in `main.rs`, the
  desktop file, metainfo, D-Bus restart service, icons and the GNOME Files
  extension. Every file named after the ID is renamed.
- **Keyring** entries are stored under the service `org.tunaos.lifafa`, so
  they never collide with upstream's.
- **Display name** "Lifafa": launcher, metainfo, the window and About titles,
  the wordmark the About window and the welcome wizard show (the name in
  Cantarell beside the icon, generated in place of upstream's "Hylki"
  artwork), the Files extension's menu item, and the tray. Beyond those,
  every string literal in `src/` that calls the app Hylki ("Welcome to
  Hylki", "Quit Hylki entirely", the MCP tools' descriptions, the sign-in
  page) names it Lifafa instead. A Rust string scanner in the script finds
  them, so identifiers (`HylkiTray`), the resource prefix, `HYLKI_*`
  variables and comments are never touched. The po catalogues translate the
  old wording, so those strings show in English.
- **Credits**: the About window's footer reads "Built on Hylki by hyprlab",
  the coffee row says it goes to Hylki's author, and the contributor lists
  are upstream's. Hylki's Discord and email rows are dropped; questions go to
  the fork's bug tracker. The metainfo credits Hylki in its second paragraph.
- **Icon**: `fork/icons/org.tunaos.lifafa.svg`, an envelope on the suite's
  blue pad, replaces Hylki's icon in the launcher's hicolor icons, the in-app
  icon resource and the PNG the app embeds for Settings → App icon.
  `org.tunaos.lifafa-symbolic.svg` replaces the symbolic icon the tray draws,
  and `fork/icons/alt/` the alternatives Settings → App icon offers, under
  upstream's file names and labels (the envelope on a blue, navy and yellow
  square, and without its pad as "Classic"). The `.svg` beside each `.png` is
  its source.
- **Directories** under the XDG config, data, cache and runtime bases are
  `lifafa` instead of `hylki`, so a native build does not share state with
  upstream either. The Files extension gets its own file name.
- **Metainfo** follows the suite's: its summary, `developer_name`, a first
  paragraph naming the Hanthor Office Suite and one crediting Hylki, then
  upstream's description under the app's name. The releases stay as upstream
  wrote them: they are the history of the code being built, renames included.
- **Links**: the About window's website and the metainfo homepage point at
  the suite; the About window's and the metainfo's bug tracker and the
  welcome page's link point at this repository's issues; the source link
  points here.
- **Manifest**: `fork/org.tunaos.lifafa.yml` is written from upstream's
  `co.hyprlab.Hylki.yml` through `tools/local-manifest.sh`. Upstream's pins
  a tagged commit of hyprlab/hylki, which would build without the rebrand, so
  the fork's builds the rebranded tree it sits in. Every module, including the
  Tuta (Rust) and Proton (Go) helpers, is upstream's recipe, except that the
  two Rust modules build in a cargo target directory under `/run/ccache`,
  install from there, and first run `fork/ci-mtimes.py apply`, so a build
  that keeps flatpak-builder's state directory recompiles only the crates
  that changed (see "Building").

Left alone on purpose: the bundled resource prefix `/co/hyprlab/Hylki`, which
is private to the process (`main.rs` pins it), the `hylki` binary and gettext
domain, which live inside the Flatpak, the Vireo and Veem carry-over
(`legacy.rs`, `carry_over.rs`), which is upstream's, and the release notes and
changelog the About window shows, which are Hylki's history.

Every rewrite names the text it expects and stops with the file and pattern
when upstream changes it. A final sweep fails on any `co.hyprlab.Hylki`,
`.join("hylki…")` or string naming Hylki left in `src/`, `data/` or
`resources/`, outside the credits and the kept files. So drift in either
direction fails the daily sync instead of shipping. The script refuses to run
on its own checkout unless `HYLKI_REBRAND_IN_PLACE=1` is set (CI sets it on
its throwaway checkout). Running it again on a rebranded tree only repeats the
sweep.

## How upstream is followed

The suite's `.github/workflows/lifafa-upstream-sync.yml` runs every Monday and
on demand. It runs `fork/sync-upstream.sh`, which applies upstream's changes
since the commit in `fork/UPSTREAM_BASE` to `lifafa/` with a three-way merge,
and records the new base. It then opens a pull request with the result. When
upstream's change conflicts with the fork's, the conflicting files keep their
markers, and the pull request says so and waits for a person. Either way,
`lifafa.yml` builds and tests the pull request, and nothing is merged without
that.

A pull request opened with the workflow's own token starts no workflows. The
`UPSTREAM_SYNC_TOKEN` secret, when set, opens it with a token that does; without
it, a push to the branch starts CI.

## Building

Releases are tagged `lifafa-v<upstream-version>-tuna<N>`, for example
`lifafa-v1.48.0-tuna1`, since the suite's plain `v*` tags release Letters,
Tables and Decks. Tags from before the move, in tuna-os/lifafa, were
`v<upstream-version>-tuna<N>`. A tag runs `.github/workflows/lifafa-flatpak.yml`,
which builds Lifafa for x86_64 and aarch64 in Flathub's GNOME builder image and
publishes it three ways:

- as OCI images on `ghcr.io/tuna-os/lifafa`, indexed in the TunaOS remote
  (`oci+https://tunaos.org/flatpak`) with the other apps of the suite;
- into the signed Flatpak repository on GitHub Pages, which existing installs
  follow (see [Updates](#updates)); the site stays on tuna-os/lifafa's
  `gh-pages` branch, because every install names that URL;
- as `.flatpak` bundles, with their `SHA256SUMS`, on the GitHub release for the
  tag. The bundles are made from the signed commits and name the Pages
  repository, so they update from it.

A manual run builds both architectures and keeps the bundles and the signed
Pages repository as workflow artifacts; it publishes the OCI images only with
`publish` ticked, and never pushes the Pages site. The workflow also runs every
week, so that a change elsewhere in the suite (to `suite-common`, say) that
breaks Lifafa's Flatpak build is found before a release needs it.

Lifafa links the suite's `suite-common` and `suite-common-core` crates by
relative path, and flatpak-builder copies only `lifafa/` into its sandbox. So
before building, `fork/with-suite-common.py` copies those crates into the tree
being built (never into the repository's own checkout).

A local build follows the same steps as `lifafa-flatpak.yml`. It needs
`flatpak-builder`, the GNOME 50 SDK, and the `rust-stable` and `golang`
extensions for 25.08 (see [HELPERS.md](HELPERS.md)):

```sh
git submodule update --init --recursive
sh native/tuta-unofficial-sdk/prepare.sh
flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json

rsync -a --delete --exclude=.git --exclude=/target \
    --exclude=/native/tuta-unofficial-sdk/target \
    --exclude=/native/tuta-unofficial-sdk/upstream ./ /tmp/suite/lifafa/
# with-suite-common.py finds the shared crates beside the tree it is given.
for d in suite-common suite-common-core flatpak; do
    rsync -a --delete --exclude=/target ../$d/ /tmp/suite/$d/
done
python3 fork/with-suite-common.py /tmp/suite/lifafa
fork/rebrand.sh /tmp/suite/lifafa
flatpak-builder --user --install --force-clean --ccache \
    --state-dir=/tmp/lifafa-state /tmp/lifafa-build \
    /tmp/suite/lifafa/fork/org.tunaos.lifafa.yml
flatpak run org.tunaos.lifafa
```

`--ccache` (the default in newer flatpak-builder) mounts the state
directory's `ccache/` at `/run/ccache`, where the Rust modules keep their
target directories; keep the same `--state-dir` between builds to reuse them.

The icon's PNGs are rendered from the SVG; after changing it, run
`rsvg-convert -w N -h N fork/icons/org.tunaos.lifafa.svg -o fork/icons/org.tunaos.lifafa-N.png`
for N of 256 and 512.

## Updates

Lifafa is published as a signed Flatpak repository on GitHub Pages,
`https://tuna-os.github.io/lifafa/repo/`. Install it from there, and
`flatpak update` follows every release:

```sh
flatpak install --user https://tuna-os.github.io/lifafa/org.tunaos.lifafa.flatpakref
```

or add the repository as a remote first:

```sh
flatpak remote-add --user --if-not-exists lifafa \
    https://tuna-os.github.io/lifafa/org.tunaos.lifafa.flatpakrepo
flatpak install --user lifafa org.tunaos.lifafa
```

Releases made from the suite also go to the TunaOS remote that carries
Letters, Tables, Decks and Mandelbrot:

```sh
flatpak remote-add --user --if-not-exists tuna-os https://tunaos.org/flatpak/tuna-os.flatpakrepo
flatpak install --user tuna-os org.tunaos.lifafa
```

Both are published until two releases have gone out on each
(docs/CUTOVER-LIFAFA-MANDELBROT.md); an install keeps following whichever it
came from.

The runtime comes from Flathub. Commits and the summary are signed with
`Lifafa (TunaOS) <noreply@tunaos.org>`, fingerprint
`F9D8 8C33 D6A7 AC85 29FE  1FD3 61BC 0585 35DF E2CF`, whose public key is
`fork/flatpak-repo.asc` and is embedded in the `.flatpakrepo` and
`.flatpakref`. The bundles on the releases from v1.47.1-tuna6 on install the
same signed commits and update from the same repository.

**Installed from a bundle of v1.47.1-tuna5 or earlier?** Those bundles name
no repository, so they never update. Switch the installed app over to the
repository; it keeps its app ID, so your accounts and settings in
`~/.var/app/org.tunaos.lifafa` stay as they are:

```sh
flatpak remote-add --user --if-not-exists lifafa \
    https://tuna-os.github.io/lifafa/org.tunaos.lifafa.flatpakrepo
flatpak install --user --reinstall lifafa org.tunaos.lifafa
```

The old bundle's `lifafa-origin` remote goes away by itself once nothing is
installed from it. Use `--system` instead of `--user` for a system-wide
install. Two shortcuts do not work: `flatpak install --reinstall` of the
`.flatpakref` is refused while the app is installed, and reinstalling from a
newer bundle keeps the old bundle's remote, which does not check signatures.

How the repository is kept, in `fork/flatpak-repo.sh`: each release is
committed on top of the previous one per arch, signed with the key in the
`LIFAFA_GPG_PRIVATE_KEY` Actions secret, with static deltas from the previous
release. The newest commit and the three before it are kept, for
`flatpak update --commit=` to roll back to; an install on an older commit
still updates, without a delta. The `gh-pages` branch holds one commit, the
current site, replaced on every release, and GitHub Pages serves it as is
(`.nojekyll`). The private key's offline copy is with the maintainers. A new
key means a new `fork/flatpak-repo.asc` and secret, and every installed
remote needs `flatpak remote-modify --gpg-import=<key> lifafa` (or to be
added again from the new `.flatpakrepo`) before it accepts the next release.

## Moving over from upstream Hylki

Lifafa starts empty. Hylki's own carry-over from its earlier names
(`src/desktop/legacy.rs`) does not fit here. It is built for a predecessor
that is being replaced: it renames that app's directories in place and takes
over its Files extension. Upstream Hylki keeps running beside the fork, so
its data has to be copied instead.

Quit both apps, then run:

```sh
fork/import-from-hylki.sh
```

It copies the config, data and cache directories from upstream's Flatpak
(`~/.var/app/co.hyprlab.Hylki`), or from a native install (`~/.config/hylki`
and its siblings), into `~/.var/app/org.tunaos.lifafa/*/lifafa`. It then
copies every keyring entry from the `co.hyprlab.Hylki` service to
`org.tunaos.lifafa` with `secret-tool`. Nothing that already exists is
overwritten, and upstream's data is left as it was. Without `secret-tool`,
passwords and sign-ins are asked for again on first use.

The fork's first release, Hylki Tuna (`org.tunaos.Hylki`, v1.47.1-tuna1), was
withdrawn after a day in favour of Lifafa, and v1.47.1-tuna2 for a misspelling
of the name; nothing is carried over from either.
