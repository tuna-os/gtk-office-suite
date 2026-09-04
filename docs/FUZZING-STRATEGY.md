# Fuzzing Expansion, Corpus Minimization, and CI Regression Strategy

**Status**: Proposed | **Horizon**: Mid-term (Q3–Q4 2026) | **Area**: Security / Quality / Robustness

---

## 1. Executive Summary

`gtk-office-suite` provides native GNOME productivity applications (Letters, Tables, Decks) handling complex binary and XML document formats (DOCX, XLSX, PPTX, ODT, ODS, ODP, Markdown, and OpenFormula expressions). Ingesting untrusted third-party documents creates significant exposure to memory corruption, parser panics, infinite recursion, and denial-of-service vulnerabilities.

While baseline `cargo-fuzz` targets exist for `letters_docx`, `tables_xlsx`, and `decks_pptx`, parser coverage remains incomplete across OpenDocument formats and formula calculation engines. Furthermore, automated corpus minimization, seed harvesting from conformance fixtures, and nightly sanitizer runs are required to ensure enterprise-grade resilience for desktop users.

---

## 2. Fuzzing Target Matrix

### 2.1 Parser & Document Targets

| Target Binary | Subsystem | Format Scope | Primary Failure Modes |
|---|---|---|---|
| `letters_docx` | `letters-core::docx` | OOXML `.docx`, `word/document.xml`, styles, relationships | XML bomb, malformed zip, style cycle, unhandled nodes |
| `letters_odt` | `letters-core::odt` | ODF `.odt`, `content.xml`, meta, manifest | XML parsing panic, namespace recursion, table sizing panic |
| `tables_xlsx` | `tables-core::xlsx` | OOXML `.xlsx`, shared strings, sheet XML, workbook parts | Shared string bounds, worksheet dimension overflow |
| `tables_ods` | `tables-core::ods` | ODF `.ods`, table columns/rows, formula strings | Repeated column/row memory amplification, style table lookups |
| `decks_pptx` | `decks-core::pptx` | OOXML `.pptx`, slide XML, presentation layouts, shapes | Shape geometry tree recursion, placeholder resolution panic |
| `decks_odp` | `decks-core::odp` | ODF `.odp`, draw pages, frame containers, animations | Frame nesting exhaustion, master slide reference cycles |

### 2.2 Computation & Engine Targets

| Target Binary | Subsystem | Format Scope | Primary Failure Modes |
|---|---|---|---|
| `tables_formula` | `tables-core::engine` | OpenFormula / Excel syntax AST parsing and evaluation | Tokenizer panics, division-by-zero, cyclic formula evaluation, stack overflow |
| `suite_markdown` | `letters-core::markdown` | CommonMark + GFM extensions | AST deeply nested block/inline list explosions |

---

## 3. Seed Corpus Management and Minimization

1. **Conformance Seed Sourcing**:
   - Ingest existing test fixtures from `tests/fixtures/` and `conformance/` corpora.
   - Extract raw XML payload streams into dedicated `fuzz/corpus/<target>/` seeds.
2. **Corpus Minimization Protocol**:
   - Periodic execution of `cargo fuzz cmin <target>` to eliminate redundant inputs while preserving edge-coverage frontiers.
   - Limit individual corpus entry sizes to under 64 KB to optimize fuzzer throughput (target >1,000 exec/s per core).
3. **Dictionary Provisioning**:
   - Maintain format-specific dictionaries (`fuzz/dict/xml.dict`, `fuzz/dict/formula.dict`, `fuzz/dict/zip.dict`) containing tokens like `w:p`, `w:r`, `table:table-row`, `<office:document>`, and formula functions (`SUM`, `VLOOKUP`, `INDEX`).

---

## 4. Sanitizers and Execution Environments

To identify latent undefined behavior and subtle memory safety risks in unsafe dependencies (e.g., C libraries or raw pointers in rendering bridges):

- **AddressSanitizer (ASan)**: Standard nightly target for out-of-bounds access and use-after-free detection.
- **UndefinedBehaviorSanitizer (UBSan)**: Active during fuzzing runs to catch integer overflow in layout arithmetic and coordinate transforms.
- **MemorySanitizer (MSan)**: Track uninitialized memory reads across FFI boundaries.

---

## 5. CI / Nightly Quality Gate Integration

1. **Pull Request Smoke Verification**:
   - `cargo test --manifest-path fuzz/Cargo.toml` executes regression seed corpora against mock harnesses.
   - Ensures no PR introduces a crash on previously discovered reproduction seeds.
2. **Nightly Continuous Fuzzing Job**:
   - Run LibFuzzer on each target for 15 minutes per nightly cycle in the CI matrix.
   - Automatically artifact and isolate crashes (`fuzz/artifacts/<target>/crash-*`).
3. **Bug Triage & Reproduction Workflow**:
   - Triage crashes with `cargo fuzz run <target> <crash_file>`.
   - Convert verified crashes directly into unit test regressions before applying fixes.

---

## 6. Milestones and Rollout

- **Phase 1 (Q3 2026)**: Add `fuzz/dict/` dictionaries and implement `tables_formula` and `letters_odt` targets.
- **Phase 2 (Q3 2026)**: Integrate conformance suite seed generation and automated corpus minimization.
- **Phase 3 (Q4 2026)**: Complete `tables_ods` and `decks_odp` targets; enforce nightly continuous fuzzing in CI.
