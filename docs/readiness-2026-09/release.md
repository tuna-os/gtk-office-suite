## September readiness: executable release exit criteria

Reuse this issue as release signoff owner, related #299/#407 and #390. Reference `e7e4df6`. Existing scripts/release_gate.py checks source metadata; it does not prove installed app behavior.

Depends on #436–#441, #354, #322, #374, #313/#241 and #400.

- [ ] Select a candidate commit and require all capability evidence to identify that exact commit and packaged dependency lock.
- [ ] All P0 data-loss paths fixed; all admitted daily-driver journeys pass. Deferred features have explicit recorded scope and honest UI/docs.
- [ ] Build/install/launch each Flatpak; test file-manager MIME activation, open/save portals, recent files, drag/drop and sandbox file access.
- [ ] Test prior-release upgrade with settings, open documents and interrupted recovery checkpoints.
- [ ] Validate icons/schemas/desktop metadata/translations and diagnose GTK criticals/crashes.
- [ ] Exercise supported architectures and reconcile flathub versus development manifests and locked source archives.
- [ ] Record reproducible-input checksums and compare clean builds; do not equate metadata validation with binary reproducibility.
- [ ] Publish machine-readable capability matrix, JUnit, independent-reader results, visual/a11y evidence and performance measurements with the release.
- [ ] Resolve concrete security/dependency blockers using #268/#269/#225/#347 and verify fixes; no duplicate strategic security project required.

Publishing packages or submitting to Flathub is a separate rollout action after these checks. This issue is done only with downloadable evidence for an installed release candidate.
