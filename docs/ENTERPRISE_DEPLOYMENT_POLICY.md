# GTK Office Suite Enterprise Fleet Deployment & Administrative Policy Specification

**Status**: Draft / Strategic Planning  
**Target Milestone**: Q4 2026 / Q1 2027  
**Tracking Issue**: #678  
**Maintainer**: Strategist Agent (ACMM L6)

---

## 1. Overview & Objectives

As GTK Office Suite expands across enterprise, educational, and public sector organizations, Linux system administrators require centralized tools for managing application settings, security policies, and feature permissions across fleet deployments.

This specification outlines the **dconf / GSettings Policy Architecture** and mandatory administrative keylock mechanism for Letters, Tables, and Decks.

---

## 2. dconf Key Hierarchy & Schema Definitions

GTK Office Suite applications store configuration under the `/org/gtkoffice/` GSettings hierarchy. Enterprise policies utilize dconf profiles (`/etc/dconf/profile/user` and `/etc/dconf/db/site.d/`).

### 2.1 Schema Breakdown (`org.gtkoffice.suite.gschema.xml`)

| GSettings Path | Key | Type | Default | Description |
|---|---|---|---|---|
| `/org/gtkoffice/suite/` | `auto-save-interval` | `i` | `300` | Document auto-save interval in seconds |
| `/org/gtkoffice/suite/` | `default-document-format` | `s` | `"odf"` | Mandatory default save format (`"odf"` or `"ooxml"`) |
| `/org/gtkoffice/suite/` | `telemetry-enabled` | `b` | `false` | Client-side privacy-preserving diagnostic metrics |
| `/org/gtkoffice/suite/plugins/` | `enabled` | `b` | `true` | Allow sandboxed WASM plugin loading |
| `/org/gtkoffice/suite/plugins/` | `require-signature` | `b` | `true` | Require signed WASM binaries for execution |

---

## 3. Administrative Locks (`/etc/dconf/db/site.d/locks/`)

System administrators can lock settings so end-users cannot override them via the GTK settings dialog.

### Example Keyfile: `/etc/dconf/db/site.d/00-enterprise-policy`
```ini
[org/gtkoffice/suite]
auto-save-interval=120
default-document-format='odf'
telemetry-enabled=false

[org/gtkoffice/suite/plugins]
enabled=false
```

### Example Lockfile: `/etc/dconf/db/site.d/locks/00-enterprise-policy`
```
/org/gtkoffice/suite/auto-save-interval
/org/gtkoffice/suite/default-document-format
/org/gtkoffice/suite/telemetry-enabled
/org/gtkoffice/suite/plugins/enabled
```

---

## 4. Mass Deployment & Ansible Tooling

Flatpak installations packaged for enterprise deployment can embed dconf keyfiles directly within system Flatpak overrides or deployment scripts:

```bash
# Apply enterprise overrides across flatpak deployments
flatpak override --system --env=GSETTINGS_BACKEND=dconf org.gtkoffice.Letters
dconf update
```

---

## 5. Roadmap & Implementation Phases

- **Phase 1 (Q4 2026)**: Finalize enterprise GSettings schema manifests and dconf lock parser in `suite-common`.
- **Phase 2 (Q1 2027)**: Ansible role & Flatpak administrative override collection in public repository.
- **Phase 3 (Q1 2027)**: Verification suite for dconf key lock enforcement in CI.
