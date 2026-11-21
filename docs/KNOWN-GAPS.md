# Known gaps

Status: **draft for Phase 4** of [RENDER-PARITY-ROADMAP.md](RENDER-PARITY-ROADMAP.md).
Phase 4 moves an app to "usable for everyday documents" only with a
documented set of known gaps beside the scorecard and the Tier C run on the
release candidate. This is that set, kept current as gaps are found and
closed. It does not claim any app is usable yet: Phase 3 (Tier C green on
the real corpus) has not exited.

Each entry says what differs from LibreOffice (the render lab's reference),
how it shows, and where it is tracked. "Open PR" means a fix is written and
waiting to merge; the gap stays listed until it has.

## Letters

| Gap | How it shows | Tracked |
|---|---|---|
| Table cell lists ignore the indent the paragraph declares | Numbered/bulleted items in a table cell sit about 25 px right of Writer's (transparency-information, sma-positive-result) | Open PR #2376 |
| Word bullet glyphs other than the default dot (`-`, `o`, Symbol/Wingdings) | Every bullet level draws `•` | Open PR #2374 |
| Legacy Word form checkboxes (`FORMCHECKBOX`) | No box drawn | Open PR #2372 |
| A ruled table's top rule takes no height | Text below ruled tables drifts up by the rule width; a full page can gain a line Writer pushes to the next page (annex-a page 5) | Open PR #2382 |
| ODF links whose span names a character style | Drawn in the span style's colour (Word's Hyperlink blue) where Writer draws its link navy | Open PR #2383 |
| Content controls (SDT) placeholders inside table cells | Placeholder text in cells not shown (tier-2-3 notification) | Needs cell-level SDT support in the DOCX reader |
| EMF/WMF images | Not drawn | Not started |
| ODT frames, page-break spacing and one-sided margins | A frame's line too short; space above lost after a page break; a style giving one margin loses the other | Open PRs #2371, #2375, #2377 |
| Small drift on long forms | Up to ~4 pt by mid-page on crime-supervisor-declaration-form, one paragraph wrapping a word differently | Not yet root-caused |

## Tables

| Gap | How it shows | Tracked |
|---|---|---|
| Sheet pictures (logos) | Not drawn; rows under a form's logo look empty | Open PR #2380 |
| Sheet pictures on save | Pictures are dropped on save (the package scan warns) | Not started |
| Formulas referring to another workbook | Shown as `#REF!` instead of the cached value | Open PR #2381 (loads the cached value; the link itself is not kept, as the package scan warns) |
| Hidden rows/columns in ODS (`collapse`/`filter`) | Shown | Open PR #2368 |
| Sheet view zoom (`zoomScale`) | Opens at 100% where Excel opens at the file's zoom | Not started |

Render-lab note: the Tables `scale` metric compared our grid with a
printout shrunk by the file's print scale; open PR #2384 compares against
the cells at their own size.

## Decks

| Gap | How it shows | Tracked |
|---|---|---|
| Explicit table cell borders (`a:lnL`/`lnT`/`lnR`/`lnB`) | Not drawn; only the table style's rules show | Open PR #2365 |
| Chart titles | Not drawn | Open PR #2379 |
| Theme-coloured, horizontal and percent-labelled charts | Bars in the accent colour, columns where the file has bars, labels without % | Open PR #2363 |
| Paragraph line spacing (`a:lnSpc`, ODP `fo:line-height`) | Lines at default spacing | Open PR #2353 |
| A deck opened with no session bus | Decks aborts (Mandelbrot watch) | Open PR #2373 |
| Text columns (`numCol`) | One column | Open PR #2366 |
| Mirrored pictures lose the mirror when cropped, and in export | Picture unmirrored | Open PR #2367 |
| All caps / small caps (`cap`) | Text drawn as typed | Open PR #2378 |
| Shrink-to-fit (`normAutofit`) and trailing line breaks | A break that ends a paragraph took no line, so centred titles ending in `<a:br/>` sat half a line low; the fit scaled the font alone where Impress closes the line spacing to 90%/80% first | Open PR #2388 (Impress's own search; whole-point rounding of shrunk sizes not repeated) |
| Table style headers and first columns | Header rows drawn bold in every style, and a marked first column painted as the header, where Impress bolds only Light Style 1's and paints the first column as the header only in the medium styles | Open PR #2387 |
| ODP flip and caps | Not read from ODP | Not started |

## Not gaps

Differences the render lab reports that are not rendering bugs, so they are
not listed above:

- Fonts that differ between the Flatpak runtime and the reference container
  shift OCR word positions by a point or two.
- A Tier C capture is a window, not a page: Tables shows the grid on screen
  where LibreOffice prints a page (`compare.py` `window_to_page`).
