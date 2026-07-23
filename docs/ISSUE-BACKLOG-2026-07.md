# Scoped Issue Backlog

Date: 2026-07-21  
Milestone: [Product quality and daily-driver readiness](https://github.com/tuna-os/gtk-office-suite/milestone/2)

This is the execution index for the
[Product and Quality Roadmap](PRODUCT-QUALITY-ROADMAP-2026-07.md). Issues are
ordered by dependency, not by issue number alone. The coordinating issue is
[#95](https://github.com/tuna-os/gtk-office-suite/issues/95).

## Phase 0 — Truth and data safety

These issues are release blockers and should land before new editor features.

- [#96 — Layered capability scorecard](https://github.com/tuna-os/gtk-office-suite/issues/96)
- [#97 — Tables canonical workbook and live undo](https://github.com/tuna-os/gtk-office-suite/issues/97)
- [#98 — Tables multi-sheet isolation](https://github.com/tuna-os/gtk-office-suite/issues/98)
- [#99 — Shared dirty/save/close/autosave/recovery lifecycle](https://github.com/tuna-os/gtk-office-suite/issues/99)
- [#100 — Decks imported-master preservation](https://github.com/tuna-os/gtk-office-suite/issues/100)
- [#101 — Real preference bindings](https://github.com/tuna-os/gtk-office-suite/issues/101)
- [#102 — Warning cleanup and changed-crate warning gate](https://github.com/tuna-os/gtk-office-suite/issues/102)

Recommended order: #97 and #98 together; #99 in parallel; then #100 and #101.
#96 begins immediately but should only turn claims green after the corresponding
journeys exist. #102 is continuous and must not become a cosmetic rewrite.

## Phase 1 — Canonical controllers

- [#103 — GTK-free canonical controllers](https://github.com/tuna-os/gtk-office-suite/issues/103)

The Tables portion of #103 is delivered through #97/#98. Decks should absorb
#100 into a canonical `Deck` controller. Letters then moves toward a
model-authoritative controller without replacing GtkTextView.

## Testing and interoperability foundation

These run alongside Phases 0 and 1. They prove the repaired architecture and
become prerequisites for later feature issues.

- [#104 — Deterministic GUI journeys and state snapshots](https://github.com/tuna-os/gtk-office-suite/issues/104)
- [#105 — Versioned office corpus and loss budgets](https://github.com/tuna-os/gtk-office-suite/issues/105)
- [#106 — Property, fuzz, Unicode, and formula differential testing](https://github.com/tuna-os/gtk-office-suite/issues/106)
- [#107 — Deterministic visual golden scenarios](https://github.com/tuna-os/gtk-office-suite/issues/107)
- [#108 — Fast, GUI, and nightly CI gates](https://github.com/tuna-os/gtk-office-suite/issues/108)

Dependency order: #103 enables headless journeys; #104 supplies GUI evidence;
#105 defines interoperability truth; #106 and #107 extend coverage; #108 makes
the completed instruments enforceable.

## Phase 2 — Daily-driver application depth

### Letters

- [#109 — WYSIWYG styled pagination and Unicode offsets](https://github.com/tuna-os/gtk-office-suite/issues/109)
- [#110 — Tables, lists, paragraphs, and sections](https://github.com/tuna-os/gtk-office-suite/issues/110)
- [#111 — Comments, tracked changes, TOC, and bidi](https://github.com/tuna-os/gtk-office-suite/issues/111)

#109 and #110 follow the Letters controller work in #103. #111 follows both
and requires format-loss policy from #105.

### Tables

- [#112 — Sparse virtual grid, structural edits, and performance](https://github.com/tuna-os/gtk-office-suite/issues/112)
- [#113 — Fill, references, filters, names, and printing](https://github.com/tuna-os/gtk-office-suite/issues/113)
- [#114 — Charts, pivots, arrays, and protection](https://github.com/tuna-os/gtk-office-suite/issues/114)

All three depend on #97/#98. #112 establishes the scalable storage/rendering
base, #113 completes routine authoring, and #114 is deliberately last and
upstream-engine-led.

### Decks

- [#115 — Direct manipulation and arrange tools](https://github.com/tuna-os/gtk-office-suite/issues/115)
- [#116 — Themes, layouts, styling, and image crop](https://github.com/tuna-os/gtk-office-suite/issues/116)
- [#117 — Presenter view, export, animations, media, and review](https://github.com/tuna-os/gtk-office-suite/issues/117)

#115/#116 require the canonical complete-Deck state from #100/#103. Advanced
scope in #117 follows their daily-driver journeys and requires an ADR before
admitting costly animation/media formats.

## Phase 3 — GNOME-native product polish

- [#118 — Adaptive, contextual, theme-correct editor design](https://github.com/tuna-os/gtk-office-suite/issues/118)
- [#119 — Recent files, templates, portals, drag/drop, and help](https://github.com/tuna-os/gtk-office-suite/issues/119)
- [#120 — Complete keyboard and screen-reader journeys](https://github.com/tuna-os/gtk-office-suite/issues/120)

#118 coordinates rather than duplicates existing HIG issues #73–#80 and
#93–#94. #120 extends existing accessibility issue #87 from node presence to
complete workflows and correct spatial bounds.

### GNOME audit and scaffolding resources

Use [hanthor/gnome-gui-spec](https://github.com/hanthor/gnome-gui-spec) as the
agent-facing skill and pattern resource for GNOME design audits, intent mapping,
widget selection, and UI scaffolding. Start with its `SKILL.md` and
`INTENT-MAP.md`, then select only the relevant component skill or application
audit. In particular, reuse its tabbed-document, toast-feedback,
sidebar-navigation, and preferences-dialog patterns instead of inventing local
variants.

The repository's `skills/vision-check` and `skills/broadway-inspect` skills are
the local verification companions for visual review and interactive GTK
inspection. `AGENT-GNOME-REFERENCE.md` and `AGENT-REFERENCE-LIBRARY.md` provide
the suite-specific routing notes.

These resources accelerate audits and scaffolding; the current GNOME HIG and
libadwaita/GTK documentation remain authoritative. If a copied skill or audit
conflicts with the current platform API or HIG, record the discrepancy and
follow upstream GNOME guidance. Do not depend on the historical `/tmp` checkout
path documented in older notes—use the upstream repository or a pinned,
reviewed project copy.

## Phase 4 — Compatibility and release confidence

- [#121 — Unsupported-feature inspector and opaque pass-through](https://github.com/tuna-os/gtk-office-suite/issues/121)
- [#122 — Flatpak, upgrade, localization, recovery, and release gate](https://github.com/tuna-os/gtk-office-suite/issues/122)

#121 implements the loss policy established in #105. #122 is the final release
qualification gate and consumes the evidence published by #96 and #108.

## Backlog rules

1. Do not close an issue from model-only tests when it scopes GUI integration.
2. Every mutation issue includes undo, dirty-state, save/reopen, and failure
   behavior where applicable.
3. Every persisted feature names its DOCX/ODT, XLSX/ODS, or PPTX/ODP evidence.
4. Prefer upstreaming missing engine/format support over building a parallel
   local implementation.
5. New scope enters through #95 and the roadmap, with dependencies and a test
   instrument identified before implementation.
6. GNOME-facing work starts with the relevant `hanthor/gnome-gui-spec` skill or
   audit and records which pattern was applied; visual and interaction tests
   still prove the result in this suite.
