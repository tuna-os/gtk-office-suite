## September readiness: make every testing instrument executable in CI

Reuse this issue for validator-test wiring, coordinated with #241 (GTK execution), #354 (GUI) and #441 (evidence). Audit reference: `e7e4df6`.

- [x] Run dependency-free Python validator tests (tests/test_release_gate.py,
      tests/test_validate_parity.py) on PRs, including changes to the tests
      themselves — the `python-checks` job in `ci.yml`, which runs on every
      push and pull request with no path filter, so a change to the tests
      is itself gated.
- [x] Run model/controller/property tests with locked dependencies; keep
      failures and minimized Unicode regressions visible — `cargo nextest
      run --workspace` with a committed `Cargo.lock`; JUnit uploaded on
      every outcome.
- [x] Run GTK tests on their initialized GTK main thread under an isolated
      display; fix the recurring nightly failure before treating coverage
      as a gate — done in #241: the dispatcher was already there, but the
      tests were skipping *and passing* with no display, and the nightly
      coverage job had no display either. Both now run under
      `xvfb-run`, and a widget test without a display fails.
      See [gtk-threading.md](gtk-threading.md).
- [x] Trigger interop on changed format/model/bridge/save code and require
      it for release; nightly-only execution must not certify a different
      release revision — `interop-corpus` runs on every PR, the LibreOffice
      oracle now also runs on pull requests that touch format or model code
      (`nightly.yml`), and on a tag
      `release-revision.yml` refuses to certify a release whose oracle
      evidence was recorded at another revision. See
      below for what that was hiding.
- [x] Separate fast core, GUI and nightly/release lanes; cache builds, bound
      runtime, publish JUnit/artifacts and collect skip reasons — the lanes
      and the `if: always()` artifact uploads were already there; skip
      reasons now are too, and collecting them could not be done the
      obvious way. See [below](#the-skips-a-junit-report-cannot-show).
- [ ] Wire corpus validation, parity validation, validator self-tests and
      release contract into required checks — three of the four run on
      every pull request; whether they are *required* is a
      branch-protection setting, which the repository cannot read — but a
      failed run can. A `Screenshots` dispatch on `955f628` tried to push
      its refresh to `main` and was refused:

      ```
      remote: error: GH006: Protected branch update failed for refs/heads/main.
      remote: - Required status check "test" is expected.
      ```

      So `test` **is** required, observed rather than assumed, and the
      setting announces itself to anything that tries to bypass it. The
      other three are still unknown by the same argument, and nothing here
      tries to push to `main` to find out. (The deadlock that run hit — a
      `[skip ci]` commit needing the check it forbade — is fixed
      separately; this entry is only about what it proved.) The
      release contract is the exception and it is worse than unrequired:
      `release-gate.yml` only triggers on `flatpak/**`, `flathub/**`,
      `po/**`, `Cargo.lock` and its own path, so ordinary pull requests
      never run it — and `scripts/release_gate.py` is **currently failing
      on `main`**: `tables/src/window.rs` is 2343 lines against a 2300-line
      ceiling. It crossed at `5c7f436` (2329) and nothing noticed, because
      nothing runs it. Raising the ceiling would be relaxing the gate to
      get green, so it is recorded here rather than patched in passing.
- [x] Prove enforcement by deliberately breaking a referenced test, fixture
      and required evidence entry — the validators have negative unit tests
      for each, and the end-to-end wiring is now proved too: see below.
- [x] Keep expensive visual/oracle matrices scheduled, with explicit
      current-revision release runs — `nightly.yml` and `gui-stress.yml`.

## The claim no lane was checking

`validate_capabilities.py` compares each cited test against an inventory of
what a lane collected, and a lane only speaks for the namespaces it
declares — so an id in a namespace *no* lane covers was skipped by every
lane and silently accepted. Demonstrated on the real ledger, before the
fix, with a claim citing a path that does not exist:

```
$ python3 conformance/validate_capabilities.py \
    --ledger ledger-with-a-phantom-claim.json --collected gui-inventory.json
CAPABILITY LEDGER OK: 10/10 verified; checked structure, collected tests
```

The Rust lane skipped it too, for the same reason. `--require-coverage`
catches exactly this, and its own documentation says it is "for the job
that holds every inventory" — a job that did not exist, and which nothing
passed the flag from: the option was dead code.

The same run exposed a concrete instance rather than a hypothetical one:
`ci.recorded-journey-evidence` cites `tests/test_gui_evidence.py::...`, and
**no lane collected the `tests/` namespace at all**.

`conformance/lanes.json` now declares which job collects which namespace,
and:

- `--lanes` checks every cited test against that map. It needs no test run,
  so it gates every pull request from the `python-checks` job.
- `--lanes --lane <job> --collected <inventory>` additionally asserts the
  job's declared namespaces against what it really collected, so the map
  cannot drift into fiction. All three lanes assert their own row.
- The `python-checks` job now collects its own tests, closing the `tests/`
  hole.

Two things this shook out while being verified, both of which a hand-waved
version would have shipped:

- Declaring the `tests/` *directory* was itself the bug in miniature: it
  vouched for `tests/nonexistent/...` too, and the phantom claim still
  passed. Namespaces are now the files a lane actually runs, derived by
  `collect_test_inventory.py` from what it collected rather than from the
  prefix it was handed. A test asserts no lane declares a bare `tests/`.
- `pytest --collect-only -q -q` prints per-file *counts*, not node ids,
  unless something else has added a `-v` — which the GUI runner does and a
  plain invocation does not. Copying the GUI lane's flags produced an
  inventory of zero tests, which would have made the new check vacuous.

Use warm runtime measurements to set budgets; an aspirational time estimate is not evidence. Existing tests may be reused, but a passing test command that collected zero relevant tests is a failure of the gate.

## The verdict reached on different code

The LibreOffice oracle and the parity corpus need LibreOffice installed, so
they were nightly-only: they ran against whatever `main` was at 05:00.
Nothing connected that to a release, and the numbers were not hypothetical
when this was written:

```
last successful nightly   b92a82e   2026-09-11 09:17Z   (nightly run 54)
main                      e517c96   2026-09-11 22:22Z   (five merges later)
```

A tag cut that evening would have been certified by an oracle run against
five merges' worth of different code, and the ledger would have recorded the
claim as verified without anything noticing that the two revisions
disagreed.

`conformance/lanes.json` now states, per lane, whether it
`runs_on_every_revision`, and `--release-revision` refuses to certify a
release with claims resting on a lane where that is false unless the
evidence carries the release revision. `release-revision.yml` passes
`$GITHUB_SHA` on tags only, so pull requests are unaffected;
`docs/RELEASE.md` says how to satisfy it (dispatch the lane at the release
commit, record what it found).

The property is a declared field rather than something inferred from the
lane's triggers, because the oracle is no longer purely scheduled — it now
runs on format-touching pull requests too, and that still does not mean it
ran at the revision being released. A lane that omits the field is refused
rather than assumed trustworthy.

Two things this shook out:

- **The check could have passed by having nothing to check.** No claim
  cited the oracle at all, so the first working version of the rule was
  vacuous — it validated a ledger in which the expensive lane certified
  nothing. A lane the rule watches that no claim cites is now itself an
  error, which is what forced `suite.libreoffice.interop-oracle` into the
  ledger.
- **The lane map vouched for tests that pass without running.** The `test`
  lane declares crate-level namespaces like `letters-core::`, and nextest
  reports the oracle targets under the same crate. Without LibreOffice
  those tests do not skip — they return early and report **ok**. Measured
  by running the test binary with nothing on `PATH`:

  ```
  running 1 test
  skipping: soffice not installed
  test we_read_soffice_output ... ok
  ```

  So on a pull request the lane offered a pass, from a run with no
  LibreOffice in it, as live evidence for the one claim only the nightly
  oracle can support. Lanes can now declare an `excludes` list for
  namespaces they do not run, and the `test` lane names the oracle targets
  there. (`REQUIRE_SOFFICE=1` is what turns the absence into a failure, and
  only the oracle job sets it.)

  This is also why collecting *skip reasons* would not have found these:
  they are not recorded as skips. That item is now done, and the section
  below is about the other half of the same lesson — the skips a report
  does not contain.

`--head` was the same shape of dead option as `--require-coverage` before
it: the staleness check existed, nothing passed it a revision. It still
applies to every claim, which is why the release rule is scoped to the
lanes that can actually be stale rather than reusing it.

## The filter that listed the wrong crates

The oracle's pull-request trigger shipped with this list:

```
letters-core/**  tables-core/**  decks-core/**
suite-export/**  interop/**  Cargo.lock  nightly.yml
```

`suite-common-core/**` is missing from it, and that crate holds
`interop.rs`, `atomic_save.rs`, `zip_guard.rs`, `format.rs`, `units.rs` and
`style.rs` — the packaging and save code the oracle exists to check. A
change to the ZIP writer could have merged without the oracle ever seeing
it. All three crates the oracle tests depend on it, so the omission was not
subtle; it was simply a hand-written list in YAML that nothing checked.

Found by noticing the oracle *not* running on a pull request that touched
`suite-common/` and asking why rather than moving on. (`suite-common/` is
GTK glue — file dialogs, toasts, `gtk_test` — and correctly does not
trigger it. The two crates are easy to confuse, which is part of why a list
maintained by hand drifts.)

This is the same defect class the journey selector closed: a decision about
what runs, living somewhere untestable, failing silently in the direction of
running too little. `tests/test_oracle_triggers.py` now derives the required
set instead of trusting the list — the crates named by the workflow's own
`cargo test -p` commands, plus every workspace crate those depend on,
transitively, read from the manifests. It fails naming
`suite-common-core` against the old filter, and the parser raises rather
than returning an empty list when it cannot find the filter, so the check
cannot pass by finding nothing to check.

Worth stating plainly: both halves of this — the omission and the reason it
went unnoticed — were in work merged hours earlier in the same session. The
lesson that a hand-maintained list needs a derived check was available and
not applied the second time.

## The skips a JUnit report cannot show

The obvious way to collect skip reasons is to read them out of the JUnit
report the `test` lane already publishes. It does not work, and the reason
is the same shape as everything else on this page: **the report does not
contain the skips.** nextest omits `#[ignore]` tests entirely — not as
`<skipped>`, but as no `<testcase>` at all. Measured on this workspace,
which has seven ignored tests:

```
tests="848"  skipped="0"      # on the <testsuites> element
```

A collector built on that report would have published a
clean sheet, indefinitely, while seven tests sat out every run — a check
reporting success without having checked, which is the defect this whole
area keeps producing.

So the inventory comes from the test *lister*, which does report them:

```
$ cargo nextest list --workspace --run-ignored all --message-format json
... "ignored": true ...
```

The lister says *which*. It does not say *why*: libtest drops the string
from `#[ignore = "reason"]` and neither `--list` nor nextest's JSON carries
it. Measured directly on a test binary:

```
$ ./target/debug/deps/stateful-0e642a1bac66350f --list --ignored
seed_campaign: test

1 test, 0 benchmarks
```

The reason is nowhere in it. So the reason has to come from the source, and
the listing is what keeps the source scan honest — a grep for `#[ignore]`
alone is exactly the "source-string existence" evidence the ledger rejects.
Each side covers the other's blind spot, and
`conformance/collect_skip_reasons.py` rejects all four ways they can
disagree:

| what it sees | why it is rejected |
|---|---|
| ignored, bare `#[ignore]` | a test that sits out every run without saying why is indistinguishable from one that was forgotten |
| ignored, `#[ignore = ""]` | an explicit empty reason is not a reason |
| a reason no listed test matches | the test was renamed, deleted or `cfg`'d out; a stale reason reads as current |
| an empty listing over a tree that has `#[ignore]` in it | the listing is of the wrong workspace, or was taken without `--run-ignored all` — so every check above would hold vacuously |

The last row is the one worth keeping. Without it the collector passes on a
listing that covers nothing, which is how `--require-coverage` and `--head`
both went dead earlier on this page.

Matching is per *binary*, not per function name. `seed_campaign` exists in
two integration targets in each of the three core crates, so a scan keyed
on the function name would let an explained copy vouch for a bare one. The
listing names the binary, which names the file, so each attribute is looked
for only where its own test lives.

Runtime skips — pytest's `skipTest`, nextest's `<skipped>` — *are* in JUnit
with their message, so the same report folds those in rather than leaving
two half-reports to reconcile. A runtime skip with no message is an error
for the same reason a bare `#[ignore]` is.

One bare `#[ignore]` existed when this was written: `dump_failures` in
`letters-core/tests/corpus_debug.rs`, a diagnostic printer with no
assertions. It now says so, and says what gates instead — the
`markdown_corpus.rs` round-trip ratchet it prints the failures of.
