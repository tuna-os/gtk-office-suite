# ADR-0008: Privacy-Preserving Client Telemetry and Diagnostic Feedback Architecture

- **Status**: Proposed
- **Date**: 2026-09-04
- **Authors**: Strategist Agent
- **Target**: GTK Office Suite (`letters`, `tables`, `decks`) & shared crates

---

## Context and Problem Statement

`gtk-office-suite` delivers modern native Linux desktop document productivity applications built with GTK4 and Rust. As adoption grows, understanding application stability, crash frequency, document format parsing bottlenecks, and UI interaction latency is vital for prioritizing optimization work.

However, desktop productivity applications process sensitive user documents. Traditional telemetry frameworks risk inadvertently recording private content, personal identifiable information (PII), or file paths. A clear privacy architecture and consent model are required before embedding any diagnostic collection mechanism.

---

## Decision Drivers

1. **User Privacy & Consent**: Strict opt-in requirements; default disabled state; complete transparency regarding stored data.
2. **Zero PII & Content Leakage**: Absolute prohibition against recording text, cell contents, slide text, file paths, hostnames, or usernames.
3. **Data Minimization**: Local collection, aggregation, and batching to minimize telemetry payload sizes.
4. **Desktop Native UX**: Seamless integration with GTK4 / Libadwaita preferences and GNOME Privacy Settings.

---

## Proposed Architecture

### 1. Opt-In Consent Flow & Settings Gate

- Telemetry is **disabled by default**.
- Upon first launch or in the Application Preferences (`AdwPreferencesWindow` -> Privacy tab), users receive an explicit opt-in toggle: *"Help improve GTK Office Suite by sending anonymous crash reports and performance statistics"*.
- The setting state is saved in GSettings (`org.gnome.gtk-office-suite.privacy.enable-telemetry`).

### 2. Metric Collection Scope & Schemas

Only non-identifying aggregate metrics are collected when enabled:

- **Stability Events**: Crash signals, panic trace hashes (sanitized of local file paths), abnormal exit codes.
- **Performance Metrics**: Document load latency (ms), render duration (ms), Typst export duration (ms), memory usage watermark (MB).
- **Format Interoperability**: ODF/OOXML feature parse status flags (e.g. `unsupported_element_encountered: "draw:custom-shape"`).
- **Session Aggregates**: App startup time, active window session duration bucket.

### 3. Data Sanitization & Local Pipeline

```
[ Application Event / Panic ]
          │
          ▼
[ Path & PII Stripper ] (replaces /home/user/... with <path>)
          │
          ▼
[ Consent Validator ] (checks GSettings opt-in toggle)
          │
          ▼
[ Local Cache / Ring Buffer ] (stores up to 100 anonymous events locally)
          │
          ▼
[ Async Out-of-Process Uploader ] (batched HTTP POST with HTTPS / rate-limited)
```

### 4. Implementation Boundaries

- Telemetry logic lives in `suite-common-core` behind a `--cfg telemetry` feature flag.
- UI preference toggles are implemented in `suite-common` / app window controllers.
- External telemetry endpoint uses standard HTTPS payload structures defined by organizational observability standards.

---

## Consequences

### Positive
- Actionable insight into crash hotspots and format parser bottlenecks.
- Data-backed prioritization for performance optimization and interop budgets.
- Full user trust maintained through strict opt-in controls and verified data boundaries.

### Negative
- Requires maintaining local aggregation buffers and sanitization filters.
- Diagnostic collection depends on user consent rate.
