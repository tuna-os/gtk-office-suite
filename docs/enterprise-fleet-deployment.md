# Enterprise Fleet Deployment & Security Policy Architecture

This specification outlines system configuration policy management, locked `dconf` profiles, and unattended deployment standards for `gtk-office-suite` applications (**Letters**, **Tables**, **Decks**) across enterprise Linux workstation fleets.

---

## 1. Overview & Strategic Goals

As desktop Linux adoption expands across enterprise environments (e.g., Ubuntu Pro, Red Hat Enterprise Linux, Fedora Workstation/Silverblue), system administrators require centralized governance over application defaults, telemetry isolation, update channels, and security settings.

`gtk-office-suite` applications store user preferences using `GSettings` backed by `dconf`. By establishing standardized system-wide `dconf` profile definitions, administrators can enforce security baselines, lock administrative keys, and automate fleet-wide application deployments.

---

## 2. dconf Architecture & Profile Hierarchy

In a standard GNOME enterprise workstation environment, `dconf` resolves settings using a keyfile database profile specified in `/etc/dconf/profile/user`.

### 2.1 System Profile Definition (`/etc/dconf/profile/user`)

```ini
user-db:user
system-db:site
system-db:distro
```

### 2.2 Enterprise Keyfile Database (`/etc/dconf/db/site.d/00-gtk-office`)

Enterprise defaults and locked policies are configured in `/etc/dconf/db/site.d/00-gtk-office`:

```ini
# Letters policy settings
[org/gnome/Letters]
auto-save-interval=300
default-export-format='odt'
telemetry-enabled=false
cloud-sync-enabled=false

# Tables policy settings
[org/gnome/Tables]
auto-save-interval=300
calculation-engine='ironcalc-fast'
telemetry-enabled=false

# Decks policy settings
[org/gnome/Decks]
auto-save-interval=300
presenter-dual-screen=true
telemetry-enabled=false
```

### 2.3 System Lock Profile (`/etc/dconf/db/site.d/locks/gtk-office`)

To prevent end users from altering mandatory security and telemetry settings, key locks are placed in `/etc/dconf/db/site.d/locks/gtk-office`:

```ini
/org/gnome/Letters/telemetry-enabled
/org/gnome/Letters/cloud-sync-enabled
/org/gnome/Tables/telemetry-enabled
/org/gnome/Decks/telemetry-enabled
```

After deploying policy keyfiles and locks, administrators update the binary `dconf` database:

```bash
dconf update
```

---

## 3. Flatpak Unattended Fleet Administration

For Flatpak installations managed via Ansible, Puppet, Salt, or OSTree enterprise scripts, `gtk-office-suite` apps consume system `dconf` overrides mounted via Flatpak portal bindings.

### 3.1 Unattended System Installation Command

```bash
flatpak install --system --noninteractive -y flathub org.gnome.Letters org.gnome.Tables org.gnome.Decks
```

### 3.2 Security Portal Permissions Baseline

Enterprise deployment manifests must enforce the following sandbox boundaries:

| Application | Network Egress | File System Access | GPU Acceleration |
|---|---|---|---|
| Letters | None (offline default) | `--filesystem=host-os:ro`, `--filesystem=xdg-documents` | Optional (`--device=dri`) |
| Tables | None (offline default) | `--filesystem=host-os:ro`, `--filesystem=xdg-documents` | Optional (`--device=dri`) |
| Decks | None (offline default) | `--filesystem=host-os:ro`, `--filesystem=xdg-documents` | Required (`--device=dri`) |

---

## 4. Verification & Audit Metrics

Automated compliance checks within the `release_gate.py` harness verify:
1. Valid XML schemas in `flatpak/*.gschema.xml`.
2. Binary schema compilation compatibility via `glib-compile-schemas`.
3. Non-blocking error behavior when mandatory dconf keys are locked by system policy.
