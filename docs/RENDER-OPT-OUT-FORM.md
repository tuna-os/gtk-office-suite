# Opt Out Form: floating-image clearance reduction

Issue [#1376](https://github.com/tuna-os/gtk-office-suite/issues/1376)
tracks the published accessible opt-out form. This is its first reduction;
the real document's green/green acceptance criteria remain open.

## Observed mismatch

In the `render-real-letters` artifact of
[run 41190814907](https://github.com/tuna-os/gtk-office-suite/actions/runs/41190814907),
compare `letters/real-opt-out-form-accessible-version/lo-1.png` with
`A-1.png`. Writer's first title line ends with **Armed**, while Letters'
ends with **UK**. The second lines consequently differ too. The labels and
answer boxes farther down the page also drift vertically; page 2 starts
Part 2 lower in Letters. These are separate observations, not evidence
that changing the title alone fixes the whole form.

The source ODT's opening image uses `style:wrap="parallel"`, with a
paragraph anchor and a width of `1.76042in`. The title follows several
empty paragraphs and is right aligned. Its graphic style specifies no
clearance. Letters' `shape_beside_floats` always reserves `WRAP_GAP_PT = 9`
on either side of an object. This is a candidate contributor to the
shorter first title line; contour wrapping and the title's leading spaces
also need checking against Writer before attributing the entire mismatch
to that constant.

## Floating-image-gap

`tools/render-lab/fixtures.py` now generates
`letters/floating-image-gap.odt`, an independent small ODT with:

- A 144 × 72 pt floating test image at the left text margin.
- Parallel rectangular wrapping and explicitly zero graphic margins.
- Three short paragraphs with 24 pt line heights beside the image.
- A fourth paragraph below it, at the left margin.

There are no headings, tables, logos, hyperlinks, inherited styles or
contours. Short paragraphs avoid conflating clearance with widening a
single paragraph after it passes the bottom of a float. The generated
package includes its ODF media type and manifest and uses the lab's own
four-colour image, not the Ministry of Defence logo.

At a 72 pt page margin, the image ends at x = 216 pt. The first three
lines should start at that edge when clearance is zero; the fourth should
start at x = 72 pt. A local probe using the production ODT reader and
`layout::pango::Typeset::draw_page` on `0f483d81` measured line origins:

| Line | x (pt) | top (pt) |
|---|---:|---:|
| First | 225 | 72 |
| Second | 225 | 96 |
| Third | 225 | 120 |
| Below | 72 | 144 |

The native screenshot was inspected: the extra gap is visible, and the
fourth paragraph returns to the margin. This establishes the native side
of the reduction, not a LibreOffice comparison or a CI verdict.

## Reproduce and finish

Generate with `python3 tools/render-lab/fixtures.py render-lab-out/fixtures`,
then run `tools/render-lab/run.sh --app letters` and inspect
`letters/floating-image-gap` in `render-lab-out/report.html`.
The regular render-parity issue synchronizer can track this fixture
separately if CI grades it non-green.

The local host has no container engine or GTK application build. Its
isolated LibreOffice installation fails at startup with a UNO
`DeploymentException`, so the full lab and Writer reference have not been
run locally. No baseline verdict is inferred from the native probe.

Next, verify the requested clearance against Writer, preserve imported
object clearances in the model/layout if confirmed, and remeasure the full
real corpus. `baseline-real.json` must be updated from that measurement
only when it improves; the existing amber/amber entry remains unchanged
in this reduction PR. The later paragraph/answer-box drift and Part 2
pagination must still be resolved before #1376 can close.
