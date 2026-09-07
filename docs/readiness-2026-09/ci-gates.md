## September readiness: make every testing instrument executable in CI

Reuse this issue for validator-test wiring, coordinated with #241 (GTK execution), #354 (GUI) and #441 (evidence). Audit reference: `e7e4df6`.

- [ ] Run dependency-free Python validator tests (tests/test_release_gate.py, tests/test_validate_parity.py) on PRs, including changes to the tests themselves.
- [ ] Run model/controller/property tests with locked dependencies; keep failures and minimized Unicode regressions visible (#377/#371/#358/#324).
- [ ] Run GTK tests on their initialized GTK main thread under an isolated display; fix the recurring nightly failure before treating coverage as a gate.
- [ ] Trigger interop on changed format/model/bridge/save code and require it for release; nightly-only execution must not certify a different release revision.
- [ ] Separate fast core, GUI and nightly/release lanes; cache builds, bound runtime, publish JUnit/artifacts and collect skip reasons.
- [ ] Wire corpus validation, parity validation, validator self-tests and release contract into required checks.
- [ ] Prove enforcement by deliberately breaking a referenced test, fixture and required evidence entry.
- [ ] Keep expensive visual/oracle matrices scheduled, with explicit current-revision release runs.

Use warm runtime measurements to set budgets; an aspirational time estimate is not evidence. Existing tests may be reused, but a passing test command that collected zero relevant tests is a failure of the gate.
