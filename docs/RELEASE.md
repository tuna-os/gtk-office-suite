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

The release gate proves packaging and platform contracts, not LibreOffice
feature parity. See `docs/PARITY.md` and the generated capability matrix for
format-specific loss budgets. The GUI smoke journey remains the authority for
launch, open/save, portal, and keyboard behavior; the Flatpak job must run it
against the installed app when the runner provides a display server.
