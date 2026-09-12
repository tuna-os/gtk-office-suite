# September readiness dependency: GTK tests must run on one owning main thread

Audited on `e7e4df6`; closed out on `758a528`. This was a P0 testing
prerequisite for #354, #313 and #441. Related reports #304/#308/#332/#355/#376
describe the same failure family; the fix is shared and results attach across
them rather than spawning more reports.

## What was already in place

`suite-common/src/gtk_test.rs` is the shared main-thread dispatcher: GTK is
thread-affine and `gtk::init` succeeds at most once per process, while Rust's
harness spawns a thread per test even at `--test-threads=1`. Four hand-rolled
variants of this had accumulated across the workspace and were consolidated
into one. A panic inside the closure is resumed on the calling test's thread,
so a failing assertion fails a named test rather than aborting the binary.

## What was still broken

**The dispatcher skipped — and passed — whenever GTK could not initialise.**
That reads as reasonable and was not. The PR `test` job in `ci.yml` ran
`cargo nextest run --workspace` with **no display at all**, so every GTK
widget test in Letters skipped on every pull request. Measured on a clean
checkout:

```
$ cargo test -p letters                     # no DISPLAY
test result: ok. 95 passed; 0 failed; 0 ignored
$ cargo test -p letters -- --nocapture | grep -c "SKIP: GTK"
36
```

Thirty-six tests did not run and the harness reported `0 ignored`. Nothing
could see it — including the capability-ledger gate in the very next CI step,
which asserts no collected test was skipped: a skip that counts as a pass is
invisible to a report that looks for skips. The nightly `coverage` job had the
same gap, so it was measuring coverage over code no test executed.

Under a display those same 36 tests run and pass, so this was never a
correctness problem in the tests — they had simply stopped running.

**The GUI harness killed processes by name across the whole machine.**
`tests/gui/framework/base.py` ran `pkill -x <app>` before each launch, twice.
That matches every process with that name on the host: a developer's own open
copy of Letters and its unsaved work, or another run's app on a different
private display.

**The GUI runner did not verify its display came up.** `run_gui_tests.sh`
started Xvfb and slept one second. An Xvfb that could not start left `DISPLAY`
pointing at nothing and the journeys failed later with a timeout that said
nothing about the cause.

## Acceptance
- [x] Repeat all GTK widget and bridge tests under isolated Xvfb without
      thread panics — `xvfb-run -a cargo test --workspace`; the 36 Letters
      widget tests run and pass, `letters bridge` continues to run under
      Xvfb in `gui-tests.yml`.
- [x] Run the actual coverage command with the same execution policy — the
      nightly `coverage` job installs Xvfb and runs
      `xvfb-run -a cargo llvm-cov --workspace`.
- [x] Fail setup if the display is unavailable — `run_gui_tests.sh` now polls
      `xdpyinfo` for up to ten seconds and exits 1 naming the cause
      (verified: `GUI_TEST_REUSE_DISPLAY=1 DISPLAY=:123` exits 1 with
      "GUI setup failed: no X display at :123").
- [x] Retain stderr/backtraces and coverage artifacts — unchanged; nextest's
      JUnit report is already uploaded on failure and the failure-artifact
      capture already retains app stdout/stderr, the AT-SPI tree and the
      input trace.
- [x] **Do not mask GTK initialization failure with test skips** — a widget
      test with no display now fails. Skipping requires the run to declare
      itself display-less with `SUITE_GTK_TESTS=skip`, so a CI job that
      forgets Xvfb cannot skip silently.
      `gtk_widget_tests_require_a_display_unless_the_run_opts_out` is the
      canary: one clearly-named failure instead of 36 quiet passes.
- [x] No global process killing — ownership moved to
      `tests/gui/framework/owned_processes.py`, which terminates only
      processes the harness registered.
      `tests/test_gui_harness.py::test_an_unregistered_process_of_the_same_name_survives`
      launches a same-named process the harness did not start and asserts it
      is still alive afterwards; a source guard fails if `pkill`/`killall`
      is reintroduced anywhere under `tests/gui/` (verified by reintroducing
      the call and watching it fail).

A serialized harness still creates worker threads, so `--test-threads=1`
remains insufficient evidence of main-thread correctness — the dispatcher,
not the thread count, is what makes these tests sound.

## The failure that could not say why

The canary above works: a widget test with no display fails instead of
passing quietly. What it did not do was say *what* went wrong. `gtk_thread`
built its shared worker from `gtk4::init().is_ok()` — a boolean — so every
diagnosis collapsed to one sentence:

```
GTK could not be initialised, so this widget test could not run.
```

That was enough while the only cause was "CI forgot Xvfb". It stopped being
enough when `bridge::tests::non_buffer_state_survives_an_edit` failed in one
of **two concurrent `test` runs of the same commit** and passed in the
other: an intermittent initialisation failure, with nothing in the message
to work from, on a lane that normally has a display.

So the reason is kept rather than discarded. `GTK_THREAD` holds
`Result<ThreadPool, String>`, each of the four ways initialisation can fail
says which one it was, and the message carries the `DISPLAY` state beside
it:

```
GTK could not be initialised, so this widget test could not run:
gtk::init failed: Failed to initialize GTK (DISPLAY unset).
```

`DISPLAY unset`, `DISPLAY set but empty` and `DISPLAY=:99` fail identically
inside GTK and need different fixes — "give the run a display" versus "the
display you gave it is not answering" — which is the distinction a bare
boolean threw away. `unavailable_reason()` exposes the same text so the
canary reports it too, and `describe_display` is split out from the
environment read for the same reason `value_opts_out` is: a test that wrote
`DISPLAY` would be visible to every widget test running beside it.

Deliberately **not** added: a retry. A transient failure might well deserve
one, but nothing yet says what the transient cause is, and guessing at a
retry before seeing a cause would paper over whatever this actually is. The
next occurrence will print its reason; that is the evidence to decide on.
