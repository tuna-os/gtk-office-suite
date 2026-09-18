# ADR-0007: Enterprise Fleet Deployment and Centralized dconf Policy Framework

Date: 2026-09-16 · Status: proposed

## Context

As `gtk-office-suite` (Letters, Tables, Decks) approaches production readiness for enterprise desktop environments, IT administrators require centralized policy management and security controls across managed Linux workstation fleets. Currently, runtime parameters—such as document auto-recovery intervals, atomic save thresholds, telemetry/privacy toggles, and default export formats—are configured via standard GSettings (`org.gnome.Letters`, `org.gnome.Tables`, `org.gnome.Decks`).

Without a formal dconf policy schema and lock specification, enterprise deployments cannot enforce administrative guardrails, prevent unmanaged local setting overrides, or standardize compliance parameters across workstation fleets.

## Decision

1. **Standardized dconf Policy Lock Architecture.**
   Enterprise administrators can lock down key operational parameters using standard system-wide dconf keyfiles (e.g. `/etc/dconf/db/local.d/locks/gtk-office-suite`).

2. **Mandatory Administrative Policy Keys.**
   The suite will respect mandatory enterprise keys for:
   - `auto-save-interval-seconds`: Mandatory save frequency enforcing crash recovery policies.
   - `atomic-save-mode`: Enforces atomic transaction semantics (`direct-atomic` vs `tempfile-swap`).
   - `telemetry-opt-out`: System-level mandatory opt-out for diagnostic metrics in corporate environments.
   - `default-export-format`: Enterprise default formats (ODT/ODP/ODS vs CommonMark/PDF/A).
   - `external-plugin-execution`: Strict policy controlling third-party WASM or IPC extension loading.

3. **Flatpak Enterprise Override Compatibility.**
   Provision enterprise dconf profile bindings within Flatpak sandboxes, mapping system-level `/etc/dconf` profiles into application host context via Flatpak portals.

## Consequences

- IT administrators can manage GTK Office Suite settings alongside standard GNOME desktop policies using Ansible, Puppet, or systemd deployment scripts.
- Application code verifies policy lock state before permitting runtime user overrides in GSettings configuration UI.
- Release gating will incorporate dconf lock verification tests to ensure settings immutability under administrative locks.
