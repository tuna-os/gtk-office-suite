# Document Interoperability Loss-Budget Strategy & Inspection Policy

**Last updated**: 2026-09-02 | **Status**: Draft Roadmap Strategy

---

## Executive Summary

As a GNOME-native office suite, seamless document compatibility across OpenDocument Format (ODF: `.odt`, `.ods`, `.odp`) and Office Open XML (OOXML: `.docx`, `.xlsx`, `.pptx`) is essential for Linux desktop adoption. 

This document defines the organization's **interoperability loss-budget framework**, **unsupported-feature inspection policy**, **non-destructive fallback standards**, and automated **LibreOffice oracle verification protocols**.

---

## 1. Interoperability Loss-Budget Framework

A **loss budget** defines the acceptable threshold of document feature degradation when importing, editing, or exporting third-party document formats.

### Fidelity Tiers

| Loss Tier | Description | Allowed Threshold | Example Document Features |
|---|---|---|---|
| **Tier 0: Zero Loss** | Text content, core formatting, cell values, formula semantics | 0% loss (100% exact round-trip) | Plain text, bold/italic/underline, inline colors, paragraph alignment, basic table cells, arithmetic formulas |
| **Tier 1: Non-Destructive Preservation** | Features visible in external suites but uneditable in GTK Office Suite | Preserved in document DOM / byte stream | Complex macro scripts, custom XML parts, advanced shape groupings, embedded fonts |
| **Tier 2: Visual Degradation with Warning** | Features degraded to simpler static representations upon edit | Permitted with explicit UI notification | SmartArt converted to static image shapes, complex chart animations flattened to static frames |
| **Tier 3: Unsupported Features** | Unhandled attributes safely stripped with audit log entry | Statically bounded log entries | Vendor-specific extension attributes (`v:shapes`, proprietary printer settings) |

---

## 2. Unsupported-Feature Inspector Specification

When opening or saving documents containing unsupported or degraded features, the application must provide transparent user feedback without modal interruption:

1. **Non-Blocking Banner Notification**: Display an info infobar detailing unsupported elements (e.g., *"This document contains VBA macros which are preserved but cannot be executed"*).
2. **Document Health Inspector**: An accessible dialog under `File -> Document Details -> Compatibility` listing all detected unhandled elements and their preservation status.
3. **Loss Telemetry Audit Log**: Append structured entries to debug logs to track feature usage frequency in real-world corpora.

---

## 3. Non-Destructive Round-Trip Rules

To ensure GTK Office Suite can be safely used in heterogeneous environments (e.g., editing a document shared with Microsoft Word or LibreOffice Writer):

- **DOM Preservation Nodes**: Unrecognized XML elements/attributes during ODF (`content.xml`) or OOXML (`document.xml`) parsing must be stored in pass-through extension nodes within internal document models (`letters-core`, `tables-core`, `decks-core`).
- **Export Re-serialization**: Pass-through extension nodes must be re-emitted into exported XML streams at their original schema locations.
- **Round-Trip Ratchet**: Automated tests must assert byte-level structural preservation when loading, round-tripping, and saving complex benchmark files.

---

## 4. Automated Oracle Verification Protocol

Oracle verification uses automated headless LibreOffice instances (`soffice`) to guarantee round-trip integrity:

1. **Bi-directional Oracle Pipeline**:
   - Write document in GTK Office Suite → Load in LibreOffice → Export from LibreOffice → Reload in GTK Office Suite.
   - Load LibreOffice reference file → Export from GTK Office Suite → Verify rendered structural tree via LibreOffice CLI / AT-SPI audit.
2. **Automated Loss Gate**:
   - CI runs nightly `soffice` oracle suites over the ratcheted parity corpora.
   - Any drop in element survival count blocks release qualification.

---

## 5. Implementation Roadmap

- **Phase 1 (Q4 2026)**: Implement DOM pass-through storage for `docx` and `odt` style extensions in `letters-core`.
- **Phase 2 (Q1 2027)**: Implement non-blocking Compatibility Inspector UI across Letters, Tables, and Decks.
- **Phase 3 (Q1 2027)**: Expand `soffice_oracle.rs` integration test suite to cover multi-sheet XLSX features and ODP slide transitions.
