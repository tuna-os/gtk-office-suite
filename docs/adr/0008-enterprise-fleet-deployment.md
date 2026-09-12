# ADR 0008: Enterprise Fleet Deployment, DConf Policy Enforcement, and Managed Configuration

## Status

Proposed.

## Context

As GTK Office Suite (Letters, Tables, Decks) transitions from a standalone user desktop application toward enterprise Linux fleet deployment, institutional IT administrators require centralized administration tools, locked policy enforcement, and managed environment defaults. Standard Linux enterprise deployment relies on GNOME GSettings / dconf profiles, keyfile backend policy overrides (`/etc/dconf/db/local.d/`), and centralized template distribution.

Currently:
1. Application settings (autosave intervals, default document format, hardware acceleration, recovery behavior) lack explicit lockable schema annotations for enterprise keyfiles.
2. Privacy & telemetry preferences lack mandatory system-wide override locks for high-security environments.
3. Centralized corporate template distribution and custom enterprise defaults are not standardized across `suite-common`.

## Decision

We establish an enterprise fleet deployment and managed policy architecture across `suite-common` and application binaries:

1. **System-wide DConf Keyfile & Mandatory Policy Locks**:
   - Annotate all application GSettings schemas with enterprise policy paths under `org.gnome.GtkOffice`.
   - Support `/etc/dconf/db/local.d/locks/` enforcement so system administrators can lock sensitive keys (e.g., external URL opening, telemetry opt-out, network synchronization, default template URLs).
   - `suite-common` settings helpers must gracefully handle read-only / locked GSettings keys without throwing runtime panics or breaking UI interactions.

2. **Enterprise Template & Preset Provisioning**:
   - Standardize system-wide template paths (`/usr/share/gtk-office/templates/` and `/etc/gtk-office/templates/`) alongside user templates.
   - Applications must fall back cleanly to system templates when user template directories are missing or read-only.

3. **Silent Automated Update & Fleet Management Guidelines**:
   - Provide Flatpak enterprise deployment guides and systemd unit guidelines for unattended background updates in managed fleet environments.

## Consequences

- **Pros**:
  - Unblocks enterprise Linux adoption across corporate, government, and institutional workstation fleets.
  - Ensures compliance with organizational security and privacy requirements through dconf policy locking.
  - Simplifies IT administration with standardized system-wide templates and configuration overrides.
- **Cons**:
  - Requires writing and maintaining dconf lock test suites in `suite-common` to ensure locked keys do not trigger UI panics.
