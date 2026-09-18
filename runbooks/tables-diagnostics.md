# Runbook: Tables Diagnostic & Triage Procedures

## Overview

This runbook details operational troubleshooting and diagnostic triage procedures for `tables` and its headless spreadsheet engine `tables-core` (IronCalc-backed model) within `gtk-office-suite`.

## Common Incident Scenarios & Diagnostics

### 1. Calculation Engine & Formula Evaluation Errors

**Symptoms:**
- Formula cells displaying unexpected `#ERROR!`, `#REF!`, or cyclic dependency errors.
- UI freeze during recalculation on sheets with high formula density.

**Triage & Remediation:**
1. Check terminal output with Rust backtrace enabled:
   ```bash
   RUST_BACKTRACE=1 flatpak run org.tunaos.tables /path/to/workbook.xlsx
   ```
2. Verify OpenFormula test suite coverage and parser behavior against LibreOffice Calc parity test suite:
   ```bash
   cargo test -p tables-core --test lo_parity
   ```
3. Inspect calculation dependency tree in debug mode for recursive reference cycles.

### 2. XLSX / ODS / CSV Import & Export Failures

**Symptoms:**
- Failed workbook import, missing cell styling, or corrupt workbook warning on save.
- Error dialog: "Could not open document" or "Failed to export spreadsheet".

**Triage & Remediation:**
1. Run file validation via standard CLI fuzz corpus seed tests:
   ```bash
   cargo test -p tables-core --test test_openformula_compliance
   ```
2. Test calamine / rust_xlsxwriter parsing pipelines:
   - Check if ZIP package structure is well-formed using `unzip -t /path/to/workbook.xlsx`.
   - Inspect XML structure in `xl/worksheets/sheet1.xml` for malformed namespaces.

### 3. Undo/Redo State Inconsistency

**Symptoms:**
- Discrepancy between visual grid state and underlying IronCalc model after undoing batch cell edits.

**Triage & Remediation:**
1. Check `suite-common-core` undo stack command serialization.
2. Confirm that transactional undo commands accurately capture cell ranges and style state before mutative operations.

## Escalation Protocol

If spreadsheet model corruption or calculation panics occur in production builds:
1. File an incident report with steps to reproduce and anonymized sample workbook.
2. Record systemd journal entries:
   ```bash
   journalctl --user -f -u flatpak-org.tunaos.tables
   ```
