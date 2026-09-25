# Render Parity Phase 3: Real-World Document Corpus & Editing-Journey Specification

Date: 2026-09-25 · Status: **milestone specification, active planning for Phase 3** · Companion to [RENDER-PARITY-ROADMAP.md](RENDER-PARITY-ROADMAP.md)

## Purpose and Scope

Phase 1 ("One renderer per app") established unified GTK-free layout engines across Letters, Tables, and Decks, eliminating multi-engine divergence between screen, print, and PDF. Phase 2 ("Close the gap to LibreOffice, feature by feature") drives all 43 single-feature synthetic fixtures to green across Tier A and Tier B.

Single-feature fixtures prove that isolated attributes (e.g. bold runs, freeze panes, bullet markers) are drawn. However, isolated fixtures cannot prove that real, multi-page, multi-style documents hold together without pagination drift, clipped boxes, or layout collapse.

Phase 3 transitions the suite from synthetic micro-fixtures to **real-world production documents and interactive editing verification**:

1. **Curate a 90-document real-world corpus** (30 documents per application) with open licenses and recorded provenance.
2. **Define multi-page visual comparison metrics and ratcheting budgets** across document flows.
3. **Establish closed-loop editing journeys with visual round-trip verification**, connecting interactive editing, persistence, and external office rendering.
4. **Automate Tier C Flatpak Wayland validation** as the gating requirement for Phase 4 ("Earn usable").

---

## Part 1: Real-World Document Corpus Taxonomy & Provenance

### 1.1 Document Corpus Architecture

The corpus contains exactly 30 documents per application (90 documents total), evenly distributed across native OpenDocument formats (ODT, ODS, ODP) and Microsoft Office Open XML formats (DOCX, XLSX, PPTX).

Documents are grouped into three complexity tiers per application:

#### Letters (Word Processor) — 15 ODT, 15 DOCX

* **Tier S — Correspondence & Single-Page Documents (10 documents: 5 ODT, 5 DOCX)**
  * Formal business letters, executive memos, job applications, invoices.
  * *Key verification targets*: page margins, letterheads, font family fidelity, paragraph spacing, line height, date fields, signature blocks.
* **Tier M — Structured Reports & Multi-Page Documents (12 documents: 6 ODT, 6 DOCX)**
  * Project proposals, academic papers, policy briefs (3–6 pages).
  * *Key verification targets*: multi-level heading hierarchies (H1–H4), nested numbered/bulleted lists, embedded multi-column tables with borders and fills, headers and footers with `{page}` and `{total}` fields, page breaks.
* **Tier L — Complex Pagination & Layout Stress (8 documents: 4 ODT, 4 DOCX)**
  * Technical specifications, legal contracts, annual reviews (7–15 pages).
  * *Key verification targets*: pagination stability across page breaks, widow/orphan prevention, mixed margins, landscape section breaks, embedded images with captions, footnotes.

#### Tables (Spreadsheet) — 15 ODS, 15 XLSX

* **Tier S — Financial & Budget Models (10 documents: 5 ODS, 5 XLSX)**
  * Monthly operational budgets, expense reports, mortgage amortization schedules.
  * *Key verification targets*: currency and percentage number formatting, formula recalculation (`SUM`, `AVERAGE`, `IF`), frozen header rows and columns, negative value coloring.
* **Tier M — Structured Analytical Datasets (10 documents: 5 ODS, 5 XLSX)**
  * Inventory ledgers, sales logs, survey aggregates (100–300 rows, 10–20 columns).
  * *Key verification targets*: alternating row fills (zebra banding), text wrapping within defined column widths, explicit column/row sizing, right-aligned numbers, center-aligned dates.
* **Tier L — Multi-Sheet Workbooks with Visual Overlays (10 documents: 5 ODS, 5 XLSX)**
  * Executive dashboards, financial statements with multiple tabs.
  * *Key verification targets*: cross-sheet formula references, merged header ranges, embedded bar/line charts anchored to cell coordinates, custom grid border weights and colors.

#### Decks (Presentations) — 15 ODP, 15 PPTX

* **Tier S — Business Decks & Agendas (10 documents: 5 ODP, 5 PPTX)**
  * Status updates, pitch decks, meeting overviews (8–12 slides).
  * *Key verification targets*: 16:9 widescreen geometry, title and subtitle placeholder positioning, theme typeface inheritance, primary bullet hierarchies.
* **Tier M — Rich Media & Diagrammatic Presentations (10 documents: 5 ODP, 5 PPTX)**
  * System architectures, workflow diagrams, product showcases (10–15 slides).
  * *Key verification targets*: colored shapes with custom fills and borders, text boxes with vertical centering, slide tables with padded cells, embedded images, rotated labels.
* **Tier L — Master-Style & Layout Inheritance Cases (10 documents: 5 ODP, 5 PPTX)**
  * Conference keynote decks, organizational master templates (15–25 slides).
  * *Key verification targets*: slide master and layout inheritance, placeholder text overrides, mixed 4:3 and 16:9 aspect ratios, multi-column slide layouts, speaker notes on every slide.

### 1.2 Open Provenance and Licensing Contract

To ensure that the corpus can be committed, distributed, and inspected publicly without licensing friction:

1. **Permitted Licenses**: Every document must be dedicated to the public domain or licensed under standard permissive terms:
   * **CC0-1.0** (Creative Commons Zero / Public Domain)
   * **CC-BY-4.0** (Creative Commons Attribution)
   * **Public Domain / Open Government Data** (US Federal Government, European Union open publications, UK Open Government Licence).
2. **Permitted Sources**:
   * LibreOffice Template Center (curated CC0/CC-BY submissions).
   * Government open data portals (data.gov, data.europa.eu).
   * Academic repositories releasing open instructional materials.
3. **Forbidden Content**:
   * Proprietary enterprise documents, NDAs, copyrighted literature, or confidential materials.
   * Documents with active macro scripts (VBA, StarBasic) or external OLE links.
   * Personal identifiable information (PII); all sample identities must be synthetic.
4. **Corpus Directory & Manifest**:
   Corpus files are housed under `tools/render-lab/corpus/` accompanied by `corpus-manifest.json`:

```json
{
  "id": "letters-report-annual-review",
  "app": "letters",
  "format": "docx",
  "tier": "M",
  "file": "letters/docx/annual-review.docx",
  "title": "Annual Performance Review Template",
  "pages": 4,
  "license": "CC0-1.0",
  "source_url": "https://templates.libreoffice.org/template-center/annual-review",
  "author": "Open Source Community Contributors",
  "sha256": "4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a",
  "verification_targets": ["headings", "page-margins", "table-borders", "header-footer"]
}
```

---

## Part 2: Multi-Page Visual Comparison Metrics & Ratcheting Budgets

### 2.1 Multi-Page Alignment & Metric Computation

In Phase 1 and Phase 2, single-feature fixtures produce 1–2 pages. In Phase 3, documents span multiple pages, requiring metric aggregation that detects pagination cascading while isolating localized layout errors:

1. **Page-by-Page Decomposition**:
   * Headless LibreOffice and the application test renderer generate reference and candidate PNGs for every page: `ref-001.png`, `candidate-001.png`, etc.
   * Total page counts are compared first: `page_count_delta = candidate_pages - ref_pages`.
2. **Aggregate Metrics**:
   * **Mean SSIM**: Arithmetic mean of grayscale SSIM across all corresponding pages:
     $$\text{SSIM}_{\text{doc}} = \frac{1}{N} \sum_{i=1}^{N} \text{SSIM}(P_{\text{ref}, i}, P_{\text{cand}, i})$$
   * **Global Word-Found Rate**: Aggregate OCR word-bounding box recall via Tesseract:
     $$\text{WordFoundRate}_{\text{doc}} = \frac{\sum \text{WordsFound}_i}{\sum \text{WordsExpected}_i}$$
   * **Median Word Displacement**: Median spatial displacement (in typographical points) for matched words across all pages.
   * **Ink Check**: Binary confirmation that every non-blank reference page contains corresponding rendered ink.

### 2.2 Ratcheting Verdicts and Loss Budgets

Each real-world document is assigned a ratcheted verdict in `tools/render-lab/baseline-phase3.json`:

| Metric | 🟢 Green (Production Parity) | 🟠 Amber (Acceptable Layout) | 🔴 Red (Visual Regression) |
|---|---|---|---|
| **Page Count Delta** | $= 0$ (identical count) | $\le 1$ page delta | $> 1$ page delta |
| **Mean SSIM** | $\ge 0.88$ | $\ge 0.72$ | $< 0.72$ |
| **Word-Found Rate** | $\ge 92\%$ | $\ge 75\%$ | $< 75\%$ |
| **Median Displacement** | $\le 4.5\text{ pt}$ | $\le 12.0\text{ pt}$ | $> 12.0\text{ pt}$ |
| **Blank Pages** | $0$ false blanks | $0$ false blanks | Any missing page ink |

**Ratcheting Rule**: Similar to Phase 1, verdicts may move towards green, never towards red. Any PR that degrades a document's verdict from green to amber or amber to red triggers immediate CI failure.

---

## Part 3: Closed-Loop Editing-Journey Render Parity

### 3.1 The Editing-to-Rendering Disconnect

A document editor can fail in two distinct ways:
1. It fails to draw an imported file correctly (render engine bug).
2. It modifies a document interactively, but serializes invalid XML or omits formatting during save (model-to-storage bug).

Phase 3 introduces **Closed-Loop Editing Journeys** to ensure that interactive changes persist faithfully and re-render accurately in external tools.

```
[Corpus Document]
       │
       ▼
1. Open in Application ──► 2. Execute Scripted Edits ──► 3. Screen Snapshot (Tier A/B)
                                 │                                    │
                                 ▼                                    │
                         4. Save to Disk                              │
                                 │                                    │
                                 ▼                                    ▼
                         5. Headless LibreOffice ───────► 6. Visual Comparison
                            Export to PDF/PNG                (Canvas vs Oracle)
```

### 3.2 Closed-Loop Journey Protocol

Each application executes a standardized suite of 5 interactive journeys against real documents:

1. **Open and Mutate**:
   * Open a Tier M document.
   * Execute AT-SPI or core mutation commands (e.g. insert paragraph, modify cell value, change shape fill).
2. **In-Session Render Assertion (Tier A/B)**:
   * Capture the live canvas offscreen surface. Assert that edited content immediately updates with zero stale pixel artifacts.
3. **Atomic Save**:
   * Execute `save()` through the transaction engine. Verify atomic file write to disk.
4. **External Oracle Re-Render**:
   * Invoke `soffice --headless --convert-to pdf` on the newly saved document, rasterized at 96 DPI.
5. **Round-Trip Parity Assertion**:
   * Compare the modified application canvas render against the LibreOffice render of the saved file.
   * Assert that unedited pages remain identical to baseline within recorded budgets, and the edited region matches LibreOffice's representation.

### 3.3 Core Journey Test Matrix

| App | Baseline Document | Injected Mutation | Persistence & Render Assertion |
|---|---|---|---|
| **Letters** | `annual-review.docx` | Insert 2-row table into Section 2; apply Heading 2 style to title. | Table borders and header cells render on canvas and in LO-reloaded PDF at same coordinates. |
| **Letters** | `project-proposal.odt` | Append 500-word paragraph; verify pagination boundary reflow. | Page count advances by exactly 1; no orphaned heading at bottom of page 3. |
| **Tables** | `quarterly-budget.xlsx` | Modify cell D4 value from 1200 to 2500; update formula in D10. | Formula recalculation displays on grid; number format `$2,500.00` persists in reloaded sheet. |
| **Tables** | `inventory-log.ods` | Toggle header row fill to accent blue; apply frozen pane at row 2. | Frozen separator remains pinned during scroll; fill color matches in Calc within $\Delta E \le 10$. |
| **Decks** | `conference-keynote.pptx` | Add bullet point to slide 4; recolor accent shape to corporate green. | Text box dimensions auto-fit; shape fill matches Impress render within budget. |

---

## Part 4: Tier C Nightly Automation & Release Gating

### 4.1 Tier C Execution Environment

Tier C verifies that what reached the screen in unit tests and Broadway is identical to what runs on a real end-user desktop:

1. **Environment**: A Fedora Cloud VM booted under QEMU/KVM (`tools/render-lab/vm/`) running Mutter on Wayland.
2. **Execution**: Shipped Flatpak bundles (`org.tunaos.letters`, `org.tunaos.tables`, `org.tunaos.decks`) installed in user session.
3. **Capture**: QMP `screendump` captures full desktop display at $1600 \times 1400$, verifying compositor integration and system font fallback.

### 4.2 Phase 3 Exit and Phase 4 ("Earn Usable") Criteria

Phase 3 is considered complete and eligible for Phase 4 transition when:

1. **90/90 Corpus Coverage**: All 90 real-world documents are incorporated into the render lab with locked baselines.
2. **Zero Red Documents**: No document in the 90-file corpus produces a red verdict across any tier.
3. **Green Parity Threshold**: At least $75\%$ (68/90) of the corpus documents achieve full green verdicts.
4. **Closed-Loop Convergence**: All 15 editing journeys complete with matching visual round-trips.
5. **Tier C Verification**: Tier C runs green in nightly CI for 7 consecutive runs.

Upon meeting these criteria, the suite officially enters Phase 4, permitting the root `README.md` status table to elevate Letters, Tables, and Decks to **"usable for everyday documents"**.
