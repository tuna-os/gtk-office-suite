# Document Template Architecture & Style Governance Strategy

> **Status**: Strategic Proposal  
> **Horizon**: Mid-term (Q4 2026 / Q1 2027)  
> **Maintainer**: Strategist Agent  

---

## 1. Executive Summary

As **gtk-office-suite** (Letters, Tables, Decks) transitions into a mature desktop productivity suite on Linux, enterprise and organizational adoption requires robust **Document Template Architecture & Style Governance**.

Currently, users start from blank document defaults or import existing files. Providing a GTK-free, standardized template engine and governance framework allows organizations to distribute corporate letterheads, slide masters, and financial spreadsheet templates with enforced styling rules.

---

## 2. Strategic Objectives & Principles

1. **GTK-Free Schema (`suite-common-core`)**: Template metadata, manifest schemas, and style rules must reside in `suite-common-core` without GTK dependencies, enabling CLI validation and automated linting.
2. **Native GNOME Template Chooser (`suite-common`)**: Standardized AdwDialog-based template selectors across Letters, Tables, and Decks.
3. **Open Standards Alignment**: Map template structures directly to ODF/OOXML templates (`.ott`, `.ots`, `.otp`, `.dotx`, `.xltx`, `.potx`) where applicable.
4. **Corporate Governance & Presets**: Support system-wide (`/usr/share/gtk-office-suite/templates/`) and user-local (`~/.local/share/gtk-office-suite/templates/`) template repositories.

---

## 3. Architecture Overview

```
+-----------------------------------------------------------------------+
|                           suite-common-core                           |
|  - TemplateManifest (JSON/TOML schema)                                |
|  - TemplateCategory & Metadata (author, license, thumbnail, tags)     |
|  - StyleGovernanceRules (font constraints, color palette enforcement)  |
+-----------------------------------------------------------------------+
                                   |
                                   v
+-----------------------------------------------------------------------+
|                              suite-common                             |
|  - TemplateChooserDialog (AdwDialog widget for template selection)   |
|  - TemplateManager (discovers system & user template directories)     |
+-----------------------------------------------------------------------+
                                   |
           +-----------------------+-----------------------+
           |                       |                       |
           v                       v                       v
     +-----------+           +-----------+           +-----------+
     |  Letters  |           |  Tables   |           |   Decks   |
     +-----------+           +-----------+           +-----------+
```

---

## 4. Phased Rollout Plan

### Phase 1: Core Manifest & Metadata Schema
- Define `TemplateManifest` struct in `suite-common-core`.
- Add support for manifest parsing and thumbnail resolution.

### Phase 2: GUI Chooser & Application Integration
- Implement `TemplateChooserDialog` in `suite-common` using GTK4 / Libadwaita grid widgets.
- Integrate initial "New from Template" actions in Letters, Tables, and Decks app menus.

### Phase 3: Style Governance & Organization Presets
- Add corporate style policy enforcement (e.g. restrict non-approved fonts, mandatory color palettes).
- System-wide template deployment directory support for Flatpaks and Linux distros.

---

## 5. Verification & Testing Strategy

- **Unit Tests**: Test template manifest parsing and validation in `suite-common-core`.
- **Interoperability**: Verify template conversion and export to LibreOffice template formats (`.ott`, `.ots`, `.otp`).
- **GUI Tests**: Dogtail / AT-SPI integration tests covering template picker dialog interaction.
