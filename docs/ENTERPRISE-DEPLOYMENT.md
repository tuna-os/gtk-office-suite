# Enterprise Fleet Deployment & Managed Policy Architecture

This document defines the deployment architecture, configuration lockdown capabilities, and administrative management frameworks for deploying GTK Office Suite across managed Linux enterprise environments (governments, educational institutions, enterprise fleets, and managed workstations).

---

## 1. Overview & Objectives

GTK Office Suite (Letters, Tables, Decks) uses **dconf / GSettings** for configuration persistence. In an enterprise environment, system administrators require the ability to:
1. Enforce mandatory security and privacy policies (e.g. telemetry controls, auto-save limits, cloud integration boundaries).
2. Provision system-wide default settings across fleet endpoints.
3. Lock specific configuration keys so end users cannot override organizational policy via GUI preferences.
4. Deploy standardized corporate document templates centrally.

---

## 2. Configuration Lockdown via dconf Profiles

dconf supports a layered database architecture where administrative policies override user defaults.

### 2.1 Profile Configuration (`/etc/dconf/profile/user`)

To enforce corporate policies on desktop workstations, create or modify `/etc/dconf/profile/user`:

```text
user-db:user
system-db:local
```

### 2.2 Enterprise Keyfile Provisioning (`/etc/dconf/db/local.d/00-gtk-office-suite`)

System administrators specify default settings for all suite applications in keyfiles placed within `/etc/dconf/db/local.d/`:

```ini
# Enforce security and operational defaults across GTK Office Suite
[org/gnome/desktop/office/letters]
auto-save-interval=300
enable-remote-templates=false
pdf-a-compliance-level='pdf-a-2b'

[org/gnome/desktop/office/tables]
auto-save-interval=300
calculation-engine-threads=4

[org/gnome/desktop/office/decks]
auto-save-interval=300
presenter-mode-dual-screen=true
```

### 2.3 Policy Lockdown Enforcement (`/etc/dconf/db/local.d/locks/00-gtk-office-suite`)

To prevent end-users from changing administrative settings in the application GUI or via user-level `gsettings` commands, create a lockfile under `/etc/dconf/db/local.d/locks/`:

```text
/org/gnome/desktop/office/letters/enable-remote-templates
/org/gnome/desktop/office/letters/pdf-a-compliance-level
/org/gnome/desktop/office/tables/auto-save-interval
/org/gnome/desktop/office/decks/auto-save-interval
```

After modifying system keyfiles or lockfiles, compile the dconf database:

```bash
sudo dconf update
```

---

## 3. Managed Template & Corporate Asset Distribution

Enterprise templates (letterheads, financial reporting sheets, presentation slide decks) are central to organizational document standardization.

### 3.1 System-Wide Template Directory

Deploy organization-approved templates to the system-wide XDG data directories:

- Letters Templates: `/usr/share/gtk-office-suite/templates/letters/`
- Tables Templates: `/usr/share/gtk-office-suite/templates/tables/`
- Decks Templates: `/usr/share/gtk-office-suite/templates/decks/`

Applications automatically index templates from `/usr/share/gtk-office-suite/templates/` alongside user templates in `~/.local/share/gtk-office-suite/templates/`.

---

## 4. Flatpak Fleet Deployment & Maintenance

GTK Office Suite is packaged as Flatpaks (`org.gnome.Letters`, `org.gnome.Tables`, `org.gnome.Decks`).

### 4.1 System-Wide Installation

Deploy Flatpak bundles system-wide via OSTree repo or Flatpak system installation:

```bash
flatpak install --system flathub org.gnome.Letters org.gnome.Tables org.gnome.Decks
```

### 4.2 Sandboxing & System Portals

Enterprise deployments leverage XDG Desktop Portals (`org.freedesktop.portal.FileChooser`, `org.freedesktop.portal.Print`) to maintain sandbox isolation without compromising enterprise file share access (NFS, SMB, Samba).

---

## 5. Summary & Fleet Compliance Checklist

- [ ] Configure `/etc/dconf/profile/user` on gold disk image or Ansible fleet manifest.
- [ ] Deploy `/etc/dconf/db/local.d/00-gtk-office-suite` keyfile with target settings.
- [ ] Add lock rules in `/etc/dconf/db/local.d/locks/00-gtk-office-suite` for restricted keys.
- [ ] Execute `dconf update` post-provisioning.
- [ ] Synchronize central template assets to `/usr/share/gtk-office-suite/templates/`.
