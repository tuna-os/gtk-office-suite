# PLAN — Solid core for the GTK Office Suite

Goal: every app has a GTK-free core crate that owns its document model and
logic, tested at the unit level; widget code only renders and wires signals.
Others (humans and agents) build on the cores without touching GTK.

Method: TDD wherever the work is model/logic (phases 1–4: write the failing
test first, always). GUI work (phase 5) is verified by smoke tests instead —
red-green there means AT-SPI assertions, not unit tests.

Status legend: each phase lists its acceptance criteria. A phase is done when
its criteria pass in CI, not before.

---

## Phase 0 — Foundations (DONE, PR #86)

- [x] Local toolchain + GTK dev libs working; `cargo test --workspace` green
- [x] Launch-dead apps fixed (tables/decks `return;`, letters orphaned editor)
- [x] Gating AT-SPI smoke tests + honest `gui-tests.yml`
- [x] CLAUDE.md, docs/DEVELOPMENT.md, docs/TESTING.md

## Phase 1 — letters-core: a real document model (the big one)

The problem: Letters' model is `{ text: String }` of Markdown while actual
formatting lives only in GtkTextBuffer tags. Markdown cannot express what the
UI applies (highlight, columns, headers/footers, line spacing), so save/load
silently drops formatting.

New crate `letters-core` (no GTK deps):

1. **Model**: `Document { paragraphs: Vec<Paragraph> }`;
   `Paragraph { style: ParaStyle, runs: Vec<Run> }`;
   `Run { text: String, style: RunStyle }` (bold/italic/underline/strike/
   highlight/link); `ParaStyle` (heading level, alignment, list kind, line
   spacing). Design for span-merge invariants (adjacent runs with equal style
   are merged) — TDD these invariants first.
2. **Markdown I/O**: `Document ⇄ Markdown` via pulldown-cmark, with explicit
   documented lossiness (what Markdown can't hold). Round-trip tests:
   md → Document → md is identity for supported constructs.
3. **DOCX I/O**: `Document ⇄ DOCX` via rdocx, replacing the ad-hoc
   `docx_bridge` logic. Round-trip golden-file tests with fixtures in
   `letters-core/tests/fixtures/`.
4. **Editing ops as pure functions**: the markdown-macro expansion
   (`**x**` → bold), list continuation, and find/replace logic currently in
   `window.rs` re-written against `(Document | &str, cursor) → Edit`.
   Port the existing behavior test-first; the GtkTextBuffer versions then
   delegate.
5. **GTK bridge (in letters, not letters-core)**: `Document → TextBuffer`
   render and `TextBuffer → Document` capture. This is the only place tags
   are read/written. Smoke test: format text, save, reload, formatting
   survives.

Acceptance: formatting round-trips through save/load (docx + md); word count,
find/replace, macros unit-tested in letters-core; `letters/src/window.rs`
shrinks materially.

## Phase 2 — tables-core extraction (establishes the pattern cheaply)

Move `engine.rs` (IronCalc wrapper), `undo.rs` command types, and XLSX
import/export into `tables-core`. IronCalc already owns the model, so this is
mostly mechanical — but add the missing tests as things move (TDD for any
behavior that isn't already covered):

- cell get/set/formula evaluation edge cases (1-based indexing, number
  formatting boundaries)
- undo/redo command round-trips
- XLSX round-trip golden files (calamine in, rust_xlsxwriter out)

Acceptance: `cargo test -p tables-core` runs without GTK headers installed;
tables binary only contains widgets/rendering.

## Phase 3 — decks-core extraction

Same move for Decks: slide model, PPTX I/O (zip + quick-xml), undo commands,
markdown import. PPTX round-trip golden-file tests. Acceptance mirrors
phase 2.

## Phase 4 — Typst crate instead of Typst CLI

Replace `export.rs` shelling-out with the `typst` library crate (same engine,
no external binary for Flatpaks to carry). One shared implementation in
`suite-common-core` (or a small `suite-export` crate) used by all three apps.
TDD: given a model document, the generated Typst source is asserted
(deterministic string), and a compile-to-PDF smoke test checks output is a
valid non-empty PDF. Remove the CLI from Flatpak manifests.

## Phase 5 — App/UX debt that tests exposed (GUI tier)

- Bundle zspell dictionaries in the Flatpaks — spell check is silently off
  ("no dictionary found") for every user.
- A11y audit follow-up: PageContainer now allocates correctly, but audit all
  custom widgets; every interactive widget must appear in the AT-SPI tree
  (this is also what makes the suite testable).
- Extend smoke tests as behaviors become assertable (tables cell entry,
  decks slide add).

## Phase M — Measurement: conformance corpora + feature scorecard (cross-cutting)

"Are we actually becoming a fully-featured office suite?" must be answerable
from CI output, not vibes. Two instruments, started alongside phase 2 and
grown with every phase:

1. **Ported/mature test corpora** (vendored under `conformance/`, each with
   LICENSE + provenance noted — only permissive/compatible sources):
   - **CommonMark spec examples** (~650 cases, CC-BY-SA): used as an input
     corpus for letters-core Markdown *round-trip idempotence*
     (parse → Document → serialize → parse = same Document). We measure the
     pass-rate; 100% is not required to merge, regressions are.
   - **python-docx / python-pptx / openpyxl test fixtures** (MIT): real-world
     .docx/.pptx/.xlsx files driving open-without-crash and round-trip tests
     for letters-core, decks-core, tables-core.
   - **IronCalc's own function tests** (MIT) as a baseline, extended with a
     table-driven spreadsheet-function conformance suite keyed to the ODF
     OpenFormula function list.
   - Rejected for licensing: LibreOffice/Gnumeric test documents (MPL/GPL —
     incompatible with vendoring into this Apache-2.0 repo).
2. **Feature scorecard**: `FEATURES.md` per app — the target feature list of
   a credible office suite (formatting, styles, tables-in-docs, images,
   charts, print, formats...) where a feature may only be checked off with a
   link to the test that proves it. A small script
   (`conformance/scorecard.py`) counts corpus pass-rates + checked features
   and writes them to the CI job summary, so every PR shows the number going
   up (or refuses to let it silently go down).

Acceptance: CI job summary shows per-app scorecard; conformance pass-rates
are tracked in-repo and monotonically enforced (a PR that lowers a rate
fails).

## Phase 6 — Guardrails so the foundation stays solid

- `CONTEXT.md` at root: domain glossary + the layering rule.
- `docs/adr/0001-core-shell-split.md`: record the architecture decision and
  the engine choices (IronCalc, rdocx, pulldown-cmark, typst crate, zspell).
- CI: `cargo clippy -- -D warnings` on the core crates (app crates once
  their warning debt is paid); coverage reporting on `*-core` crates.

---

## Sequencing

Phase 2 first (small, proves the pattern), then 1 (large), 3, 4 in parallel
where convenient, 5 and 6 alongside. Phase M starts with phase 2 (fixture
corpus + scorecard harness land with tables-core) and every later phase adds
its corpus before its implementation — the conformance tests are the TDD
red state for each phase. Each phase lands as its own PR with
tests in the same commits as the code they test.

## Engine decisions (settled — do not churn)

IronCalc, rdocx, pulldown-cmark, zip+quick-xml, zspell stay. Typst moves
from CLI to library (phase 4). The document engine gap in Letters is filled
by our own `letters-core` model — no third-party rich-text document model
crate exists at the needed maturity.
