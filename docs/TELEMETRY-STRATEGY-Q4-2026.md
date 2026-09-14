# Strategic Plan: Privacy-Preserving Telemetry & Diagnostic Crash Reporting (Q4 2026)

**Tracking Issue**: [#638](https://github.com/tuna-os/gtk-office-suite/issues/638)  
**Author**: Strategist Agent (ACMM L6 — full mode)  
**Target Horizon**: Q4 2026  

---

## 1. Overview & Strategic Objectives

As `gtk-office-suite` transitions into daily-driver adoption across Linux desktop distributions and enterprise environments, maintaining high reliability requires field feedback on application crashes, startup failures, and key performance metrics.

To honor the project's core privacy principles, telemetry and crash reporting must be strictly **opt-in**, **zero-knowledge**, and fully controllable via `dconf` enterprise policy controls.

---

## 2. Architecture & Design Principles

1. **Strict Opt-In & Transparent Governance**:
   - Telemetry collection is disabled by default.
   - On first launch, a non-intrusive `AdwBanner` or dialog presents clear opt-in options.
   - Enterprise administrators can enforce mandatory opt-out or lock policy via `dconf` keys (`org.gnome.Letters.telemetry-enabled`, `org.gnome.Tables.telemetry-enabled`, `org.gnome.Decks.telemetry-enabled`).

2. **Decoupled Asynchronous Processing**:
   - Zero telemetry or crash serialization logic may run on the main GTK GUI thread.
   - Diagnostic payloads are buffered locally in `$XDG_DATA_HOME/<app>/diagnostics/` and dispatched via background workers or cron tasks.

3. **Privacy & Payload Anonymization**:
   - Payloads contain strictly non-identifying data: OS version, Flatpak commit SHA, app version, panic message string, backtrace (with stripped paths/usernames), and GPU driver string.
   - Payload schemas will be published in `docs/TELEMETRY-SPEC.md`.

---

## 3. Implementation Milestones

- **Phase 1 (Q4 2026 - Early)**: Define `suite-common-core` diagnostic event types and `dconf` policy schema bindings.
- **Phase 2 (Q4 2026 - Mid)**: Implement crash handler hook (`std::panic::set_hook`) to serialize panic backtraces securely to disk.
- **Phase 3 (Q4 2026 - Late)**: Add AT-SPI / GTK consent dialogs and enterprise lock compliance unit tests.

---
