# ODF Macro & Embedded Dynamic Content Sandboxing Policy

## 1. Overview & Objectives

As GTK Office Suite (Letters, Tables, Decks) expands OpenDocument Format (ODT, ODS, ODP) import/export and compatibility features, document files sourced from external environments or legacy suites may contain embedded StarBasic macros, dynamic script bindings, or active external URL links.

This document establishes the strategic security architecture and default policy governing embedded dynamic content to ensure user safety and preserve Flatpak host isolation.

## 2. Core Security Posture

1. **Disabled-by-Default Execution**: Embedded scripts, StarBasic macros, and dynamic event bindings are disabled by default. No script execution occurs automatically upon opening a document.
2. **Flatpak Sandbox Isolation**: The office suite runtime must never request or utilize subshell execution privileges (`host-spawn`, un-sandboxed shell invocation) to run document-embedded scripts.
3. **Explicit User Consent & Trust Banners**: When a document containing embedded dynamic scripts is opened, an infobar/toast alert notifies the user that scripts have been disabled. Enablement requires explicit per-document interactive confirmation.
4. **External Resource Inspection**: External link dereferencing (e.g. dynamic images, linked OLE objects, remote dataset references) must adhere to explicit URI scheme whitelist policies (`https://`, `http://`), blocking arbitrary local path traversal or shell execution URIs (`file://`, `ghelp://`).

## 3. Implementation Roadmap & Milestones

- **Phase 1 (Q4 2026)**: Parser-level detection and reporting of StarBasic script blocks (`META-INF/manifest.xml` and `Basic/` streams in ODF zip archives) with infobar warnings.
- **Phase 2 (Q1 2027)**: Implementation of isolated WASM / pure Rust sandbox environment for evaluating safe document macro extensions, if user demand warrants.
- **Phase 3 (Q2 2027)**: Security audit and ratcheted test suite verifying zero host leakage on malformed/malicious ODF fixture corpora.

---
*Maintained by GTK Office Suite Strategic Planning Group.*
