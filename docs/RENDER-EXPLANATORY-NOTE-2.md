# Explanatory note: paragraph spacing at a page-master switch

The first reduction for [#1377](https://github.com/tuna-os/gtk-office-suite/issues/1377)
is `letters/odt-page-break-spacing` in `tools/render-lab/fixtures.py`.
It isolates paragraph space before at an explicit page break and a switch
between two ODT page masters with identical geometry. There are no lists,
headers, footers, fields or copied government text in the fixture.

## Evidence from the real document

In the issue's [render report](https://github.com/tuna-os/gtk-office-suite/actions/runs/41190814907),
compare `letters/real-explanatory-note-2/A-3.png` with `lo-3.png`:

- Letters puts “Clause 2: Section 1: consequential amendments” about 24 pt
  above Writer's position. The body remains displaced by that amount.
- Letters consequently fits “Background note” and part of its first
  paragraph on page 3. Writer starts that heading and paragraph on page 4.
- Page 1's body positions agree much more closely. A global margin or font
  adjustment would change the pages that already agree.

In the source ODT, the clause 2 heading uses automatic paragraph style
`P52`, which names page master `MP1`, sets `fo:break-before="page"`,
`fo:margin-top="0.3333in"` (24 pt), and `fo:line-height="115%"`.
The preceding section uses `MP0`. The reduced fixture retains the master
switch, hard break and space before, with a simpler font and page size.

## Reduced result and next step

A local probe using `odt::read` and the production `layout::pango::Typeset`
on the generated fixture confirms:

| Paragraph | Parsed space before | Parsed page break | Line top |
|---|---:|---|---:|
| First page | 24 pt | false | 96 pt |
| Explicit break, same master | 24 pt | true | 72 pt |
| New master | 24 pt | true | 72 pt |

All pages have a 72 pt top margin. The reader preserves the space; layout
removes it after the breaks. `place_paragraph` in
`letters-core/src/layout/mod.rs` applies space before at the document start,
but suppresses it at subsequent page starts. The fixture lets the synthetic
render workflow compare these cases against Writer independently of the
real document. It does not change layout policy: existing DOCX tests
explicitly require suppression after a page break, so a fix needs to
establish the ODT-specific behavior without regressing those tests.

The local container render workflow was unavailable (no container engine);
the isolated LibreOffice installation also failed to start. The fixture's
LibreOffice verdict must therefore be measured in CI. No synthetic or real
baseline verdict is asserted here.

The real report also shows footer differences: later sections lose the
centered sans-serif formatting of “OFFICIAL”, and Writer wraps that word
in its footer. Those are separate from this spacing reduction. The real
issue remains open until both tiers are measured green and the full real
corpus ratchet passes; `baseline-real.json` is unchanged.
