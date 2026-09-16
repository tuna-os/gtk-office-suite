# GTK Office Suite WASM & IPC Extension Architecture Specification

## Overview

This specification establishes the architecture for safe, sandboxed third-party plugins and extensions in GTK Office Suite (Letters, Tables, Decks).

## Strategic Motivation

1. **Ecosystem Growth**: Third-party integrations (e.g. Zotero reference management, language translation, template generators, custom document exporters) increase adoption without increasing core codebase bloat.
2. **Security & Isolation**: GTK applications written in Rust must remain protected against plugin crashes, memory corruption, and unauthorized host filesystem/network access.
3. **Main Loop Protection**: Heavy plugin processing or long-running algorithms must never block the GTK main thread or AT-SPI accessibility interfaces.

## Architecture Principles

### 1. WebAssembly (Wasmtime) Sandbox
- Plugins execute within an isolated WebAssembly sandbox managed by a dedicated worker thread via `wasmtime`.
- Host capability security: No direct disk or network access unless explicitly granted by manifest capabilities and user permission dialogs.

### 2. IPC Extension Protocol
- External process plugins communicate over unix domain sockets using structured JSON-RPC / CBOR payloads.
- Enables language-agnostic plugin development (Python, Node.js, C++) for complex desktop tooling.

### 3. Core Event Loop Integration
- `suite-common-core` provides pure Rust trait definitions for plugin hooks:
  - Document lifecycle hooks (`on_document_open`, `on_document_save`, `on_content_change`).
  - Command palette action registrations.
  - Custom UI panel/sidebar render requests.

## Roadmap & Milestones

- **Phase 1 (Q4 2026)**: Pure Rust `suite-common-core` plugin host trait definitions and capability manifest schema.
- **Phase 2 (Q1 2027)**: Wasmtime runtime integration and isolated background worker execution.
- **Phase 3 (Q1 2027)**: GTK UI widget integration for plugin sidebars and action dialogs.
