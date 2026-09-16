# Enterprise Fleet Deployment & Administrative Policy Framework

This document outlines guidelines and administrative policy management for enterprise IT deployment of **GTK Office Suite** (Letters, Tables, Decks) across managed Linux workstation fleets.

---

## 1. Overview

Enterprise Linux deployments require centralized configuration, policy enforcement, and compliance management. GTK Office Suite integrates natively with GNOME desktop infrastructure, relying on **`dconf`** key-value storage and GSettings schemas for application configuration and lockdown.

---

## 2. Centralized Configuration via `dconf` Profiles

Administrators can enforce system-wide default settings and read-only administrative locks using standard `/etc/dconf/db/local.d/` policy files.

### Key Configuration Directives

- **Auto-Save Frequency**: Enforce auto-save intervals (in seconds) across all applications.
- **Default Document Formats**: Mandate strict ODF (ISO/IEC 26300) defaults for interop compliance.
- **Telemetry & Diagnostics**: Globally opt-in or opt-out of anonymous crash reporting and telemetry collection.
- **External Network Access**: Restrict automatic online update checks or external web service integrations for air-gapped environments.

### Example Policy File (`/etc/dconf/db/local.d/00-gtk-office-enterprise`)

```ini
[org/gnome/GtkOfficeSuite/settings]
auto-save-interval=300
default-export-format='odf'
telemetry-enabled=false
check-updates-online=false
```

### Locking Policy Settings (`/etc/dconf/db/local.d/locks/gtk-office`)

To prevent non-administrative users from overriding security and compliance settings:

```text
/org/gnome/GtkOfficeSuite/settings/telemetry-enabled
/org/gnome/GtkOfficeSuite/settings/default-export-format
```

After updating configuration files, run `dconf update` to recompile the system binary database.

---

## 3. Flatpak Fleet Distribution

For Flatpak-based enterprise image deployments:

1. **Custom Enterprise Remote**: Host enterprise-approved Flatpak builds on internal OCI registry mirrors.
2. **Override Permissions**: Enforce file-system access boundaries using Flatpak overrides:
   ```bash
   flatpak override --system --filesystem=host-os org.gnome.Letters
   ```
3. **Pre-compiled GSettings Schemas**: Ensure custom enterprise schema extensions are compiled into `/usr/share/glib-2.0/schemas/` or the Flatpak bundle.

---

## 4. Compliance & Audit Verification

Enterprise deployments should validate policy enforcement during post-provisioning audits:

```bash
dconf read /org/gnome/GtkOfficeSuite/settings/telemetry-enabled
```

For questions or enterprise integration support, open a planning issue on the project roadmap repository.
