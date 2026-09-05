# Observability Assessment & Telemetry Guidelines

This document establishes the observability baseline, diagnostic facilities, and telemetry policy for GTK Office Suite (`letters`, `tables`, `decks`, `suite-common`).

---

## 1. Existing Diagnostic Facilities

### 1.1 Local Console & Stderr Diagnostics
The GTK Office Suite desktop applications currently use local standard error (`stderr`) for operational error reporting and status messages.

Key logged event categories:
* **File & I/O Operations**: Document open, save, and export failures (e.g., PDF export error handling in `letters/src/window.rs` and `decks/src/main.rs`).
* **Preferences & State Persistence**: `GSettings` write failures in window and application preference modules.
* **Component Subsystems**: Spell check dictionary loading and initialization diagnostics in `letters/src/spell.rs`.
* **Test & Harness Execution**: Display availability checks (`SKIP: GTK not initialized`) and test oracle fallbacks.

### 1.2 GLib & GTK Runtime Debugging
Because the suite relies on GTK4 and GLib, standard GTK runtime environment variables control native widget logging and event inspection:
* `G_MESSAGES_DEBUG=all`: Enables debug message output across all GLib/GTK log domains.
* `GTK_DEBUG=interactive`: Enables the GTK Inspector for runtime widget tree inspection.
* `RUST_LOG=debug`: Controls log verbosity for Rust components when logging primitives are attached.

---

## 2. Telemetry & Data Boundary Policy

### 2.1 Backend Status: Unconfigured
No central collector or external telemetry ingestion backend (OpenTelemetry Collector, Prometheus gateway, or GA4 endpoint) is configured for GTK Office Suite.

### 2.2 Strict Data Flow Boundaries
In compliance with project telemetry guidelines:
* **No External Data Exporters**: No code may transmit diagnostic metrics, traces, or user event logs off-box without explicit backend confirmation from operators.
* **Zero Secret Exposure**: Collector endpoints, API keys, credentials, or authentication tokens must never be hardcoded or checked into repository source control.
* **Bounded Attribute Limits**: Any future metric labels or span attributes must use strictly bounded cardinality (enums or low-cardinality error classes). High-cardinality data such as document file paths, arbitrary user content, or full text strings are strictly prohibited in telemetry attributes.

---

## 3. Future Tracing & Metrics Architecture Recommendations

Should external observability be requested in future releases:

1. **Structured Logging Migration**:
   * Transition `eprintln!` calls to standard `tracing` macros (`tracing::error!`, `tracing::warn!`, `tracing::info!`) scoped with module target names.
2. **OpenTelemetry SDK Integration**:
   * Introduce request-path spans for long-running document operations (e.g., document compilation, PDF export, large spreadsheet evaluation).
3. **Metrics Endpoint / Exposition**:
   * For headless background processes or automated test runners, expose bounded metrics using a standard Prometheus pull endpoint (`/metrics`) if operational monitoring is provisioned.
