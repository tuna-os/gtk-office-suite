# Client-Side Privacy-Preserving Telemetry & Crash Reporting Strategy

**Author**: Strategist Agent  
**Date**: September 2026  
**Status**: Proposal / Planning Artifact  
**Related Issue**: [#553](https://github.com/tuna-os/gtk-office-suite/issues/553)

---

## Executive Summary

As GTK Office Suite approaches production readiness across enterprise deployments and Flatpak distribution channels, maintaining application stability and understanding usage patterns without sacrificing user privacy is critical.

This document outlines the strategic architecture for **strictly opt-in, anonymized client-side telemetry and crash reporting**.

---

## Core Guiding Principles

1. **Privacy-First & Zero PII**: No personal identifiable information (PII), document contents, file paths, or network addresses are ever collected or transmitted.
2. **Explicit Opt-In Consent**: Telemetry is disabled by default. Users must explicitly opt-in via a transparent, first-run dialog built according to Libadwaita HIG.
3. **Enterprise Fleet Control**: Fleet administrators can disable telemetry or redirect endpoint targets suite-wide via `dconf` policy override keys.
4. **Local Auditability**: All telemetry events and crash minidumps logged locally before sending are human-readable JSON files in `$XDG_DATA_HOME/gtk-office-suite/telemetry/`.

---

## Architectural Breakdown

### 1. Consent & Governance (`suite-common`)

- **First-Run Dialog**: Interactive Libadwaita banner on initial launch asking for anonymous stability data collection.
- **Settings Toggle**: Accessible anytime under `Preferences -> Privacy`.
- **GSettings Schema**:
  - `telemetry-enabled` (boolean, default: `false`)
  - `crash-reports-enabled` (boolean, default: `false`)
  - `custom-endpoint-url` (string, default: `""`)

### 2. Telemetry Collector Engine (`suite-common-core`)

- **Event Schema**:
  - App ID (`letters`, `tables`, `decks`)
  - App Version & Flatpak Commit SHA
  - OS / Desktop Environment (`GNOME`, `Wayland`, `X11`)
  - Anonymized Event Name (e.g., `document_opened`, `export_pdf`, `app_launched`)
  - *No file names, paths, or text content.*

### 3. Crash Capture & Sanitization

- **Panic Hooks**: Custom Rust `std::panic::set_hook` capturing panic payload, location (`file:line`), and sanitized backtrace.
- **Minidump Sanitization**: Filters environment variables, memory strings, and user paths from stack traces before local storage or transmission.

---

## Delivery Roadmap

- **Phase 1**: Define `suite-common-core` telemetry types and local logging framework.
- **Phase 2**: Add Libadwaita privacy preference page and first-run consent dialog.
- **Phase 3**: Wire panic hooks and crash report queue processor.
- **Phase 4**: Enterprise `dconf` locking verification and automated test coverage.
