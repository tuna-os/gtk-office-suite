# Office suite: roadmap to dependable daily use

GitHub execution tracker: https://github.com/tuna-os/gtk-office-suite/issues/443

Audit: 2026-09-07, current upstream revision `e7e4df6`. The initial local checkout was `449473a` (128 commits behind); it has been fast-forwarded. Historical issues #95–#122 are closed. This roadmap tracks remaining behavioral evidence and confirmed defects, not a replay of that backlog. Source inspection and passing unit tests are not a full application certification.

## Definition of done

Letters, Tables and Decks must reliably complete create/open → edit → undo/redo → save/Save As → close → reopen with supported content intact, explicit handling of unsupported content, usable keyboard/accessibility workflows, and recoverable unsaved work. The user's highest priority is eliminating crashes and flakiness across varied scenarios.

“Done” means current-revision tests prove the behavior, not that an issue was closed or a model field exists. No known P0 crash/data-loss issue, silently skipped release-critical test, or unapproved format loss may remain. Advanced features already admitted by newer ADRs require evidence or an explicit revised scope decision; collaboration, plugin marketplaces and enterprise policy systems do not block dependable everyday editing.

## Architecture to preserve

1. `suite-common-core` owns GTK-free lifecycle policy, durability, compatibility reports and genuinely shared primitives. `suite-common` adapts those policies to GTK dialogs/actions/settings.
2. Each app's core/controller owns canonical document state, stable identities and undo/save revisions. GUI actions invoke those same tested commands. Snapshot/thumbnail/print/accessibility views derive from that state.
3. Letters keeps a complete per-tab document and non-buffer sidecars. The GtkTextBuffer bridge must preserve everything an unrelated edit should not change. Migrate incrementally; do not introduce a second unsynchronized copy.
4. Tables keeps IronCalc as the calculation authority and WorkbookController as the mutation authority. Sparse/virtual views do not reimplement formulas or own independent sheets.
5. Decks uses one controller for slides, objects, masters and undoable gestures. Canvas, inspector, presenter and export share model geometry.
6. One save transaction computes compatibility, serializes, commits, then updates clean state/identity/recovery. Cancel/failure cannot advance the savepoint. Recovery checkpoints have generation/ownership semantics separate from real saves.
7. GTK widget tests run on their owning main thread. GUI journeys run real processes in private display/bus/filesystem environments; clean up only owned processes.

## Dependency-ordered execution

Each row's marker and figure are counted from the linked readiness
document's own checklist by
`tests/test_roadmap_figures.py::ReadinessExecutionList`, so this list cannot
drift from the work it summarizes. `[x]` means every row in that document is
ticked, `[~]` that some are, `[ ]` that none are. Edit the document; the
list follows.

- [x] #437 — durable shared saves — [atomic-save.md](atomic-save.md) 8/8
- [x] #436 — Letters save transactions — [save-transactions.md](save-transactions.md) 6/6
- [x] #241 — GTK test thread/display ownership — [gtk-threading.md](gtk-threading.md) 6/6
- [~] #354 — deterministic GUI infrastructure — [gui-testing.md](gui-testing.md) 9/10
- [~] #313 — CI validator and test gates — [ci-gates.md](ci-gates.md) 7/8
- [~] #442 — crash and flake campaign — [crash-stress.md](crash-stress.md) 2/8
- [~] #441 — capability evidence ledger — [capability-evidence.md](capability-evidence.md) 3/7
- [~] #322 — recovery lifecycle — [recovery.md](recovery.md) 2/9
- [~] #374 — live interoperability loss budgets — [interoperability.md](interoperability.md) 0/9
- [~] #438 — Letters fidelity — [letters-fidelity.md](letters-fidelity.md) 0/6
- [~] #439 — Tables format safety and authoring — [tables-readiness.md](tables-readiness.md) 1/7
- [ ] #440 — Decks authoring and presentation — [decks-readiness.md](decks-readiness.md) 0/7
- [ ] #400 — performance, rendering and accessibility evidence — [performance-accessibility.md](performance-accessibility.md) 0/7
- [ ] #326 — installed-release signoff — [release.md](release.md) 0/9
- [ ] #650 — headless document conversion CLI — [headless-conversion-spec.md](headless-conversion-spec.md) 0/3

| Phase | Execution issue | Exit evidence |
|---|---|---|
| P0 — save safety | #437 shared durable writes → #436 Letters save transactions; #439 Tables format-safe saves | Failure injection preserves original bytes, dirty state and recovery; no extension/content mismatch |
| P0 — trustworthy testing | #241 GTK thread/display ownership; #354 GUI isolation; #313 CI self-tests | No global process killing, hidden collection or swallowed failures; artifacts on setup/test failure |
| P0 — crash campaign | #442 seeded repetition/matrix → stateful actions and fault injection | Replayable seeds/traces; failures retained even when a rerun passes |
| P0 — evidence | #441 capability ledger | Collected tests and observed results linked by feature/layer/revision |
| P0 — recovery | #322 transactional checkpoints | Multi-document kill/restart and interrupted-save recovery without stale/mixed generations |
| P1 — interoperability | #374 live loss-budget enforcement | Both format directions, package relationships and GUI warning/cancel tests |
| P1 — Letters | #438 complete document/bridge/layout fidelity | Styled Unicode ODT/DOCX journeys preserve edited and untouched fields |
| P1 — Tables | #439 workbook authoring depth and format contracts | Multi-sheet structural/formula/protection journeys plus minimized Unicode regression |
| P1 — Decks | #440 authoring/master/presenter/export | PPTX/ODP journeys and external-display/PDF evidence |
| P1 — visual/a11y/performance | #400 with #354 and related accessibility issues | Wide/narrow, light/dark/high-contrast, 1x/2x; budgets and screen-reader evidence |
| Release | #326 | Installed Flatpak candidate, portals/MIME, upgrade/recovery and exact-revision evidence bundle |

Existing reports #377/#371/#358/#324 (Tables Unicode), #304/#308/#332/#355/#376 (GTK coverage) and #361/PR #362 (temporary snapshots) should be coordinated into the respective owners. Related architecture #284/#285 remains relevant when shared primitives or GUI decomposition are touched. Do not close duplicate reports without checking their evidence.

## Testing strategy

| Lane | Scope | Policy |
|---|---|---|
| Fast PR | Core/controller unit and state-machine/property tests; validator self-tests; collection guards; lint; format/package checks | Fixed reproducible regressions, no GUI dependency for core logic |
| GUI PR | Actual affected-app journeys; GTK bridge round-trips; baseline crash seeds | Private bus/display/XDG; bounded condition waits; strict failures and JUnit/artifacts |
| Nightly | Repeated seeds, input/format fuzzing, LO oracle, display matrix, larger fixtures and performance | Resource-bounded campaigns; minimize failures; retain every first-attempt result |
| Release | All admitted formats/apps on candidate build, full matrix, recovery failures, installed Flatpaks, accessibility/upgrade | Missing/failed/skipped required evidence blocks signoff |

Use fixtures spanning empty/large/sparse/image-heavy content, Unicode/RTL, malformed files, different save targets, multi-window focus changes, clipboard ownership, repeated undo/redo, and failure at each persistence boundary. See [crash campaign](crash-stress.md) for the full matrix. Existing model corpora and LibreOffice oracles remain useful but cannot prove GUI wiring.

## First implementation slice

Opened PRs: [#444 — shared save safety](https://github.com/tuna-os/gtk-office-suite/pull/444)
and [#445 — GUI crash/flake infrastructure](https://github.com/tuna-os/gtk-office-suite/pull/445).
The latter is draft pending live GUI verification. Neither closes its broader
acceptance contract by itself.

- Shared atomic save: regression tests reproduced stale-temp-symlink overwrite and permission widening. Exclusive temporary files, permissions, owned cleanup and Unix directory sync implemented locally. On updated main: 89 shared-core unit tests plus one parity-conformance test passed. Full format/GUI integration is still pending.
- GUI harness: private schemas/XDG/display allocation, owned process cleanup, correct snapshot artifact source; duplicate named-range test exposed under a unique class name. The still-skipped #137 reproduction needs live verification.
- Repeated journey runner: `tests/gui/stress.py` is on main and runs nightly (`.github/workflows/gui-stress.yml`) in the GUI
  container. It records seeds, revision, binary SHA-256, every attempt's order, verdict, log, JUnit and artifacts, classifies
  failures (product crash / assertion / timeout / infrastructure), reports the first-attempt failure rate, and never retries.
  Its first display-matrix run found #516 (Tables lost the name box below the narrow breakpoint, so `Ctrl+G` had nothing to
  focus and silently swallowed every keystroke at 400px); that is fixed, and a full matrix pass then turned up further
  narrow-width gaps (#520), so the nightly stays on the baseline display until those close. It repeats existing journeys; stateful random edits and save-fault injection are next. The corpus and engine
  work from the same slice is still on PR #445.
- Dependency-free Python tests and CI wiring cover validators, duplicate collection and stress orchestration. GUI failure artifacts are uploaded even when smoke fails.

Run after building current binaries and installing Xvfb, Matchbox, xdotool and the documented GUI dependencies:

```sh
python3 tests/gui/stress.py --repeat 20 --seed 20260907 --output /tmp/office-stress-baseline
python3 tests/gui/stress.py --repeat 1 --matrix display --seed 20260907 --output /tmp/office-stress-display
```

Output directories must be new. These commands exercise the real
applications and may take substantial time; syntax and orchestration checks
do not count as a completed GUI campaign.

The note that used to stand here — that the host lacked Xvfb/Matchbox and
that toolbox startup failed — is out of date. Xvfb, Matchbox, xdotool,
at-spi2-core, ffmpeg and dogtail are all present; the only obstacle was
that `gi` is built for a different Python than `/usr/bin/python3`, which is
exactly what `GUI_TEST_PYTHON` exists for:

```sh
GUI_TEST_PYTHON=/usr/bin/python3.12 tests/gui/run_gui_tests.sh test_smoke.py
```

Find the right interpreter from the extension module's own filename —
`ls /usr/lib/python3/dist-packages/gi/_gi.cpython-*.so` names the version it
was built for. GUI journeys, and therefore GUI verification of GUI changes,
run here.
