# Render Parity Roadmap

Date: 2026-09-24 · Status: **active, supersedes feature work until Phase 2 exits**

## Why this exists

The apps do not work as WYSIWYG editors. Most of what the document models
hold is never drawn, and nothing in CI would notice. Every existing
compatibility number (CommonMark 651/652, LibreOffice ↔ Letters 109/109, …)
measures **file content**. None of them measures **what the user sees**.
A feature could be marked ✅ in PARITY.md while being invisible on screen,
and many are.

This roadmap sets one rule and builds the tooling to enforce it:

> **A visual feature is not done until a screenshot of the running app,
> taken on the same document, matches LibreOffice's rendering of that
> document within a recorded budget.**

That is a comparison against LibreOffice on screen, not only on the saved
file.

## Where we actually are (audit 2026-09-24)

Taken from the code, not from PARITY.md. File:line references are on the
audit branch.

### Letters: the editing surface is not a page layout

- `PageContainer` (`letters/src/page_container.rs`) draws grey page
  rectangles, then places **one `GtkTextView` spanning every page and
  every gap**. Text does not reflow per page. It runs over page gaps and
  top/bottom margins.
- Pagination (`letters/src/layout.rs`) lays out *unstyled* text in the
  default font. Headings, spacing, images and page breaks don't affect
  where pages break. The result only decides how many grey rectangles to
  draw.
- PDF export goes through **Typst** (`suite-export`), a second,
  independent layout engine. Print preview is a third one (plain unstyled
  Pango). Screen, print and PDF therefore can never agree.
- These exist in the model but are **not drawn**: font family, font size,
  text colour, super/subscript, space before/after, left/right/first-line
  indents, tab stops, block quotes, named styles, footnote bodies, real
  columns, rich headers and footers, and `{total}` in page numbers.
- Lists are literal `- ` / `1. ` text, not list layout. Tables are
  **pipe-separated text** (`| a | b |`), not a table.
- "100%" zoom is not a physical size. `PageContainer` scales the page to
  the window width, `((width − 48) / page_width_pt).min(1.5)`, which treats
  points as pixels and caps at 1.5 px/pt. A Letter page is 918 px wide in
  a 1100 px window, while 100% at 96 DPI would be 816 px, and it shrinks
  with the window. The zoom slider multiplies that. WYSIWYG needs 100% to
  mean 96/72 px per point, with fit-to-width as a separate zoom mode.

### Tables: the model drops most cell formatting on import

- `grid_render.rs::draw_grid` draws values, number formats, borders (always
  black; `CellBorder.color` ignored), numeric conditional fills, sizes and
  selection.
- These exist in the model but are **not drawn**: merged cells, frozen
  panes, charts on the grid (they only appear in the chart dialog preview),
  and border colour.
- These are **not in the model at all**: per-cell font, bold/italic, size,
  text colour, fill, horizontal/vertical alignment and wrap. Every cell is
  left-aligned, one line, in the default font.

### Decks: shape styling is hardcoded

- `canvas.rs::draw_slide_multi` draws every rectangle **hardcoded blue** and
  every ellipse **hardcoded red**. Text colour is guessed from background
  brightness.
- Run colour, font family, highlight, links and super/subscript are not
  drawn. Master ellipses and images are skipped.
- These are **not in the model at all**: fill, stroke, opacity, gradients,
  shadows, placeholders with inherited layout styles, tables, charts and
  groups.

### Tooling: nothing looks at pixels

- `tests/gui/visual_golden.py` is a stub. It compares images if you hand it
  two, but nothing captures them and `tests/gui/goldens/` does not exist.
- The LibreOffice oracle tests compare text and structure only.
- The README screenshots come from demo documents written to avoid the gaps
  above.

## The instrument: the render lab

`tools/render-lab/` answers one question per feature: *did it render, and
how close is it to LibreOffice?*

```
 fixtures.py ──► one tiny .docx/.xlsx/.pptx per feature  (+ manifest.json)
      │
      ├──► LibreOffice headless ──► PDF ──► pdftoppm @96dpi ──► reference PNG
      │
      ├──► Tier A: in-app offscreen render of the document surface ──► PNG
      ├──► Tier B: app under gtk4-broadwayd, screenshot by headless Chromium
      ├──► Tier X: app under Xvfb + WM, screenshot of the X root (existing)
      └──► Tier C: shipped Flatpak in a GNOME VM (QEMU), screendump
                         │
                 compare.py ──► per-fixture metrics ──► report.html + summary.json
```

### Fixtures: one feature per file

`fixtures.py` generates about 45 documents with python-docx, openpyxl and
python-pptx. These are foreign writers, so our readers see input we didn't
produce. Each file isolates one feature (`letters/bullet-list`,
`tables/merged`, `decks/shapes` …). Its manifest entry states in one line
what must be visible. When something regresses, the red row names the
feature.

A second corpus of *real* documents (LibreOffice-authored templates,
government forms, public-domain reports) is added in Phase 3. The
single-feature corpus finds bugs; the real corpus shows whether users
would notice.

### Reference: LibreOffice

`soffice --headless --convert-to pdf`, then `pdftoppm -r 96`, gives one
PNG per page or slide at the same 96 DPI our apps draw at 100% zoom. The
fonts are pinned in the container so both renderers see the same faces:
Liberation, Carlito, Caladea, DejaVu, Noto. Otherwise we would be
measuring font substitution, not layout.

How the reference lines up with each app:

| App | Our surface | LibreOffice surface | Alignment |
|---|---|---|---|
| Letters | page N of the page view at 100% | PDF page N | Exact: same page box, same DPI |
| Decks | slide canvas scaled to the slide size | PDF page = slide | Exact: same aspect, resampled |
| Tables | column/row headers plus the used range, as drawn at 100% zoom with "Show gridlines" off | PDF of the sheet with headings on and gridlines off, cropped to its ink (the same headers + used range) | The size ratio of the two is reported as `scale`. Then both are cut to their cells: Calc prints headings boxed in black and we shade them, so headings are left out. Ours is resampled onto LibreOffice's cells. Gridlines are off on both sides because they are view furniture (Calc prints them black, screens draw them faint) and a printed gridline hides a thin cell border. |

### Capture tiers: what each one proves

| Tier | How | Proves | Cost | Runs |
|---|---|---|---|---|
| **A: offscreen** | Test-only `render-dump` action. It renders the document widget through the **same** snapshot path GTK uses on screen (`gtk::WidgetPaintable` → `gsk::Renderer::render_texture`) and saves a PNG per page/slide. Decks and Tables can reuse their Cairo `draw_*` functions directly. | Our drawing code draws the feature. No window manager, so no flake. | ~1 s/fixture | every PR |
| **B: Broadway + browser** | App runs with `GDK_BACKEND=broadway` against `gtk4-broadwayd`. Playwright Chromium connects first, then the app starts **maximized** in a 1100×1700 viewport, the same size Tier A uses (see the Broadway notes below). The page is found in the screenshot by template matching against the app's own on-screen render from the same process (`A-1.png`, or `S-1.png` for Decks), and rejected if the best match is off by more than 4 grey levels. | The pixels reached a **real display client** through a real GDK backend. The widget mapped, allocated and painted inside a real window. This catches the class of bug where `snapshot()` works offscreen but the widget is never mapped (see PR #86). Anyone can open the live session in a browser. | ~5 s/fixture | every PR |
| **X: Xvfb** | Existing harness (`tests/gui/`), X11 backend, matchbox WM, mss capture. | The same as B on the X11 backend, and it keeps AT-SPI journeys and synthetic input working. | ~5 s | every PR (journeys) |
| **C: VM** | QEMU/KVM boots a GNOME image, installs the **built Flatpak bundle**, launches it with the fixture on Wayland/Mutter with the GL renderer, and captures with a QMP `screendump`. | The **shipped artifact** renders on a real desktop: the Flatpak runtime's own fonts, GL/Vulkan GSK renderer, portals, fractional scaling. Tiers A/B/X run the debug binary on the software renderer and cannot see these. | ~3 min/run | nightly + release gate (needs `/dev/kvm`: GitHub-hosted runners have it) |

A/B/X should agree with each other almost pixel for pixel. **A
disagreement between tiers is a bug in its own right**, usually "drawn
offscreen but not on screen". LibreOffice is compared against Tier A, and
Tier B is compared against Tier A: `compare.py` reports the mean grey-level
difference between them per fixture ("Tier A↔B"). Above 3 grey levels it is
listed as a disagreement in the report and in `summary.json`
(`tier_disagreements`). Agreeing tiers measure 0.0–1.7.

Broadway notes, learned the hard way (GTK 4.14):

- GDK's Broadway backend starts with a hard-coded 1024×768 monitor. The
  app presents its window before broadwayd's real screen size arrives, so
  a default-sized window is clamped to 1024×768 and never grows. The only
  toplevels GDK resizes when the screen size arrives are maximized ones
  (`_gdk_broadway_display_size_changed`). So Tier B seeds
  `window-maximized=true`, and the browser viewport is exactly the window
  size Tier A uses.
- The Broadway renderer rasterizes a cairo node's recording surface from
  (0, 0) without subtracting the node's origin. A cairo node whose bounds
  don't start at (0, 0) shows its content shifted by that origin and
  clipped. This is still the case on GTK `main`. `snapshot.append_cairo`
  always folds the current offset into the bounds, so custom widgets must
  build a `gsk::CairoNode` at (0, 0) and wrap it in a `gsk::TransformNode`
  (see `PageContainer::snapshot`). Other renderers draw both forms
  identically. We confirmed this on X11 with the cairo and GL renderers.

### Metrics

Pixel-exact equality with LibreOffice is neither possible nor the goal: the
two apps use different line-breakers, hinting and spacing heuristics. Each
fixture reports four numbers instead, and the budget is set per fixture:

1. **Ink**: does the region LibreOffice drew anything in contain
   non-background pixels in ours? 0 means *not rendered*. This is the
   binary check the product is failing today, and it gates first.
2. **Text geometry**: Tesseract word boxes on both images. We record the
   fraction of LibreOffice's words found in ours, the median word-centre
   displacement in points, and the line count delta. This catches wrong
   fonts, sizes, spacing, indents, margins and alignment without caring
   about anti-aliasing. Also **lost lines**: LibreOffice's text lines (as
   Tesseract groups them) of three or more words with half or more of
   their words missing from ours. Any lost line keeps a fixture from
   green: the word fraction is page-wide, so a page's one broken line
   (a header, a caption) could not move it (see below).
3. **Colour**: whether each salient colour cluster in the reference (a
   fill, a text colour) is present in ours within ΔE 10, and roughly where.
4. **SSIM**: structural similarity on grayscale images downsampled 4×. A
   single overall "looks like it" number, used for trend lines and not
   gated alone.
5. **Scale** (Tables only): the size of our headers + used range relative
   to LibreOffice's for the same cells. More than ±10% off is not green.

Every fixture gets a verdict: **red** (ink = 0, or words found < 50%),
**amber** (rendered but outside budget) or **green**. The verdicts are
committed as `tools/render-lab/baseline.json` and **ratcheted**: a fixture
may move toward green, never away.
The same rule applies to the existing oracle corpora.

#### Known metric artifacts (left amber on purpose)

A fixture stays amber, rather than having a threshold tuned, when its
images show our rendering matches LibreOffice's and the metric is what
disagrees. Recorded here with the measurement, so nobody "fixes" them by
loosening a budget:

- **`letters/font-sizes`** (2026-09-25). Every line's ink box is within a
  pixel of LibreOffice's (measured row and column extents). Tesseract reads
  the 8 pt line as "spt text" on LibreOffice's page and as one token
  ("bpetext") on ours, so 8 of 10 words match (below the 0.9 green bar).
  The matcher then pairs LibreOffice's unmatched "text" with the nearest
  remaining "text" on the next line, and every later pairing shifts by a
  line: the median displacement (14.8 pt) is that cascade, not a layout
  error. A globally optimal matcher would fix the displacement but not the
  word count, so it would not change this verdict, and it was not
  worth a shared-metric change on its own.
- **`letters/table`** (2026-09-25). Cell text sits within 0.2 pt of
  LibreOffice's; Tesseract reads one cell label in ours ("R1C1" as "rici",
  unhinted glyphs at 12 px) and so 6 of 9 cell words match.

Both are OCR of very small or very similar glyphs, not rendering. They go
green when the metric can read them (a larger OCR scale for small text is
the next candidate, and it would have to be checked against every app's
verdicts first, since `compare.py` is shared).

The opposite blind spot existed too, so look at the images of a green
fixture as well: **`letters/page-numbers`** (2026-09-25) was green on
first run while every page's header read "Page  of" with no numbers (the
PAGE/NUMPAGES fields were read as empty). Two missing words out of about
500 on a page moved the word fraction by less than a percent. The lost
lines check closes it: that header is half its words gone. Diffed on two
CI datasets, all 46 fixtures of all three apps: on the one with the empty
header, the only verdicts to change were `letters/page-numbers` A and B
(green to amber); on the current one, none changed (`letters/table` has a
lost line, one OCR-misread cell label row, and was amber already).

### Report

`report.html` lays each fixture out as LibreOffice | ours (Tier A) | ours
(Tier B) | diff overlay, with metrics, verdict and the manifest's "expect"
line. CI uploads it as an artifact and links it from the PR comment
(`tests/gui/pr_comment.py`). A reviewer can see in ten seconds whether a
PR made rendering better or worse.

## The gate: everything is verified in CI

The render lab is built so that agents (the hive) and humans can drive
development from CI output alone, without anyone looking at screenshots.
`.github/workflows/render-parity.yml` is the single source of truth.

| Signal | Where | Who uses it |
|---|---|---|
| `summary.json` | report artifact | machine-readable verdicts, metrics and deltas for every fixture |
| Job summary + sticky PR comment | every PR | what this PR made better or worse, with a link to the side-by-side report |
| **Ratchet** (`compare.py --baseline tools/render-lab/baseline.json`) | every PR, gating | fails if any fixture's verdict gets worse (**regressed**), or gets better or is new without the baseline being updated in the same PR (**stale**). Every gain is locked in by the PR that made it. |
| **One issue per non-green fixture** (`sync_issues.py`) | every push to main and nightly | labels `render-parity`, `app:<app>`, and `blocked` when the fixture needs a Phase 1 architecture item. The acceptance criterion is "CI reports this fixture green". Deduplicated by a hidden marker; closes itself when the fixture turns green on main. |
| Tier C VM report | nightly + manual dispatch | shipped Flatpak on a real GNOME Wayland session. Non-gating until it has run green for a week. |

The baseline stores verdicts only (green/amber/red/missing), not raw
metrics, so it is stable across runs and diffs cleanly in review.

### The loop an agent follows

1. Pick an open `render-parity` issue without the `blocked` label.
2. Reproduce with `tools/render-lab/run.sh --app <app>`, or read the
   report artifact from the linked run.
3. Fix the drawing code, the model, or the reader.
4. Run `tools/render-lab/run.sh --app <app> --update-baseline` and commit
   `tools/render-lab/baseline.json` with the fix.
5. CI proves it: the ratchet passes, the PR comment shows the fixture
   moving to green, and nothing else regressed. After merge, the issue
   closes itself.

Blocked issues wait for their Phase 1 item. Each of those gets its own
tracking issue, and its exit criterion is again a set of fixtures CI
reports as at least amber.

### Rules the humans enforce in review

1. **PARITY.md gets a `Render` column.** A row may show ✅ only if it
   names a render-lab fixture that is green. Existing ✅ rows without one
   become 🟡 "file-only". `scripts/validate_parity.py` enforces this (to
   do).
2. **A PR that adds or changes a visual feature must add or update a
   fixture.** The fixture's "expect" line is the spec.
3. **README status is generated from `summary.json`** (to do). The
   "usable for" table cannot claim more than the green fixtures support.

## Phases

### Phase 0: See the truth (done 2026-09-24)

Exit: the lab runs in the container and produces a baseline report for all
three apps. We expect it to be mostly red.

The first baseline was Tier A 1 green / 31 amber / 10 red.

- [x] Honest README banner and status table.
- [x] This roadmap.
- [x] `tools/render-lab/`: Containerfile, fixture generator, LibreOffice
      reference renderer, comparison metrics, HTML report, driver script.
- [x] Tier A `render-dump` test action in Decks, Tables and Letters
      (`GTK_OFFICE_TEST_MODE` only) (#888).
- [x] Tier B Broadway capture (`capture.py --tier B`): headless Chromium
      screenshots the Broadway page, located by geom.json and verified
      against the app's own render. Tiers A and B agree to within about
      1 grey level; see "Broadway notes" (#888).
- [ ] Tier C VM harness (`tools/render-lab/vm/`). Boots a GNOME image
      under QEMU/KVM, installs the CI Flatpak bundle, opens each fixture
      and takes a `screendump` over QMP. It needs `/dev/kvm`, which the
      2026-09-24 dev box lacks, so it is built and validated on a GitHub
      runner.
- [x] `render-parity.yml`: Tier A+B on every PR, ratchet, sticky PR
      comment, report artifact, issue sync on main, nightly Tier C (#888).
      Each app runs as its own parallel job, and every run publishes
      `baseline.proposed.json` (#946, #955). Tier C has not had a green run.
- [x] Commit the first `baseline.json`, and let the issue sync open the
      first backlog: 41 `render-parity` issues (#888).
- [ ] Close or consolidate the 60+ duplicate strategist issues so the
      `render-parity` backlog is what agents find.
- [ ] `visual_golden.py`: delete it, or make it the Tier A vs Tier B
      comparator. Stop advertising a harness that doesn't run.

### Phase 1: One renderer per app (architecture; ADR 0009 accepted)

**Status (2026-09-24): exit criterion met.** No fixture is red in any app,
in either tier. On main, Tier A was 21 green / 21 amber / 0 red that day; it
began the day at 1 / 31 / 10. The counts have moved since, so read them from
`tools/render-lab/baseline.json`, the live source of truth, not from this
page. The items below record what landed and what's left.


WYSIWYG is only true when the screen, print and PDF come from **one**
layout and one draw routine. ADR 0009 is proposed; this phase accepts it
and does the part that matters.

- **Letters page layout engine** (`letters-core::layout`, GTK-free).
  *Landed (#960, ADR 0010): the render tree, a pluggable measurer, and a
  shared page-drawing routine behind `render`. `letters/pagination` is
  green. Print, Print Preview and Export as PDF draw from the same laid-out
  pages (#973, ADR 0010 stage 2).*
  - It takes the document model and produces a serializable render tree:
    pages → blocks → lines → glyph runs, plus boxes for list markers,
    table cells, images, headers/footers and footnotes. It uses Pango
    without a widget context.
  - One `draw(tree, cairo)` routine drives the screen, print, PDF (Cairo
    `PdfSurface`) and thumbnails.
  - Typst export stays for now as an alternative "typeset" export, not as
    the definition of what the page looks like.
- **Letters editing surface on that tree.** Replace the single
  `GtkTextView` with a custom `PageView` widget: the caret, selection and
  hit-testing map through the render tree, text input goes through
  `GtkIMContext`, and accessibility through `GtkAccessibleText` (GTK ≥ 4.14).
  - Staged: first a **read-only page view** (a toggle, like "Print Layout"
    vs "Draft"). It is immediately useful and lab-testable. *(Done in
    #960: `PageView`, real size at 100%.)*
  - Then editing on it. Then remove the TextView path. *(Editing landed:
    #974 caret and input, #976 screen readers and lists, #986/#987/#1015 a
    live model with incremental relayout, and #1022 made Print Layout the
    default view. The TextView path is still there.)*
  - This is the largest single item on the roadmap and it is unavoidable.
    There is no configuration of one `GtkTextView` that produces per-page
    layout.
- **Tables cell-style model** *(landed: #940 xlsx read, write and draw;
  #941 spreadsheet metrics; #937 merges; #938 charts; #954 and #961 in
  review for borders and ODS; #962 the Format inspector)*: font (family,
  size, bold, italic,
  underline, colour), fill, horizontal/vertical alignment, wrap, indent,
  border colour and rotation. Add them to `tables-core::sheet` with xlsx
  and ods read/write, then draw them. Merges, frozen panes and chart
  overlays go into `draw_grid`.
- **Decks shape-style model** *(landed: #948 presets, fills, gradients
  from the theme's format scheme, and outlines; #956 tables; #932
  inherited placeholder geometry; #963 text sizes; #964 run colours.
  Inherited text styles, paragraphs and bullets are in progress in #966)*:
  fill (solid/gradient/none), stroke (colour,
  width, dash), placeholders with inherited layout and master text styles,
  run colour and font family, vertical anchor and autofit. Delete the
  hardcoded blue and red.

Exit: every feature fixture is at least **amber** (rendered, even if not
yet close). Zero reds.

The design layer that sits on these models is iWork-inspired, Rust-fast
and HIG-native: format inspector, insert bar, previewed styles, smart
guides, Magic Move and more. It is specified in
[DESIGN-UI.md, "Ideas taken from iWork"](DESIGN-UI.md#ideas-taken-from-iwork-direction-set-2026-09-24).
Each of those items lands with its render-lab fixture, never ahead of
what the canvas draws.

### Collaboration track (RFC-0001)

The project owner wants CRDT collaboration (2026-09-24).
[RFC-0001](rfc/0001-crdt-collaboration.md) is the design: offline-first,
the file stays the document of record, per-user undo, the library chosen
by measurement. It runs alongside Phase 1 and is gated by it, because a
CRDT replicates a *model*. It can't replicate state that only lives in
widgets.

- **Tables** has a GTK-free model, and with the cell-style model it holds
  everything a sheet shows. That unblocks RFC Phase 1, the library
  measurement spike (Automerge vs Loro vs yrs on a recorded Tables
  session), and then RFC Phase 2: Tables, session-scoped, LAN or relay,
  off by default.
- **Decks** needs the shape-style model first: objects must carry their
  own style before two people can edit it.
- **Letters** needs the page-layout engine's GTK-free document model
  (RFC Phase 0). The single `GtkTextView` is the blocker the RFC names.

Status (2026-09-25): the spike (RFC Phase 1) is done, and the library is
**Loro**, behind a `collab` feature that is off by default. A delete beats a
concurrent move, and only inputs are replicated. The decisions are recorded
in [RFC-0001](rfc/0001-crdt-collaboration.md). Letters' Phase 0 model has
shipped. Tables' edit ops, which are the Phase 2 prerequisite, are in progress.

### Phase 2: Close the gap to LibreOffice, feature by feature

Work in order of the most-used feature among red and amber fixtures. Each
PR moves named fixtures to green with a before/after report. Rough order:

1. Letters: fonts and sizes → paragraph spacing and indents → lists →
   headings → tables → images → headers/footers → pagination and page
   breaks → footnotes → columns.
2. Decks: title and body placeholders → shape fills and strokes → text
   colours and fonts → images → backgrounds and themes → tables.
3. Tables: alignment and fonts → fills → number formats (already partly
   done) → merges → borders → wrap → frozen panes → charts → conditional
   formatting.

Exit: every single-feature fixture is green at Tier A and B, and the tiers
agree.

### Phase 3: Real documents

- Build a real-world corpus: about 30 per app, with licences recorded, from
  the LibreOffice template gallery, public-sector forms, open-data
  spreadsheets and conference decks.
- Score each whole document by average SSIM, word-found rate and word
  displacement. Set budgets and ratchet them.
- Add **editing journeys with a render check**: type a paragraph, apply
  bold, add a bullet, save, reopen, then compare the screen to LibreOffice
  opening the saved file. This closes the loop between the editor and the
  file.

Exit: Tier C nightly is green on the real corpus with budgets met.

### Phase 4: Earn "usable"

Only after Phase 3 does the README table move an app to "usable for
everyday documents". The claim is backed by the scorecard, the Tier C VM
run on the release candidate, and a documented set of known gaps.

## What stops until Phase 1 exits

The open "strategist" issues (WASM plugins, CRDT sync, extension
marketplaces, enterprise dconf, suite-convert variants): #764 through #869,
more than 60 near-duplicates. They are out of scope until the editors
render documents correctly. They should be closed or consolidated into one
tracking issue labelled `later`. New feature work that isn't in Phase 1–2
waits too. The exceptions are security fixes and data-loss bugs.

## Running it

```bash
podman build -t gui-test   tests/gui/container
podman build -t render-lab tools/render-lab
tools/render-lab/run.sh                      # all apps, all available tiers
tools/render-lab/run.sh --app decks --tier A # one app, one tier
xdg-open render-lab-out/report.html
```

Outside the container you need LibreOffice, poppler-utils, tesseract, the
fonts listed in the Containerfile, and Python with Pillow, numpy,
python-docx, openpyxl and python-pptx.
