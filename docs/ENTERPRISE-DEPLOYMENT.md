# Enterprise Fleet Deployment & dconf Policy Architecture

> Specification and operational guide for enterprise fleet administration, dconf policy lockdowns, system-wide overrides, and managed deployment profiles across Letters, Tables, and Decks.

---

## 1. Executive Summary

As GTK Office Suite reaches production readiness, enterprise and public-sector Linux desktop environments require centralized policy administration capabilities equivalent to Windows Group Policy or macOS Configuration Profiles.

Without standardized dconf schema paths and locking mechanisms, enterprise system administrators cannot enforce security policies (e.g. disabling external macro execution or external image loading), enforce document save locations, or lock down application settings against end-user modification.

This document establishes the architecture for enterprise policy administration across `org.tunaos.letters`, `org.tunaos.tables`, and `org.tunaos.decks`.

---

## 2. Enterprise Policy Schema (`org.gnome.gtk-office-suite.enterprise`)

All enterprise policy keys map to GSettings schema locations under `/org/gnome/gtk-office-suite/enterprise/` or per-app subpaths (`/org/gnome/gtk-office-suite/enterprise/letters/`, etc.).

### 2.1 Core Enterprise Policy Keys

| Key Path | Type | Default | Description & Lock Behavior |
|----------|------|---------|-----------------------------|
| `allow-external-remote-assets` | `b` | `true` | When set to `false` and locked via dconf, external network image fetching in document preview/renderers is disabled. |
| `force-pdfa-compliance-level` | `s` | `"none"` | Enforces mandatory PDF/A export levels (`"pdfa-2b"`, `"pdfa-3b"`, `"none"`). Settings dialog options are disabled when locked. |
| `mandatory-document-templates-url` | `s` | `""` | Enforces a corporate remote template catalog endpoint, overriding default community catalogs. |
| `disallow-unencrypted-exports` | `b` | `false` | When `true`, user cannot export documents without setting an owner/user encryption key or password. |
| `telemetry-opt-out-mandatory` | `b` | `false` | When `true`, client-side diagnostic telemetry is permanently suppressed regardless of user setting. |

### 2.2 dconf Profile Hierarchy and Lockdown Mechanics

Linux desktop environments apply dconf settings via profile layers (`/etc/dconf/profile/user`):

```
user-db:user
system-db:site
system-db:vendor
```

To enforce a policy override and prevent user modification, administrators place key-value pairs in `/etc/dconf/db/site.d/` and lock them in `/etc/dconf/db/site.d/locks/`:

```ini
# /etc/dconf/db/site.d/00-gtk-office-enterprise
[org/gnome/gtk-office-suite/enterprise]
allow-external-remote-assets=false
telemetry-opt-out-mandatory=true
force-pdfa-compliance-level='pdfa-2b'
```

```
# /etc/dconf/db/site.d/locks/00-gtk-office-enterprise
/org/gnome/gtk-office-suite/enterprise/allow-external-remote-assets
/org/gnome/gtk-office-suite/enterprise/telemetry-opt-out-mandatory
/org/gnome/gtk-office-suite/enterprise/force-pdfa-compliance-level
```

When `glib-compile-schemas` and `dconf update` are run, `suite-common` detects GSettings key writability via `g_settings_is_writable()`. Locked keys automatically freeze corresponding UI controls in Letters, Tables, and Decks, rendering them read-only with a tooltipped "Enforced by system policy" lock icon.

---

## 3. Flatpak Distribution & Managed Fleet Deployment

### 3.1 Overrides via Flatpak Systems Integration

When shipping as Flatpaks (`org.tunaos.letters`, `org.tunaos.tables`, `org.tunaos.decks`), enterprise administrators can deploy policy overrides system-wide via Flatpak override directories without modifying container images:

```bash
# System-wide Flatpak dconf override mount
flatpak override --system --filesystem=/etc/dconf org.tunaos.letters
flatpak override --system --env=GSETTINGS_BACKEND=dconf org.tunaos.letters
```

### 3.2 Non-Interactive Administrative Overrides

Enterprise software management tools (Ansible, Puppet, Chef, Salt) can query and verify policy enforcement statelessly using standard GSettings CLI tools:

```bash
gsettings get org.gnome.gtk-office-suite.enterprise allow-external-remote-assets
```

---

## 4. Implementation Roadmap

1. **Phase 1 (Q4 2026)**: Compile and bundle `flatpak/org.gnome.gtk-office-suite.enterprise.gschema.xml` into base Flatpaks and `suite-common`.
2. **Phase 2 (Q4 2026)**: Integrate `g_settings_is_writable()` checks into UI preferences dialogs across Letters, Tables, and Decks.
3. **Phase 3 (Q1 2027)**: Publish Ansible playbook templates and enterprise deployment documentation in `docs/ENTERPRISE-DEPLOYMENT.md`.

---
