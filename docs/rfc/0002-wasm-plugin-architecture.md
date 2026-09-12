# RFC 0002: GTK Office Suite Extension API & WebAssembly Runtime Plugin Architecture

- **Status**: Draft / Proposed
- **Author**: Strategist
- **Created**: 2026-09-12
- **Target Release**: Q4 2026

---

## 1. Context & Motivation

GTK Office Suite (Letters, Tables, Decks) provides GNOME-native productivity tools built in Rust and GTK4/libadwaita. As adoption grows, enterprise and power users require custom extensions—such as Zotero/Mendeley reference integration in Letters, domain-specific formula functions in Tables, custom theme and template generators in Decks, and automated PDF/ODF export filters.

Currently, adding such capabilities requires modifying core Rust binaries, which increases binary complexity and presents security risks when loading unverified native code inside Flatpak containers. 

This RFC proposes a standard **Extension API and WebAssembly (WASM) / IPC Runtime Architecture** for GTK Office Suite.

---

## 2. Strategic Objectives

1. **Memory & Process Isolation**: Execute third-party plugins in a sandboxed WASM runtime engine (via `wasmtime` or `wasmer`) or isolated D-Bus IPC sidecars, preventing host crashes or unauthorized filesystem/network access.
2. **Deterministic UI Threading**: Guarantee that plugin execution occurs off the GTK main loop, streaming asynchronous updates back via core event channels.
3. **Cross-App Host API Parity**: Expose unified host bindings (`suite-plugin-api`) across Letters (document model), Tables (spreadsheet grid), and Decks (slide canvas).
4. **Flatpak Security Compliance**: Ensure extension capabilities conform to Flatpak portal security boundaries without requesting host permissions (`--filesystem=host` or `--socket=session-bus`).

---

## 3. Architecture & Plugin Lifecycle

### 3.1 Plugin Manifest (`plugin.json`)

Every extension is packaged as a standard archive containing a `plugin.json` manifest:

```json
{
  "id": "org.gtkoffice.zotero-bridge",
  "name": "Zotero Reference Bridge",
  "version": "1.0.0",
  "target_apps": ["letters"],
  "entrypoint": "plugin.wasm",
  "capabilities": [
    "document:read",
    "document:insert-citation",
    "network:http-localhost"
  ],
  "author": "Community Maintainers"
}
```

### 3.2 Host API & Sandbox Interfaces

The host environment (`suite-common-core::plugin`) provides standard WASM imports:

```rust
pub trait ExtensionHostApi {
    fn get_active_document_metadata(&self) -> Result<DocumentMetadata, PluginError>;
    fn insert_formatted_text(&mut self, text: &str, style: StyleOptions) -> Result<(), PluginError>;
    fn register_custom_function(&mut self, name: &str, func_ptr: u32) -> Result<(), PluginError>;
    fn notify_user(&self, toast_message: &str) -> Result<(), PluginError>;
}
```

### 3.3 Application Extension Points

- **Letters**: Document text selection, citation insertions, custom spellcheck/grammar filters, Pandoc export pipeline hooks.
- **Tables**: Custom OpenFormula function evaluation (WASM-accelerated), cell data transformers, external SQL/CSV data connectors.
- **Decks**: Slide layout generators, custom chart components, export filters (SVG/PDF/HTML presentation suites).

---

## 4. Implementation Phasing (Q4 2026)

| Phase | Milestone | Deliverables |
|-------|-----------|--------------|
| **Phase 1** | Host API Specification & Manifest Schema | Define `suite-common-core::plugin` traits and JSON manifest validator. |
| **Phase 2** | Sandboxed WASM Engine Integration | Integrate WASM runtime host with strict fuel limits and memory bounds. |
| **Phase 3** | UI Integration & Preference Surface | Add "Extensions" settings page in `suite-common` for plugin enabling/disabling. |
| **Phase 4** | Reference Plugin Implementation | Deliver Zotero citation plugin for Letters & custom math functions for Tables. |

---

## 5. Security & Governance

- Plugins must explicitly declare requested capabilities in `plugin.json`.
- Users must approve plugin capability grants upon installation.
- Malformed or non-responding plugins are terminated cleanly by the WASM host watcher after a 5-second timeout without impacting the GTK host window.
