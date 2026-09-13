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

Deliberately **not** added: a retry. That was written as a "not yet" and
has since become a "not ever", with a reason. `gtk4::init()` is **not
idempotent after a failure**: called a second time it returns `Ok` without
a usable display. A retry was implemented, passed the unit tests, and
passed two mutations — and then a canary widget test run with `DISPLAY=:77`
(nothing serving it) went from `FAILED` to `ok. 1 passed`. It masked every
display-less run, which is precisely what the row above exists to prevent,
so it was discarded. `init_with_retries` should not come back.

What the reasons have since said, and what has been ruled out:

The occurrences print `DISPLAY=:0` with a socket present — "refused by a
live display rather than handed a missing one", the case
`describe_display_and_server` was added to name. Seven so far, each a
*different* Letters widget test, while every other widget test in the same
run initialised fine.

**It is a flake, and that is now demonstrated rather than inferred.** Two
runs of one commit — a documentation-only commit, whose previous head had
a clean suite — failed in different places, and each run passed the test
the other failed:

```
run 1:  FAIL (144/915) letters bridge::tests::document_round_trips_through_buffer
run 2:  FAIL (170/915) letters doc_tab::tests::header_and_footer_reach_the_page_view
        PASS (148/915) letters bridge::tests::document_round_trips_through_buffer
        PASS (155/915) letters bridge::tests::page_breaks_survive_the_buffer_round_trip
```

That last line is the fourth occurrence's victim passing. No defect in the
code under test can behave this way; only the environment can.

**Two claims made here earlier were wrong, and the correction matters more
than the claims did.** With four samples this file said the failures were
all `letters::bridge` tests landing in a band of "118 to 145 of some 900",
and inferred from the band that the failure is not something that builds up
over a run. The fifth occurrence (144) fit. The sixth does not: it is
`letters::doc_tab`, at 170.

The band was an artifact of the sample, not a finding. Letters' widget
tests *occupy* roughly indices 118–170 of this workspace's run order, so
any failure among them lands in that range whatever causes it — the band
described where the candidates are, not when the failure happens. Six
samples spread across the whole of it, with the failing test differing run
to run, fit a per-process chance that any one widget test's X connection is
refused, and carry no information about position at all.

So the position argument against connection accumulation is withdrawn. What
still stands against it is the measurement: eight iterations of the widget
tests against one persistent display, roughly 750 GTK inits, zero refusals.
That is evidence about a long-lived display, and it is the only evidence
here that is.

The rate is worth recording too, because "intermittent" undersells it: of
roughly ten `test`-lane runs across one afternoon's pull requests, four
failed this way. At better than one in three it is a gate, not a
curiosity.

One pull request took three of those four, consecutively — the two runs
above plus a third on its base merge, at 157 of 921
(`bridge::tests::prose_containing_pipes_is_not_captured_as_a_table`). It
could not land, and nothing about its diff was involved in any of the
three failures.

That clustering is worth a note rather than a conclusion. If each widget
test carried an independent chance of refusal, the ~150 of them per run
and a one-in-three run-failure rate put that chance near 0.2%, and three
consecutive run failures would be a 1-in-27 coincidence — unlikely, not
impossible. So it is either bad luck or something about a particular
runner, and two samples cannot tell those apart. Recorded so the next
person can check whether the failures follow a runner rather than a
commit, which is the question the numbers now raise and cannot answer.

`Xvfb -maxclients n` remains the lever if a client-slot limit is ever
shown to be the cause, and a nextest test group with `max-threads = 1`
over the Letters widget tests is the lever for reducing how many X
connections are open at once. The second is not a masking fix — it changes
scheduling, not assertions, so unlike the retry it cannot turn a
display-less run green — but it costs run time and rests on a hypothesis
nothing here has confirmed, so it should be a deliberate decision rather
than a reflex.

Still open, and the next step is a reproduction rather than another
mitigation: the failure has only ever appeared in a full-workspace
`cargo nextest run`, and the position in the run order is the only clue
that the rest of the workspace matters. The same note is in
`suite-common/src/gtk_test.rs`, where somebody debugging it will be
reading.
