# Document Template Engine & Corporate Governance Strategy

> Architectural specification for corporate style governance, ODF template compatibility (OTT/OTS/OTP), and GTK4/Libadwaita template selector UI across Letters, Tables, and Decks.

---

## Executive Summary

Enterprise and organizational adoption of desktop office suites relies heavily on standard document templates (letterheads, financial models, presentation slide master decks, compliance reports). Currently, `gtk-office-suite` supports blank document creation and file opening, but lacks a formal template management pipeline, schema definition, and corporate style governance engine.

This document establishes the strategic architecture for the **Document Template Engine**, specifying:
1. **Template Schema & Format Specs**: Support for ODF template standards (`.ott`, `.ots`, `.otp`) and native JSON/YAML template manifests.
2. **Corporate Style Governance Engine**: Enforcing brand guidelines (color palettes, font stacks, header/footer locks, margin constraints).
3. **Platform Integration**: Seamless interaction with `XDG_TEMPLATES_DIR` (`~/.templates`) and GTK4/Libadwaita portal dialogs.
4. **GTK-Free Engine Crate (`suite-common-core`)**: Pure Rust parser, evaluator, and template generator, fully unit-tested outside the GTK loop.

---

## Architectural Layout

```
                  ┌──────────────────────────────────────────────┐
                  │          GNOME / GTK4 App Layer              │
                  │  (AdwViewStack / AdwCarousel Template UI)    │
                  └──────────────────────┬───────────────────────┘
                                         │
                                         ▼
                  ┌──────────────────────────────────────────────┐
                  │    suite-common / Template Dialog Helper     │
                  │  (XDG Template Enumeration, Thumbnail Cache) │
                  └──────────────────────┬───────────────────────┘
                                         │
                                         ▼
                  ┌──────────────────────────────────────────────┐
                  │       suite-common-core (GTK-Free Engine)    │
                  │ ┌──────────────────┐ ┌─────────────────────┐ │
                  │ │ ODF Parsers      │ │ Corporate Governance│ │
                  │ │ (.ott/.ots/.otp) │ │ Style Enforcer      │ │
                  │ └──────────────────┘ └─────────────────────┘ │
                  └──────────────────────────────────────────────┘
```

---

## Key Requirements & Phases

### Phase 1: Engine Core & ODF Template Parsers (`suite-common-core`)
- **ODF Template Compatibility**: Parse OpenDocument Template packages (`.ott` for Letters, `.ots` for Tables, `.otp` for Decks). Extract XML styles, default master pages, and embedded assets.
- **Template Manifest (`template.json`)**: Define light metadata schema for templates (author, version, target app, categories, corporate tags).

### Phase 2: Corporate Governance Engine
- **Style Enforcement Rules**: Restrict font families to approved corporate typography (e.g., Cantarell, Inter), restrict color swatches, and mandate standardized legal notices/disclaimers in document footers.
- **Policy Files (`corporate-policy.toml`)**: Allow system administrators to deploy system-wide templates (`/etc/gtk-office-suite/templates/`) and strict governance policies.

### Phase 3: UI & XDG Integration
- **Libadwaita Selector Grid**: Responsive `AdwViewStack` template picker on application startup / "New from Template..." dialog.
- **XDG User Templates**: Auto-discover templates stored in `~/.templates` and Flatpak sandbox portals.

---

## Parity & Verification Baseline

- **Unit Tests**: 100% test coverage in `suite-common-core` for template parsing, variables substitution (e.g., `${USER}`, `${DATE}`, `${COMPANY}`), and policy validation.
- **ODF Interop Baseline**: Parity test suite against LibreOffice OTT/OTS/OTP sample files with `REQUIRE_SOFFICE=1` gating.
