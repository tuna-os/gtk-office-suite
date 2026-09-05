# Desktop Suite Plugin Extension Architecture Strategy

**Last updated**: September 2026  
**Status**: Proposal / Planning Artifact  

---

## 1. Executive Summary

As **gtk-office-suite** matures across Letters, Tables, and Decks, third-party extensibility becomes essential for enterprise workflows, custom document processing filters, domain-specific spreadsheet functions, and third-party AI/cloud integrations.

This document outlines the strategic roadmap for establishing a safe, memory-secure, sandbox-compliant plugin host platform based on WebAssembly (WASM / WASI) and IPC sidecar protocols.

---

## 2. Core Strategic Objectives

1. **Memory Safety & Sandboxing**: Execute external plugins inside a WebAssembly sandbox or isolated IPC process without compromising GTK host stability or leaking system privileges.
2. **GTK-Free Plugin Trait Core**: Expose core extension traits directly from `suite-common-core` (document transform filters, custom formula functions, export backends) so plugin contracts remain fully unit-testable and decoupled from GTK4 UI code.
3. **Flatpak Portal Compliance**: Integrate permissioned plugin access models with XDG Desktop Portals to ensure plugins strictly respect Flatpak sandbox boundaries.
4. **Ecosystem & Marketplace Readiness**: Establish versioned manifest standards (`plugin.json` / `manifest.toml`) for community contribution and potential Flathub/ecosystem distribution.

---

## 3. Architecture Overview

```
 +-------------------------------------------------------+
 |                 GTK Host Application                  |
 |            (Letters / Tables / Decks)                 |
 +---------------------------+---------------------------+
                             |
                   `suite-common-core`
             (Extension Host Trait Dispatcher)
                             |
             +---------------+---------------+
             |                               |
 +-----------v-----------+       +-----------v-----------+
 | WASM Runtime Host     |       | IPC Sidecar Host      |
 | (Wasmtime / WASI)     |       | (Unix Domain Socket)  |
 +-----------------------+       +-----------------------+
 | - Formula Functions   |       | - External Converters |
 | - Document Filters    |       | - Heavy Processing    |
 | - Custom Importers    |       | - Language Runtimes   |
 +-----------------------+       +-----------------------+
```

---

## 4. Phased Implementation Roadmap

### Phase 1: Core Host Traits & Specification (Q4 2026)
- Define `PluginHost`, `DocumentFilter`, and `CustomFunction` traits in `suite-common-core`.
- Design serialization protocols (CBOR/JSON) for host-plugin message exchange.
- Implement manifest validation parser and plugin lifecycle state machine.

### Phase 2: WASM Sandboxed Execution Engine (Q1 2027)
- Embed `wasmtime` / `wasmer` host runtime within `suite-common-core`.
- Implement WASI sandbox isolation with strict memory and CPU execution quotas.
- Provide Rust / C / AssemblyScript SDKs for external plugin authors.

### Phase 3: IPC Sidecar Host & Portal Integration (Q2 2027)
- Enable out-of-process IPC sidecar plugins over Unix domain sockets for heavy computation or legacy language bindings.
- Wire Flatpak dynamic permission prompts via GNOME XDG desktop portals.

---

## 5. Ecosystem & Governance

- All plugin APIs MUST adhere to semantic versioning (`v1alpha1`, `v1beta1`, `v1`).
- Plugins MUST NOT mutate GTK widget state directly; all UI interactions MUST go through canonical GTK-free command actions.
- Security audits and capability verification MUST gate ecosystem marketplace registration.
