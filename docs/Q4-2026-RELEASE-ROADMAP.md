# Q4 2026 Release Roadmap — "Ship it Properly"

**Status**: Execution plan for gtk-office-suite v2.0 production release  
**Horizon**: October–December 2026  
**Owners**: architect (product), quality (verification), ops (release)  
**Last updated**: 2026-09-30

---

## Executive Summary

gtk-office-suite enters Q4 as a functionally complete daily-driver suite (Letters/Tables/Decks v2.0 all ship as Flatpaks with measured LibreOffice format parity). Q4 2026 gates the transition from "works for users who accept beta status" to "production-ready Linux desktop standard."

Three parallel release pillars must converge by end of Q4:

1. **Reproducible builds + Flatpak distribution** — verify package integrity end-to-end
2. **Accessibility + internationalization compliance** — screen-reader journeys + localization parity
3. **Interoperability loss-budget audit** — document what cannot round-trip, why, and user recourse

---

## Release Gates & Verification

### Gate 1: Reproducible Flatpak Builds (P0, Oct–Nov 2026)

**Owner**: ops / quality  
**Tracking**: #122, #578

Flatpak builds must be deterministic so users can verify signatures and distributors can certify origin.

| Deliverable | Status | Evidence |
|---|---|---|
| `.github/workflows/flatpak-build.yml` uses `flatpak-builder` reproducible flags | ⬜ | Build log diffs |
| Baseline build artifacts (`.appimage`, Flathub JSON) checksummed and pinned | ⬜ | SHA256 manifest in release notes |
| Two independent builds produce byte-identical Flatpak bundles | ⬜ | CI side-by-side rebuild comparison |
| GSettings schema migration tested in container (dconf upgrades from 1.x settings) | ⬜ | Automated test: fresh + upgraded profile |
| Flatpak runtime version pinned and documented (org.gnome.Platform >= 47) | ⬜ | `flatpak/org.tunaos.*.json` references |

**Target milestone**: All checks green by **2026-11-15**. PRs land on `release/q4-flatpak-reproducibility`.

---

### Gate 2: Accessibility Compliance (P0, Oct–Dec 2026)

**Owner**: quality  
**Tracking**: #120, #306

Screen-reader users and high-contrast theme users must be able to use all three applications for their primary workflows.

#### 2a. AT-SPI Navigation (Orca)

| Workflow | Target | Status | Evidence |
|---|---|---|---|
| Letters: open → edit paragraph → save (keyboard-only, Orca narration) | ✅ All steps audible + actionable | ⬜ | CI video + Orca logs |
| Tables: navigate grid → edit cell → navigate with arrow keys (Orca grid mode) | ✅ Spatial extent announced, cell moves announced | ⬜ | CI grid interaction capture |
| Decks: navigate slide thumbnails → edit text box (Orca outline) | ✅ Slide count, text layer announced | ⬜ | CI slide navigation capture |

Accessibility test harness: `tests/gui/test_a11y_journeys.py` runs automated Orca narratives + validates announced text against expected content.

#### 2b. High-Contrast Theme Support

| App | Dark mode | HC Black | HC White | Status |
|---|---|---|---|---|
| Letters | ✅ Tested | ⬜ | ⬜ | Start: 2026-10-01 |
| Tables | ✅ Tested | ⬜ | ⬜ | Start: 2026-10-15 |
| Decks | ✅ Tested | ⬜ | ⬜ | Start: 2026-11-01 |

Each app must pass: correct foreground/background contrast ratios (WCAG AA 4.5:1 for text), readable canvas overlays, focus indicators visible.

#### 2c. Keyboard Navigation

| Control | Letters | Tables | Decks | Target |
|---|---|---|---|---|
| Ctrl+Tab / Shift+Tab between panes | ✅ | ✅ | ⬜ | 2026-11-30 |
| Tab through toolbar → document → status | ✅ | ✅ | ⬜ | 2026-11-30 |
| Alt+F for File menu | ✅ | ✅ | ⬜ | 2026-12-15 |

**Target milestone**: All three journeys complete + CI passing by **2026-12-15**.

---

### Gate 3: Interoperability & Loss Budgets (P1, Nov–Dec 2026)

**Owner**: quality / architect  
**Tracking**: #105, #121

Document which LibreOffice / ODF features round-trip, which are lossy, and where users can recover data.

#### 3a. Loss-Budget Matrix

Publish `docs/INTEROP-LOSS-BUDGET.md`:

| Format | Feature | Status | Loss Type | Recovery |
|---|---|---|---|---|
| DOCX | Tracked changes | ⚠️ Unsupported | Converted to comments | User can re-inspect in LibreOffice |
| DOCX | Embedded charts | ⚠️ Partial | Rendered to image | Export + re-link source |
| ODT | Fields (date/page#) | ⚠️ Unsupported | Static text snapshot | Re-insert in LibreOffice |
| ODS | Pivot tables | 🔴 Not supported | Converted to static grid | Rebuild in Calc |
| ODP | Animations | 🔴 Not supported | Removed on save | Rebuild in Impress |

Baseline corpus: conformance/ corpus.json with representative .docx/.xlsx/.odp/.odt/.ods files; `tests/parity_conformance.rs` captures loss on round-trip and reports per-feature statistics.

#### 3b. Unsupported Feature Inspector

UI feature: when saving a document that contains unsupported features (tracked changes, embedded objects, animations), show a dialog:

```
⚠️ This document contains features that cannot be saved:

• Tracked changes (5 edits) → will be converted to comments
• Embedded OLE objects (2) → will be removed

[Learn more]  [Save anyway]  [Cancel]
```

Inspector stores feature inventory in document metadata so re-opening shows what was lost.

**Target milestone**: Loss-budget matrix + inspector UI by **2026-12-31**.

---

## Distribution & Localization

### Flatpak Distribution Channels (Oct 2026)

- **Flathub** (stable channel): v2.0 published and verified for 1 week beta before promotion to default
- **TunaOS Nightly** (unstable): bleeding-edge builds for early adopters
- **Fedora Silverblue** / downstream distros: coordinate package reviews + sign-off

### Internationalization (Ongoing through Q4)

| Language | Strings coverage | Status | Target |
|---|---|---|---|
| English | 100% | ✅ | — |
| Spanish | ≥ 90% | ⬜ | 2026-11-30 |
| French | ≥ 90% | ⬜ | 2026-11-30 |
| German | ≥ 90% | ⬜ | 2026-11-30 |
| Chinese (Simplified) | ≥ 85% | ⬜ | 2026-12-15 |

Translation freeze: **2026-11-15**. All new features lock strings after that date so translation can finalize.

---

## Version & Milestones

### Versioning Scheme

- **v2.0.x (October)** — Q4 beta releases, bug fixes for release candidates
- **v2.1.0 (November)** — feature freeze; accessibility + interop work
- **v2.2.0 (December 15)** — production release candidate
- **v2.3.0 (January 2027)** — post-release patch maintenance

### Weekly Release Cadence (Oct–Dec)

| Milestone | Release | Focus | Status |
|---|---|---|---|
| Oct 1–7 | v2.0.1 | Reproducible build setup | ⬜ Planned |
| Oct 8–14 | v2.0.2 | Early accessibility fixes | ⬜ Planned |
| Oct 15–21 | v2.0.3 | Orca integration testing | ⬜ Planned |
| Oct 22–28 | v2.0.4 | HC theme support (Letters) | ⬜ Planned |
| Oct 29–Nov 4 | v2.1.0-rc1 | Feature freeze; quality focused | ⬜ Planned |
| Nov 5–11 | v2.1.0-rc2 | Accessibility gap fixes | ⬜ Planned |
| Nov 12–18 | v2.1.1 | i18n strings freeze cutoff | ⬜ Planned |
| Nov 19–25 | v2.1.2 | Loss-budget audit + inspector | ⬜ Planned |
| Nov 26–Dec 2 | v2.2.0-rc1 | Final verification | ⬜ Planned |
| Dec 3–15 | v2.2.0 | Production release | ⬜ Planned |
| Dec 16–31 | v2.3.0 | Post-release patches | ⬜ Planned |

---

## Tracking & Reporting

### Issue Taxonomy

Issues for Q4 work use labels:

- `q4-release` — blocks production release
- `a11y-gate` — accessibility verification task
- `interop-audit` — interoperability loss documentation
- `flatpak-repro` — reproducible build gate task

### Weekly Sync

Every Monday 10:00 UTC: quality + architect + ops review release gate status against this roadmap. CI dashboard shows gate status live at release-gates.example.com (internal).

### Release Sign-Off

Merge this document to main; architect + quality + ops review and approve via PR comment before **2026-10-01** to lock execution plan.

---

## Success Criteria

By **2026-12-31**, gtk-office-suite v2.2.0 ships to Flathub production with:

- ✅ Reproducible Flatpak builds verified by independent rebuild
- ✅ Orca screen-reader accessible on all primary workflows (Letters/Tables/Decks)
- ✅ High-contrast theme support (WCAG AA compliance)
- ✅ Keyboard-navigable entire suite
- ✅ Interoperability loss budget documented + inspector UI live
- ✅ Localized to ≥5 languages (≥90% coverage)
- ✅ Zero critical security issues (CVSS ≥7.0)

**Release definition**: Users can adopt gtk-office-suite for daily productivity work with confidence that:
1. The binaries they install are signed and reproducible
2. Screen-reader users and users with visual impairments can fully operate the suite
3. Documents round-trip to LibreOffice with known, documented loss
4. The applications are localized to their language

---

## Related Documents

- [ROADMAP.md](ROADMAP.md) — quarterly goals and project mission
- [CI-QUALITY-GATES.md](CI-QUALITY-GATES.md) — automated verification harness
- [ACCESSIBILITY-COMPLIANCE-ROADMAP.md](ACCESSIBILITY-COMPLIANCE-ROADMAP.md) — detailed a11y audit plan
- [PYTHON-DEPRECATION.md](PYTHON-DEPRECATION.md) — legacy app retirement
- [GNOME-HIVE-MONITOR.md](GNOME-HIVE-MONITOR.md) — desktop integration checklist
