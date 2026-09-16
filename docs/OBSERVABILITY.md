# Observability Assessment & Stack Guidelines

## Overview

The `gtk-office-suite` repository contains the GTK 4 / Libadwaita desktop productivity suite for TunaOS, comprising:
- **Letters (`letters`, `letters-core`)**: Document editing engine, Markdown/DOCX I/O, typography, and spell checking.
- **Tables (`tables`, `tables-core`)**: Spreadsheet engine (IronCalc), sheet model, formula evaluation, and XLSX/ODS/CSV I/O.
- **Decks (`decks`, `decks-core`)**: Presentation editor, slide canvas layout, master slide inheritance, and PPTX/ODP I/O.
- **Suite Common (`suite-common`, `suite-common-core`)**: Shared undo/redo command bus, atomic document save, autosave recovery, and modal dialogs.
- **Suite Export (`suite-export`)**: In-process Typst PDF compiler and embedded font manager.

This document defines the observability architecture, client-side diagnostic facilities, triage guidelines, and future metrics boundaries across the suite.

## Observability Posture & Data Flow Safety

Per organization operational policy:
- **No Remote Exporters Configured:** No external telemetry collector, Prometheus remote-write exporter, or cloud monitoring pipeline is configured or permitted without explicit operator approval.
- **Client-Side Diagnostics Only:** Observability is strictly local to the desktop execution environment. No user document content, keystroke logs, or telemetry payloads are transmitted off-box.
- **Strict Cardinality Bounds:** Any future in-process telemetry or metric collection must strictly avoid unbounded dimensions (e.g. document text, file paths, user identifiers, or arbitrary error strings).

## Client-Side Diagnostic Facilities

### 1. GLib Structured Logging & Debug Domains

The applications integrate with GLib's structured logging infrastructure. Debug tracing can be selectively enabled using the `G_MESSAGES_DEBUG` environment variable:

```bash
# Enable verbose logging across all suite components
G_MESSAGES_DEBUG=all flatpak run org.tunaos.letters
G_MESSAGES_DEBUG=all flatpak run org.tunaos.tables
G_MESSAGES_DEBUG=all flatpak run org.tunaos.decks

# Target specific log domains
G_MESSAGES_DEBUG=letters-core flatpak run org.tunaos.letters
G_MESSAGES_DEBUG=tables-core flatpak run org.tunaos.tables
G_MESSAGES_DEBUG=decks-core flatpak run org.tunaos.decks
```

### 2. Runtime Graphics & Toolkit Diagnostics

For rendering anomalies, font shaping issues, or Wayland/X11 compositing failures:
- `GSK_RENDERER=cairo` or `GSK_RENDERER=ngl`: Selects software or GPU renderers.
- `GTK_DEBUG=interactive`: Launches the GTK Interactive Inspector for live widget tree inspection.
- `RUST_BACKTRACE=1`: Produces full panic stack traces on stdout/stderr for triage.

### 3. Systemd User Journal Integration

Under standard TunaOS desktop sessions, standard streams from desktop applications and Flatpak instances are collected by systemd-journald:

```bash
journalctl --user -f -u flatpak-org.tunaos.letters
journalctl --user -f -u flatpak-org.tunaos.tables
journalctl --user -f -u flatpak-org.tunaos.decks
```

## Error Presentation & Diagnostic Triage

1. **User-Facing Notifications:** `suite-common::show_error_dialog` presents actionable modal dialogs with clear error explanations for document parse, save, or export failures.
2. **Contextual Help & Diagnostics:** `suite-common::show_help_dialog` provides users with built-in format capabilities, autosave recovery paths, and shortcut reference guides.
3. **Operational Runbooks:**
   - **Letters Diagnostics:** [`runbooks/letters-maintenance-diagnostics.md`](../runbooks/letters-maintenance-diagnostics.md)
   - **Tables Diagnostics:** [`runbooks/tables-diagnostics.md`](../runbooks/tables-diagnostics.md)
   - **Decks Diagnostics:** [`runbooks/decks-diagnostics.md`](../runbooks/decks-diagnostics.md)
   - **Suite Export Diagnostics:** [`runbooks/suite-export-diagnostics.md`](../runbooks/suite-export-diagnostics.md)

## Future Roadmap (OpenTelemetry & Bounded Metrics)

When an authorized backend is deployed:
1. **Loopback Metric Exporters:** Bounded Prometheus scrape endpoints (`http://127.0.0.1:PORT/metrics`) exposing application performance counters (e.g. `render_duration_seconds`, `save_operations_total`, `autosave_cycles_total`).
2. **OpenTelemetry Request Spans:** In-process spans measuring format import/export latencies without capturing payload contents.
