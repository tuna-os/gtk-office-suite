# Flathub submission prep (ADR 0003 §7 — prep only)

These manifests are the Flathub-layout variants of `../flatpak/*.json`:
they build from the release tag over git (not `type: dir`), drop the
network build-args, and trim `--filesystem=host:ro` to `xdg-documents`.

## Remaining steps to submit (human action)

1. Generate offline cargo sources against the tagged Cargo.lock:

       python3 flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json
       # from https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo

   Note: the rdocx git pin (hanthor/rdocx) is resolved by the generator;
   publishing letters-core/rdocx to crates.io first (upstream PR
   tensorbee/rdocx#6) simplifies this.

2. Local verification build:

       flatpak-builder --force-clean build flathub/org.tunaos.letters-rust.json

3. Fork flathub/flathub, branch `new-pr`, add one app dir per submission
   (start with Letters), open the PR per
   https://docs.flathub.org/docs/for-app-authors/submission

Metainfo (screenshots, releases, launchable, OARS) validates clean with
`appstreamcli validate --pedantic` as of v1.1.0.
