# Enterprise Fleet Deployment & dconf Policy Strategy

This document establishes the deployment patterns, dconf keyfile policies, and centralized configuration management strategy for enterprise fleet administration of GTK Office Suite (Letters, Tables, Decks).

## Overview

GTK Office Suite ships primarily via Flatpak (`org.gnome.Letters`, `org.gnome.Tables`, `org.gnome.Decks`). Enterprise sysadmins managing fleet deployments via Ansible, Puppet, Salt, or systemd/dconf keyfiles require standardized policy overrides to enforce corporate compliance, lockdown settings, and uniform default configurations across managed workstations.

---

## 1. GSettings Schemas & dconf Keyfile Profiles

GTK Office Suite applications store configuration using GSettings schemas defined in `flatpak/*.gschema.xml`. In managed GNOME desktop environments, system administrators can enforce system-wide defaults and mandatory lockouts using dconf profile paths (`/etc/dconf/db/local.d/`).

### Recommended Enterprise Keyfile: `/etc/dconf/db/local.d/00-gtk-office-enterprise`

```ini
# System-wide defaults for GTK Office Suite enterprise deployments

[org/gnome/letters]
autosave-interval=300
default-format='odt'
telemetry-enabled=false

[org/gnome/tables]
autosave-interval=300
default-format='ods'
recalculate-on-open=true

[org/gnome/decks]
autosave-interval=300
default-format='odp'
```

### System Lockdown Locks: `/etc/dconf/db/local.d/locks/gtk-office`

To prevent end-users from overriding security and autosave compliance policies:

```ini
/org/gnome/letters/telemetry-enabled
/org/gnome/letters/autosave-interval
/org/gnome/tables/autosave-interval
/org/gnome/decks/autosave-interval
```

After placing keyfiles, compile dconf databases:
```bash
dconf update
```

---

## 2. Flatpak Enterprise Fleet Deployment

### System-Wide Installation
Enterprise deployments should install Flatpak bundles system-wide rather than per-user:

```bash
flatpak install --system --noninteractive flathub org.gnome.Letters
flatpak install --system --noninteractive flathub org.gnome.Tables
flatpak install --system --noninteractive flathub org.gnome.Decks
```

### Environment Overrides & Portal Policies
For hardened enterprise deployments, permission overrides can be applied globally via flatpak override:

```bash
# Example: Restrict filesystem access to standard document directories
flatpak override --system --filesystem=xdg-documents org.gnome.Letters
```

---

## 3. Roadmap for Enterprise Management (Q4 2026 / 2027)

1. **Schema Validation Gating**: Ensure automated release gates verify GSettings schema backward compatibility and non-breaking key migrations (#578).
2. **Policy Template Distribution**: Ship tested `.gschema.override` and dconf policy keyfile templates alongside Flatpak releases.
3. **Headless Administrative Utilities**: Integrate dconf verification checks into headless CLI document tooling (`suite-convert`).

---

*Maintained by the GTK Office Suite Architecture & Operations Group.*
