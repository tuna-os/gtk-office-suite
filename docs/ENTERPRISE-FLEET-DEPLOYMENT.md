# Enterprise Fleet Deployment & Policy Configuration Strategy

## Overview

As `gtk-office-suite` (Letters, Tables, Decks) transitions into production desktop deployment, enterprise IT deployment requires central policy configuration management, administrative lockouts, and compliance controls.

This document outlines the architecture for system-wide configuration policy profiles, administrative locks via `dconf`, and deployment patterns across managed Flatpak desktop environments.

---

## 1. Key Architecture & Policy Hierarchy

`gtk-office-suite` applications consume settings defined under the `/org/gnome/desktop/office/` and `/org/gtk-office/` GSettings schema hierarchies.

### Policy Scope Matrix

| Domain | Key Hierarchy | Managed Behaviors | Lockable |
|---|---|---|---|
| **Cloud & Sync** | `org.gtk-office.storage` | Allowed WebDAV endpoints, auto-sync intervals, remote save targets | Yes |
| **Telemetry & Diagnostics** | `org.gtk-office.telemetry` | Opt-in metrics, crash reporting, log verbosity | Yes |
| **Document Security** | `org.gtk-office.security` | Enforce client-side encryption, macro sandbox rules | Yes |
| **UI & Extension** | `org.gtk-office.plugins` | Allowed WASM/IPC extension origins, plugin enablement | Yes |

---

## 2. Dconf Profile & Key Locking

Enterprise administrators deploy immutable policy files into `/etc/dconf/db/local.d/`:

### `/etc/dconf/db/local.d/00-enterprise-office`
```ini
[org/gtk-office/security]
require-encryption=true
allow-external-plugins=false

[org/gtk-office/telemetry]
enabled=false
```

### `/etc/dconf/db/local.d/locks/office`
```ini
/org/gtk-office/security/require-encryption
/org/gtk-office/security/allow-external-plugins
/org/gtk-office/telemetry/enabled
```

When locked, application settings controls in Letters, Tables, and Decks UI display read-only administrative lock indicators and ignore local user overrides.

---

## 3. Flatpak Enterprise Distribution Guidelines

1. **System Profile Injection**: Flatpak builds must bind `/etc/dconf` from the host system or mount dconf overrides via standard portal configurations.
2. **Offline Installation**: Deploy offline Flatpak bundles (`.flatpak`) via central management tools (Ansible, Puppet, Fleet Commander).
3. **Reproducible Builds**: All release builds must compile against fixed Rust toolchains and pinned crate versions to satisfy enterprise compliance audits.

---

## 4. Implementation Roadmap

- [ ] **Phase 1**: Add GSettings enterprise security schemas to `flatpak/*.gschema.xml`.
- [ ] **Phase 2**: Wire read-only `GSettings` UI bindings in `suite-common` settings dialogs.
- [ ] **Phase 3**: Validate dconf lock enforcement in AT-SPI integration test suites.
