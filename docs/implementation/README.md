# Implementation specifications for the remaining office roadmap

Requested by the maintainer on 2026-10-13: write enough detail that an agent can
implement a bounded task without rediscovering product intent or architecture.
These documents specify **remaining work**, not completed features. The finish
line remains the full [roadmap](../../ROADMAP.md) and
[#1190](https://github.com/tuna-os/gtk-office-suite/issues/1190), including
installed candidate evidence and authorized release/Flathub rollout.

## Read and assign

| Specification | Ownership | Required result |
|---|---|---|
| [Tables](tables.md) | Rust workbook/controller, IronCalc integration, native spreadsheet UI | Trustworthy ordinary XLSX authoring, preserved admitted workbook semantics, coherent calculation and editing |
| [Letters](letters.md) | Canonical document model, native page editor, shared typesetting | Trustworthy DOCX/ODT authoring, structured/review semantics and independently verified page output |
| [Decks](decks.md) | Slide/object/controller, reusable design model, GTK canvas/presenter | Editable and preserved presentation designs, accurate output and dependable presenting |
| [Shared](shared.md) | Core save/recovery policy, evidence, security, accessibility and release | Common safety invariants and exact-revision installed-app qualification |

Use the stable package IDs in these files in PR descriptions and issue comments.
Do not turn their headings into completed checkboxes merely because code was
written. Each package's acceptance contract is the closure rule.

### What is already a checkpoint

PR #1550 merged as `6dacab4d`. Its source baseline includes the product
assessments, Decks reachable accessible plain-text editing and run reconciliation,
Decks missing-source/recovery-copy and missing-image preflight, and Tables atomic
compound protection enforcement. Keep those regressions. Do not recreate them.
Their scope does not include complete rich-text selection UI, general package
preservation, atomic external-writer exclusion or full protection persistence.

PR #1552 is a separate in-progress XLSX protection-persistence implementation.
Until its actual core/GUI results and merge are recorded, it is a candidate, not
a completed dependency. Read its final diff rather than implementing the same
fields in parallel. User research edits outside this specification remain owned
by the maintainer; do not stage or overwrite them.

## Mandatory agent handoff contract

For each assigned package, the implementer must receive:

1. **Package ID and admitted scope.** Copy its exact acceptance criteria and
   non-goals. A smaller substitute requires an explicit scope decision.
2. **Starting revision and prerequisites.** Name prerequisite PRs/commits and
   check they actually landed; an open issue's title is not evidence.
3. **Files owned by the task.** Source modules, tests, fixtures and docs. If two
   tasks share a model, serializer or GUI file, one owns integration and the
   others agree the type/signature before edits.
4. **One authoritative state contract.** State fields, units, stable identities,
   mutation boundary, undo grouping, savepoint/recovery semantics and which
   outputs are derived. Never add a second formula/document/layout authority.
5. **Failure and loss behavior.** Which inputs reject, warn or preserve; what
   stays unchanged on cancel/failure; which errors must be visible and accessible.
6. **Proof to return.** Exact test IDs, candidate revision, commands, actual
   results/skips, artifact URLs and inspected images. Proposed test names in a
   spec are not test results. The result must distinguish model, format, GUI,
   renderer, independent reader and installed-app evidence.

A completion report contains changed files, observed behavior, evidence,
remaining limitations and merge URL. Do not accept “builds”, “looks correct”,
“tests should pass” or “scaffold ready” as package completion.

## Execution protocol

1. Read the package, relevant readiness page and current source. Inspect
   constructors/callers/references before changing exported types. The files in
   these specs are entry points, not permission to ignore other callers.
2. Reproduce the missing behavior on the real path. For a bug retain the minimal
   failure and input; for a format feature prepare a foreign-writer fixture.
3. Implement pure parsing, geometry, calculation policy and document operations in
   the appropriate GTK-free core. GTK code wires actions and renders state.
4. Keep one user operation one history transaction. Preflight compound changes
   before mutation; denied commands cannot partially update data, metadata,
   selection, dirty state or redo. Internal restoration has its own trusted path.
5. Migrate all consumers of changed model fields, including load/save, clipboard,
   recovery, screen, export, thumbnails, accessibility and collaboration's existing
   serialization where compiled. Remove obsolete compatibility paths instead of
   leaving conflicting implementations.
6. Add deterministic consumer-level regressions for plausible loss/crash/semantic
   errors. Do not test source wording, forwarding, mock echoes or incidental
   defaults. Keep core tests GTK-free; GTK tests need an actual display/thread.
7. Commit a bounded implementation PR early. Run heavy builds, per-app render and
   export jobs in **parallel CI**, not on a disk-constrained workstation. Do not
   push an unrelated batch merely to obtain another lane's result.
8. Exercise actual application actions through existing AT-SPI journeys. For any
   visible document change add/extend render-lab fixtures and inspect the report
   against LibreOffice; a roundtrip test is not visual proof.
9. After proof, update the package's linked readiness entry, public feature docs
   and `conformance/capabilities.json` with actual collected IDs and revision.
   Update measured source-size tables when affected; never lower ceilings as a
   substitute for sensible ownership.
10. Merge after required checks and affected behavior pass. Record unrelated
    known failures separately, without suppressing them or calling the release
    green. Use a new branch/PR for the next bounded package. Final release claims
    require the shared candidate gates even if every feature PR merged.

## Verification commands and instruments

Run only the affected core subset while iterating; exact filters are given in the
app specs and must be checked against current test inventory. Full workspace
verification belongs in CI. Existing entry points include:

```sh
xvfb-run -a cargo test --workspace
cargo clippy --workspace
python3 conformance/validate_capabilities.py
python3 conformance/validate_parity.py
python3 tests/test_roadmap_figures.py
python3 tests/test_feature_docs.py
tests/gui/run_gui_tests.sh test_smoke.py -k Tables
tests/gui/run_gui_tests.sh test_smoke.py -k Letters
tests/gui/run_gui_tests.sh test_smoke.py -k Decks
tools/render-lab/run.sh --app tables
tools/render-lab/run.sh --app letters
tools/render-lab/run.sh --app decks
REQUIRE_SOFFICE=1 cargo test -p decks-core -p letters-core -- --test-threads=1
```

The LibreOffice commands require Writer/Impress and a working conversion path;
`soffice --version` alone is not sufficient. Package/sandbox/reference environment
limitations must be reported. Do not substitute `SUITE_GTK_TESTS=skip` and describe
those widget tests as passing. Release workflows additionally validate collected
IDs, real outcomes and scheduled evidence at the exact candidate revision.

## Parallelism and dependency rules

- App-local reader/render/authoring tasks can proceed independently **after** the
  authoritative model contract is agreed. Model and writer migrations are not
  independent if either invents a different representation.
- Share infrastructure once: SH-002 conflict policy and SH-003 recovery envelopes
  are consumed by all apps, not reimplemented in each window module.
- Within one app, separate importer and exporter owners can work in parallel on
  one agreed model; one integration owner owns end-to-end tests and loss policy.
- Render fixtures may be authored alongside implementation, but a baseline cannot
  be accepted before the actual candidate screenshots are inspected.
- Accessibility and performance are acceptance of each feature, not a final
  cleanup task. Architecture decomposition must preserve behavior, not become an
  excuse to postpone missing product workflows.
- Manual Orca, external-display observation, elapsed stability streaks, repository
  settings and Flathub authorization cannot be completed by code delegation.
  Keep them visible as explicit prerequisites with owners and evidence requests.

## Scope reconciliation and deferred portfolio

Older ADRs and issue closures are historical decisions, not automatic present-day
truth. ADR 0003's exclusions for review, pivots, arrays and master editing overlap
newer ADRs and shipped paths. App specs must reconcile those explicitly; never
silently discard a currently advertised feature by citing an older exclusion.
The intended products are GNOME-native Rust applications, not copied Office
ribbons or Numbers' multiple-table canvas.

The #1190 decision parks CRDT collaboration beyond its spike, WASM extensions,
enterprise policy, telemetry, encryption/signatures and `suite-convert` in the
later portfolio (#1216). This registry preserves them rather than disguising them
as completed or allowing them to expand everyday-editing packages:

| Deferred item | Existing design/constraint | Admission prerequisite and acceptance boundary |
|---|---|---|
| Collaboration beyond spike | [RFC-0001](../rfc/0001-crdt-collaboration.md), Loro; #1043 | Dependency advisory cleared; agreed identity/operation/recovery protocol; deterministic concurrent convergence, offline/reconnect and format-loss tests before enabling by default |
| WASM extensions | No runtime added by these specs | Separate security/API/lifetime/resource-limit specification; sandbox escape and document mutation/undo contracts; explicit maintainer admission |
| Enterprise dconf/policy | Existing ordinary settings are not managed policy | Specify locked/read-only setting precedence and administrator deployment; normal settings remain independently usable |
| Telemetry | Not needed for daily-driver correctness | Explicit privacy/consent/data-retention decision; no unrequested collection added while fixing diagnostics |
| Encryption/signatures | Protection flags are not encryption | Separate cryptographic format/threat/key-lifecycle design and independent interoperability corpus; never market sheet locks or password hashes as confidentiality |
| Headless `suite-convert` | Tracked #579; deferred by #1190 | Admit CLI contracts for formats, loss confirmation, exit codes and output transactions; reuse cores without GUI or second serializer |
| Translations | Gettext wiring admitted by ADR 0003; actual translations community-owned | Extract actual current strings, provide translator context, exercise locale/font/RTL UI; language completeness requires native review |

These are admission specifications, not promises of hidden implementations. If the
maintainer admits one into this milestone, expand it into the same concrete
file/API/error/test contract before implementation and update #1190's scope. Full
Office/Numbers macro, automation, cloud-service and proprietary-format parity is
not implied by “viable alternative.” Unsupported content still requires honest
preservation or loss reporting in the current products.

## Closure audit

Before claiming the roadmap complete, map **every remaining readiness row and
admitted app capability** to a package, then to current evidence. Confirm all app
packages and SH-001…SH-008 acceptance criteria, exact candidate identity, no P0
loss/crash, installed-app tests, manual accessibility, architecture coverage,
security floor and authorized rollout. A missing artifact, unrun test, stale
revision or absent human/environment prerequisite means the item remains open.
