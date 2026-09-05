# Document Template Engine and Corporate Style Governance Strategy

## Executive Summary

As **gtk-office-suite** (Letters, Tables, Decks) transitions from core format editing capabilities to enterprise adoption, document template management and corporate style governance represent a high-leverage strategic requirement. 

Currently, `suite-common-core/src/templates.rs` defines a static set of built-in templates compiled directly into the binary as slice literals. Enterprise deployments demand extensible template discovery, custom user templates, and central administration of brand typography, color palettes, and document layout standards.

This document details the architectural strategy and release roadmap for expanding the template model into a dual built-in/file-backed discovery system with corporate style enforcement.

---

## Architectural Objectives

1. **Extensible Template Discovery (XDG Standard)**
   - Maintain fast, zero-dependency built-in system templates.
   - Support loading custom user templates from `~/.local/share/gtk-office-suite/templates/` and enterprise-managed templates from `/usr/share/gtk-office-suite/templates/` or `/etc/gtk-office-suite/templates/`.
   - Manifest format: UTF-8 JSON / TOML template descriptor paired with standard content payload files (`.md`, `.csv`, `.pptx`, `.odt`, `.ods`, `.odp`).

2. **Corporate Style Governance Schema**
   - Provide a declarative style profile specification (`style-profile.json`) defining standard brand colors, primary/secondary font families, heading scales, page margin defaults, and mandatory headers/footers.
   - Interface with Letters style hierarchy, Tables style themes, and Decks master slide styling.

3. **Template Registry & Provider Model**
   - Extract template resolution out of pure GTK code into `suite-common-core::templates`.
   - Implement provider abstraction: `BuiltinTemplateProvider`, `XdgTemplateProvider`, and `CorporateStyleProvider`.

---

## Implementation Roadmap

### Phase 1: File-Backed Template Discovery (Q4 2026)
- Expand `DocumentTemplate` in `suite-common-core/src/templates.rs` to support dynamic loading from disk.
- Implement scanner for XDG data directories.
- Unit test template parsing and invalid manifest fallbacks in `suite-common-core`.

### Phase 2: Style Governance & Profile Parsing (Q1 2027)
- Define `StyleProfile` and `BrandPalette` data structures in `suite-common-core`.
- Integrate style profile defaults into Letters document initialization and Decks master slide engine.

### Phase 3: GUI Template Picker & Enterprise Deployment UI (Q1 2027)
- Add "Import Custom Template..." action in standard start pages and File -> New dialogs.
- Surface brand compliance warnings when document styling departs from loaded enterprise style profile.

---

