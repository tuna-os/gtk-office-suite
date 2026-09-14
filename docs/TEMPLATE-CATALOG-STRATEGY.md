# Document Template Catalog & Design Asset Exchange Strategy

**Status**: Draft / Strategic Proposal  
**Author**: Strategist Agent (ACMM L6)  
**Target Release**: Q4 2026 / 2027.1  
**Related Issue**: #513  

---

## Executive Summary

As `gtk-office-suite` (Letters, Tables, Decks) matures beyond post-v1 functionality into a daily-driver GNOME-native office suite, user acquisition and workflow efficiency depend heavily on rich, accessible document templates. 

This document defines the strategic framework for establishing a community template catalog and asset exchange system. By decoupling template manifests, previews, and metadata from application binaries into a versioned JSON schema managed by `suite-common-core`, we enable both curated first-party asset bundles and community-driven template repositories.

---

## Architecture & Data Flow

```
+-------------------------------------------------------------+
|             Community / Organization Repositories           |
|  (HTTPS Endpoints / Flatpak Bundle / Local User Dir)        |
+-------------------------------------------------------------+
                              |
                              v [Index Manifest: templates.json]
+-------------------------------------------------------------+
|                     suite-common-core                       |
|  - Manifest Parser & Validator                              |
|  - Local Cache Manager (~/.cache/gtk-office-suite/templates)|
|  - Category & Tag Indexing Engine                           |
+-------------------------------------------------------------+
                              |
                              v [Template Selection API]
+-------------------------------------------------------------+
|           GTK4 / Libadwaita Application Layer               |
|  - Letters (ODT / OTT / CommonMark)                         |
|  - Tables (ODS / OTS / CSV)                                 |
|  - Decks (ODP / OTP)                                        |
+-------------------------------------------------------------+
```

---

## Strategic Goals

1. **First-Party Core Templates**: Provide modern, HIG-compliant default templates for resumes, formal reports, financial spreadsheets, and presentation pitch decks.
2. **Standardized Manifest Specification**: Establish a standard `templates.json` v1 format for describing document templates, metadata, license details, thumbnails, and variable bindings.
3. **Decoupled Architecture**: Keep core app binaries minimal by lazy-downloading or caching template assets independently.
4. **Community Exchange & Governance**: Create guidelines for third-party template submissions, licensing verification (CC-BY, CC0, MIT), and security sandboxing against malicious embedded document macros/scripts.

---

## Roadmap Integration

- **Phase 1 (Q4 2026)**: Implement template manifest loader and parser in `suite-common-core`. Add initial set of 12 built-in HIG-styled templates.
- **Phase 2 (Q1 2027)**: Integrate custom user template installation (`~/.local/share/gtk-office-suite/templates/`).
- **Phase 3 (Q2 2027)**: Enable remote template catalog indexing and community repository subscription support.
