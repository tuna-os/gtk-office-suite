# Annex A annual report: reduction notes

Issue: [#1378](https://github.com/tuna-os/gtk-office-suite/issues/1378).
Evidence: the `render-real-letters` artifact of
[main run 41190814907](https://github.com/tuna-os/gtk-office-suite/actions/runs/41190814907),
revision `249b89695c9611d75d19778591a2ba326dd8d147`.

## Underline position

Compare `letters/real-annex-a-annual-report-form/A-2.png` with `lo-2.png`,
around the “PART 2: Key information” heading. Letters' underline touches
the bottom of the capitals; Writer draws it lower. The same difference
appears in the PART 3, PART 4 and PART 6 headings and the underlined table
headings on page 5. This is visible independently of OCR's interpretation
of those words.

`letters/underline-position` in `tools/render-lab/fixtures.py` isolates
bold, single-underlined Liberation Sans at 12, 18 and 28 pt. The installed
Arial-compatible face avoids measuring missing-font substitution. The
short, identical text at each size fits on one line, includes descenders,
and contains no tables, heading styles, page breaks or other decorations.
The existing `letters/char-emphasis` covers regular Serif underline and
does not isolate this combination.

Generate and compare it through the usual lab:

```sh
tools/render-lab/run.sh --app letters
# In render-lab-out/report.html, inspect letters/underline-position.
```

The shared Pango shaper currently supplies `pango::Underline::Single`
in `letters-core/src/layout/pango.rs`. Both paragraph and table text use
that shaper. A follow-up should compare underline position and thickness
with Writer across the fixture's sizes, including screen/PDF agreement;
moving text or changing OCR thresholds would not fix the decoration.

## Pagination still needs a separate reduction

On page 5, Letters also draws “Please refer to your lease for the
consequences of non-payment”; Writer starts page 6 with that sentence.
The remaining text on Letters' page 6 is consequently about one line
higher. The table geometry and text on the preceding pages are otherwise
closely aligned. The original paragraph has neither a direct page-break
property nor a direct keep-with-next property, so the screenshots alone
do not establish which spacing or pagination rule causes the difference.

Adding the underline fixture does not establish a new green grade for
Annex A. Keep #1378 open and retain its amber/green real baseline until
the rendering fixes are measured against the full real Letters corpus.
Update `tools/render-lab/baseline-real.json` only from that measured run,
with no other real document regressing.
