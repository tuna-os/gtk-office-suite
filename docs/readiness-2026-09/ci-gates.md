## September readiness: make every testing instrument executable in CI

Reuse this issue for validator-test wiring, coordinated with #241 (GTK execution), #354 (GUI) and #441 (evidence). Audit reference: `e7e4df6`.

- [x] Run dependency-free Python validator tests (tests/test_release_gate.py,
      tests/test_validate_parity.py) on PRs, including changes to the tests
      themselves — the `python-checks` job in `ci.yml`, which runs on every
      push and pull request with no path filter, so a change to the tests
      is itself gated.
      The list of files it runs is no longer trusted to be complete.
      `ci.yml` names them one per line, twice — once to run them and once to
      collect them for the capability inventory — and nothing compared either
      list with the directory, so a new `tests/test_*.py` passed locally,
      never ran here, and reported nothing when it broke. "Runs on every push
      with no path filter" only gates the files somebody remembered to list.
      `tests/test_ci_test_list.py` now resolves both directions: every
      top-level test file must appear in every pytest step this workflow has,
      and every file a step names must exist. The steps are discovered from
      the workflow rather than named, so a third one is covered the moment it
      exists.
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
      release contract into required checks — the required-check configuration
      was directly inspected through GitHub's branch-protection API on
      2026-10-13: only `test` is required; `required_pull_request_reviews` is
      null and `enforce_admins` is false. `Python validators and their tests`,
      `interop-corpus`, and `RustSec advisory gate` run but are not required.
      `release-gate.yml` still has a PR path filter, so its contract check is
      absent from ordinary PRs. No settings were changed: making the failing
      RustSec gate required would intentionally block non-admin merges until
      #1549 is resolved. The existing `test` requirement was also observed
      directly, not inferred from the repository's YAML.
      **2026-10-15 (SH-001):** the proposal is no longer prose. It is
      generated from the workflows' real job names and checked on every
      pull request — see [the required-check boundary](#the-required-check-boundary-sh-001)
      below, which also records the API output observed that day (still only
      `test`). The item stays open until an administrator applies the list;
      the validator cannot check off a repository setting.
      That list used to omit the very files the gate polices. It enforces
      per-module line ceilings — `letters/src/window.rs`,
      `tables/src/window.rs`, `decks/src/window.rs`,
      `suite-common/src/lib.rs` — and triggered on none of them, so a pull
      request that grew one could not run the check that would have
      rejected it. `tables/src/window.rs` duly crossed its 2300-line
      ceiling at `5c7f436` (2329) and sat over it on `main`, reaching 2343,
      until #594 moved the chart dialog out and brought it to 2226. Nothing
      reported the breach for its whole duration.
      This was the third instance of one defect — a hand-written path
      filter in YAML omitting the code it exists to check, after the
      oracle's missing `suite-common-core` and the journey selector's
      fallbacks. So the four paths are in the trigger now, and
      `tests/test_release_gate.py` **derives** the requirement from
      `release_gate.py`'s own `module_ceilings` rather than restating it:
      adding a ceiling without a trigger fails, and a parse that finds no
      ceilings or no filter fails too rather than holding vacuously. Three
      mutations confirm each direction.
      An earlier revision of this entry recorded the gate as *currently
      failing on main*. That was true when written and is not now, which is
      the trouble with putting a transient status in prose — the line
      outlived the breach it described. What replaces it is structural: what
      the trigger covers, and what derives it.
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

## The required-check boundary (SH-001)

A passing pull request is only as strong as what branch protection requires,
and on 2026-10-13 that was `test` alone. Re-inspected on **2026-10-15** at
`main` = `d550048c2931d6a5910d7c42cfbf378f08b28944`:

```
$ gh api repos/tuna-os/gtk-office-suite/branches/main/protection
required_status_checks: {"strict": false, "contexts": ["test"],
                         "checks": [{"context": "test", "app_id": 15368}]}
enforce_admins: false          required_signatures: false
required_linear_history: false allow_force_pushes: false
required_conversation_resolution: true
(no required_pull_request_reviews; GET repos/.../rulesets returns [])
```

**Nothing was changed.** Applying a list is a repository-administrator
decision and a separate settings change from the validator that proposes it
(SH-001's acceptance keeps them apart). The account that produced this page
can read the setting; it was deliberately not used to write it.

### Lane evidence, not lane exit codes

Each lane now records *what ran and how it ended*, at *which revision*, and
the ledger is judged against that record:

| Lane | Workflow → job | Outcomes now read from | Notes |
|---|---|---|---|
| `test` | `ci.yml` → `test` | nextest JUnit | unchanged source; now stamped with the revision |
| `python-checks` | `ci.yml` → `python-checks` | pytest `--junitxml` plus the collection | previously collection only |
| `smoke` | `gui-tests.yml` → `smoke` | pytest `--junitxml` of the journeys and the harness run | previously collection only; the change-based selection (#108) is recorded as `deselected` and printed, never counted |
| `oracle` | `nightly.yml` → `oracle` | nextest JUnit (was `cargo test`, whose outcomes nothing could read back) | previously no ledger check at all |

`validate_capabilities.py --lane <job> --collected … --results … --candidate <sha>`
rejects, for that lane's own namespaces only:

- an inventory with no tests in the lane's namespaces, or a report with no
  outcomes — a missing result, not a successful empty set;
- a collected test with no outcome (the run stopped early), unless the lane
  declares `selective` and the inventory lists it as deselected — and a
  "deselected" test that did report an outcome, which means the selection
  listing is not the one that ran;
- a cited test in the lane's namespaces the run did not collect, whatever the
  inventory claims to cover;
- an inventory recorded at another revision, or at none (`--candidate`): an
  older nightly is evidence about older code.

Statically, on every pull request (`--lanes`):

- **Per-layer namespaces** (`lanes.json` `layers`): a `letters-core::` unit
  test filed as `gui` evidence is refused even though a lane collects and
  passes it; the `gui`/`a11y` layers may not share a prefix with
  `model`/`format`/`persistence`/`bridge`.
- **Release-critical evidence runs on the candidate**: every test a
  release-critical capability cites must be in a lane marked
  `runs_on_candidate` (`test`, `smoke`) — the lanes `release-revision.yml`
  re-runs at the tag. There, `--every-release-critical-result` treats a
  release-critical test missing from the reports as never run, rather than
  as another lane's business, and the journeys step no longer ends in
  `|| true`.

Each rule has a negative test in `tests/test_validate_capabilities.py`
(`LayerNamespaces`, `ReleaseCriticalLanes`, `LaneInventories`,
`LaneScopedEvidence`, `EveryReleaseCriticalResult`,
`CollectorRecordsRevisionAndSelection`, `MainWithLaneReports`), and
`TheWorkflowsWireTheChecks` fails if a lane's own job stops passing
`--results`, `--candidate` or `--revision` — a flag nothing passes is how
`--require-coverage` and `--head` went dead.

### Failure semantics a required list has to respect

- A required context that **never reports** — a path-filtered workflow, a job
  skipped by `if:`, PR activity `types` that omit `synchronize` — leaves the
  check pending and blocks every pull request it did not run on.
- A job **skipped because a job it needs failed** reports *skipped*, which
  branch protection counts as passing. Requiring it proves nothing unless
  what it needs is required too.
- **One check name from two jobs** satisfies one rule with either.
- `continue-on-error` marks a job advisory by design.
- Contexts are pinned to GitHub Actions (`app_id` 15368), as `test` already
  is, so another app cannot satisfy them by posting a status of that name.

`conformance/required-checks.json` classifies every job a pull request can
start as `required`, `pending` (should be required; a named issue blocks it)
or `optional` with a reason, and `conformance/required_checks.py` (run in
`python-checks`) refuses a classification the workflows contradict: an
unclassified or vanished job, a required job with any of the gaps above, a
duplicated or underivable name, or a lane `lanes.json` says runs on every
revision that is not required. `tests/test_required_checks.py` holds the
mutation tests and asserts the table below is the generated one.

### The proposal for administrator review

Generated by `python3 conformance/required_checks.py --markdown` — do not edit
by hand:

<!-- required-checks:begin (generated by conformance/required_checks.py --markdown) -->

| Check name | Workflow → job | Policy | Why |
|---|---|---|---|
| `Python validators and their tests` | `ci.yml` → `python-checks` | required | Reports on every pull request; a failure blocks the merge. |
| `flatpak (decks)` | `ci.yml` → `flatpak` | required | Reports on every pull request; a failure blocks the merge. |
| `flatpak (letters)` | `ci.yml` → `flatpak` | required | Reports on every pull request; a failure blocks the merge. |
| `flatpak (tables)` | `ci.yml` → `flatpak` | required | Reports on every pull request; a failure blocks the merge. |
| `interop-corpus` | `ci.yml` → `interop-corpus` | required | Reports on every pull request; a failure blocks the merge. |
| `test` | `ci.yml` → `test` | required | Reports on every pull request; a failure blocks the merge. |
| `smoke` | `gui-tests.yml` → `smoke` | required | Reports on every pull request; a failure blocks the merge. |
| `Export PDF vs LibreOffice PDF` | `render-parity.yml` → `export-report` | required | Reports on every pull request; a failure blocks the merge. |
| `Render scope` | `render-parity.yml` → `scope` | required | Reports on every pull request; a failure blocks the merge. |
| `Tiers A+B vs LibreOffice` | `render-parity.yml` → `report` | required | Reports on every pull request; a failure blocks the merge. |
| `Real documents (decks)` | `render-real.yml` → `lab` | required | Reports on every pull request; a failure blocks the merge. |
| `Real documents (letters)` | `render-real.yml` → `lab` | required | Reports on every pull request; a failure blocks the merge. |
| `Real documents (tables)` | `render-real.yml` → `lab` | required | Reports on every pull request; a failure blocks the merge. |
| `RustSec advisory gate` | `ci.yml` → `advisories` | pending | #1549: Fails on main today: Typst's direct serde_yaml pulls in unsafe-libyaml (RUSTSEC-2024-0449). Required now, it would block every merge until SH-007 removes or waives the path; the failure must stay visible, not be suppressed. |
| `WopiHost over MemoryStore (simulator, no Collabora)` | `collab-spike.yml` → `wopi-sim` | optional | Path-filtered to tools/collab-spike, docs/collab-spec and its own workflow (RFC-0004 spikes, not wired into the apps); it reports nothing on other pull requests, so requiring it would block them forever. |
| `cargo check --workspace --all-targets` | `collab-spike.yml` → `check` | optional | Path-filtered to tools/collab-spike, docs/collab-spec and its own workflow (RFC-0004 spikes, not wired into the apps); it reports nothing on other pull requests, so requiring it would block them forever. |
| `dcp-core, dcp-conformance, schemas` | `collab-spike.yml` → `core` | optional | Path-filtered to tools/collab-spike, docs/collab-spec and its own workflow (RFC-0004 spikes, not wired into the apps); it reports nothing on other pull requests, so requiring it would block them forever. |
| `dcp-interop (MatrixStore and WopiHost over MatrixStore, one Synapse)` | `collab-spike.yml` → `interop` | optional | Path-filtered to tools/collab-spike, docs/collab-spec and its own workflow (RFC-0004 spikes, not wired into the apps); it reports nothing on other pull requests, so requiring it would block them forever. |
| `dcp-matrix against Synapse (relaxed + default rate limits)` | `collab-spike.yml` → `matrix` | optional | Path-filtered to tools/collab-spike, docs/collab-spec and its own workflow (RFC-0004 spikes, not wired into the apps); it reports nothing on other pull requests, so requiring it would block them forever. |
| `does this pull request touch dcp-wopi?` | `collab-spike.yml` → `changes` | optional | Path-filtered like the rest of collab-spike.yml; it only decides whether the Collabora job runs. |
| `real Collabora Online round trip` | `collab-spike.yml` → `collabora` | optional | Runs on a schedule, a dispatch, a [collabora] commit message, or when a pull request touches dcp-wopi; skipped otherwise. It needs the collabora/code image and is a spike, not a merge gate. |
| `drop` | `drop-bot-review-requests.yml` → `drop` | optional | Bot-authored pull requests only, and only on open/reopen/ready: housekeeping, not a check of the change. |
| `image` | `feature-verification.yml` → `image` | optional | Runs only when the verify-video label asks for a recording; skipped, with no runner, otherwise. |
| `publish` | `feature-verification.yml` → `publish` | optional | Posts the recorded clips; runs only after a recording. |
| `record` | `feature-verification.yml` → `record` | optional | Needs the label-gated image job; skipped otherwise. Recording never decides a verdict (docs/CI-VIDEO-EVIDENCE.md). |
| `corpus (decks)` | `file-corpus.yml` → `corpus` | optional | Path-filtered to the corpus and GUI harness; it reports nothing on other pull requests, so requiring it would block them forever. |
| `corpus (letters)` | `file-corpus.yml` → `corpus` | optional | Path-filtered to the corpus and GUI harness; it reports nothing on other pull requests, so requiring it would block them forever. |
| `corpus (tables)` | `file-corpus.yml` → `corpus` | optional | Path-filtered to the corpus and GUI harness; it reports nothing on other pull requests, so requiring it would block them forever. |
| `build` | `gui-container.yml` → `build` | optional | Path-filtered to the test container's own files. |
| `vlm-audit (decks)` | `gui-tests.yml` → `vlm-audit` | optional | Scheduled/dispatch only and continue-on-error: model judgments flake and need API keys, so they inform rather than gate (AGENTS.md). |
| `vlm-audit (letters)` | `gui-tests.yml` → `vlm-audit` | optional | Scheduled/dispatch only and continue-on-error: model judgments flake and need API keys, so they inform rather than gate (AGENTS.md). |
| `vlm-audit (tables)` | `gui-tests.yml` → `vlm-audit` | optional | Scheduled/dispatch only and continue-on-error: model judgments flake and need API keys, so they inform rather than gate (AGENTS.md). |
| `Fuzz importer smoke tests` | `nightly.yml` → `fuzz-smoke` | optional | Skipped on pull requests by design (nightly cost). |
| `coverage` | `nightly.yml` → `coverage` | optional | Skipped on pull requests by design (nightly cost). |
| `oracle` | `nightly.yml` → `oracle` | optional | Path-filtered to format/model code (runs_on_every_revision: false). release-revision.yml refuses a release whose oracle evidence was recorded at another revision instead. |
| `Reproducible build (compare)` | `release-gate.yml` → `reproducible-compare` | optional | Path-filtered to release inputs; compares the two reproducible builds. |
| `Reproducible build (first)` | `release-gate.yml` → `reproducible-build` | optional | Path-filtered, and only when a release input changed (not a module-ceiling file alone); two full builds per run. Nightly on main. |
| `Reproducible build (second)` | `release-gate.yml` → `reproducible-build` | optional | Path-filtered, and only when a release input changed (not a module-ceiling file alone); two full builds per run. Nightly on main. |
| `aarch64-tests` | `release-gate.yml` → `aarch64-tests` | optional | Path-filtered, and only when a release input changed. Nightly on main. |
| `contract` | `release-gate.yml` → `contract` | optional | Path-filtered to release inputs and the module-ceiling files. Requiring the release contract needs it to report on every pull request first (an unfiltered trigger for this job), which is a cost decision for the administrator. |
| `offline-flatpak (aarch64, ubuntu-24.04-arm)` | `release-gate.yml` → `offline-flatpak` | optional | Path-filtered to release inputs; offline Flatpak builds on both architectures. |
| `offline-flatpak (x86_64, ubuntu-24.04)` | `release-gate.yml` → `offline-flatpak` | optional | Path-filtered to release inputs; offline Flatpak builds on both architectures. |
| `source-archive` | `release-gate.yml` → `source-archive` | optional | Path-filtered, and only when a release input changed. Nightly on main. |
| `Flatpak bundle (decks)` | `render-parity.yml` → `flatpak` | optional | Runs only on the schedule or a dispatch that asks for the VM tier; skipped on pull requests. |
| `Flatpak bundle (letters)` | `render-parity.yml` → `flatpak` | optional | Runs only on the schedule or a dispatch that asks for the VM tier; skipped on pull requests. |
| `Flatpak bundle (tables)` | `render-parity.yml` → `flatpak` | optional | Runs only on the schedule or a dispatch that asks for the VM tier; skipped on pull requests. |
| `Prior-release bundle (decks)` | `render-parity.yml` → `flatpak-prior` | optional | Runs only on the schedule or a dispatch that asks for the VM tier; skipped on pull requests. |
| `Prior-release bundle (letters)` | `render-parity.yml` → `flatpak-prior` | optional | Runs only on the schedule or a dispatch that asks for the VM tier; skipped on pull requests. |
| `Prior-release bundle (tables)` | `render-parity.yml` → `flatpak-prior` | optional | Runs only on the schedule or a dispatch that asks for the VM tier; skipped on pull requests. |
| `Tier C (Flatpak in a GNOME VM)` | `render-parity.yml` → `vm` | optional | Tier C needs the bundles above and is continue-on-error; it never runs on an ordinary pull request. |
| `export-lab (name not derivable)` | `render-parity.yml` → `export-lab` | optional | Started only for the apps a pull request reaches, like `lab`. `export-report` is the required verdict: it runs on every pull request and fails when an export lab in scope did not succeed. |
| `lab (name not derivable)` | `render-parity.yml` → `lab` | optional | Started only for the apps a pull request reaches (tools/render-lab/real_scope.py), so its check names are not known before the run. `report` is the required verdict: it runs on every pull request and fails when a lab in scope did not succeed. |

<!-- required-checks:end -->

To apply it, an administrator reviews the list and then runs:

```sh
python3 conformance/required_checks.py --payload > required.json
gh api -X PATCH repos/tuna-os/gtk-office-suite/branches/main/protection/required_status_checks \
  --input required.json
gh api repos/tuna-os/gtk-office-suite/branches/main/protection > protection.json
python3 conformance/required_checks.py --compare protection.json   # must print MATCHES
```

Against the 2026-10-15 API output, `--compare` reports the twelve proposed
contexts other than `test` as *proposed but not required*. The three
`Real documents (<app>)` contexts joined the proposal when `render-real.yml`
stopped being path-filtered: it now starts on every pull request, and an app
the change cannot reach (`tools/render-lab/real_scope.py`, derived from the
Cargo manifests) passes in seconds rather than going unreported. Before that,
DCK-006 (#1574) changed only `decks-core`, the lab did not run, and a real deck
went from amber to red on `main`. `render-parity.yml` uses the same scope the
other way round: its `Render scope` job lists the apps a pull request reaches,
lab and export-lab jobs start only for those, and the two report jobs —
which run on every pull request and fail when the scope job or a lab in
scope did not succeed — are the required verdict instead of six per-app
lab checks. Decisions left to the administrator, not made here:

- `RustSec advisory gate` is `pending` on #1549 (SH-007): it fails on `main`,
  and requiring it now would block every merge. Its failure stays visible.
- The release contract (`release-gate.yml` → `contract`) cannot be required
  while its workflow is path-filtered; requiring it means letting it report
  on every pull request first, which is a runner-cost decision.
- `smoke` has needed reruns (one on PR #1573's head); requiring it makes a
  journey flake block merges until it is rerun.
