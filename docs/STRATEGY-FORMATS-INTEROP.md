# Format Interop & Structured Document Filter Architecture Strategy (v1.1)

## Executive Summary

As **gtk-office-suite** advances toward its Q4 2026 production release targets and daily-driver status, establishing robust document compatibility across legacy and open document formats (ODT, DOCX, ODP, PPTX, ODS, XLSX) is paramount for end-user adoption. This strategy document specifies the architecture, loss budgets, and execution roadmap for format interop and structured filter pipelines in v1.1.

---

## 1. Architectural Strategy

To prevent GTK UI code (`window.rs`) from coupling to document conversion logic, document import/export filter pipelines must follow the core suite isolation rule:

1. **Pure Rust Engines**: All parsing, AST transformations, and format conversion logic reside strictly in GTK-free crates (`suite-common-core`, `letters-core`, `tables-core`, `decks-core`).
2. **Streaming & Asynchronous Filters**: Conversion between native AST and foreign document encodings must operate via streaming pipelines with asynchronous worker thread dispatch, preventing UI main thread freezes during heavy document conversion.
3. **Loss Inspection & Capability Metadata**: Export and import pipelines expose exact loss annotations (e.g. unsupported macro elements, complex chart types, or proprietary metadata) via standardized structured reporting APIs (`FilterReport`).

---

## 2. Format Parity & Loss Budgets

Format fidelity is tracked and ratcheted through quantitative loss budgets:

| Format Family | Target Compliance | Maximum Allowed Degradation | Verification Suite |
|---------------|-------------------|-----------------------------|--------------------|
| **ODT / OpenDocument Text** | 100% Core Structure | 0% Text/Style Loss | `letters lo_parity` (109/109 baseline) |
| **DOCX / Office Open XML** | 95% Layout Parity | < 2% Visual Shift | `letters docx_parity` |
| **ODP / OpenDocument Presentation** | 100% Core Elements | 0% Slide Hierarchy Loss | `decks lo_parity` (9/9 baseline) |
| **OpenFormula / ODS** | 100% Expression Engine | 0% Math Operator Mismatch | `tables formula_parity` (107/107 baseline) |

---

## 3. Implementation & Testing Roadmap

### Phase 1: Loss Budget Inspector & Telemetry (Q4 2026)
- Implement `FilterReport` metadata collection in `suite-common-core`.
- Wire GTK dialog toasts for loss warnings without blocking document open/save paths.

### Phase 2: Versioned Fixture Corpus Expansion (Q1 2027)
- Expand test fixture corpora in `conformance/fixtures/` covering edge-case ODF/OOXML structures.
- Integrate headless background comparison scripts into CI nightly jobs (`REQUIRE_SOFFICE=1`).

### Phase 3: Universal Filter API (v1.1 Target)
- Unify CLI document conversion (`suite-convert`) and GUI export actions under shared `suite-common-core::filter` traits.

---

*Maintained by the strategist agent (tuna-os hive ACMM L6)*
