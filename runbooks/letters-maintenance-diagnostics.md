# Runbook: Letters Maintenance Diagnostic & Triage Procedures

## Overview

This runbook details operational troubleshooting and diagnostic triage procedures for `letters` (both Python maintenance and Rust rewrite `letters-core` components) within `gtk-office-suite`.

## Common Incident Scenarios & Diagnostics

### 1. WebKitGTK Rendering & GPU Process Crashes

**Symptoms:**
- White screen or process crash when loading document editor view.
- Console error: `WebKitWebProcess crashed` or `Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display`.

**Triage & Remediation:**
1. Check journald logs:
   ```bash
   journalctl --user -f -o cat | grep -i webkit
   ```
2. Disable hardware acceleration compositing as a temporary fallback:
   ```bash
   WEBKIT_DISABLE_COMPOSITING_MODE=1 flatpak run org.tunaos.letters
   ```
3. Verify GTK / Wayland renderer status:
   ```bash
   GSK_RENDERER=cairo flatpak run org.tunaos.letters
   ```

### 2. Pandoc / WeasyPrint Export Failures (Legacy Python Maintenance)

**Symptoms:**
- Exporting to DOCX, ODT, or PDF fails with empty file output or traceback.

**Triage & Remediation:**
1. Test external CLI binaries in the host/sandbox environment:
   ```bash
   pandoc --version
   weasyprint --version
   ```
2. Inspect log outputs for stdout/stderr captured during conversion operations.

### 3. File I/O & Dirty Page Loss Safeguards

**Symptoms:**
- Document state not persisting across window closing or tab switching.

**Triage & Remediation:**
1. Check unsaved state dirty flag handling (`AdwTabView` page close signals).
2. Validate crash recovery backup files written under `$XDG_CACHE_HOME/letters/`.

## Escalation Protocol

If rendering or export regressions persist across builds:
1. File an incident report using `.github/ISSUE_TEMPLATE/incident_report.md`.
2. Attach `journalctl --user` output and sandbox environment info (`flatpak info org.tunaos.letters`).
