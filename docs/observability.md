# Observability Assessment & Stack Guidelines

## Overview

The `gtk-office-suite` monorepo houses the GTK 4 / Libadwaita desktop applications for TunaOS (`letters`, `tables`, `decks`) implemented in Rust (`letters-core`, `tables-core`, `decks-core`, `suite-common-core`) with Flatpak distribution artifacts.

This document outlines the operational observability posture, client diagnostic logging guidelines, and maintenance expectations across desktop components.

## Managed-Project Observability Posture

Per operational policy:
- **No backend exporter configured:** No external metrics or telemetry exporter (e.g. Prometheus exporter, OpenTelemetry collector) is configured or permitted to be added without explicit operator change control.
- **Client-Side Diagnostics:** Observability relies on structured GLib console logging, environment variable diagnostics (`G_MESSAGES_DEBUG`), systemd user journal (`journalctl --user`), and standard library diagnostic outputs.
- **Data Flow Safety:** No telemetry or telemetry data flow is transmitted off-device.

## Operational Guidelines & Logging Standards

1. **Log Domain Structuring:** Desktop crates and legacy maintenance components log under standardized domains (e.g. `letters-core`, `suite-common`).
2. **GLib Logging Flags:** Diagnostic tracing can be enabled at runtime via:
   ```bash
   G_MESSAGES_DEBUG=all flatpak run org.tunaos.letters-rust
   ```
3. **Error Reporting:** Critical errors (file parse failures, IPC bridge errors, export errors) must emit actionable messages via `eprintln!` or `g_warning!`/`g_error!` with context.
4. **Future Telemetry Policy:** If telemetry is approved by operators in the future, it must use local loopback listeners and explicit user opt-in control.
