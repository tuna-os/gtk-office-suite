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
- [ ] Trigger interop on changed format/model/bridge/save code and require
      it for release; nightly-only execution must not certify a different
      release revision — `interop-corpus` runs on every PR; the
      release-revision rule is not enforced yet.
- [ ] Separate fast core, GUI and nightly/release lanes; cache builds, bound
      runtime, publish JUnit/artifacts and collect skip reasons — the lanes
      and the `if: always()` artifact uploads exist; skip *reasons* are not
      collected, though #241 removed the skips that mattered most by making
      them failures.
- [ ] Wire corpus validation, parity validation, validator self-tests and
      release contract into required checks — all four run; whether they are
      *required* is a branch-protection setting, not visible from the
      repository.
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
