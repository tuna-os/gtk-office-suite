# Reproducible release gate

Every release tag is qualified as three independent Flatpaks: Letters,
Tables, and Decks. The release gate checks the manifest, desktop entry, icon,
GSettings schema, AppStream metainfo, translation source list, and locked Rust
dependency graph before any bundle is published.

## Required evidence

The tag workflow must produce:

- an installable `.flatpak` bundle for each app and architecture;
- AppStream, desktop-file, schema, and translation validation results;
- a sorted SHA-256 checksum file covering the bundles and release metadata;
- the capability matrix from `conformance/scorecard.py`;
- the known compatibility limits and recovery instructions below.

Builds use the checked-in `Cargo.lock`, pinned Flatpak runtime, and
`SOURCE_DATE_EPOCH` from the tag commit. A rebuild from the same commit and
runtime should produce the same source-input digest; binary differences must
be investigated before publication.

## Interop evidence must be current

The LibreOffice oracle and the LibreOffice-authored parity corpus need
LibreOffice installed, so they do not run on every revision: `nightly.yml`
runs them daily against whatever `main` was at 05:00, and on pull requests
that touch format or model code. A tag cut at any other time inherits a
verdict reached on **different code**.

So on a tag, `.github/workflows/release-revision.yml` runs

```bash
python3 conformance/validate_capabilities.py \
  --lanes conformance/lanes.json --release-revision "$GITHUB_SHA"
```

which refuses the release if any claim resting on a lane marked
`runs_on_every_revision: false` records a different revision, unless the
release commit differs from the recorded one only in release metadata
(`conformance/capabilities.json` and `flatpak/*.metainfo.xml`). A commit
cannot hold its own SHA, so recording a lane's run always takes a commit
on top of the code it ran.

If it fails, the fix is to run the lane at the release revision — not to
lower the claim's status or relax the rule.

## Automatic releases

`.github/workflows/autorelease.yml` cuts a release after a merge to `main`
(a patch bump; documentation-only changes do not count) and every Wednesday
(a minor bump) when `main` has changed since the last tag, and on demand
(`workflow_dispatch`: `bump` = minor, patch or major; `force` to release
an unchanged `main`):

1. it runs the scheduled lanes, `nightly.yml`'s oracle and
   `collab-letters-gui.yml`'s journeys, at `main`'s head X;
2. when both pass, `tools/release/cut_release.py` records X as the
   `revision` of every claim resting on them and adds an AppStream
   `<release>` (the version, the date and the merged changes) to each app's
   metainfo;
3. it commits that on top of X, checks it with `--release-revision`, tags
   the commit `vN` and pushes only the tag. `main` is not pushed to;
4. it creates the tag's GitHub release as a draft and dispatches
   `publish-flatpak.yml` with `publish` on the tag, which runs the evidence
   and soak gates before publishing anything, and `release-gate.yml`. The
   soak is 20 consecutive runs of every journey, which takes about eight
   hours, and a hosted job is cut off at six, so `release-soak.yml` shards
   them (two attempts a job, in parallel) and runs the display matrix as a
   job of its own; a tag publishes only when every shard passed;
5. once the Flatpaks are published, `publish-flatpak.yml` builds each app's
   `/app` tree in the GNOME SDK for x86_64 and aarch64, attaches it to the
   release as `<app>-<arch>.tar.gz` (with a `.sha256`), and publishes the
   draft. FlatPark repackages these tarballs (extra-data) and its update
   resolvers read them from `releases/latest`, which is why the release
   stays a draft until they are attached.

Merges arrive faster than a release takes (the lanes run for a while), so the
workflow's concurrency group keeps one run going and one waiting: a burst of
merges becomes one release, cut from whatever `main` is when the waiting run
starts. The evidence and soak gates still decide whether a tag publishes.

A manual release follows the same steps: run the lanes at the commit, run
`cut_release.py --version <v> --revision <sha>`, commit, tag and push the
tag. A tag pushed with `GITHUB_TOKEN` starts no workflow, so the automatic
run dispatches the publish itself, and a manual `publish` dispatch runs
both gates and needs them to pass. A lane the rule watches that no
claim cites is also refused, so the check cannot pass by having nothing to
check.

The same workflow's `release-critical-tests` job re-runs the lanes marked
`runs_on_candidate` in `conformance/lanes.json` (the Rust workspace and the
release-critical journeys) at the release revision and validates with
`--release-critical --every-release-critical-result --candidate <sha>`: a
release-critical test that failed, was skipped, or is simply absent from
those reports refuses the release, and so does an inventory recorded at any
other revision. A release-critical claim citing a test outside those lanes is
refused on every pull request already (SH-001).

## Upgrade and recovery policy

Flatpak updates preserve each app's GSettings, recent-file URI policy, and
crash-recovery snapshots because those live in the per-user data/config/state
directories rather than the deployed bundle. Before a release, CI installs an
older bundle, seeds a settings/recovery fixture, upgrades to the candidate,
and verifies that the fixture remains readable. Removing an app is not a data
wipe; users must explicitly clear application data when they want that.

If an upgrade cannot launch, use Flatpak's transactional rollback or reinstall
the previous bundle from its checksum-matched release artifact. Documents are
always user-owned files accessed through the document portal; no document
contents are included in a crash report. Crash reporting is opt-in and limited
to diagnostic metadata, with paths and document text redacted.

## Known limits

The release gate proves packaging and platform contracts, and that the
LibreOffice interop evidence was recorded at the revision being released —
not LibreOffice feature parity itself. See `docs/PARITY.md` and the generated capability matrix for
format-specific loss budgets. The GUI smoke journey remains the authority for
launch, open/save, portal, and keyboard behavior; the Flatpak job must run it
against the installed app when the runner provides a display server.
