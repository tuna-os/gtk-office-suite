# Q4 2026 Enterprise Deployment & WASM/IPC Plugin Extension Architecture

This planning document outlines the roadmap standards and guidelines for Enterprise Fleet Deployment and Third-Party WASM/IPC Plugin Extensibility across GTK Office Suite (Letters, Tables, Decks).

---

## 1. Enterprise Fleet Deployment Guidelines

### 1.1 Policy Management (`dconf` integration)

To support enterprise deployment environments (e.g. system administrators managing GNOME desktop fleets via Ansible, SaltStack, or Fleet Commander), GTK Office Suite applications query system lock-down and default policies stored in GSettings / `dconf`.

Key Policy Keys:
- `/org/gnome/gtk-office-suite/policy/disable-external-services`: Restricts online telemetry, cloud storage connectors, and remote template fetching.
- `/org/gnome/gtk-office-suite/policy/default-export-format`: Enforces ODF standard format default vs Microsoft OOXML.
- `/org/gnome/gtk-office-suite/policy/autosave-interval-seconds`: Mandatory auto-save intervals.

### 1.2 Enterprise OS Overlay (`systemd-sysext`)

Applications and supplementary enterprise plugins can be packaged as immutable system extensions (`systemd-sysext`) or distributed via flatpak system overrides (`/var/lib/flatpak/overrides`).

---

## 2. Third-Party Plugin & Extension Architecture

### 2.1 WASM-Based Plugin Runtime Architecture

GTK Office Suite adopts WebAssembly (WASM) via `wasmtime` or `wasmer` inside GTK-free core crates (`suite-common-core`) to execute third-party document transformations, custom export filters, and formula macros without risking application memory safety or system compromise.

- **Sandbox Boundaries**: No direct host filesystem or network access without explicit user permission grants.
- **Capabilities API**:
  - `document:read` — Read AST / structure of open document.
  - `document:write` — Inject structural elements or execute transformations.
  - `calc:function` — Register custom spreadsheet formula functions in Tables.

### 2.2 IPC Extension Bus

For heavyweight external plugins (e.g., Python scripts, Zotero citation managers, desktop AI assistants), GTK Office Suite exposes a DBus / IPC interface adhering to standard freedesktop portal patterns.

- DBus service name: `org.gnome.GtkOfficeSuite.Plugin1`
- Method invocation protocol: JSON-RPC 2.0 over unix domain sockets / DBus session bus.

---

## 3. Implementation Phases & Readiness Criteria

| Milestone | Target | Focus | Deliverables |
|---|---|---|---|
| Q4 2026 Phase 1 | Oct 2026 | Enterprise Policy Schema | `org.gnome.gtk-office-suite.gschema.xml` policy definitions |
| Q4 2026 Phase 2 | Nov 2026 | WASM Engine & Sandbox | `suite-common-wasm` crate runtime & capability grants |
| Q4 2026 Phase 3 | Dec 2026 | IPC Interface & Reference | DBus extension service & sample plugin implementation |

---
*Maintained by the strategist agent (ACMM L6)*
