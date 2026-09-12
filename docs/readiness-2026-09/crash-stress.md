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

- [x] Repeated journey runner with replayable seed/order, display matrix, per-attempt timeout/log/JUnit/artifacts and nonzero
      aggregate failure — `tests/gui/stress.py`, run nightly by `.github/workflows/gui-stress.yml` in the same container
      the recorded journeys use. A campaign records the revision, the app binaries' SHA-256, the seed, each attempt's
      order, verdict, duration, JUnit report, log and retained artifacts, and classifies every failure as product crash,
      assertion mismatch, timeout, infrastructure or unclassified. There is no retry: a green rerun does not unfail the
      first attempt. Display axis covers 400/800/1280/1920 widths, light and dark, and scale 1 and 2; **high contrast is
      not covered** — it needs a theme the container does not ship, and an env var that changes nothing would be a worse
      lie than a visible gap.
- [ ] Fixed regression seeds on every PR; a bounded random-seed campaign nightly; larger soak and complete matrix on the release candidate.
- [~] Stateful sequences per app; cross-app clipboard and multi-window close/save races. All three apps are done and
      green, each running fixed seeds on every PR and 400 seeds a night. Tables (`tables-core/tests/stateful.rs`) found
      two identity bugs and #527 on its first runs. Decks (`decks-core/tests/stateful.rs`) found that align and
      distribute mis-positioned circles by their radius on apply and again on every undo. Letters
      (`letters-core/tests/stateful.rs`) was parked on #532 — a newline inside a table cell split the cell paragraph and
      left two paragraphs claiming the same cell — and stayed parked rather than narrowing the generator to route around
      it; fixing #532 then exposed four more defects behind it, including a panic in `Document::locate` on a document
      whose every paragraph was a deleted cell.
      **The cross-app clipboard is now covered** (`CrossAppClipboardSmoke` in
      `tests/gui/test_smoke.py`): the harness can launch a second application beside the primary one
      and move synthetic input between their windows, which is what the X11 clipboard actually needs
      — one live process owns the selection and another asks it for a format, and a single app
      copying to itself never exercises that. Tables → Letters and Letters → Tables both round-trip,
      and each direction was verified to *detect* a broken transfer by making the source app publish
      nothing and confirming the journey failed. A third journey records what happens when the
      copying app exits first: with no clipboard manager in the container the selection is gone and
      the paste is a silent no-op, asserted as "nothing, and no crash" rather than wished away.
      The multi-window races are still not started, and are a different problem: the apps are
      single-instance (`HANDLES_OPEN`), so a second window of the *same* app is not a second
      process and the second-app support does not reach it.
- [~] Save/recovery fault injection at each transaction boundary; bounded malformed-file fuzzing with minimized fixtures. The save transaction is instrumented: `suite-common-core/src/atomic_save.rs::fault` arms any of the six boundaries of a durable write, and any arrival at one, then a sweep asserts what the caller was promised at each. It replaced a test that made a write fail by chmod-ing the directory to 0555, which proves nothing when the suite runs as root — the write simply succeeded and the assertion never fired. It found that the two-atomic-write autosave slot could pair the new bytes with the previous generation's identity (see `recovery.md`). Malformed-file fuzzing is now started: `{letters,decks,tables}-core/tests/malformed_inputs.rs`
      run seeded structural and mutation cases on every pull request and at campaign scale nightly,
      each crate replays every retained crash input from its `tests/crashes/` directory, and the
      libFuzzer lane covers nine readers instead of three. It found three defects in
      `read_sheet_props_from_xlsx` on its first run, including a hang. See `interoperability.md`.
- [~] Always retain stderr/backtrace, core dump where supported, screenshot, AT-SPI tree, last snapshot, action trace
      and saved output fixtures on failure. The harness captured six of these, and captured **none of them for the one
      class of failure where they matter most**: unittest skips `tearDown` when `setUp` raises, the app is launched *in*
      `setUp`, and the capture was called only from `tearDown`. Measured against a stub binary that panicked on launch —
      in the CI video mode a startup failure left exactly one file, `journey.mp4`; with video off it left no directory at
      all. The app's own stderr, carrying a panic with its file and line, reached neither the artifacts nor the run log,
      because nothing in the harness reads the process pipes except the capture that never ran. What a reader saw was
      `Timed out after 15.0s waiting for application 'letters' in the AT-SPI registry` over a silent video. The recorder
      had already been given an `addCleanup` for exactly this reason; the other six artifacts had not.
      Capture is now a cleanup too, idempotent with the `tearDown` path, and writes `captured.json` saying per artifact
      whether it landed and why not — a guarded capture could otherwise retain nothing and report it only as a printed
      warning. A passing journey still retains nothing, which is the regression this change most risked, so a real
      built app proves that rather than a stub.
      Still missing, and not claimed: **core dumps** and **saved output fixtures** (the files a journey wrote).
- [ ] Track first-attempt failure rate and classify product crash, assertion mismatch, timeout, infrastructure setup and nondeterministic rendering separately.
- [ ] No retry-until-green, weakened assertion, reduced generator alphabet or silent skip counts as a fix. Diagnostic reruns retain the original failure.
- [ ] Each confirmed crash becomes a deterministic failing regression before the fix; close only with same-seed replay and relevant matrix evidence.

Initial stability target: zero crashes/data-loss/assertion failures in 20 consecutive runs of each critical journey per app at baseline, plus the admitted display matrix. This is a release threshold, not proof of zero defects; publish run counts, seeds and limitations. Longer overnight controller/fuzz campaigns stay resource-bounded.
