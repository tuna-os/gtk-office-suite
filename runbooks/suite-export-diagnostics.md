# Runbook: Suite Export (Typst PDF) Diagnostic & Triage Procedures

## Overview

This runbook details operational troubleshooting and diagnostic triage procedures for `suite-export` (in-process Typst PDF rendering and export subsystem) within `gtk-office-suite`.

## Common Incident Scenarios & Diagnostics

### 1. Typst PDF Compilation Failures

**Symptoms:**
- Export to PDF fails or hangs indefinitely.
- Error dialog: "Export PDF failed" or generic compilation error reported in modal dialog.

**Triage & Remediation:**
1. Check standard error for Typst compiler diagnostic messages:
   ```bash
   G_MESSAGES_DEBUG=all flatpak run org.tunaos.letters
   ```
2. Verify Typst source string generation from document model:
   - Ensure special characters and document markup are properly escaped.
   - Verify page dimensions and margins adhere to standard paper sizes (A4, Letter).
3. Run PDF export unit tests:
   ```bash
   cargo test -p suite-export
   ```

### 2. Font Embedding & Glyph Fallback Issues

**Symptoms:**
- Exported PDF documents displaying replacement boxes (`□`) or missing custom fonts.

**Triage & Remediation:**
1. Verify system font availability inside the Flatpak sandbox environment:
   ```bash
   flatpak run --command=fc-list org.tunaos.letters
   ```
2. Check `typst-as-lib` font resolver options (`typst-kit-fonts`, `typst-kit-embed-fonts`).
3. Ensure fallback font family definitions (Cantarell, DejaVu Sans, Liberation Sans) are present in the host/sandbox system font cache.

### 3. Memory Consumption & Temporary File Lifecycles

**Symptoms:**
- High memory usage when compiling multi-page documents containing high-resolution raster images.

**Triage & Remediation:**
1. Validate image embedding scale factors in Typst source generator.
2. Ensure temporary file buffers created during image extraction are cleaned up upon compilation completion.

## Escalation Protocol

If PDF export crashes or regressions occur:
1. File an incident report with sample input document and generated Typst error diagnostics.
2. Verify reproducibility across all three applications (`letters`, `tables`, `decks`).
