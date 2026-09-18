# Runbook: Decks Diagnostic & Triage Procedures

## Overview

This runbook details operational troubleshooting and diagnostic triage procedures for `decks` and its headless presentation engine `decks-core` within `gtk-office-suite`.

## Common Incident Scenarios & Diagnostics

### 1. Slide Canvas Rendering & Cairo Drawing Pipeline

**Symptoms:**
- Slide shapes, text frames, or vector paths rendering misaligned or clipping incorrectly.
- Canvas artifacts when panning or zooming slide view.

**Triage & Remediation:**
1. Test alternate graphics backends:
   ```bash
   GSK_RENDERER=cairo flatpak run org.tunaos.decks
   ```
2. Enable GTK inspector to verify Cairo drawing area widget allocations and transform matrices:
   ```bash
   GTK_DEBUG=interactive flatpak run org.tunaos.decks
   ```
3. Run slide rendering unit tests:
   ```bash
   cargo test -p decks-core --test lo_parity
   ```

### 2. PPTX / ODP Slide Master & Layout Inheritance

**Symptoms:**
- Missing background images or inconsistent slide fonts after loading PPTX or ODP decks.
- Slide layout failing to inherit master slide geometry or color palettes.

**Triage & Remediation:**
1. Check slide master hierarchy in `decks_core::pptx` and `decks_core::odp` readers.
2. Verify extracted media assets are safely stored in temporary storage using secure temporary directories:
   ```bash
   ls -la /tmp/
   ```
3. Run PPTX/ODP roundtrip test suite:
   ```bash
   cargo test -p decks-core --test soffice_oracle
   ```

### 3. Object Selection & Multi-Shape Transform Failures

**Symptoms:**
- Grouped shapes losing relative scale or rotation during drag transforms on canvas.

**Triage & Remediation:**
1. Verify `SlideObject` transform matrices and bounding box computation in `decks-core`.
2. Inspect undo command payloads for multi-object translation and resizing.

## Escalation Protocol

If presentation rendering or PPTX/ODP corruption is detected:
1. Capture reproduction steps and attached test presentation file.
2. Collect journal logs:
   ```bash
   journalctl --user -f -u flatpak-org.tunaos.decks
   ```
