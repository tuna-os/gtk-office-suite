# Enterprise Fleet Deployment and dconf Policy Governance Matrix

This specification establishes the centralized administrative configuration schema and deployment governance framework for `gtk-office-suite` (Letters, Tables, Decks) in enterprise Linux desktop environments.

## Objective

Provide system administrators and enterprise IT deployment teams with standard `dconf` / GSettings schema paths and policy overrides to lock down settings, enforce security postures, manage default document formats, and control external network access.

## Architecture & Policy Schema

All enterprise policy keys reside under the GSettings path `/org/gnome/gtk-office-suite/policy/`. Keys set in system-level `dconf` database profiles (`/etc/dconf/db/local.d/`) marked with `locks` (`/etc/dconf/db/local.d/locks/`) take precedence over user configuration and UI settings.

### 1. Security & Network Telemetry Locks

| Policy Key | Type | Default | Description & Governance |
|---|---|---|---|
| `disable-telemetry` | `boolean` | `false` | When `true`, forcibly disables all client-side telemetry emission and diagnostic metrics collection across the suite. |
| `disable-external-cloud-connectors` | `boolean` | `false` | Disables external cloud storage providers, remote file open dialogs, and non-local network endpoints. |
| `disable-macro-execution` | `boolean` | `true` | Enforces complete block on script/macro execution within imported sheets or documents. |
| `require-encrypted-saves` | `boolean` | `false` | Mandates client-side encryption for all newly saved ODF and OOXML documents. |

### 2. Document & Interoperability Defaults

| Policy Key | Type | Default | Description & Governance |
|---|---|---|---|
| `default-text-format` | `string` | `"odt"` | Preferred file format for Letters saves (`odt`, `docx`, `md`). |
| `default-spreadsheet-format` | `string` | `"ods"` | Preferred file format for Tables saves (`ods`, `xlsx`, `csv`). |
| `default-presentation-format` | `string` | `"odp"` | Preferred file format for Decks saves (`odp`, `pptx`). |
| `strict-odf-conformance` | `boolean` | `true` | Restricts document generation to strict ODF 1.3 standards without proprietary extensions. |

### 3. Administrative UI & Read-Only Locks

| Policy Key | Type | Default | Description & Governance |
|---|---|---|---|
| `force-read-only` | `boolean` | `false` | Mounts all opened documents in read-only mode, disabling editing and save actions. |
| `lock-preferences-ui` | `boolean` | `false` | Disables access to the application Preferences dialog in the GTK headerbar. |
| `allow-untrusted-plugins` | `boolean` | `false` | Restricts loading of third-party WASM or IPC extensions to signed administrative manifests. |

## Deployment Guidelines

### Keyfile Profile Setup

Deploy system-wide keyfiles to `/etc/dconf/db/local.d/00-gtk-office-policy`:

```ini
[org/gnome/gtk-office-suite/policy]
disable-telemetry=true
disable-external-cloud-connectors=true
default-text-format='odt'
strict-odf-conformance=true
allow-untrusted-plugins=false
```

### Locking Configuration

To prevent user overrides, place corresponding key paths in `/etc/dconf/db/local.d/locks/gtk-office`:

```ini
/org/gnome/gtk-office-suite/policy/disable-telemetry
/org/gnome/gtk-office-suite/policy/disable-external-cloud-connectors
/org/gnome/gtk-office-suite/policy/allow-untrusted-plugins
```

Then execute `dconf update` during fleet provisioning.

## Verification & Compliance Testing

1. **Unit Test Coverage**: Enforce GSettings read fallback and key locking checks in `suite-common-core`.
2. **GUI Journey Integration**: Add dogtail test cases asserting read-only UI states and disabled menu items when dconf locks are active.
3. **Audit Ledger**: Record enterprise policy capability assertions in `conformance/capabilities.json`.
