# Format Interoperability & Loss Budget Specification

This document establishes the strategic roadmap, loss budget guidelines, and automated validation gates for document format interoperability (ODF, OOXML, PDF/A) across GTK Office Suite (Letters, Tables, Decks).

## Strategic Objectives

1. **Deterministic Round-Trip Fidelity**: Ensure document conversion between native formats (ODT/ODS/ODP) and Microsoft Office formats (DOCX/XLSX/PPTX) preserves core structure, text formatting, formulas, and visual layout.
2. **Quantified Loss Budget Enforcement**: Standardize explicit loss budget budgets (zero data loss for text/formulas; controlled visual degradation for unmapped features with explicit user warnings).
3. **Automated LibreOffice Oracle Gating**: Expand `REQUIRE_SOFFICE=1` CI nightly parity suites to cover all core conversion pathways and prevent regression.
4. **PDF/A Archival Export Conformance**: Provide verified ISO 19005-compliant PDF/A-2b archival export across all applications.

## Loss Budget Matrix

| Document Element | Native ODF | Import OOXML | Export OOXML | Loss Budget Policy |
|------------------|------------|--------------|--------------|---------------------|
| Text & Headings  | 100%       | 100%         | 100%         | Zero loss           |
| Table Structure  | 100%       | 100%         | 100%         | Zero loss           |
| Formulas & Math  | 100%       | 100%         | 100%         | Zero loss           |
| Embedded Media   | 100%       | 95%          | 95%          | Lossless compression|
| Custom Macros    | 0% (strip) | 0% (strip)   | 0% (strip)   | Strip with warning  |

## CI & Automated Parity Validation

- CI nightly workflow executes `REQUIRE_SOFFICE=1 cargo test -p decks-core -p letters-core -p tables-core -- --test-threads=1`.
- Capability evidence ledger (`conformance/capabilities.json`) tracks verified ODF/OOXML round-trip revisions.

---
*GTK Office Suite Strategic Architecture Specification*
