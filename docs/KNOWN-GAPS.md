# Known gaps

Status: **draft for Phase 4** of [RENDER-PARITY-ROADMAP.md](RENDER-PARITY-ROADMAP.md).
Phase 4 moves an app to "usable for everyday documents" only with a
documented set of known gaps beside the scorecard and the Tier C run on the
release candidate. This is that set, kept current as gaps are found and
closed. It does not claim any app is usable yet: Phase 3 (Tier C green on
the real corpus) has not exited.

Each entry says what differs from LibreOffice (the render lab's reference),
how it shows, and where it is tracked. "Open PR" means a fix is written and
waiting to merge. "Fixed on main" means it has merged but no Tier C run on
a release has confirmed it yet; the row goes once one has.

## Letters

| Gap | How it shows | Tracked |
|---|---|---|
| Table cell lists ignore the indent the paragraph declares | Numbered/bulleted items in a table cell sit about 25 px right of Writer's (transparency-information, sma-positive-result) | Fixed on main (#2409: cell lists resolve their declared indent); open PR #2410 keeps them on save |
| Word bullet glyphs other than the default dot (`-`, `o`, Symbol/Wingdings) | Every bullet level draws `•` | Fixed on main (#2374) |
| Legacy Word form checkboxes (`FORMCHECKBOX`) | No box drawn | Fixed on main (#2372) |
| A ruled table's top rule takes no height | Text below ruled tables drifts up by the rule width; a full page can gain a line Writer pushes to the next page (annex-a page 5) | Fixed on main (#2382) |
| ODF links whose span names a character style | Drawn in the span style's colour (Word's Hyperlink blue) where Writer draws its link navy | Fixed on main (#2383) |
| Content controls (SDT) placeholders inside table cells | Placeholder text in cells not shown (tier-2-3 notification) | Open PR #2394 (a control around a cell or row keeps it) |
| EMF/WMF images | Not drawn | Not started |
| ODT frames, page-break spacing and one-sided margins | A frame's line too short; space above lost after a page break; a style giving one margin loses the other | Fixed on main (#2371, #2375, #2377) |
| Numbered items from 10 on | A wider label ("10.") measured as "1.", so the text did not tab on past it | Open PR #2412 |
| Small drift on long forms | Up to ~4 pt by mid-page on crime-supervisor-declaration-form, one paragraph wrapping a word differently | Not yet root-caused |

## Tables

| Gap | How it shows | Tracked |
|---|---|---|
| Sheet pictures (logos) | Not drawn; rows under a form's logo look empty | Fixed on main (#2380) |
| Sheet pictures on save | Pictures are dropped on save (the package scan warns) | Fixed on main (#2380: saved at their cell, offset and size; shapes still dropped) |
| Formulas referring to another workbook | Shown as `#REF!` instead of the cached value | Fixed on main (#2381: loads the cached value; the link itself is not kept, as the package scan warns) |
| Hidden rows/columns in ODS (`collapse`/`filter`) | Shown | Fixed on main (#2368) |
| Fonts the Flatpak doesn't have (Aptos, Tahoma, Verdana, Segoe UI, Yu Gothic, ...) | The Flatpak bundles no fonts. For a face the GNOME runtime lacks, fontconfig there falls back differently from the Ubuntu reference container, which sends Verdana and Tahoma to DejaVu Sans. Text then runs wider or narrower than LibreOffice's, so lines and columns wrap differently. Aptos Narrow in draft-schedule-4-complaint-log falls back to a much wider face (Tier A green, Tier C amber at ssim 0.60). Seven Letters documents are green in Tiers A and B and amber in Tier C, set in Tahoma, Verdana, Segoe UI, Yu Gothic or Humnst777 | Not started: needs a decision on which fonts or fontconfig aliases the Flatpak ships, so its fallbacks match what users of those documents expect |
| Sheet view zoom (`zoomScale`) | Opens at 100% where Excel opens at the file's zoom | Not started |
| ODS conditional number formats (`style:map` by sign) | Negatives as `-2` and zero as `0` where Calc shows `(2)` and `-` (hmt-annual-report core tables) | Fixed on main (#2390) |
| Merged blocks bordered cell by cell; fills on half pixels | Inner borders drawn through merged text; grey seams between filled rows after a fractional-height row (ewco-claim-form) | Fixed on main (#2391) |
| Rich text in a cell (xlsx runs) | Every run in the cell's one font: a bold title's small italic version line, bold labels in a notes box | Fixed on main for xlsx (#2392: read, drawn, saved); ODS `text:span` styles not read |
| Hyperlinked cells | Drawn in the cell style's blue where Calc draws a cell's link navy (otsi-annual-review, record-of-non-lethal-actions) | Open PR to port #2393 to main: it merged into its stacked base after that branch had merged, so main never got it |

Render-lab note: the Tables `scale` metric compared our grid with a
printout shrunk by the file's print scale; open PR #2384 compares against
the cells at their own size.

## Decks

| Gap | How it shows | Tracked |
|---|---|---|
| Explicit table cell borders (`a:lnL`/`lnT`/`lnR`/`lnB`) | Not drawn; only the table style's rules show | Fixed on main (#2401: edges, no-fill and the cell's typeface); #2365 is superseded |
| Chart titles | Not drawn | Fixed on main (#2379); open PR #2363 scales the title's and the chart's text size with a slide not 13.33 inches wide |
| Theme-coloured, horizontal and percent-labelled charts | Bars in the accent colour, columns where the file has bars, labels without % | Open PR #2363 |
| Paragraph line spacing (`a:lnSpc`, ODP `fo:line-height`) | Lines at default spacing | Open PR #2353 |
| A deck opened with no session bus | Decks aborts (Mandelbrot watch) | Fixed on main (#2373) |
| Text columns (`numCol`) | One column | Open PR #2366 |
| Mirrored pictures lose the mirror when cropped, and in export | Picture unmirrored | Fixed on main (#2367) |
| All caps / small caps (`cap`) | Text drawn as typed | Fixed on main (#2378) |
| Shrink-to-fit (`normAutofit`) and trailing line breaks | A break that ends a paragraph took no line, so centred titles ending in `<a:br/>` sat half a line low; the fit scaled the font alone where Impress closes the line spacing to 90%/80% first | Fixed on main (#2388: Impress's own search; whole-point rounding of shrunk sizes not repeated) |
| Table style headers and first columns | Header rows drawn bold in every style, and a marked first column painted as the header, where Impress bolds only Light Style 1's and paints the first column as the header only in the medium styles | Fixed on main (#2387) |
| Bold from the deck's own table styles (`ppt/tableStyles.xml`) | A custom style's header and first-column weight ignored | Fixed on main (#2405) |
| Run sizes on a deck not 10 inches wide | Rounded to half points after scaling: a 19 pt run on a 13.33-inch deck drew at 14.5 pt, not 14.25, and saved back as 19.33 pt | Fixed on main (#2407) |
| ODP flip and caps | Not read from ODP | Not started |

## Not gaps

Differences the render lab reports that are not rendering bugs, so they are
not listed above:

- Fonts that differ between the Flatpak runtime and the reference container
  shift OCR word positions by a point or two.
- xlsx column widths follow Excel, not Calc. Tables draws `<col width="w">`
  at Excel's 7 px per character (Calibri 11 or Arial 10): widths 10, 20 and
  40 are 70, 140 and 280 px. LibreOffice 24.2 draws them at the default
  font's unrounded digit width, about 7.4 px: 74, 148 and 296 px (measured
  on a ruled test workbook). A wide column can therefore wrap a line Calc
  keeps whole (competition-in-uk-electricity's 159-character column A wraps
  "markets." onto a third line). Matching Calc would put every xlsx column
  about 6% off Excel, so this is kept as a deliberate difference.
- LibreOffice lays text out with hinted glyph advances at its render size,
  so Liberation Sans (Arial) runs about 3% off ours at small sizes. A line
  near its box's width can wrap one word differently (pcm-draft-hcm-data
  slide 3: "AI | Steering" where LibreOffice keeps "AI Steering"), which the
  lab counts as a lost line. The sizes and box widths match; only the
  advances differ.
- A Tier C capture is a window, not a page: Tables shows the grid on screen
  where LibreOffice prints a page (`compare.py` `window_to_page`).
