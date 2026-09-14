# Enterprise Fleet Deployment & Policy Specification

## Overview

This specification details the enterprise management, administrative control, and deployment architecture for GTK Office Suite across organizational environments (enterprise fleet, government, educational institutions, and managed desktop infrastructure).

GTK Office Suite applications (**Letters**, **Tables**, and **Decks**) rely on standard GTK4 / libadwaita desktop integration patterns and GSettings schemas. Enterprise administrators can govern desktop behavior via standard `dconf` keyfiles, system profiles, and mandatory locks (`/etc/dconf/db/local.d/locks/`).

---

## Administrative Configuration & dconf Key Hierarchy

All managed application settings map under the GSettings path prefix `/org/gnome/gitlab/tuna-os/gtk-office-suite/`.

### Configuration Domains

1. **Security & Extension Execution (`/org/gnome/gitlab/tuna-os/gtk-office-suite/security/`)**
   - `enable-wasm-plugins` (boolean, default: `true`): Controls whether sandboxed WebAssembly extension plugins may be loaded by users.
   - `plugin-signature-policy` (enum: `enforce-signed`, `allow-unsigned`, default: `enforce-signed`): Enforces mandatory cryptographic signature validation for plugin manifests.
   - `network-clipboard-sharing` (boolean, default: `false`): Enables or disables GDK network-backed clipboard synchronization.

2. **Persistence & Data Loss Prevention (`/org/gnome/gitlab/tuna-os/gtk-office-suite/persistence/`)**
   - `autosave-interval-seconds` (integer, default: `120`): Minimum interval between background atomic saves.
   - `enforce-durable-atomic-saves` (boolean, default: `true`): Forces synchronous directory fsync operations during save transactions.
   - `default-document-format` (string, default: `odt`/`ods`/`odp`): Mandates the target format for initial file save dialogs (e.g., enforcing open standards ODF vs OOXML).

3. **Telemetry & Privacy (`/org/gnome/gitlab/tuna-os/gtk-office-suite/telemetry/`)**
   - `enable-telemetry` (boolean, default: `false`): Controls local diagnostic log aggregation and opt-in crash report metrics.
   - `allow-crash-snapshot-collection` (boolean, default: `true`): Permits local storage of red-first crash stack traces for enterprise diagnostic extraction.

---

## Administrative Locking (`dconf` Locks)

To prevent local user overrides, administrators create keyfile definitions in `/etc/dconf/db/local.d/00-enterprise-policy` and list immutable keys in `/etc/dconf/db/local.d/locks/00-enterprise-policy`:

```ini
# /etc/dconf/db/local.d/00-enterprise-policy
[org/gnome/gitlab/tuna-os/gtk-office-suite/security]
enable-wasm-plugins=false
plugin-signature-policy='enforce-signed'

[org/gnome/gitlab/tuna-os/gtk-office-suite/persistence]
default-document-format='odt'
autosave-interval-seconds=60
```

```text
# /etc/dconf/db/local.d/locks/00-enterprise-policy
/org/gnome/gitlab/tuna-os/gtk-office-suite/security/enable-wasm-plugins
/org/gnome/gitlab/tuna-os/gtk-office-suite/security/plugin-signature-policy
/org/gnome/gitlab/tuna-os/gtk-office-suite/persistence/default-document-format
```

Executing `dconf update` binds these mandatory policies across Flatpak app sandboxes via GSettings portal forwarding.

---

## Fleet Deployment Automation

For automated deployments via Ansible, Puppet, or Flatpak policy overrides:

- **Flatpak Manifest System Controls**: The sandbox manifest preserves host read-only access to `/etc/dconf` for system policy evaluation.
- **MIME & Portal Pre-configuration**: Pre-seeded XDG desktop entry defaults ensure file associations route `.odt`, `.ott`, `.ods`, `.ots`, `.odp`, and `.otp` directly to Letters, Tables, and Decks.

---

## Release Milestone Integration

This specification aligns with the **Q4 2026 Enterprise Release Milestone**, complementing the core save safety infrastructure (#437) and automated release gates (#326).
