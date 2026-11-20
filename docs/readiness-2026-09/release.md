## September readiness: executable release exit criteria

Reuse this issue as release signoff owner, related #299/#407 and #390. Reference `e7e4df6`. Existing scripts/release_gate.py checks source metadata; it does not prove installed app behavior.

Depends on #436–#441, #354, #1217 (recovery), #1206 (interoperability), #313/#241 and #1208 (performance and accessibility).

- [x] Select a candidate commit and require all capability evidence to identify that exact commit and packaged dependency lock.
      The candidate is the release tag. `release-evidence.yml` produces every kind of evidence at the tag's
      commit, and `tools/release/evidence.py` writes `evidence.json` naming that commit, together with the SHA-256
      of the `Cargo.lock` and the three Flatpak manifests read from it; it fails when the lock is missing.
      **Verified on v2.1.65** (2026-11-20): `evidence-v2.1.65.tar.gz`, attached by release-evidence run
      44725623501, names revision `7fd19c25` (the tag's commit) and `Cargo.lock` sha256 `d3bc98f9…`, the same
      as `git show v2.1.65:Cargo.lock | sha256sum`.
- [x] All P0 data-loss paths fixed; all admitted daily-driver journeys pass. Deferred features have explicit recorded scope and honest UI/docs.
      The P0s are closed: failed Save/Save As/close-after-save keep unsaved work (#436), shared saves use exclusive
      temporaries, keep permissions and are durable (#437), and completeness claims are revision-bound evidence (#441).
      No data-loss issue is open. The release-critical journeys passed at v2.1.63's commit, in the evidence gate
      `publish-flatpak.yml` runs before publishing (run 44552871670, `evidence / release-critical-tests`). Deferred
      features are `deferred` capabilities in the capability manifest, each under a waiver naming an issue, reason,
      scope and review date (`check_waivers`, capability-evidence.md). `docs/PARITY.md` describes them as users meet
      them, for example paragraph direction and password-protected files in Letters.
- [x] Build/install/launch each Flatpak; test file-manager MIME activation, open/save portals, recent files, drag/drop and sandbox file access.
      Tier C (`render-parity.yml` with `vm`, #1209) installs each app's Flatpak bundle into a clean GNOME VM and checks
      it there (`tools/render-lab/vm/installed.sh`, judged by `tools/render-lab/installed_check.py`):
      it is the default handler for its MIME types and opens a document through `xdg-open` (#1454); it saves to a
      `~/Documents` file through the sandbox, the file reopens valid and nothing is left behind (#1455); the file is
      in recent files (#1486); Save As and Open go through the GNOME file chooser portal (#1487); and a file dragged
      from Nautilus onto the window opens (#1489). Run 37740903001 on `47041899`: every check true for Letters,
      Tables and Decks.
- [x] Test prior-release upgrade with settings, open documents and interrupted recovery checkpoints.
      Tier C ends with it (`tools/render-lab/vm/upgrade.sh`, judged by `upgrade_check.py`, #1209). The newest
      release is built from its tag, installed on a clean slate, and used: two settings changed, a document opened
      and edited, autosave's checkpoint written, and the app killed mid-edit. The candidate is then installed over
      it. The settings must survive, and the candidate's first launch must take the checkpoint back.
      **Its first run found data loss in every shipped Flatpak**: snapshot ids were `<pid>-<n>`, and every Flatpak
      launch is pid 2. So the launch after a crash gave its own window the crashed document's id. Tables and Decks
      skipped the orphan as "live" and then overwrote it, and Letters recovered it and then deleted the copy. Ids are
      now unique per launch (`autosave::new_doc_id`). Run 37134693472: v2.1.0 → `111f962d` passes in all three
      apps, and Tables shows `values.xlsx (Recovered)` with the hidden row from before the crash.
- [x] Validate icons/schemas/desktop metadata/translations and diagnose GTK criticals/crashes.
      Metadata: `release-gate.yml`'s `contract` job runs `scripts/release_gate.py` (icon, manifest, desktop file, schema
      and metainfo present and consistent per app) and then `desktop-file-validate`, `glib-compile-schemas --strict`,
      `appstreamcli validate` and `msgfmt --check` on every pull request that touches them and on every tag.
      **GTK criticals** (#1209): the GUI harness reads each journey's stderr at teardown
      (`tests/gui/framework/gtk_diagnostics.py`, unit-tested by `tests/test_gtk_diagnostics.py`). A GLib/GTK CRITICAL
      fails the journey, and every CRITICAL and WARNING is logged per journey and uploaded as the smoke lane's
      `gtk-diagnostics` artifact. Measured when it went in: 118 journeys, no CRITICAL, and five WARNINGs. Three were
      the Help dialog's unescaped "&" in Pango markup, which left those group titles blank; that is fixed. Two are
      GtkLabel min-width warnings in the same dialog, kept as recorded warnings. **Crashes**: the harness already
      keeps a core dump and a gdb backtrace for a journey whose app dies (gui-stress, #1192).
- [x] Exercise supported architectures and reconcile flathub versus development manifests and locked source archives.
      Covered by `release-gate.yml`, to be ticked on the candidate that passes it: `source-archive` builds the
      locked source archive (`tools/release/source_archive.sh`: the tracked files and every crate vendored, the
      same bytes from the same revision); `offline-flatpak` builds Letters from it with no network on x86_64 and
      aarch64, through Flathub manifests derived from the development ones (`tools/release/flathub_manifests.py`,
      which change only the app's source and cargo's network use; `tests/test_flathub_manifests.py`); and
      `aarch64-tests` runs the core crates' tests on aarch64. The dictionaries the Letters manifest fetched from
      LibreOffice's `master` are pinned to a commit: their sha256 would have broken the build at the next edit.
      **Verified on v2.1.65**: release-gate run 44683901828 passed every job (source-archive, offline-flatpak on
      x86_64 and aarch64, aarch64-tests, reproducible build), and publish-flatpak run 44683898911 built and pushed
      all three apps for both architectures. Each `ghcr.io/tuna-os/<app>:v2.1.65-<arch>` and `latest-<arch>`
      image's `org.flatpak.ref` names its own app and architecture (`tools/release/check_oci_ref.py` now checks
      this in the build; v2.1.62 and v2.1.63 had shipped Decks under the Tables and Letters tags).
- [x] Record reproducible-input checksums and compare clean builds; do not equate metadata validation with binary reproducibility.
      `release-gate.yml` records the inputs' sha256 (`release-inputs.sha256`) and, separately, builds the three
      release binaries twice from clean on two runners with the commit's `SOURCE_DATE_EPOCH` and fails unless
      they are byte-identical (`reproducible-build` / `reproducible-compare`, both checksum lists kept 90 days).
      Measured first locally: two clean release builds of Letters in different target directories, same sha256
      (`ee61d9de…`).
- [x] Publish machine-readable capability matrix, JUnit, independent-reader results, visual/a11y evidence and performance measurements with the release.
      `release-evidence.yml` builds the bundle and attaches it to the release. Until #2314 it ran only on a
      `release: published` event, which a release published by `publish-flatpak.yml` with `GITHUB_TOKEN` never
      raises, so v2.1.62 and v2.1.63 went out without it. The release job now dispatches it on the tag.
      **Verified on v2.1.65**: the release job dispatched release-evidence run 44725623501, which attached
      `evidence-v2.1.65.tar.gz` to the release beside its six Flatpak bundles and six tarballs; the overview
      videos were dispatched the same way (run 44725626997).
- [ ] Resolve concrete security/dependency blockers using #268/#269/#225/#347/#1549 and verify fixes; no duplicate strategic security project required.
      #268, #269 and #347 are closed. #225 (quick-xml 0.38.4 through typst's CSL parser) is fixed in-repo by
      patching citationberg onto quick-xml 0.41 (#2312, `docs/DEPENDENCIES.md`). #1549 (unsafe-libyaml,
      unmaintained) is blocked on upstream: `typst-library` itself depends on serde_yaml, even on Typst's `main`, so
      patching hayagriva alone does not remove it. It stays a time-bound `deny.toml` ignore with a threat note,
      due for review on 2027-01-01.

Publishing packages or submitting to Flathub is a separate rollout action after these checks. This issue is done only with downloadable evidence for an installed release candidate.
