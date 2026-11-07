# Letters Flathub submission preparation

The owner opens the PR on `flathub/flathub` (#1210, ADR 0003 §7).
Tables and Decks wait until Letters is accepted. Their manifests here are
historical preparation, not verified submission inputs.

## Prepared inputs

- `org.tunaos.letters.json`: derived from `flatpak/org.tunaos.letters.json`
  by `tools/release/flathub_manifests.py`, with the build network grant
  removed and Cargo forced offline and locked. The source has both a tag
  and its full commit pin. Dictionary URLs are pinned, too.
- `cargo-sources.json`: the official Flatpak Cargo generator's offline
  registry archives, checksums, git pins, normalized git crate manifests,
  and Cargo source replacement configuration (including IronCalc and rdocx).
  `flathub_manifests.py` then moves the git crates into their own directory
  source, `cargo/git-vendor`: v2.1.0 locks rdocx-oxml 0.1.2 from both
  crates.io and git, and one shared `cargo/vendor` makes `--locked` fail.
- `letters-Cargo.lock`: exact lockfile extracted from the tag, for auditing.
- `letters-source.json`: tag, commit, lockfile digest, and generator digest.

The initial prepared version is **v2.1.0**, commit
`ba3dc8a70f537d05045db57aa3c34b51acaed51c`, the latest published tag when
these inputs were generated. **This is not v3.0 signoff.** Regenerate for
the v3.0 tag after release qualification (#1209/#1211); do not submit an
older release as the qualified candidate.

## Regenerate for the qualified tag

Run from the repository root, with tags fetched. Use the official generator
at this immutable revision:

<https://github.com/flatpak/flatpak-builder-tools/blob/9d86d336a39b9af38b86f114b6ce5820fe260f91/cargo/flatpak-cargo-generator.py>

Its SHA-256 is
`0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380`.
Install its declared Python dependencies in a virtual environment, then run
using that environment's Python (replace the tag and generator path):

```bash
python tools/release/flathub_manifests.py letters v3.0.0 /path/to/flatpak-cargo-generator.py flathub
python -m pytest tests/test_flathub_manifests.py tests/test_release_gate.py
```

This reads `Cargo.lock` from the tag's commit, never from the checkout.
Commit all four prepared inputs together. It updates Letters only.
When changing the generator revision, record its URL and digest here, too.

## Verification and owner checklist

- [ ] Confirm `letters-source.json` names the qualified release tag and
  commit, and its lock matches `git show <tag>:Cargo.lock`.
- [ ] Require **both** `release-gate.yml` `offline-flatpak` jobs to pass on
  the preparation commit. Each builds the candidate's vendored archive,
  then separately builds the committed Letters submission manifest, without
  granting the build network access. The candidate build alone does not
  prove the submission source list works.
- [ ] Record that run URL and preparation commit in #1210; download the
  `letters-submission-<arch>-<sha>` artifacts (uploaded only after the builds
  succeed). They contain the exact four prepared inputs. Do not call a
  pending, skipped, or failed build verified.
- [ ] Optionally repeat on a local Flatpak host with the GNOME 50 SDK and
  Rust extension installed:

  ```bash
  flatpak-builder --user --install-deps-from=flathub --force-clean build flathub/org.tunaos.letters.json
  ```

- [ ] Follow the [Flathub submission guide](https://docs.flathub.org/docs/for-app-authors/submission).
  Fork `flathub/flathub`, create `new-pr`, and copy
  `org.tunaos.letters.json` and `cargo-sources.json` to the submission branch
  root. The sidecar source path is relative to the manifest. Keep the lock
  and provenance in this repository as review evidence.
- [ ] Open the Letters submission PR as the owner, link #1210 and the
  successful verification run, and complete Flathub's review requirements.
  Record the submission URL on #1210. Submit Tables and Decks only after
  Letters is accepted.

The separate archive mode remains available for candidate verification:
`tools/release/source_archive.sh <revision> <out-dir>` vendors that revision's
locked graph; `flathub_manifests.py <archive> <sha256> <out-dir>` derives all
three offline manifests. Those generated manifests do not replace the
tagged Letters submission files above.
