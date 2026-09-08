# [P0] Crash and flake campaign: reproducible scenario matrix, action sequences, and fault injection

User priority (2026-09-07): the apps crash and tests are flaky; exercise a wide variety of scenarios before adding more features. Audit base: e7e4df6. Depends on #354 for isolation and #441 for honest results; feeds all app-readiness issues.

## Execution model

Build a seeded scenario runner that records the candidate commit, binary checksums, seed, action trace, environment, fixture IDs and every attempt. Replay exactly; minimize failing sequences into permanent regressions. Start with repeated existing journeys across display configurations, then add bounded stateful action generators. Repetition is a separate instrument from stateful fuzzing and neither replaces semantic assertions.

## Scenario matrix

| Axis | Required coverage |
|---|---|
| Documents | empty, single item, many pages/sheets/slides, sparse far cells, image-heavy, malformed/truncated packages |
| Text/data | ASCII, CJK, combining marks, emoji, RTL/mixed direction, long unbroken text, tabs/newlines, dates/numbers/formula errors |
| Editing | type/delete/paste, format, select, drag/resize, undo/redo branching, repeat commands, focus changes, switch tabs/sheets/slides during dialogs |
| Files | new/existing/read-only/missing, Unicode/long filenames, unsupported extension, Save As cancel, failed writes, concurrent save, externally changed file |
| Recovery | kill before/during/after checkpoint or commit, two windows/processes, multiple orphan snapshots, corrupt metadata, repeated recovery before save |
| UI | width 400/800/1280, light/dark/high contrast, scale 1/2, zoom extremes, keyboard/mouse, AT-SPI attached, settings changed live |
| Environment | X11 headless and packaged Wayland/portal sessions, primary/external display disconnect, load/memory pressure with bounded resources |

## Architecture

Controller state machines generate valid commands and assert invariants after every step: stable identities, valid selection, matching rendered/calculated state, undo reversibility, dirty/savepoint correctness and old-or-new durable bytes. The GUI driver uses real input and actions, reads normalized snapshots, and verifies reopened files. Test-only fault injection controls I/O outcomes and checkpoint barriers without changing production defaults. Production parsers remain the fuzz targets.

## Delivery and gates

- [ ] Repeated journey runner with replayable seed/order, display matrix, per-attempt timeout/log/JUnit/artifacts and nonzero aggregate failure.
- [ ] Fixed regression seeds on every PR; a bounded random-seed campaign nightly; larger soak and complete matrix on the release candidate.
- [ ] Stateful sequences per app; cross-app clipboard and multi-window close/save races.
- [ ] Save/recovery fault injection at each transaction boundary; bounded malformed-file fuzzing with minimized fixtures.
- [ ] Always retain stderr/backtrace, core dump where supported, screenshot, AT-SPI tree, last snapshot, action trace and saved output fixtures on failure.
- [ ] Track first-attempt failure rate and classify product crash, assertion mismatch, timeout, infrastructure setup and nondeterministic rendering separately.
- [ ] No retry-until-green, weakened assertion, reduced generator alphabet or silent skip counts as a fix. Diagnostic reruns retain the original failure.
- [ ] Each confirmed crash becomes a deterministic failing regression before the fix; close only with same-seed replay and relevant matrix evidence.

Initial stability target: zero crashes/data-loss/assertion failures in 20 consecutive runs of each critical journey per app at baseline, plus the admitted display matrix. This is a release threshold, not proof of zero defects; publish run counts, seeds and limitations. Longer overnight controller/fuzz campaigns stay resource-bounded.
