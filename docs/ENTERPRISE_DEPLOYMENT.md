# Enterprise Fleet Deployment & Administrative Policy Architecture

This document defines the deployment architecture, `dconf` configuration management, and administrative security policy enforcement for `gtk-office-suite` (Letters, Tables, Decks) in managed Linux environments.

---

## 1. Overview & Objectives

Enterprise deployments (e.g. enterprise Linux workstations, government agencies, educational institutions) require centralized IT control over application behavior, security baselines, and default settings. `gtk-office-suite` integrates natively with the GNOME `dconf` key-value system and GSettings API to support mandatory lock-down policies.

### Key Capabilities

1. **Mandatory Configuration Locks**: System administrators can enforce non-overridable default settings via system-level `dconf` databases (`/etc/dconf/db/site.d/`).
2. **Flatpak System Overrides**: Centralized deployment via Flatpak system installations using standardized policy profiles.
3. **Audit Compliance**: Enforceable policies for document encryption, auto-recovery retention, and external web integration.

---

## 2. GSettings & dconf Schema Hierarchy

All applications in `gtk-office-suite` store settings under standard GSettings paths:

- **Suite-wide**: `org.gnome.GtkOfficeSuite`
- **Letters**: `org.gnome.Letters`
- **Tables**: `org.gnome.Tables`
- **Decks**: `org.gnome.Decks`

### Core Policy Keys

| Schema | Key | Type | Default | Description |
|---|---|---|---|---|
| `org.gnome.Letters` | `atomic-save-enabled` | `b` | `true` | Enforces POSIX-atomic document write operations. |
| `org.gnome.Letters` | `auto-save-interval-seconds` | `i` | `300` | Auto-save frequency ceiling for crash recovery. |
| `org.gnome.GtkOfficeSuite` | `telemetry-enabled` | `b` | `false` | Controls privacy-preserving telemetry upload. |
| `org.gnome.GtkOfficeSuite` | `allow-external-plugins` | `b` | `true` | Permits loading untrusted WASM/IPC plugin binaries. |
| `org.gnome.GtkOfficeSuite` | `default-document-format` | `s` | `"odt"` | Default file format for new documents (`odt`, `docx`, `markdown`). |

---

## 3. Administrative Deployment Guide

### Setting up System-Wide dconf Locks

To enforce enterprise locks on all fleet workstations:

1. Create a profile definition file at `/etc/dconf/profile/user`:

```text
user-db:user
system-db:site
```

2. Create the administrative database directory:

```bash
mkdir -p /etc/dconf/db/site.d/
```

3. Define the key file `/etc/dconf/db/site.d/00-gtk-office-locks`:

```ini
[org/gnome/Letters]
atomic-save-enabled=true
auto-save-interval-seconds=180

[org/gnome/GtkOfficeSuite]
telemetry-enabled=false
allow-external-plugins=false
```

4. Create the lock file `/etc/dconf/db/site.d/locks/gtk-office-suite`:

```text
/org/gnome/Letters/atomic-save-enabled
/org/gnome/GtkOfficeSuite/telemetry-enabled
/org/gnome/GtkOfficeSuite/allow-external-plugins
```

5. Update the binary `dconf` databases:

```bash
dconf update
```

---

## 4. Flatpak Enterprise Fleet Packaging

For enterprise Flatpak distribution:

- Deploy the application flatpak bundle via `flatpak install --system`.
- Override network permissions centrally if offline usage is mandated:

```bash
flatpak override --system --nosocket=network org.gnome.Letters
```

---

## 5. Verification & Testing

System lock enforcement must be validated in integration tests:
- Ensure GUI surfaces disable locked settings widgets (marking them insensitive/read-only).
- Verify read paths resolve system `dconf` values prior to user settings overrides.
