## September readiness: executable release exit criteria

Reuse this issue as release signoff owner, related #299/#407 and #390. Reference `e7e4df6`. Existing scripts/release_gate.py checks source metadata; it does not prove installed app behavior.

Depends on #436–#441, #354, #1217 (recovery), #1206 (interoperability), #313/#241 and #1208 (performance and accessibility).

- [ ] Select a candidate commit and require all capability evidence to identify that exact commit and packaged dependency lock.
- [ ] All P0 data-loss paths fixed; all admitted daily-driver journeys pass. Deferred features have explicit recorded scope and honest UI/docs.
- [ ] Build/install/launch each Flatpak; test file-manager MIME activation, open/save portals, recent files, drag/drop and sandbox file access.
- [ ] Test prior-release upgrade with settings, open documents and interrupted recovery checkpoints.
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
- [ ] Exercise supported architectures and reconcile flathub versus development manifests and locked source archives.
- [x] Record reproducible-input checksums and compare clean builds; do not equate metadata validation with binary reproducibility.
      `release-gate.yml` records the inputs' sha256 (`release-inputs.sha256`) and, separately, builds the three
      release binaries twice from clean on two runners with the commit's `SOURCE_DATE_EPOCH` and fails unless
      they are byte-identical (`reproducible-build` / `reproducible-compare`, both checksum lists kept 90 days).
      Measured first locally: two clean release builds of Letters in different target directories, same sha256
      (`ee61d9de…`).
- [ ] Publish machine-readable capability matrix, JUnit, independent-reader results, visual/a11y evidence and performance measurements with the release.
- [ ] Resolve concrete security/dependency blockers using #268/#269/#225/#347 and verify fixes; no duplicate strategic security project required.

Publishing packages or submitting to Flathub is a separate rollout action after these checks. This issue is done only with downloadable evidence for an installed release candidate.
