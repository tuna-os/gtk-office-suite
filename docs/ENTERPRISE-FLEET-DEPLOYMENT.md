# Enterprise Fleet Deployment & dconf Policy Enforcement

This document defines the deployment specification, administrative policy controls, GSettings lock mechanisms, and Flatpak deployment guidelines for enterprise IT administrators managing GTK Office Suite across workstation fleets.

---

## 1. Overview & Objectives

In enterprise, government, and educational environments, desktop applications must support centralized configuration management, mandatory security policies, and standardized default preferences.

GTK Office Suite (Letters, Tables, Decks) uses **GSettings** backed by **dconf** for application configuration. This architecture natively integrates with Linux enterprise management tools (such as Ansible, Puppet, SaltStack, and Red Hat Satellite / FreeIPA / Active Directory via dconf profiles).

### Primary Objectives
- **Centralized Administration**: Allow administrators to push system-wide or role-based configuration defaults.
- **Mandatory Policy Locking**: Provide key-level lockouts for security-sensitive settings (e.g., telemetry, network connections, macro execution).
- **Flatpak Fleet Compatibility**: Support enterprise Flatpak deployment patterns (system-installed remotes, local enterprise mirrors, and dconf override mounts).

---

## 2. GSettings Schema Hierarchy & Mandatory Keys

Each application ships a compiled GSettings schema installed to `/app/share/glib-2.0/schemas/` within the Flatpak bundle, or `/usr/share/glib-2.0/schemas/` on traditional host installs.

### Key Schema Namespaces

| Application / Crate | Schema ID | Purpose |
|---------------------|-----------|---------|
| Letters | `org.gnome.Letters` | Word processor preferences, default formats, spellcheck |
| Tables | `org.gnome.Tables` | Spreadsheet defaults, calculation engine, auto-recalculate |
| Decks | `org.gnome.Decks` | Presentation defaults, slide templates, presenter mode |
| Shared Suite | `org.gnome.GtkOfficeSuite.Shared` | Fleet telemetry, auto-update checks, enterprise key storage |

---

## 3. Administrative Policy Keys & Key Locking

Enterprise administrators can enforce mandatory settings by defining a dconf profile keyfile and marking specific keys as **locked** in `/etc/dconf/db/local.d/locks/`.

### Mandatory Enterprise Settings

| Key Path | Schema ID | Recommended Value | Security Impact |
|----------|-----------|-------------------|-----------------|
| `/org/gnome/GtkOfficeSuite/Shared/telemetry-enabled` | `org.gnome.GtkOfficeSuite.Shared` | `false` | Prevents outbound diagnostic/usage data transmission |
| `/org/gnome/GtkOfficeSuite/Shared/auto-update-check` | `org.gnome.GtkOfficeSuite.Shared` | `false` | Defers update management to enterprise package infrastructure |
| `/org/gnome/Letters/default-save-format` | `org.gnome.Letters` | `'odt'` | Standardizes ISO/IEC 26300 OpenDocument format fleet-wide |
| `/org/gnome/Tables/auto-recalculate` | `org.gnome.Tables` | `true` | Ensures consistent spreadsheet calculation results |

---

## 4. dconf Profile Configuration Example

### Step 1: Define dconf Profile (`/etc/dconf/profile/user`)

```text
user-db:user
system-db:local
```

### Step 2: System Default Keyfile (`/etc/dconf/db/local.d/00-enterprise-office`)

```ini
[org/gnome/GtkOfficeSuite/Shared]
telemetry-enabled=false
auto-update-check=false

[org/gnome/Letters]
default-save-format='odt'
spellcheck-enabled=true

[org/gnome/Tables]
auto-recalculate=true
```

### Step 3: Administrative Locks (`/etc/dconf/db/local.d/locks/office-policy`)

```text
/org/gnome/GtkOfficeSuite/Shared/telemetry-enabled
/org/gnome/GtkOfficeSuite/Shared/auto-update-check
```

### Step 4: Compile dconf Database

```bash
dconf update
```

---

## 5. Enterprise Flatpak Deployment Patterns

When deploying GTK Office Suite via Flatpak across enterprise fleets:

1. **System-wide Installation**: Install applications system-wide rather than per-user:
   ```bash
   flatpak install --system enterprise-repo org.gnome.Letters org.gnome.Tables org.gnome.Decks
   ```
2. **dconf Sandbox Mounting**: The standard Flatpak sandbox exposes the host dconf database read-only at `/etc/dconf` and the user session dconf socket, ensuring host-defined dconf policy locks apply seamlessly inside the sandbox.
3. **Automated Provisioning**: Fleet management tools should deploy dconf profiles prior to user login to enforce baseline security compliance.

---

## 6. Implementation Roadmap

- **Phase 1 (Q4 2026)**: Finalize `org.gnome.GtkOfficeSuite.Shared` GSettings schema definition across crates.
- **Phase 2 (Q1 2027)**: Implement runtime enforcement check helpers in `suite-common` for locked administrative keys.
- **Phase 3 (Q1 2027)**: Publish Ansible and puppet management roles for automated enterprise fleet configuration.
