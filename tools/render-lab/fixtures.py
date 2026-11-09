#!/usr/bin/env python3
"""Generate the render-parity fixture corpus: one small document per
visual feature.

One feature per file is the point. When "bold" regresses, the report says
"letters/bold is red", not "the quarterly report is 3% different". Each
fixture is written by python-docx / openpyxl / python-pptx (independent of
our own writers, so our reader is exercised on foreign input) and listed in
manifest.json with the app, the feature id, and a one-line statement of
what must be visible on screen.

Usage: fixtures.py <out-dir>
"""

import json
import os
import re
import sys

from PIL import Image, ImageDraw

OUT = sys.argv[1] if len(sys.argv) > 1 else "render-lab-out/fixtures"
MANIFEST = []

# Fixtures that cannot go green until a Phase 1 architecture item exists
# (docs/RENDER-PARITY-ROADMAP.md). Their issues are labelled blocked so
# agents pick unblocked work first instead of hacking around the gap.
# Empty: every Phase 1 item (Letters page layout, Tables cell-style model,
# Decks shape-style model) has landed, so no fixture waits on one.
NEEDS = {}


# Fixtures whose own PDF export is pixel-compared against LibreOffice's PDF
# of the same file (docs/EXPORT-PARITY-SPEC.md). Opt-in, starting small:
# fixtures with green screenshot baselines, so an export diff measures the
# exporter and not the on-screen renderer. xlsx stays out of scope apart
# from the docs/TABLES-EXPORT-PARITY.md carve-out (spreadsheets are not a
# published rendered artifact).
#
# Tables opt-ins (all three are Tier A and Tier B green, and measure green
# on export too). The earlier NOTE claiming Calc prints black gridlines
# whenever it prints headings was wrong: the values fixture rendered
# through LibreOffice prints headings + numbers with no gridlines,
# matching the hook's show_gridlines=false output. Measured export
# verdicts (run-export.sh --app tables): values SSIM 0.942, merged 0.968,
# frozen 0.908; words 1.0 and lost_lines 0 throughout, scales within the
# 0.10 tables budget — an earlier amber read proved stale on re-measure.
EXPORT = {
    "letters/plain-paragraph",
    "letters/toc",
    "letters/char-emphasis",
    "letters/headings",
    "letters/font-families",
    "letters/font-sizes",
    "letters/text-color",
    "letters/super-subscript",
    "letters/alignment",
    "decks/autofit",
    "decks/title-layout",
    "decks/bullets",
    "decks/text-styles",
    "decks/shapes",
    "decks/image",
    "letters/bullet-list",
    "letters/numbered-list",
    "letters/columns",
    "decks/background",
    "decks/rotation",
    "decks/table",
    "letters/header-footer",
    "letters/page-numbers",
    "letters/highlight",
    "letters/image",
    "letters/indents",
    "letters/line-spacing",
    "letters/paragraph-spacing",
    "letters/page-margins",
    "letters/landscape",
    "letters/page-break",
    "letters/pagination",
    "letters/footnotes",
    "letters/footnote-continued",
    "letters/nested-list",
    "tables/values",
    "tables/merged",
    "tables/frozen",
    "tables/alignment",
    "tables/borders",
    "tables/cell-fonts",
    "tables/fills",
    "tables/col-row-size",
    "tables/chart",
    "tables/chart-area",
    "tables/chart-line",
    "tables/chart-pie",
    "tables/chart-scatter",
    "tables/conditional",
    # Print scale (#1336) and fit to page (#1330). print-scale's screenshot
    # stays red by design: the editor shows the grid at 100%, Calc prints
    # it at 46%. The export prints at the sheet's scale, so its own diff
    # measures the scaling; the cells are the values fixture's, which is
    # green on screen.
    "tables/print-scale",
    "tables/print-fit-page",
}
# NOTE (tables/chart-pie): both-green on screenshots; its export sat in the
# amber band on Tesseract exact-match noise, not an exporter gap — the Q4
# legend glyph OCRs as "ma" in LibreOffice's PDF vs "mq4" in ours (pixels
# equivalent). "ma" is in no text layer, so the scorer now leaves it out
# (compare.mark_printed) and the export is green, as the letters ones
# below are.
#
# NOTE (letters/nested-list #1127, letters/super-subscript #1115): their
# export pixels always agreed with LibreOffice's (SSIM 0.9992 / 1.0000, 4x
# crops identical); word recall alone ambered them, on reads of the
# reference that LibreOffice never printed (a bullet read as "e", H2O read
# as "ho"). The scorer now leaves out such reads, checked against the
# reference's own text layer, where nothing on our side matches them
# (compare.mark_printed, match_words), so they score green on what both
# sides print. No threshold or geometry changed for it.


def add(app, feature, path, expect):
    entry = {
        "app": app,
        "feature": feature,
        "file": os.path.relpath(path, OUT),
        "expect": expect,
        "needs": NEEDS.get(f"{app}/{feature}"),
    }
    if f"{app}/{feature}" in EXPORT:
        entry["export"] = True
    MANIFEST.append(entry)


def test_image(path):
    """A 200x120 image with hard edges and distinct colors, so its position
    and scale are measurable in a screenshot."""
    img = Image.new("RGB", (200, 120), "white")
    d = ImageDraw.Draw(img)
    d.rectangle([0, 0, 99, 59], fill=(220, 40, 40))
    d.rectangle([100, 0, 199, 59], fill=(40, 160, 60))
    d.rectangle([0, 60, 99, 119], fill=(40, 80, 220))
    d.rectangle([100, 60, 199, 119], fill=(240, 200, 30))
    img.save(path)
    return path


# ── Letters (docx) ────────────────────────────────────────────────────────
def letters(img):
    from docx import Document
    from docx.enum.section import WD_ORIENT
    from docx.enum.text import WD_ALIGN_PARAGRAPH, WD_BREAK
    from docx.oxml import parse_xml
    from docx.shared import Inches, Pt, RGBColor

    d = os.path.join(OUT, "letters")
    os.makedirs(d, exist_ok=True)
    LOREM = (
        "The quick brown fox jumps over the lazy dog. Pack my box with five "
        "dozen liquor jugs. How vexingly quick daft zebras jump. "
    )

    def doc():
        document = Document()
        style = document.styles["Normal"]
        style.font.name = "Liberation Serif"
        style.font.size = Pt(12)
        return document

    def save(document, name, expect):
        path = os.path.join(d, f"{name}.docx")
        document.save(path)
        add("letters", name, path, expect)

    x = doc()
    x.add_paragraph(LOREM * 3)
    save(x, "plain-paragraph", "One wrapped paragraph at the page's top-left margin, Liberation Serif 12pt")

    x = doc()
    p = x.add_paragraph()
    p.add_run("Bold ").bold = True
    p.add_run("Italic ").italic = True
    p.add_run("Underline ").underline = True
    r = p.add_run("Strike")
    r.font.strike = True
    save(x, "char-emphasis", "Bold, italic, underlined and struck-through words on one line")

    # Reduced from real-annex-a-annual-report-form (#1378): its bold
    # Arial headings have an underline touching the glyphs in Letters,
    # whereas Writer leaves a gap. char-emphasis uses regular Serif and
    # does not expose the Sans bold underline's position. Use the installed
    # metric-compatible face so this measures decoration, not substitution.
    x = doc()
    for size in (12, 18, 28):
        p = x.add_paragraph()
        r = p.add_run("Key ongoing maintenance")
        r.font.name = "Liberation Sans"
        r.font.size = Pt(size)
        r.bold = True
        r.underline = True
    save(x, "underline-position", "Three bold Sans lines at 12/18/28pt: underline offset and thickness match Writer")

    x = doc()
    for size in (8, 12, 18, 28, 40):
        x.add_paragraph().add_run(f"{size}pt text").font.size = Pt(size)
    save(x, "font-sizes", "Five lines at 8/12/18/28/40pt, visibly growing")

    x = doc()
    for fam in ("Liberation Sans", "Liberation Serif", "Liberation Mono", "Carlito"):
        x.add_paragraph().add_run(f"{fam}: {LOREM[:40]}").font.name = fam
    save(x, "font-families", "Four lines, each in a different font family")

    x = doc()
    p = x.add_paragraph()
    for name, rgb in (("Red ", (200, 0, 0)), ("Green ", (0, 140, 0)), ("Blue", (0, 0, 200))):
        p.add_run(name).font.color.rgb = RGBColor(*rgb)
    save(x, "text-color", "Words in red, green and blue")

    x = doc()
    p = x.add_paragraph("Read the ")
    # A real w:hyperlink, in Word's Hyperlink character look: blue,
    # underlined.
    from docx.opc.constants import RELATIONSHIP_TYPE as RT
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn
    rid = x.part.relate_to("https://example.org/guide", RT.HYPERLINK, is_external=True)
    link = OxmlElement("w:hyperlink")
    link.set(qn("r:id"), rid)
    run = OxmlElement("w:r")
    rpr = OxmlElement("w:rPr")
    color = OxmlElement("w:color")
    color.set(qn("w:val"), "0563C1")
    underline = OxmlElement("w:u")
    underline.set(qn("w:val"), "single")
    rpr.append(color)
    rpr.append(underline)
    run.append(rpr)
    text = OxmlElement("w:t")
    text.text = "project guide"
    run.append(text)
    link.append(run)
    p._p.append(link)
    p.add_run(" before you start.")
    save(x, "hyperlink", "One sentence whose words 'project guide' are a blue, underlined link")

    # Reduced from real-social-housing-sales-data-dictionary (#1367):
    # slashes must not let a URL prefix fit beside the preceding prose.
    # Writer moves it to the next line and wraps the path at hyphens.
    x = doc()
    x.sections[0].page_width = Inches(4.9527)
    x.sections[0].page_height = Inches(6)
    x.sections[0].left_margin = x.sections[0].right_margin = Inches(1.075)
    x.styles["Normal"].font.name = "Calibri"
    x.styles["Normal"].font.size = Pt(11)
    url = "https://www.gov.uk/guidance/housing-statistics-and-england-housing-survey-glossary/a-to-z"
    p = x.add_paragraph("glossary ")
    link = OxmlElement("w:hyperlink")
    link.set(qn("r:id"), p.part.relate_to(url, RT.HYPERLINK, is_external=True))
    run = OxmlElement("w:r")
    props = OxmlElement("w:rPr")
    underline = OxmlElement("w:u")
    underline.set(qn("w:val"), "single")
    props.append(underline)
    run.append(props)
    text = OxmlElement("w:t")
    text.text = url
    run.append(text)
    link.append(run)
    p._p.append(link)
    x.add_paragraph("glossary " + url)
    save(x, "url-path-wrapping", "In a narrow column, URL paths stay together at slashes and wrap at hyphens, with or without a hyperlink")

    x = doc()
    from docx.enum.style import WD_STYLE_TYPE
    pre = x.styles.add_style("HTML Preformatted", WD_STYLE_TYPE.PARAGRAPH)
    pre.font.name, pre.font.size = "Liberation Mono", Pt(10)
    pre.paragraph_format.space_after = Pt(0)
    x.add_paragraph("A code block follows.")
    for line in ("fn main() {", '    println!("hello");', "}"):
        x.add_paragraph(line, style=pre)
    x.add_paragraph("And prose again.")
    save(x, "code-block", "Three lines of code in a 10pt monospace font between two prose lines")

    x = doc()
    x.add_paragraph("Quarterly Report", style="Title")
    x.add_paragraph("Third quarter, 2026", style="Subtitle")
    x.add_paragraph(LOREM)
    save(x, "named-styles", "A large Title, a smaller Subtitle under it, then body text")

    x = doc()
    x.add_paragraph(LOREM)
    x.add_paragraph("Simplicity is prerequisite for reliability.", style="Quote")
    x.add_paragraph(LOREM)
    save(x, "block-quote", "Body text, an italic indented quotation, body text")

    x = doc()
    x.add_heading("Heading 1", level=1)
    x.add_heading("Heading 2", level=2)
    x.add_heading("Heading 3", level=3)
    x.add_paragraph(LOREM)
    save(x, "headings", "Three heading levels with distinct size/weight/colour and spacing, then body text")

    x = doc()
    for align in (WD_ALIGN_PARAGRAPH.LEFT, WD_ALIGN_PARAGRAPH.CENTER, WD_ALIGN_PARAGRAPH.RIGHT, WD_ALIGN_PARAGRAPH.JUSTIFY):
        x.add_paragraph(LOREM * 2).alignment = align
    save(x, "alignment", "Four paragraphs: left, centred, right, justified")

    x = doc()
    for spacing in (1.0, 1.5, 2.0):
        x.add_paragraph(LOREM * 2).paragraph_format.line_spacing = spacing
    save(x, "line-spacing", "Three paragraphs with single, 1.5 and double line spacing")

    x = doc()
    for before in (0, 24, 48):
        p = x.add_paragraph(f"Space before {before}pt. " + LOREM)
        p.paragraph_format.space_before = Pt(before)
    save(x, "paragraph-spacing", "Growing vertical gaps (0/24/48pt) above each paragraph")

    x = doc()
    p = x.add_paragraph(LOREM * 2)
    p.paragraph_format.first_line_indent = Inches(0.5)
    p = x.add_paragraph(LOREM * 2)
    p.paragraph_format.left_indent = Inches(1)
    save(x, "indents", "First paragraph has a 0.5in first-line indent; second is indented 1in on the left")

    x = doc()
    # 22 fillers leave room for a few lines but not the ten-line kept
    # paragraph, so keeping together must move it whole; without the
    # flag orphan/widow control would split it across the break.
    for n in range(22):
        x.add_paragraph(f"Filler line {n + 1}")
    p = x.add_paragraph(("Kept together. " + LOREM) * 5)
    p.paragraph_format.keep_together = True
    p = x.add_paragraph(("No widow control. " + LOREM) * 3)
    p.paragraph_format.widow_control = False
    save(x, "keep-widow", "A ten-line paragraph kept together moves whole past the page break; the next allows widowed lines")

    x = doc()
    p = x.add_paragraph(LOREM * 2)
    p.paragraph_format.line_spacing = Pt(28)
    p = x.add_paragraph(LOREM * 2)
    # python-docx spells a Length as an exact rule; atLeast needs raw XML.
    pPr = p._p.get_or_add_pPr()
    spacing = OxmlElement("w:spacing")
    spacing.set(qn("w:line"), "560")
    spacing.set(qn("w:lineRule"), "atLeast")
    pPr.append(spacing)
    p = x.add_paragraph(LOREM * 2)
    p.paragraph_format.line_spacing = 1.5
    save(x, "exact-line-spacing", "Exact 28pt, minimum 28pt and 1.5-spaced paragraphs")

    x = doc()
    from docx.enum.text import WD_TAB_ALIGNMENT, WD_TAB_LEADER
    p = x.add_paragraph()
    p.add_run("Chapter 1\t1")
    p.paragraph_format.tab_stops.add_tab_stop(Inches(6), WD_TAB_ALIGNMENT.RIGHT, WD_TAB_LEADER.DOTS)
    p = x.add_paragraph()
    p.add_run("Total\t12.99")
    p.paragraph_format.tab_stops.add_tab_stop(Inches(3), WD_TAB_ALIGNMENT.DECIMAL, WD_TAB_LEADER.DOTS)
    p = x.add_paragraph()
    p.add_run("Signed\tby hand")
    p.paragraph_format.tab_stops.add_tab_stop(Inches(3), WD_TAB_ALIGNMENT.CENTER, WD_TAB_LEADER.DASHES)
    save(x, "tab-leaders", "Right, decimal and centre tab stops with dot and dash leaders")

    x = doc()

    def shade(paragraph, fill):
        pPr = paragraph._p.get_or_add_pPr()
        shd = OxmlElement("w:shd")
        shd.set(qn("w:val"), "clear")
        shd.set(qn("w:fill"), fill)
        pPr.append(shd)

    def borders(paragraph, edges):
        # edges: side -> (style, eighths-of-a-point, color).
        pPr = paragraph._p.get_or_add_pPr()
        pbdr = OxmlElement("w:pBdr")
        for side, (val, sz, color) in edges.items():
            edge = OxmlElement(f"w:{side}")
            edge.set(qn("w:val"), val)
            edge.set(qn("w:sz"), str(sz))
            edge.set(qn("w:space"), "1")
            edge.set(qn("w:color"), color)
            pbdr.append(edge)
        pPr.append(pbdr)

    p = x.add_paragraph("A shaded paragraph. " + LOREM)
    shade(p, "D9D9D9")
    p = x.add_paragraph("A paragraph with a red rule below. " + LOREM)
    borders(p, {"bottom": ("single", 6, "FF0000")})
    p = x.add_paragraph("A boxed paragraph. " + LOREM)
    borders(p, {"top": ("single", 4, "000000"), "left": ("single", 4, "000000"),
                "bottom": ("single", 4, "000000"), "right": ("single", 4, "000000")})
    p = x.add_paragraph("A paragraph with a dashed blue bar on the left. " + LOREM)
    borders(p, {"left": ("dashed", 12, "0000FF")})
    save(x, "para-shading-borders", "A grey-shaded paragraph, a red rule, a boxed paragraph and a dashed blue left bar")

    x = doc()
    x.add_paragraph("Decomposed accents: cafe\u0301 nai\u0308ve (e plus combining acute, i plus combining diaeresis).")
    x.add_paragraph("Vietnamese stacked marks: tie\u0302\u0301ng Vi\u00ea\u0323t.")
    x.add_paragraph("Precomposed for contrast: café naïve.")
    save(x, "combining-marks", "Combining accents and stacked Vietnamese marks beside precomposed text")

    x = doc()
    for t in ("Apples", "Oranges", "Pears"):
        x.add_paragraph(t, style="List Bullet")
    save(x, "bullet-list", "Three items with real bullet glyphs and hanging indent (not literal '- ')")

    x = doc()
    for t in ("First", "Second", "Third"):
        x.add_paragraph(t, style="List Number")
    save(x, "numbered-list", "Items numbered 1. 2. 3. with hanging indent")

    x = doc()
    x.add_paragraph("Level one", style="List Bullet")
    x.add_paragraph("Level two", style="List Bullet 2")
    x.add_paragraph("Level three", style="List Bullet 3")
    save(x, "nested-list", "Three bullet levels, each further indented")

    # Reduced from real-dcms-business-appointment-rules-between-october (#1364):
    # In bulleted list items where numbering specifies a symbol font (such as
    # Noto Sans Symbols with a black circle bullet '●'), LibreOffice Writer
    # treats the bullet marker as an inline run on the first line, expanding
    # line height to fit the symbol font's large vertical metrics (~24.75pt / 33px for 12pt).
    # Wrapped continuation lines (plain 12pt body text) have normal line height
    # (~14.25pt / 19px). In Letters, paragraph line heights are sized solely from
    # body text runs, ignoring the bullet marker's font metrics, which loses the
    # first-line height expansion and causes cumulative vertical displacement.
    x = doc()
    x.add_paragraph("Introductory paragraph before the bullet list.")
    x.add_paragraph(
        "Transparency about the advice given on individual applications under the "
        "Business Appointment Rules helps to ensure compliance.",
        style="List Bullet",
    )
    x.add_paragraph(
        "Published information should include the full name of the applicant, former "
        "Civil Service role, new employment, and department decision.",
        style="List Bullet",
    )
    x.add_paragraph("Following paragraph after the bullet list verifying subsequent position.")
    for part in x.part.package.parts:
        if "numbering" in part.partname:
            xml = part.blob.decode("utf-8")
            xml_patched = re.sub(
                r'(<w:lvl [^>]*w:ilvl="0"[^>]*>.*?<w:lvlText [^>]*w:val=")[^"]*(".*?<w:rFonts [^>]*w:ascii=")[^"]*("[^>]*w:hAnsi=")[^"]*(")',
                r"\g<1>●\g<2>Noto Sans Symbols\g<3>Noto Sans Symbols\g<4>",
                xml,
                flags=re.S,
            )
            part._element = parse_xml(xml_patched.encode("utf-8"))
    save(x, "bullet-symbol-font-line-height", "A bullet list using Noto Sans Symbols: the symbol font glyph expands first-line height")

    x = doc()
    t = x.add_table(rows=3, cols=3)
    t.style = "Table Grid"
    for r in range(3):
        for c in range(3):
            t.cell(r, c).text = f"R{r + 1}C{c + 1}"
    save(x, "table", "A 3x3 table with visible grid borders and cell text")


    from docx.oxml import parse_xml
    from docx.oxml.ns import nsdecls

    # Reduced from the Crime Supervisor Declaration Form (#1353): a table row
    # with a declared minimum height (w:trHeight) that exceeds the remaining space
    # on the page moves to the next page in LibreOffice Writer, rather than being
    # clamped to the page foot or overflowing into the bottom margin.
    x = doc()
    s = x.sections[0]
    s.top_margin = Inches(1)
    s.bottom_margin = Inches(1)
    s.left_margin = Inches(1)
    s.right_margin = Inches(1)
    t = x.add_table(rows=2, cols=2)
    t.style = "Table Grid"
    t.rows[0].cells[0].paragraphs[0].text = "Page 1 tall row"
    t.rows[0].cells[1].paragraphs[0].text = "Content filling upper portion of page 1"
    tr0_pr = t.rows[0]._tr.get_or_add_trPr()
    tr0_pr.append(parse_xml(f'<w:trHeight {nsdecls("w")} w:val="10800"/>'))
    t.rows[1].cells[0].paragraphs[0].text = "Page 2 row with minimum height"
    t.rows[1].cells[1].paragraphs[0].text = "Short text inside a row with 180pt declared minimum height"
    tr1_pr = t.rows[1]._tr.get_or_add_trPr()
    tr1_pr.append(parse_xml(f'<w:trHeight {nsdecls("w")} w:val="3600"/>'))
    save(
        x,
        "table-row-min-height-page-break",
        "A table where a row with a declared minimum height that exceeds the page's remaining space starts on page 2",
    )

    # Reduced from real-transparency-information-about-outside-appointme (#1358):
    # In table cells containing numbered list items with declared indent
    # (w:ind w:left="405"), LibreOffice Writer aligns continuation lines with
    # the declared text indent (20.25pt / x=406px) and hangs the numeric marker
    # 18pt in front (x=382px). In Letters, table cell paragraphs did not have
    # list_indent_from_declared applied, adding an extra 18pt list indent on top
    # of the declared indent. This shifts wrapped lines 24px to the right, narrowing
    # the wrapping width, forcing lines to wrap onto an additional line, and expanding
    # row height (+18px / +13.5pt).
    x = doc()
    t = x.add_table(rows=2, cols=1)
    t.style = "Table Grid"
    c0 = t.cell(0, 0)
    c0.paragraphs[0].text = "Department’s Decision on Application"
    p1 = c0.add_paragraph(
        "You should not draw on privileged information available to you as a Crown servant.",
        style="List Number",
    )
    p1.paragraph_format.left_indent = Pt(20.25)
    p2 = c0.add_paragraph(
        "For two years from your last paid day of service, you should not become "
        "personally involved in lobbying the UK Government on behalf of Lend Lease directly "
        "or indirectly, or make use of your contacts in Government and/or Crown service to "
        "influence policy or secure business on behalf of Lend Lease.",
        style="List Number",
    )
    p2.paragraph_format.left_indent = Pt(20.25)
    c1 = t.cell(1, 0)
    c1.paragraphs[0].text = "Next row verification text"
    save(
        x,
        "table-cell-numbered-list-indent",
        "A table cell containing numbered list items with custom indent: wrapped lines align at declared indent",
    )
    # Reduced from real-sma-positive-result-call-checklist (#1365):
    # In table cells containing bullet list items, LibreOffice Writer aligns
    # continuation lines with the first line's text indent (bullet hanging
    # in front). In Letters, table cell paragraphs did not have
    # list_indent_from_declared applied, resulting in an extra 18pt indent
    # on wrapped lines and premature line wrapping.
    x = doc()
    t = x.add_table(rows=2, cols=1)
    t.style = "Table Grid"
    c0 = t.cell(0, 0)
    c0.paragraphs[0].text = "Points to cover"
    c0.add_paragraph(
        "We need to see you and your baby as soon as possible to discuss "
        "the screening test results, treatment options and next steps.",
        style="List Bullet",
    )
    c0.add_paragraph(
        "Confirm baby details and provide contact information for the clinical "
        "team with the days and times someone will be available.",
        style="List Bullet",
    )
    c1 = t.cell(1, 0)
    c1.paragraphs[0].text = "Signed: ___________________________"
    save(x, "table-cell-list-indent", "A table cell containing wrapping bullet list items: wrapped lines align with first-line text")

    # Reduced from real-tier-2-3-notification-of-dealing-with-a-non-uk-t (#1359):
    # In WordprocessingML tables, Structured Document Tags (w:sdt / content
    # controls) can wrap individual cells as direct children of w:tr
    # (<w:tr><w:tc>...</w:tc><w:sdt><w:sdtContent><w:tc>...</w:tc></w:sdtContent></w:sdt></w:tr>).
    # In Letters, rdocx iterates only direct w:tc children of w:tr, completely
    # dropping any cell wrapped in w:sdt. This drops cell text ("Click or tap to
    # enter a date.", "Choose an item.") and distorts row cell counts into jagged
    # rows, triggering lost-lines in compare.py.
    from docx.oxml import parse_xml
    from docx.oxml.ns import nsdecls

    x = doc()
    x.add_paragraph("Table with cell-level content controls (w:sdt in w:tr):")
    t = x.add_table(rows=3, cols=1)
    t.style = "Table Grid"

    # Row 0: Col 0 is standard w:tc, Col 1 is w:sdt wrapping w:tc
    t.cell(0, 0).paragraphs[0].text = "Date of activity"
    sdt0 = parse_xml(
        f'<w:sdt {nsdecls("w")}>'
        '<w:sdtPr><w:alias w:val="Date of activity"/></w:sdtPr>'
        '<w:sdtContent>'
        '<w:tc>'
        '<w:tcPr><w:tcW w:w="5000" w:type="dxa"/></w:tcPr>'
        '<w:p><w:r><w:t>Click or tap to enter a date.</w:t></w:r></w:p>'
        '</w:tc>'
        '</w:sdtContent>'
        '</w:sdt>'
    )
    t.rows[0]._tr.append(sdt0)

    # Row 1: Col 0 is standard w:tc, Col 1 is w:sdt wrapping w:tc
    t.cell(1, 0).paragraphs[0].text = "General Licence"
    sdt1 = parse_xml(
        f'<w:sdt {nsdecls("w")}>'
        '<w:sdtPr><w:alias w:val="General Licence"/></w:sdtPr>'
        '<w:sdtContent>'
        '<w:tc>'
        '<w:tcPr><w:tcW w:w="5000" w:type="dxa"/></w:tcPr>'
        '<w:p><w:r><w:t>Choose an item.</w:t></w:r></w:p>'
        '</w:tc>'
        '</w:sdtContent>'
        '</w:sdt>'
    )
    t.rows[1]._tr.append(sdt1)

    # Row 2: Standard 2-column row to verify following row alignment
    t.cell(2, 0).paragraphs[0].text = "Description of activity"
    c2 = parse_xml(
        f'<w:tc {nsdecls("w")}>'
        '<w:tcPr><w:tcW w:w="5000" w:type="dxa"/></w:tcPr>'
        '<w:p><w:r><w:t>Provision of trade financing and maritime insurance</w:t></w:r></w:p>'
        '</w:tc>'
    )
    t.rows[2]._tr.append(c2)

    save(
        x,
        "table-cell-content-control",
        "A table with cell-level content controls (w:sdt in w:tr): content controls preserve cell geometry and text",
    )


    x = doc()
    # Reduced from real-affordable-housing-supply-data-dictionary (#1357):
    # a table row near the bottom of a page whose lines cannot fit in the
    # remaining space moves whole to the next page, rather than splitting
    # to place only cell padding and leaving an empty cell box on page 1.
    # 32 exact 20pt lines fill 640pt of the 648pt body, leaving 8pt: room
    # for "Table Grid"'s zero top padding, not for a 12pt line.
    from docx.enum.text import WD_LINE_SPACING

    section = x.sections[0]
    section.page_height, section.top_margin, section.bottom_margin = Inches(11), Inches(1), Inches(1)
    for i in range(32):
        p = x.add_paragraph(f"Paragraph {i + 1} before the table, one exact 20pt line.")
        p.paragraph_format.line_spacing_rule = WD_LINE_SPACING.EXACTLY
        p.paragraph_format.line_spacing = Pt(20)
        p.paragraph_format.space_before = Pt(0)
        p.paragraph_format.space_after = Pt(0)
    t = x.add_table(rows=2, cols=2)
    t.style = "Table Grid"
    t.cell(0, 0).text = "Row 1 Col 1"
    t.cell(0, 1).text = "Row 1 Col 2"
    t.cell(1, 0).text = "Row 2 Col 1"
    t.cell(1, 1).text = "Row 2 Col 2"
    save(x, "table-row-split", "A table row near the bottom of a page moves whole to the next page when its text lines do not fit")

    x = doc()
    x.add_paragraph("Image below:")
    x.add_picture(img, width=Inches(2))
    save(x, "image", "A 2in-wide four-colour image under the caption")

    x = doc()
    s = x.sections[0]
    s.left_margin = s.right_margin = Inches(2)
    s.top_margin = Inches(2.5)
    x.add_paragraph(LOREM * 4)
    save(x, "page-margins", "Text block starts 2.5in from the top, 2in from each side")

    x = doc()
    s = x.sections[0]
    s.orientation = WD_ORIENT.LANDSCAPE
    s.page_width, s.page_height = s.page_height, s.page_width
    x.add_paragraph(LOREM * 4)
    save(x, "landscape", "The page is wider than it is tall")

    x = doc()
    s = x.sections[0]
    s.header.paragraphs[0].text = "HEADER TEXT"
    s.footer.paragraphs[0].text = "FOOTER TEXT"
    x.add_paragraph(LOREM)
    save(x, "header-footer", "HEADER TEXT at the top of the page, FOOTER TEXT at the bottom")

    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    def field(paragraph, instr, shown):
        """A simple field (PAGE, NUMPAGES) at the end of `paragraph`."""
        fld = OxmlElement("w:fldSimple")
        fld.set(qn("w:instr"), instr)
        r = OxmlElement("w:r")
        t = OxmlElement("w:t")
        t.text = shown
        r.append(t)
        fld.append(r)
        paragraph._p.append(fld)

    x = doc()
    s = x.sections[0]
    hp = s.header.paragraphs[0]
    hp.text = "Page "
    field(hp, "PAGE", "1")
    hp.add_run(" of ")
    field(hp, "NUMPAGES", "3")
    fp = s.footer.paragraphs[0]
    fp.text = "Draft, page "
    field(fp, "PAGE", "1")
    for _ in range(70):
        x.add_paragraph(LOREM)
    save(x, "page-numbers", "Each page's header reads 'Page N of M' and its footer 'Draft, page N', numbered from the layout")

    x = doc()
    # python-docx has no footnote API: the footnotes part is written by
    # hand, with Word's two separator notes and two real ones at 10pt.
    from docx.opc.constants import RELATIONSHIP_TYPE as RT
    from docx.opc.packuri import PackURI
    from docx.opc.part import Part

    W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"

    def note(n, text):
        return (
            f'<w:footnote w:id="{n}"><w:p><w:r><w:rPr><w:vertAlign w:val="superscript"/><w:sz w:val="20"/></w:rPr>'
            f'<w:footnoteRef/></w:r><w:r><w:rPr><w:sz w:val="20"/></w:rPr><w:t xml:space="preserve"> {text}</w:t></w:r></w:p></w:footnote>'
        )

    def add_notes(document, texts):
        """The footnotes part: Word's two separator notes, then `texts`."""
        notes = (
            f'<w:footnotes xmlns:w="{W}">'
            '<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>'
            '<w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>'
            + "".join(note(i + 1, t) for i, t in enumerate(texts))
            + "</w:footnotes>"
        )
        part = Part(
            PackURI("/word/footnotes.xml"),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml",
            notes.encode(),
            document.part.package,
        )
        document.part.relate_to(part, RT.FOOTNOTES)

    add_notes(x, ["The first footnote, at the foot of the page.", "A second footnote, below the first."])

    def reference(paragraph, n):
        r = paragraph.add_run()
        rpr = OxmlElement("w:rPr")
        va = OxmlElement("w:vertAlign")
        va.set(qn("w:val"), "superscript")
        rpr.append(va)
        r._r.append(rpr)
        ref = OxmlElement("w:footnoteReference")
        ref.set(qn("w:id"), str(n))
        r._r.append(ref)

    p = x.add_paragraph("A sentence with a note.")
    reference(p, 1)
    p.add_run(" " + LOREM)
    p = x.add_paragraph("Another with a second note.")
    reference(p, 2)
    save(x, "footnotes", "Superscript 1 and 2 in the text; the two notes at the foot of the page, 10pt, under a short rule")

    x = doc()
    for i in range(22):
        x.add_paragraph(f"Filler line {i + 1}.")
    add_notes(x, [LOREM * 9])
    p = x.add_paragraph("The line with a long note.")
    reference(p, 1)
    for i in range(10):
        x.add_paragraph(f"After the note {i + 1}.")
    save(x, "footnote-continued", "A long footnote starts at the foot of page 1 under its reference and continues at the foot of page 2")


    x = doc()
    cols = x.sections[0]._sectPr.find(qn("w:cols"))
    if cols is None:
        cols = OxmlElement("w:cols")
        x.sections[0]._sectPr.append(cols)
    cols.set(qn("w:num"), "2")
    cols.set(qn("w:space"), "720")
    for _ in range(14):
        x.add_paragraph(LOREM * 2)
    save(x, "columns", "Text in two columns half an inch apart, filling the left column before the right")

    # LET-005S: a document of two sections. A portrait part with its own
    # header, then a next-page section break into a landscape part with
    # narrower margins, two columns and a header of its own (unlinked from
    # the first, as Word's "Link to Previous" off writes it).
    from docx.enum.section import WD_SECTION
    x = doc()
    first = x.sections[0]
    first.header.paragraphs[0].text = "PORTRAIT PART"
    for i in range(3):
        x.add_paragraph(f"Portrait paragraph {i + 1}. " + LOREM)
    second = x.add_section(WD_SECTION.NEW_PAGE)
    second.orientation = WD_ORIENT.LANDSCAPE
    second.page_width, second.page_height = first.page_height, first.page_width
    second.left_margin = second.right_margin = Inches(0.75)
    second.top_margin = second.bottom_margin = Inches(0.75)
    second.header.is_linked_to_previous = False
    second.header.paragraphs[0].text = "LANDSCAPE APPENDIX"
    cols = second._sectPr.find(qn("w:cols"))
    if cols is None:
        cols = OxmlElement("w:cols")
        second._sectPr.append(cols)
    cols.set(qn("w:num"), "2")
    cols.set(qn("w:space"), "480")
    for i in range(6):
        x.add_paragraph(f"Landscape paragraph {i + 1}. " + LOREM * 2)
    save(x, "mixed-sections", "Page 1 portrait under 'PORTRAIT PART'; page 2 landscape with 0.75in margins, two columns and 'LANDSCAPE APPENDIX' above them")

    x = doc()
    x.add_paragraph("Page one.")
    x.add_paragraph().add_run().add_break(WD_BREAK.PAGE)
    x.add_paragraph("Page two.")
    save(x, "page-break", "Two separate pages; 'Page two.' starts at the top of the second")

    x = doc()
    for _ in range(60):
        x.add_paragraph(LOREM)
    save(x, "pagination", "Long text flows across several pages with margins on each")

    x = doc()
    p = x.add_paragraph("E = mc")
    p.add_run("2").font.superscript = True
    p.add_run(" and H")
    p.add_run("2").font.subscript = True
    p.add_run("O")
    save(x, "super-subscript", "Raised small '2' after mc, lowered small '2' in H2O")

    x = doc()
    p = x.add_paragraph()
    r = p.add_run("Highlighted")
    from docx.enum.text import WD_COLOR_INDEX

    r.font.highlight_color = WD_COLOR_INDEX.YELLOW
    save(x, "highlight", "A word on a yellow background")

    x = doc()
    # A table of contents as Word writes it: a TOC field whose result is
    # entry paragraphs in TOC1/TOC2 styles, each a title, a tab and its
    # page number behind a dotted right tab stop at the text column's edge
    # (the python-docx template is Letter with 1.25in side margins, so the
    # column is 8640 twips wide; a stop past that would overflow into the
    # margin in Word and LibreOffice, as one did at 9360).
    def toc_edge(paragraph, kind=None, instr=None):
        r = paragraph.add_run()
        el = OxmlElement("w:fldChar") if kind is not None else OxmlElement("w:instrText")
        if kind is not None:
            el.set(qn("w:fldCharType"), kind)
        else:
            el.set(qn("xml:space"), "preserve")
            el.text = instr
        r._r.append(el)

    def toc_entry(title, page, level):
        p = x.add_paragraph()
        pPr = p._p.get_or_add_pPr()
        style = OxmlElement("w:pStyle")
        style.set(qn("w:val"), f"TOC{level}")
        pPr.append(style)
        tabs = OxmlElement("w:tabs")
        tab = OxmlElement("w:tab")
        tab.set(qn("w:val"), "right")
        tab.set(qn("w:leader"), "dot")
        tab.set(qn("w:pos"), "8640")
        tabs.append(tab)
        pPr.append(tabs)
        p.add_run(title)
        r = p.add_run()
        r._r.append(OxmlElement("w:tab"))
        p.add_run(str(page))

    # Real Word files define the TOC1/TOC2 entry styles (indented one
    # step per level); without them LibreOffice falls back to Normal and
    # renders every entry unindented, which no Word-authored file does.
    styles = x.styles.element
    for level, indent in ((1, 0), (2, 283)):
        style = OxmlElement("w:style")
        style.set(qn("w:type"), "paragraph")
        style.set(qn("w:styleId"), f"TOC{level}")
        name = OxmlElement("w:name")
        name.set(qn("w:val"), f"toc {level}")
        style.append(name)
        based = OxmlElement("w:basedOn")
        based.set(qn("w:val"), "Normal")
        style.append(based)
        pPr = OxmlElement("w:pPr")
        ind = OxmlElement("w:ind")
        ind.set(qn("w:left"), str(indent))
        pPr.append(ind)
        styles.append(style)
    x.add_paragraph().add_run("Contents").bold = True
    first = x.add_paragraph()
    toc_edge(first, "begin")
    toc_edge(first, instr='TOC \\o "1-3" \\h \\z \\u ')
    toc_edge(first, "separate")
    toc_entry("Introduction", 1, 1)
    toc_entry("Method", 2, 1)
    toc_entry("Details", 2, 2)
    toc_edge(x.add_paragraph(), "end")
    x.add_heading("Introduction", level=1)
    x.add_paragraph(LOREM * 2)
    x.add_paragraph().add_run().add_break(WD_BREAK.PAGE)
    x.add_heading("Method", level=1)
    x.add_paragraph(LOREM * 2)
    x.add_heading("Details", level=2)
    x.add_paragraph(LOREM * 2)
    save(x, "toc", "A table of contents with dot leaders and page numbers 1, 2, 2 above the two pages of headings it lists")


# ── Letters (ODT) ─────────────────────────────────────────────────────────
def letters_floating_image_gap(img):
    """Reduce #1376's title beside a floating logo to the wrap clearance.

    Use ODT, as in the source, and explicitly request zero clearance.
    Three short paragraphs fit alongside the image; a fourth starts below
    it, so widening a paragraph partway down is a separate feature.
    """
    from zipfile import ZIP_STORED, ZipFile

    ns = (
        'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
        'xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" '
        'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
        'xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" '
        'xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" '
        'xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" '
        'xmlns:xlink="http://www.w3.org/1999/xlink" office:version="1.2"'
    )
    styles = f'''<office:document-styles {ns}>
      <office:styles><style:default-style style:family="paragraph">
        <style:paragraph-properties fo:margin-top="0pt" fo:margin-bottom="0pt" fo:line-height="24pt"/>
        <style:text-properties fo:font-family="Liberation Serif" fo:font-size="12pt"/>
      </style:default-style></office:styles>
      <office:automatic-styles><style:page-layout style:name="Page">
        <style:page-layout-properties fo:page-width="8.5in" fo:page-height="11in" fo:margin="1in"/>
      </style:page-layout></office:automatic-styles>
      <office:master-styles><style:master-page style:name="Standard" style:page-layout-name="Page"/></office:master-styles>
    </office:document-styles>'''
    content = f'''<office:document-content {ns}>
      <office:automatic-styles><style:style style:name="Line" style:family="paragraph">
        <style:paragraph-properties fo:margin-top="0pt" fo:margin-bottom="0pt" fo:line-height="24pt"/>
        <style:text-properties fo:font-family="Liberation Serif" fo:font-size="12pt"/>
      </style:style><style:style style:name="Float" style:family="graphic">
        <style:graphic-properties style:wrap="parallel" style:wrap-contour="false"
          style:horizontal-rel="page-content" style:horizontal-pos="from-left"
          style:vertical-rel="paragraph" style:vertical-pos="from-top" fo:margin="0pt"/>
      </style:style></office:automatic-styles>
      <office:body><office:text>
        <text:p text:style-name="Line"><draw:frame draw:style-name="Float" draw:name="Zero clearance"
          text:anchor-type="paragraph" svg:x="0pt" svg:y="0pt" svg:width="144pt" svg:height="72pt"><draw:image xlink:href="Pictures/image.png" xlink:type="simple"/></draw:frame>First line beside the image</text:p>
        <text:p text:style-name="Line">Second line beside the image</text:p>
        <text:p text:style-name="Line">Third line beside the image</text:p>
        <text:p text:style-name="Line">Below the image at the left margin</text:p>
      </office:text></office:body>
    </office:document-content>'''
    path = os.path.join(OUT, "letters", "floating-image-gap.odt")
    with ZipFile(path, "w") as z:
        z.writestr("mimetype", "application/vnd.oasis.opendocument.text", compress_type=ZIP_STORED)
        z.writestr("styles.xml", styles)
        z.writestr("content.xml", content)
        z.write(img, "Pictures/image.png")
        z.writestr("META-INF/manifest.xml", '''<manifest:manifest
          xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">
          <manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/>
          <manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>
          <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
          <manifest:file-entry manifest:full-path="Pictures/image.png" manifest:media-type="image/png"/>
        </manifest:manifest>''')
    add("letters", "floating-image-gap", path,
        "Three lines touch a floating image's right edge (zero clearance); the fourth starts at the left margin")


# ── Tables (xlsx) ─────────────────────────────────────────────────────────
def tables(img=None):
    from openpyxl import Workbook
    from openpyxl.drawing.image import Image as SheetImage
    from openpyxl.drawing.spreadsheet_drawing import AnchorMarker, TwoCellAnchor
    from openpyxl.formatting.rule import CellIsRule
    from openpyxl.styles import Alignment, Border, Font, PatternFill, Side
    from datetime import datetime
    from openpyxl.utils import get_column_letter

    d = os.path.join(OUT, "tables")
    os.makedirs(d, exist_ok=True)

    def book():
        wb = Workbook()
        return wb, wb.active

    def save(wb, name, expect, *, headings=True):
        # Print headings so LibreOffice's PDF shows the same cells as our
        # grid, but not gridlines: they are view furniture, which Calc
        # prints black and screens draw faint, and a printed gridline hides
        # a thin cell border drawn on top of it. capture.py turns Tables'
        # "Show gridlines" off to match, so both sides show the cells'
        # own borders, fills and text.
        for sheet in wb.worksheets:
            sheet.print_options.gridLines = False
            sheet.print_options.headings = headings
        path = os.path.join(d, f"{name}.xlsx")
        wb.save(path)
        add("tables", name, path, expect)

    wb, ws = book()
    for r in range(1, 6):
        for c in range(1, 5):
            ws.cell(r, c, r * 10 + c)
    save(wb, "values", "A 5x4 block of integers, right-aligned in their cells")

    # The SFI26 real document (#1339) starts with a column about 36.7 cm
    # wide, wider than both a printed page and the app viewport, and does
    # not print headings. Reduce that geometry independently of its styles,
    # links and other sheets. In particular, the printed page and screen
    # expose different portions of the column: stretching one to the other
    # in compare.align is not a measurement of the grid's scale.
    wb, ws = book()
    ws.column_dimensions["A"].width = 200
    for i in range(1, 21):
        ws.cell(i, 1, f"Row {i:02d}: a very wide column continues beyond the printed page and beyond the visible grid viewport. "
                      "The remaining text is deliberately long enough to reach both edges, even with a narrow font. "
                      "Each view exposes only part of this line of text.")
    save(wb, "wide-column-no-headings",
         "One extra-wide column of text without printed headings, clipped by the page and viewport", headings=False)

    # Reduced from the missing NHS logo in real-nbs-chis-kpi-submission-template
    # (#1337). Keep its twoCellAnchor/editAs=oneCell picture relationship,
    # without the real workbook's print scaling, merges or rich text.
    wb, ws = book()
    for col in "ABCD":
        ws.column_dimensions[col].width = 14
    for row in range(1, 9):
        ws.row_dimensions[row].height = 18
    ws["A1"] = "Worksheet picture"
    ws["D8"] = "End"
    picture = SheetImage(img or test_image(os.path.join(d, "worksheet-image.png")))
    picture.anchor = TwoCellAnchor(
        editAs="oneCell",
        _from=AnchorMarker(col=1, row=1),
        to=AnchorMarker(col=3, row=6),
    )
    ws.add_image(picture)
    save(
        wb, "worksheet-image",
        "A red/green/blue/yellow four-quadrant picture anchored from B2 to D7, "
        "between the Worksheet picture and End labels",
    )

    wb, ws = book()
    # Reduced from the Planning Inspectorate workbook (#1328): blank rows
    # have no <row> element and must inherit sheetFormatPr's height.
    ws.sheet_format.defaultRowHeight = 14.5
    for row, text in ((1, "Heading"), (4, "Notes"), (7, "Other information"),
                      (11, "Contacts")):
        ws.cell(row, 1, text)
    ws.column_dimensions["A"].width = 28
    ws.row_dimensions[7].height = 36
    save(wb, "default-row-height",
         "14.5pt default rows, including blank rows; row 7 overrides with 36pt")

    # Reduced from Hampshire Public Review: additional postcodes (#1347):
    # a 251-row, 2-column unheaded list of postcodes and premises counts
    # paginates across 6 portrait A4 pages in LibreOffice Calc when headings
    # are disabled (print_options.headings = False), while Tables captures
    # the continuous unpaginated on-screen grid. In compare.py, without
    # printed heading boxes, lo_cell_box(ref) returns None, align() does not
    # crop Tables' header bands (ox=40, oy=20), and candidate page picking
    # selects page 1 only (50 rows), squashing 73 on-screen rows onto 50 rows.
    wb, ws = book()
    ws["A1"] = "Postcode"
    ws["B1"] = "No of Premises Impacted"
    ws.column_dimensions["A"].width = 15
    ws.column_dimensions["B"].width = 25
    sample_postcodes = [
        "SO31 1FD", "PO15 7QR", "PO15 7QS", "PO15 7RP", "SO31 8AS",
        "SO30 0AW", "SO30 2EU", "PO14 4PJ", "RG24 7BB", "GU34 5HN",
    ]
    for r in range(2, 76):
        pc = sample_postcodes[(r - 2) % len(sample_postcodes)]
        ws.cell(r, 1, f"{pc[:4]} {((r * 7) % 900) + 100:03d}")
        ws.cell(r, 2, (r % 5) + 1)
    save(
        wb,
        "tall-unheaded-postcodes",
        "A 75-row, 2-column unheaded list that spans multiple vertical pages in Calc; isolates continuous viewport vs unheaded multipage print",
        headings=False,
    )

    # Reduced from Service reservoir panel engineers: contact list (#1346):
    # a 15-column unheaded contact directory (A through O) spanning 74 cm in width
    # paginates horizontally across multiple portrait A4 pages in LibreOffice Calc
    # when headings are disabled (print_options.headings = False). Tables captures
    # the continuous unpaginated on-screen viewport (columns A through G). In
    # compare.py, without printed heading boxes, lo_cell_box(ref) returns None,
    # align() skips cropping on-screen header bands (ox=40, oy=20), and candidate page
    # picking selects page 1 only (columns A through D), squashing columns A through G
    # onto columns A through D (a 1.45× horizontal squeeze, displacement 79.5pt, SSIM 0.22).
    wb, ws = book()
    ws["A1"] = "Service Reservoir Panel Engineers Contact Directory"
    ws["A1"].font = Font(name="Arial", size=14, bold=True)
    ws["A2"] = "Panel engineers contact directory with assignments and address details"
    ws["A2"].font = Font(name="Arial", size=10, italic=True)
    ws["A3"] = "Columns D through L and O may contain blank contact fields"
    ws["A3"].font = Font(name="Arial", size=9)

    contact_headers = [
        "Surname", "Forename", "Title", "Company name", "Address line 2",
        "Address line 3", "Town", "Post Town", "County", "Post Code",
        "Phone Number", "Mobile Number", "Email Address", "Existing panel",
        "Joint corresponding panel",
    ]
    col_letters = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O"]
    col_widths = [16, 18, 10, 22, 20, 22, 16, 18, 16, 14, 18, 18, 26, 22, 26]
    for c_idx, (header, width) in enumerate(zip(contact_headers, col_widths), start=1):
        cell = ws.cell(5, c_idx, header)
        cell.font = Font(name="Arial", size=11, bold=True)
        col_letter = col_letters[c_idx - 1]
        ws.column_dimensions[col_letter].width = width

    sample_engineers = [
        ("Cavanagh", "Stephen Paul", "Mr", "Binnies UK Ltd", "One City Place", "Queens Road",
         "Chester", "Cheshire", "Cheshire", "CH1 3BQ", "01244 304125", "07795 542996",
         "cavanaghsp@example.org", "Service Reservoirs Panel", "Joint Technical Panel"),
        ("Hawley", "Robin William", "Mr", "Independent Consultant", "10 London Road", "Trent Valley",
         "Lichfield", "Staffordshire", "Staffordshire", "WS14 9EJ", "01543 256618", "07887 896476",
         "robin.hawley@example.org", "Service Reservoirs Panel", "Supervising Panel"),
        ("Morgan", "Claire Louise", "Dr", "Arup Engineering", "4 Pierhead Street", "Capital Waterside",
         "Cardiff", "Cardiff", "Glamorgan", "CF10 4QP", "02920 473727", "07712 345678",
         "claire.morgan@example.org", "Service Reservoirs Panel", "All Reservoirs Panel"),
        ("Thurston", "David George", "Eur Ing", "Stantec UK Ltd", "Buckingham Court", "Kingsmead Business Park",
         "High Wycombe", "High Wycombe", "Buckinghamshire", "HP11 1JU", "01494 526240", "07801 987654",
         "david.thurston@example.org", "Service Reservoirs Panel", "Service Reservoirs Panel"),
    ]
    for r_idx, engineer in enumerate(sample_engineers, start=6):
        for c_idx, val in enumerate(engineer, start=1):
            ws.cell(r_idx, c_idx, val)

    save(
        wb,
        "wide-unheaded-contact-list",
        "A 15-column unheaded contact directory spanning multiple horizontal pages in Calc; isolates continuous viewport vs unheaded horizontal multipage print",
        headings=False,
    )

    # Reduced from Record of non-lethal actions (template) (#1345):
    # a single-column unheaded instruction sheet (Column A) spanning ~26 cm in width
    # and 21 rows totaling >800 pt in height paginates vertically across multiple
    # portrait A4 pages in LibreOffice Calc when headings are disabled
    # (print_options.headings = False). Tables captures the continuous unpaginated
    # on-screen viewport (column A + row/col header bands). In compare.py,
    # without printed heading boxes, lo_cell_box(ref) returns None, align() skips
    # cropping on-screen header bands (ox=40, oy=20), and candidate page picking
    # selects page 1 only (rows 1..17), compressing all 21 rows into the space of
    # 17 rows and 1019 px into 728 px (scale = 1.37×, displacement 82.1pt, SSIM 0.04).
    wb, ws = book()
    instructions_data = [
        ("Record of Non-Lethal Actions Guidance and Instructions", Font(name="Arial", size=14, bold=True), 45),
        ("When to use this template", Font(name="Arial", size=12, bold=True), 32),
        (
            "You can use this template to record observations of problem bird numbers and non-lethal deterrence measures taken before applying for or renewing wildlife protection licences.",
            Font(name="Arial", size=10),
            54,
        ),
        (
            "You do not have to use this template: records may be submitted in another format provided they contain the same statutory fields and dates.",
            Font(name="Arial", size=10),
            32,
        ),
        ("Find out more about recording non-lethal actions and licence conditions online.", Font(name="Arial", size=10, italic=True), 28),
        ("New licence applications - daily record requirements", Font(name="Arial", size=12, bold=True), 36),
        (
            "You must submit a record of non-lethal deterrence covering a minimum of one month if applying to shoot wild birds to reinforce scaring activities.",
            Font(name="Arial", size=10),
            42,
        ),
        ("State the number of birds observed present for each species that you wish to control under the proposed licence.", Font(name="Arial", size=10), 36),
        (
            "Include detailed descriptions of all active non-lethal methods deployed, including shooting to scare, acoustic deterrents, or human disturbance.",
            Font(name="Arial", size=10),
            42,
        ),
        ("Use the second sheet of this workbook 'Daily record' to record active control methods on each date of action.", Font(name="Arial", size=10), 38),
        (
            "Use the fourth sheet of this workbook 'Long-term methods' to record static deterrents (such as netting, lasers, habitat modification, or visual decoys).",
            Font(name="Arial", size=10),
            44,
        ),
        ("Renewing your licence and reporting actions - monthly record", Font(name="Arial", size=12, bold=True), 36),
        (
            "You must submit a completed non-lethal action log if you have taken action under your licence or wish to apply for an annual renewal.",
            Font(name="Arial", size=10),
            42,
        ),
        (
            "Record monthly summaries of active deterrents across the entire licence validity period, stating peak and average bird counts.",
            Font(name="Arial", size=10),
            42,
        ),
        ("Document species-specific reactions to each scaring intervention and note any habituation observed over time.", Font(name="Arial", size=10), 38),
        ("Use the third sheet of this workbook 'Monthly summary' to record summary statistics and licence reference numbers.", Font(name="Arial", size=10), 38),
        ("Static deterrent maintenance logs must accompany monthly activity logs to demonstrate ongoing compliance.", Font(name="Arial", size=10), 42),
        ("Statutory guidance and references", Font(name="Arial", size=12, bold=True), 32),
        ("Review national wildlife management licensing policies and bird control standards before submitting records:", Font(name="Arial", size=10), 32),
        ("Licence classes A06 and A07: Protecting fisheries and angling clubs from fish-eating predators", Font(name="Arial", size=10), 36),
        ("Licence classes A08 and A09: Licences to prevent serious agricultural and horticultural damage", Font(name="Arial", size=10), 36),
    ]

    ws.column_dimensions["A"].width = 125
    for r_idx, (text, font_style, r_height) in enumerate(instructions_data, start=1):
        cell = ws.cell(r_idx, 1, text)
        cell.font = font_style
        ws.row_dimensions[r_idx].height = r_height

    save(
        wb,
        "oversized-unheaded-instructions",
        "A single-column unheaded instruction sheet wider and taller than portrait A4; isolates continuous viewport vs unheaded two-dimensional multipage print",
        headings=False,
    )

    # Reduced from OTSI Annual Review 2025-2026: Tables (#1344):
    # a wide borderless single-column sheet (Column A width ~24 cm / ~896px)
    # with unprinted headings (headings=False) and hyperlinked table contents
    # styled with font color #0563C1. In headless LibreOffice Calc PDF export,
    # borderless text crops to ink width (~412px) while Calc overrides hyperlink
    # colors to #000080. Tables captures the full used extent of Column A (896px)
    # plus header bands (ox=40, oy=20). In compare.py, lo_cell_box(ref) returns
    # None due to missing print headings, align() skips cropping header bands,
    # and resizes 936px onto 412px (scale 1.74×, 2.27× horizontal squash),
    # yielding displacement 12.5pt, SSIM 0.67, words 84%, and colors 0%.
    wb, ws = book()
    ws.column_dimensions["A"].width = 115
    ws.row_dimensions[1].height = 27
    ws.row_dimensions[2].height = 20
    ws.row_dimensions[3].height = 21

    cell1 = ws.cell(1, 1, "OTSI Annual Review 2025/26 Tables")
    cell1.font = Font(name="Arial", size=16, bold=True, color="000000")

    cell2 = ws.cell(2, 1, "This dataset contains nine tables for OTSI's annual review")
    cell2.font = Font(name="Arial", size=12)

    cell3 = ws.cell(3, 1, "Contents")
    cell3.font = Font(name="Arial", size=12, bold=True, color="000000")

    links = [
        ("Cover sheet", "#'Cover sheet'!A1"),
        ("Notes", "#Notes!A1"),
        ("Table A1", "#'Table A1'!A1"),
        ("Table A2", "#'Table A2'!A1"),
        ("Table A3", "#'Table A3'!A1"),
        ("Table A4", "#'Table A4'!A1"),
        ("Table B1", "#'Table B1'!A1"),
        ("Table B2", "#'Table B2'!A1"),
        ("Table B3", "#'Table B3'!A1"),
        ("Table B4", "#'Table B4'!A1"),
        ("Table C1", "#'Table C1'!A1"),
        ("Contact Sheet", "#'Contact Sheet'!A1"),
    ]
    for idx, (label, target) in enumerate(links, start=4):
        ws.row_dimensions[idx].height = 21
        c = ws.cell(idx, 1, label)
        c.hyperlink = target
        c.font = Font(name="Arial", size=12, color="0563C1", underline="single")

    save(
        wb,
        "wide-borderless-hyperlinks",
        "A wide borderless single-column cover sheet with hyperlinked contents and unprinted headings; isolates column ink vs viewport extent and hyperlink styling",
        headings=False,
    )

    # Reduced from BICS sectoral consumption estimates (#1343):
    # a 6-column unheaded worksheet with a 2-tier merged header (SIC07 4-digit,
    # Sector Description, Consumption Range (GWh) 2027-2031) spanning 27.5 cm in width.
    # In headless LibreOffice Calc, the wide table paginates horizontally across
    # multiple portrait A4 pages: Column A (2.54 cm / ~14 units) alone on pages 1-2,
    # Column B (16.91 cm / ~70 units) alone on pages 3-4, and Columns C-F (~8 cm total)
    # on pages 5-6. Tables captures the continuous on-screen grid (columns A through F, ~1077 px).
    # In compare.py, with print_options.headings = False, lo_cell_box(ref) returns None,
    # align() skips cropping on-screen header bands (ox=40, oy=20), and pick_sheet_page()
    # selects page 1 (Column A alone, ink width 91 px), resampling the 1077 px wide
    # 6-column sheet down into 91 px (an 11.8× horizontal squeeze, scale 6.72×, SSIM 0.22,
    # words found 0%, red verdict).
    wb, ws = book()
    font = lambda **kw: Font(name="Calibri", size=kw.pop("size", 11), **kw)
    ws.merge_cells("A1:A2")
    ws.merge_cells("B1:B2")
    ws.merge_cells("C1:F1")
    ws["A1"] = "SIC07 4-digit"
    ws["A1"].font = font(bold=True)
    ws["A1"].alignment = Alignment(vertical="center")
    ws["B1"] = "Sector Description"
    ws["B1"].font = font(bold=True)
    ws["B1"].alignment = Alignment(vertical="center")
    ws["C1"] = "Consumption Range (GWh)"
    ws["C1"].font = font(bold=True)
    ws["C1"].alignment = Alignment(horizontal="center")
    ws["C2"] = "2027-2028"
    ws["C2"].font = font(bold=True)
    ws["D2"] = "2028-2029"
    ws["D2"].font = font(bold=True)
    ws["E2"] = "2029-2030"
    ws["E2"].font = font(bold=True)
    ws["F2"] = "2030-2031"
    ws["F2"].font = font(bold=True)

    ws.column_dimensions["A"].width = 14
    ws.column_dimensions["B"].width = 70
    ws.column_dimensions["C"].width = 12
    ws.column_dimensions["D"].width = 10
    ws.column_dimensions["E"].width = 10
    ws.column_dimensions["F"].width = 10

    sample_bics_rows = [
        ("1041", "Manufacture of oils and fats", "2.9 - 4.2", "3 - 4.3", "3.1 - 4.4", "3.2 - 4.6"),
        ("1062", "Manufacture of starches and starch products", "<1", "<1", "<1", "<1"),
        ("1081", "Manufacture of sugar", "170 - 190", "180 - 200", "180 - 200", "190 - 210"),
        ("2011", "Manufacture of industrial gases", "56 - 300", "58 - 300", "59 - 310", "61 - 320"),
        ("2013", "Manufacture of other inorganic basic chemicals", "81 - 990", "83 - 1000", "85 - 1000", "88 - 1100"),
        ("2014", "Manufacture of other organic basic chemicals", "29 - 120", "30 - 120", "31 - 120", "32 - 130"),
        ("2015", "Manufacture of fertilisers and nitrogen compounds", "37 - 360", "37 - 370", "39 - 380", "40 - 390"),
        ("2016", "Manufacture of plastics in primary forms", "120 - 490", "130 - 500", "130 - 510", "140 - 530"),
        ("2311", "Manufacture of flat glass", "11 - 25", "11 - 26", "11 - 26", "12 - 27"),
        ("2312", "Manufacture of hollow glass", "42 - 150", "43 - 150", "44 - 150", "45 - 160"),
        ("2410", "Manufacture of basic iron and steel and of ferro-alloys", "210 - 890", "220 - 910", "220 - 930", "230 - 960"),
        ("2420", "Manufacture of tubes, pipes, hollow profiles and related fittings", "15 - 39", "16 - 40", "16 - 41", "17 - 42"),
    ]
    for r_idx, row_data in enumerate(sample_bics_rows, start=3):
        for c_idx, val in enumerate(row_data, start=1):
            cell = ws.cell(r_idx, c_idx, val)
            cell.font = font()

    save(
        wb,
        "wide-unheaded-consumption-estimates",
        "A 6-column unheaded consumption estimates sheet with 2-tier header spanning multiple horizontal pages in Calc; isolates continuous viewport vs unheaded horizontal multipage print",
        headings=False,
    )

    # Reduced from Draft schedule 4 - complaint log (#1327):
    # a 3-column unheaded worksheet with wide columns (widths 60, 101, 180)
    # totaling 341 character units (~2,389 px) paginates horizontally across
    # multiple A4 pages in LibreOffice Calc (Col A on page 1, Col B on page 2,
    # Col C on page 3) when headings are disabled, while Tables renders the
    # continuous used extent in its viewport (showing Col A and Col B side-by-side).
    # In compare.py, candidate page picking chooses page 1 only and align() skips
    # cropping on-screen header bands (ox=40, oy=20), compressing both Col A and
    # Col B onto page 1's bounds, yielding scale 1.66×, displacement 25.7pt,
    # and SSIM 0.13.
    wb, ws = book()
    ws.title = "Actionable Complaints"
    ws.column_dimensions["A"].width = 60
    ws.column_dimensions["B"].width = 101
    ws.column_dimensions["C"].width = 180

    title_cell = ws.cell(1, 1, "Log of Actionable Complaints")
    title_cell.font = Font(name="Arial", size=14, bold=True)

    headers = ["Field", "Options", "Notes"]
    header_fill = PatternFill(start_color="D9E1F2", end_color="D9E1F2", fill_type="solid")
    for col_idx, h in enumerate(headers, start=1):
        cell = ws.cell(3, col_idx, h)
        cell.font = Font(name="Calibri", size=11, bold=True)
        cell.fill = header_fill

    complaint_rows = [
        (
            "Reference",
            "Text and/or numerical field",
            "For Veterinary Business use as needed. This field does not need to be included if the Complaint Log is submitted to RCVS.",
        ),
        ("Practice name", "Text field", ""),
        ("Veterinary Business name", "Text field", ""),
        (
            "Pet Owner name",
            "Text field",
            "Field to be anonymised if Complaint Log shared with the RCVS",
        ),
        (
            "Complaint handler name",
            "Text field",
            "Field to be anonymised if Complaint Log shared with the RCVS",
        ),
        ("Date the complaint became an Actionable Complaint", "Date field", ""),
        ("Date the Actionable Complaint was acknowledged in writing", "Date field", ""),
        (
            "Summary of the Actionable Complaint",
            "Text field",
            "For Veterinary Business use as needed. This field does not need to be included if the Complaint Log is submitted to the RCVS.",
        ),
        (
            "Nature of the Actionable Complaint category",
            "Select one or two options only from a pre-defined list (see Lookups tab)",
            "Nature of the Actionable Complaint - Veterinary Business interpretation of the Pet Owner concerns. This field can be updated as the Veterinary Business progresses through its Complaint Process.",
        ),
        (
            "Sub nature of the Actionable Complaint",
            "Select one or two options only per nature of actionable complaint category from a pre-defined list (see Lookups tab)",
            "Sub-nature of the Actionable Complaint - Veterinary Business interpretation of the Pet Owner concerns. This field can be updated as the Veterinary Business progresses through its Complaint Process.",
        ),
        ("Animal species", "Select one from pre-defined list (see Lookups tab)", ""),
        ("Animal age", "Numerical field", ""),
        (
            "Animal alive",
            "Select one of alive or deceased.",
            "Animal alive or deceased to be recorded at the point at which the complaint becomes an Actionable Complaint.",
        ),
        ("Steps taken by Veterinary Business to investigate the complaint", "Text field", ""),
        ("Date the final response to the Actionable Complaint was issued", "Date field", ""),
        ("Outcome", "Select one or more from a pre-defined list (see Lookups tab)", ""),
        ("Actions taken by the practice/Veterinary Business", "Select one or more from a pre-defined list (see Lookups tab)", ""),
    ]

    for r_idx, (field, options, notes) in enumerate(complaint_rows, start=4):
        c1 = ws.cell(r_idx, 1, field)
        c1.font = Font(name="Calibri", size=11, bold=True)
        c2 = ws.cell(r_idx, 2, options)
        c2.font = Font(name="Calibri", size=11)
        if notes:
            c3 = ws.cell(r_idx, 3, notes)
            c3.font = Font(name="Calibri", size=11)

    save(
        wb,
        "wide-unheaded-complaint-log",
        "A 3-column unheaded complaint log worksheet with wide columns (60, 101, 180) spanning multiple horizontal pages in Calc; isolates continuous viewport vs unheaded horizontal multipage print",
        headings=False,
    )

    # Reduced from 2020 - Deep sea stock (#1340):
    # a 24-column landscape unheaded worksheet (Columns A..X, ~28.5 cm wide) with
    # multi-tier grouped headers and continuous top border spanning repeated empty
    # cells (F4:G4). In headless LibreOffice Calc PDF export, landscape A4 with
    # 0.354 in margins has printable width 27.89 cm, so Calc splits Column X
    # onto Page 2/3. Because the sheet omits printed headings, lo_cell_box(ref)
    # returns None, align() skips cropping on-screen header bands (ox=40, oy=20),
    # and pick_sheet_page() selects Page 1 only (columns A..W), compressing
    # all 24 columns and header bands onto 23 columns, yielding scale 1.52×,
    # displacement 61.6pt, SSIM 0.09, and words found 14%.
    wb, ws = book()
    ws.page_setup.orientation = ws.ORIENTATION_LANDSCAPE
    ws.page_setup.paperSize = ws.PAPERSIZE_A4

    thin = Side(style="thin", color="000000")

    ws["A1"] = "Deep Sea Species Statistics: total landings (tonnes live weight) in 2020"
    ws["A1"].font = Font(name="Arial", size=10, bold=True)

    ws["A2"] = "27-Jan-21"
    ws["A2"].font = Font(name="Arial", size=9, bold=True)
    ws["B2"] = "This weeks report includes swap numbers 1448-1502"
    ws["B2"].font = Font(name="Arial", size=9, italic=True)

    # Row 4: Group headers with top border
    ws["C4"] = "Landings into"
    ws.merge_cells("C4:G4")
    ws["C4"].alignment = Alignment(horizontal="center")

    ws["M4"] = "Total landings"
    ws.merge_cells("M4:O4")
    ws["M4"].alignment = Alignment(horizontal="center")

    ws["Q4"] = "Uptake of quota"
    ws.merge_cells("Q4:U4")
    ws["Q4"].alignment = Alignment(horizontal="center")

    for col in range(2, 25):  # B..X: continuous top border across header
        ws.cell(4, col).border = Border(top=thin)

    # Rows 5-7: Subheaders
    ws["B5"] = "Stock"
    ws.merge_cells("B5:B7")
    ws["B5"].alignment = Alignment(horizontal="center", vertical="center")

    subheaders = {
        3: "Scotland",
        4: "England, Wales & N.I.",
        5: "Abroad",
        6: "by UK vessels",
        7: "U.K",
        13: "Quota",
        14: "2019",
        15: "2020",
        24: "UK",
    }
    for col, text in subheaders.items():
        ws.cell(5, col, text).font = Font(name="Arial", size=9)

    for col in (3, 4, 5, 6, 24):
        ws.cell(6, col, "%" if col < 24 else "Quota").font = Font(name="Arial", size=9)

    for col in (3, 4, 5, 6, 24):
        txt = "2019" if col in (3, 6) else ("2020" if col == 4 else ("change" if col == 5 else "2016"))
        ws.cell(7, col, txt).font = Font(name="Arial", size=9)

    for col in range(2, 25):
        ws.cell(7, col).border = Border(bottom=thin)

    # Data rows
    species = [
        ("Tusk IV", [41.2, 63.0, 53.1, 1.7, 0.7], 107),
        ("Ling IV", [2574.5, 2441.4, -5.2, 458.6, 346.7], 2515),
        ("Tusk V,VI,VII", [90.9, 85.4, -6.1, 1.2, 0.1], 265),
        ("Ling VI-X,XII,XIV", [2114.9, 1636.0, -22.6, 403.7, 273.4], 3896),
        ("Black Scabbardfish V-VII,XII", [44.9, 20.0, -55.6, 0.0, 0.0], 194),
        ("Roundnose/Roughead Grenadier Vb,VI,VII", [6.2, 5.9, -4.7, 0.0, 0.0], 216),
        ("Blue Ling Vb,VI,VII", [718.3, 715.5, -0.4, 0.0, 0.0], 979),
    ]

    ws.cell(9, 1, "Deep Sea").font = Font(name="Arial", size=9, bold=True)
    for idx, (name, vals, quota) in enumerate(species, start=10):
        ws.cell(idx, 2, name).font = Font(name="Arial", size=9)
        for c_idx, val in enumerate(vals, start=3):
            ws.cell(idx, c_idx, val).font = Font(name="Arial", size=9)
        ws.cell(idx, 24, quota).font = Font(name="Arial", size=9)

    # Footnote
    ws.cell(18, 2, "2019 landings are for the nearest comparable week last year therefore").font = Font(
        name="Arial", size=8, italic=True
    )
    ws.cell(19, 2, "comparisons will be approximate, and should be treated accordingly.").font = Font(
        name="Arial", size=8, italic=True
    )

    # Columns
    ws.column_dimensions["A"].width = 3
    ws.column_dimensions["B"].width = 22
    for c in range(3, 24):
        col_letter = get_column_letter(c)
        ws.column_dimensions[col_letter].width = 7
    ws.column_dimensions["X"].width = 9

    save(
        wb,
        "wide-unheaded-deep-sea-stock",
        "A 24-column landscape unheaded fisheries statistics sheet with grouped headers, repeated empty cell borders, and horizontal multipage split",
        headings=False,
    )

    # Reduced from Insolvency Service Transactions April 2026 (#1331):
    # an 11-column unheaded ledger of spend transactions exceeding 290 width
    # units (~75 cm) paginates horizontally across multiple pages in LibreOffice
    # Calc when headings are disabled, while Tables renders the continuous used
    # extent. In compare.py, candidate page picking chooses page 1 only and
    # align() fails to crop on-screen header bands (ox=40, oy=20), compressing
    # all 11 columns onto page 1's bounds, yielding scale 1.69×, SSIM 0.12, and
    # 34% words found.
    wb, ws = book()
    ws.title = "Transactions"
    tx_headers = [
        "Department Family",
        "Entity",
        "Date",
        "Expense Type",
        "Expense Area",
        "Supplier",
        "Transaction Number",
        "Amount",
        "Description",
        "Supplier Type",
        "VAT RegNo",
    ]
    tx_widths = {
        "A": 13,
        "B": 21,
        "C": 11,
        "D": 43,
        "E": 56,
        "F": 55,
        "G": 19,
        "H": 11,
        "I": 45,
        "J": 13,
        "K": 14,
    }
    for col_letter, width in tx_widths.items():
        ws.column_dimensions[col_letter].width = width

    for c_idx, h in enumerate(tx_headers, start=1):
        cell = ws.cell(1, c_idx, h)
        cell.font = Font(name="Calibri", size=11)

    transactions = [
        ("AGY", "The Insolvency Service", datetime(2026, 4, 1), "Agency Staff & Contractor Costs", "Shared Contracts", "Alexander Mann Solutions", 300074809, 115658.80, "Staff Costs - Non-Perms", "Large", 7444926120),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 1), "IT - Run", "Change and Technology Services", "Wavenet Limited", 300074673, 58239.90, "ICT", "Large", ""),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 1), "IT - Run", "Investigations & Enforcement Services", "BlueLight Commercial Limited", 300074568, 41755.19, "ICT", "Small", 349449851),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 1), "IT - Run", "Change and Technology Services", "Nine23 Ltd", 300074501, 29403.85, "ICT", "Small", 141851034),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 1), "Agency Staff & Contractor Costs", "Projects", "SmartSourcing", 300074503, 27957.60, "Staff Costs - Non-Perms", "Medium", 788507869),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 8), "Agency Staff & Contractor Costs", "Shared Contracts", "Alexander Mann Solutions", 300074676, 111388.46, "Staff Costs - Non-Perms", "Large", 7444926120),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 8), "Forensic Accounting", "Finance, Commercial, Sustainability & Property", "Ernst & Young LLP", 300074897, 47940.00, "Investigation & Enforcement", "Large", 773831408),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 12), "IT - Transform", "Projects", "Great Wave AI Limited", 300074674, 162000.00, "ICT", "Micro", 464432884),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 13), "IT - Refresh", "Change and Technology Services", "Risk Ledger Ltd", 300074701, 45600.00, "ICT", "Small", 294427378),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 15), "Agency Staff & Contractor Costs", "Shared Contracts", "Alexander Mann Solutions", 300074755, 85867.16, "Staff Costs - Non-Perms", "Large", 7444926120),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 15), "IT - Transform", "Projects", "NTT Data UK Limited", 300074751, 82692.00, "ICT", "Large", 159334590),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 15), "INSSpire Award Vouchers", "Shared Contracts", "Edenred (UK Group) Limited", 300074753, 65600.00, "Staff Costs - Perms", "Medium", ""),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 21), "IT - Run", "Investigations & Enforcement Services", "NEC Software Solutions UK Ltd", 300074890, 69168.00, "ICT", "Large", ""),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 21), "IT - Run", "Change and Technology Services", "Prolifics", 300074863, 28996.25, "ICT", "Micro", 639210253),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 22), "Agency Staff & Contractor Costs", "Shared Contracts", "Alexander Mann Solutions", 300074903, 81587.87, "Staff Costs - Non-Perms", "Large", 7444926120),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 24), "IT - Run", "Change and Technology Services", "Prolifics", 300074964, 42109.50, "ICT", "Micro", 639210253),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 24), "IT - Run", "Investigations & Enforcement Services", "Insight Direct (UK) Ltd", 300074961, 40935.66, "ICT", "Large", 746075129),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 27), "IT - Run", "Investigations & Enforcement Services", "Intaforensics Ltd", 300074565, 162470.30, "ICT", "Micro", 900353372),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 30), "Solicitors Investigations", "Investigations & Enforcement Services", "Mills & Reeve LLP", 300074602, 52877.05, "Legal - Outsourced Investigation", "Large", 104834588),
        ("AGY", "The Insolvency Service", datetime(2026, 4, 30), "Storage - OR", "Shared Contracts", "Iron Mountain (UK) Ltd", 300074948, 30890.86, "Off-Site Storage", "Large", 607937516),
    ]

    for r_idx, row in enumerate(transactions, start=2):
        for c_idx, val in enumerate(row, start=1):
            cell = ws.cell(r_idx, c_idx, val)
            cell.font = Font(name="Calibri", size=11)
            if isinstance(val, datetime):
                cell.number_format = "mm-dd-yy"
            elif isinstance(val, float):
                cell.number_format = "#,##0.00"

    save(
        wb,
        "wide-unheaded-transactions",
        "An 11-column unheaded financial transactions ledger spanning multiple horizontal pages in Calc; isolates continuous viewport vs unheaded horizontal multipage print",
        headings=False,
    )

    # Reduced from the June 2026 forecast evaluation report (#1333):
    # Its cover sheet uses a 119-character wide column without printed headings.
    # Cell B2 places right-aligned, top-aligned multiline heading text in that
    # wide column, Row 3 has a centered section title, and Row 4 has indented
    # items (indent=2). In LibreOffice Calc's A4 portrait print, the column
    # exceeds printable page width and right-aligned text is clipped at the
    # margin, whereas Tables draws the full column on screen.
    wb, ws = book()
    ws.column_dimensions["A"].width = 8.78
    ws.column_dimensions["B"].width = 119
    ws.row_dimensions[2].height = 48
    ws.row_dimensions[3].height = 24
    ws["B2"] = "Forecast evaluation report\nAnnual database"
    ws["B2"].font = Font(name="Calibri", size=16, color="E7A614")
    ws["B2"].alignment = Alignment(horizontal="right", vertical="top", wrap_text=True)
    ws["B3"] = "Economic model development database"
    ws["B3"].font = Font(name="Calibri", size=14)
    ws["B3"].alignment = Alignment(horizontal="center")
    ws["B3"].fill = PatternFill("solid", fgColor="F5D0A9")
    ws["B4"] = "Model improvements"
    ws["B4"].font = Font(name="Calibri", size=11, underline="single")
    ws["B4"].alignment = Alignment(horizontal="left", indent=2)
    save(
        wb,
        "wide-column-right-align",
        "A wide unprinted column with right-aligned multiline heading, centered title and indented entries",
        headings=False,
    )

    wb, ws = book()
    # Reduced from real-competition-in-uk-electricity-markets-2025-data
    # (#1336): its cover sheet has pageSetup scale="46". Keep this grid
    # small enough to fit the capture, so viewport clipping cannot explain
    # the difference between screen geometry and the printed reference.
    for r in range(1, 6):
        for c in range(1, 5):
            ws.cell(r, c, r * 10 + c)
    ws.page_setup.scale = 46
    save(
        wb,
        "print-scale",
        "The values grid printed at 46% scale; compare with the unscaled values fixture",
    )

    wb, ws = book()
    # Reduced from the Contents sheet of the DPA return (#1330): Calc's
    # printed page is smaller than the editor's unscaled grid. Keep this
    # independent of hyperlinks, hidden rows, merges and workbook selection.
    wb, ws = book()
    ws.page_setup.paperSize = ws.PAPERSIZE_A4
    ws.page_setup.orientation = "landscape"
    ws.page_setup.scale = 62
    ws.sheet_properties.pageSetUpPr.fitToPage = True
    ws.print_area = "A1:D8"
    for col in "ABCD":
        ws.column_dimensions[col].width = 35
    ws["A1"] = "A wide worksheet heading reaches the final column"
    ws["A1"].font = Font(name="Arial", size=28)
    for row in range(3, 9):
        ws.row_dimensions[row].height = 24
        for col in range(1, 5):
            ws.cell(row, col, f"Row {row} column {col}")
    save(wb, "print-fit-page", "A wide landscape A4 worksheet fitted to one printed page, with a 62% stored print scale")

    wb, ws = book()
    # Reduced from real-fruit-and-vegetable-aid-scheme-annual-stats-2025
    # (#1334): wide columns (widths 45, 42, 60) exceed standard A4 portrait
    # printable width (~17.4 cm) and span across multiple vertical pages
    # (40 rows). LibreOffice Calc's default print order paginates in "Down,
    # then across" sequence, placing column A on pages 1-2, column B on pages
    # 3-4, and column C on pages 5-6. Tables captures the continuous on-screen
    # grid with all columns visible in the viewport.
    for r in range(1, 41):
        ws.cell(r, 1, f"Category item {r}")
        ws.cell(r, 2, f"Subcategory detail {r}")
        ws.cell(r, 3, r * 100)
    ws.column_dimensions["A"].width = 45
    ws.column_dimensions["B"].width = 42
    ws.column_dimensions["C"].width = 60
    save(
        wb,
        "paginated-columns",
        "A multi-page sheet with columns wider than page printable width, paginating down-then-across in Calc",
    )

    wb, ws = book()
    # #1352: the reservoir list's reference PDF splits a wide sheet across
    # pages, while the editor capture includes columns from the next page.
    # Keep text, rows and styles trivial so this measures horizontal paging.
    ws.page_setup.paperSize = ws.PAPERSIZE_A4
    ws.page_setup.orientation = "portrait"
    ws.page_setup.scale = 100
    ws.sheet_properties.pageSetUpPr.fitToPage = False
    for col, label in zip("ABCDEF", ("First", "Second", "Third", "Fourth", "Fifth", "Sixth")):
        ws.column_dimensions[col].width = 22
        ws[f"{col}1"] = label
        ws[f"{col}2"] = "Page boundary probe"
    ws.print_area = "A1:F2"
    save(
        wb,
        "horizontal-pagination",
        "A wide sheet at 100% scale splits across portrait A4 pages; "
        "columns on later pages must not be squeezed into the first page",
    )

    wb, ws = book()
    # #1349: the supervising panel engineers contact list sets landscape A4
    # with fit-to-width (ODF scale-to-X="1", fitToWidth=1, fitToHeight=0).
    # Calc scales wide sheets down so all columns fit across one page width,
    # fitting far more rows onto page 1 than the editor's 100% on-screen grid.
    # Keep cells, rows and text simple to isolate fit-to-width print scaling.
    ws.page_setup.paperSize = ws.PAPERSIZE_A4
    ws.page_setup.orientation = "landscape"
    ws.page_setup.fitToWidth = 1
    ws.page_setup.fitToHeight = 0
    ws.sheet_properties.pageSetUpPr.fitToPage = True
    for c, col in enumerate([chr(ord("A") + i) for i in range(12)], start=1):
        ws.column_dimensions[col].width = 18
        ws.cell(1, c, f"Header {col}")
        for r in range(2, 22):
            ws.cell(r, c, f"R{r}C{c}")
    save(
        wb,
        "fit-to-width",
        "A wide landscape A4 worksheet fitted to 1 page wide (fitToWidth=1, fitToHeight=0), "
        "scaling columns to fit the page width while rows paginate vertically",
    )

    wb, ws = book()
    # Reduced from HMT Annual Report and Accounts Core Tables (#1351): a single
    # sheet with 74 rows prints across two vertical A4 pages in LibreOffice
    # Calc when headings are disabled, while Tables captures the entire
    # continuous used extent. In compare.py, candidate page picking chooses only
    # page 1 and align() compresses all rows into the first page, resulting in
    # a 1.33× scale squash, 80pt vertical displacement, and low SSIM.
    wb, ws = book()
    ws["A1"] = "HM Treasury Core Financial Summary"
    ws["A1"].font = Font(name="Calibri", size=14, bold=True)
    headers = ["Category", "2021-22", "2022-23", "2023-24", "2024-25", "2025-26"]
    for c, h in enumerate(headers, start=1):
        cell = ws.cell(3, c, h)
        cell.font = Font(name="Calibri", size=11, bold=True)
    ws.column_dimensions["A"].width = 30
    for c in "BCDEF":
        ws.column_dimensions[c].width = 14
    for r in range(4, 75):
        ws.cell(r, 1, f"Programme expenditure line {r - 3:02d}")
        for c in range(2, 7):
            ws.cell(r, c, (r * 100) + c * 7)
    save(
        wb,
        "tall-unprinted-sheet",
        "A 74-row sheet without printed headings that spans multiple vertical pages in Calc; isolates continuous viewport vs paginated print comparison",
        headings=False,
    )

    wb, ws = book()
    pivot_rows = [
        ("Region", "Apparel - Sum of Sales", "Electronics - Sum of Sales"),
        ("North", 50, 100),
        ("South", 0, 200),
    ]
    for row_index, row in enumerate(pivot_rows, start=1):
        for col_index, value in enumerate(row, start=1):
            ws.cell(row_index, col_index, value)
    ws.column_dimensions["B"].width = 24
    ws.column_dimensions["C"].width = 27
    save(
        wb,
        "pivot-crosstab",
        "A column-field pivot output: North and South sales by Apparel and Electronics",
    )

    wb, ws = book()
    # Name the face and size, as Excel and Calc always do. openpyxl's bare
    # Font(bold=True) writes a <font> with no name, which Calc draws in its
    # own serif fallback: that measured the fallback, not the style.
    font = lambda **kw: Font(name="Calibri", size=kw.pop("size", 11), **kw)
    ws["A1"] = "Bold"
    ws["A1"].font = font(bold=True)
    ws["A2"] = "Italic"
    ws["A2"].font = font(italic=True)
    ws["A3"] = "Big"
    ws["A3"].font = font(size=20)
    ws["A4"] = "Red"
    ws["A4"].font = font(color="C00000")
    save(wb, "cell-fonts", "Bold, italic, 20pt and red text in A1:A4; row 3 is taller")

    wb, ws = book()
    # Reduced from the July 2026 fiscal-risks introduction (#1326): these
    # font colours live after accent6 in the workbook's theme palette.
    from openpyxl.styles import Color
    ws.column_dimensions["A"].width = 44
    for row, (text, theme) in enumerate(
        (("Hyperlink theme colour", 10), ("Followed hyperlink theme colour", 11)), start=1
    ):
        cell = ws.cell(row, 1, text)
        cell.font = font(color=Color(theme=theme), underline="single")
    save(wb, "hyperlink-theme-colors", "Underlined hyperlink blue and followed-hyperlink purple text")

    wb, ws = book()
    for i, color in enumerate(("FFC7CE", "C6EFCE", "FFEB9C", "BDD7EE"), start=1):
        ws.cell(i, 1, color).fill = PatternFill("solid", fgColor=color)
    save(wb, "fills", "Four cells with pink, green, yellow and blue backgrounds")

    wb, ws = book()
    thin, thick = Side(style="thin"), Side(style="thick")
    ws["B2"] = "thin"
    ws["B2"].border = Border(left=thin, right=thin, top=thin, bottom=thin)
    ws["D2"] = "thick"
    ws["D2"].border = Border(left=thick, right=thick, top=thick, bottom=thick)
    save(wb, "borders", "B2 boxed with a thin border, D2 with a thick one")

    wb, ws = book()
    rows = [(0.153, "0.0%"), (1234.5, '"$"#,##0.00'), (45000, "yyyy-mm-dd"), (0.5, "# ?/?"), (1234567.891, "#,##0.00")]
    for i, (v, fmt) in enumerate(rows, start=1):
        c = ws.cell(i, 1, v)
        c.number_format = fmt
    save(wb, "number-formats", "15.3%, $1,234.50, 2023-03-15, 1/2, 1,234,567.89")

    wb, ws = book()
    for i, h in enumerate(("left", "center", "right"), start=1):
        c = ws.cell(i, 1, h)
        c.alignment = Alignment(horizontal=h)
    ws.column_dimensions["A"].width = 30
    save(wb, "alignment", "Column A is wide; texts sit left, centre and right")

    wb, ws = book()
    ws["A1"] = "Merged A1:C2"
    ws.merge_cells("A1:C2")
    ws["A1"].alignment = Alignment(horizontal="center", vertical="center")
    save(wb, "merged", "One merged block spanning A1:C2 with centred text and no inner gridlines")

    wb, ws = book()
    ws["A1"] = "This long text wraps inside a narrow cell"
    ws["A1"].alignment = Alignment(wrap_text=True)
    ws.column_dimensions["A"].width = 12
    save(wb, "wrap-text", "A1 text wraps onto several lines; row 1 grows")

    # The MenB real workbook (#1325) uses a wide column with Arial 12 as
    # the *workbook default*, not the Calibri 11 assumed by the XLSX width
    # conversion. The Tables capture wraps its heading and shifts the later
    # rows; Calc's printed page clips the heading at the right edge. This
    # isolates the default-font/column-width hypothesis; it does not model
    # the source's stored row heights. Keep the text synthetic.
    wb, ws = book()
    wb._fonts[0] = Font(name="Arial", size=12)
    ws.column_dimensions["A"].width = 100.6640625
    ws["A1"] = "Young adult programme coverage for participating regions up to the end of September"
    ws["A1"].font = Font(name="Arial", size=16, bold=True)
    ws["A1"].alignment = Alignment(wrap_text=True)
    ws["A2"] = "Following section"
    ws["A2"].font = Font(name="Arial", size=12)
    save(wb, "wrap-wide-default-font", "A bold wrapped heading in a wide Arial 12 workbook, followed by a section label")

    wb, ws = book()
    ws["A1"] = "A merged cell with wrapped text needs the combined width of its columns to fit its row."
    ws.merge_cells("A1:B1")
    ws["A1"].alignment = Alignment(wrap_text=True)
    ws.column_dimensions["A"].width = 12
    ws.column_dimensions["B"].width = 12
    save(wb, "wrap-merged", "Wrapped text across merged columns; row height follows the combined column width")

    wb, ws = book()
    ws.column_dimensions["B"].width = 40
    ws.row_dimensions[3].height = 60
    ws["B3"] = "wide and tall"
    save(wb, "col-row-size", "Column B is ~3x default width, row 3 ~3x default height")

    wb, ws = book()
    for i in range(1, 11):
        ws.cell(i, 1, i * 10)
    ws.conditional_formatting.add(
        "A1:A10", CellIsRule(operator="greaterThan", formula=["50"], fill=PatternFill("solid", bgColor="FFC7CE"))
    )
    save(wb, "conditional", "A6:A10 (values > 50) are pink; A1:A5 are not")

    # TBL-003: a value filter over a labeled column (only North shows;
    # South/East rows are hidden by the filter, not by hand) plus a
    # conditional rule owned by the later sheet — the visible
    # filtered/cleared comparison for worksheet-scoped rules and
    # persistent filters.
    wb, ws = book()
    for r, (region, amount) in enumerate(
        (("Region", "Amount"), ("North", 10), ("South", 20), ("North", 30), ("East", 40)), start=1
    ):
        ws.cell(r, 1, region)
        ws.cell(r, 2, amount)
    ws.auto_filter.ref = "A1:B5"
    ws.auto_filter.add_filter_column(0, ["North"])
    later = wb.create_sheet("Later")
    for i in range(1, 6):
        later.cell(i, 1, i * 10)
    later.conditional_formatting.add(
        "A1:A5", CellIsRule(operator="greaterThan", formula=["25"], fill=PatternFill("solid", bgColor="FFC7CE"))
    )
    save(wb, "filtered", "Only the North rows show (South/East filtered out); the Later sheet carries its own pink rule")

    wb, ws = book()
    for r in range(1, 40):
        for c in range(1, 10):
            ws.cell(r, c, f"{r},{c}")
    ws.freeze_panes = "B2"
    # Saved scrolled: the pane after the freeze starts at E20, so the view
    # is row 1 and column A (frozen) beside E20:I39. A print can't scroll,
    # but print titles are the print form of a freeze: with row 1 and
    # column A repeated and the print area E20:I39, Calc prints exactly
    # that view. Without frozen panes the grid shows A1:I39 instead and
    # most of the words are missing, so this can't pass by accident.
    ws.sheet_view.pane.topLeftCell = "E20"
    ws.print_title_rows = "1:1"
    ws.print_title_cols = "A:A"
    ws.print_area = "E20:I39"
    save(wb, "frozen", "Row 1 and column A stay put beside E20:I39 (the view is saved scrolled there), with freeze lines")

    wb, ws = book()
    from openpyxl.chart import BarChart, Reference

    for i, v in enumerate((3, 7, 5, 9), start=1):
        ws.cell(i, 1, f"Q{i}")
        ws.cell(i, 2, v)
    ch = BarChart()
    ch.add_data(Reference(ws, min_col=2, min_row=1, max_row=4))
    ch.set_categories(Reference(ws, min_col=1, min_row=1, max_row=4))
    # Small enough that LibreOffice's printed range (to column I) holds the
    # whole chart; at openpyxl's default 15 cm it clipped the fourth bar.
    ch.width, ch.height = 9, 6
    ws.add_chart(ch, "D2")
    save(wb, "chart", "A bar chart with four bars (3,7,5,9) anchored at D2")

    # The other chart kinds Excel and Calc share, on the same data and at
    # the same size, so each compares against LibreOffice the way the bar
    # chart does.
    from openpyxl.chart import AreaChart, LineChart, PieChart, ScatterChart, Series

    for name, make, what in (
        ("chart-line", LineChart, "A line chart through 3,7,5,9 over Q1..Q4 anchored at D2"),
        ("chart-area", AreaChart, "An area chart of 3,7,5,9 over Q1..Q4 anchored at D2"),
        ("chart-pie", PieChart, "A pie chart of 3,7,5,9 (Q1..Q4) with its legend, anchored at D2"),
    ):
        wb, ws = book()
        for i, v in enumerate((3, 7, 5, 9), start=1):
            ws.cell(i, 1, f"Q{i}")
            ws.cell(i, 2, v)
        ch = make()
        ch.add_data(Reference(ws, min_col=2, min_row=1, max_row=4))
        ch.set_categories(Reference(ws, min_col=1, min_row=1, max_row=4))
        if make is LineChart:
            # Straight segments, as a new line chart draws in Excel and
            # Calc; openpyxl marks its series smooth.
            ch.series[0].smooth = False
        ch.width, ch.height = 9, 6
        ws.add_chart(ch, "D2")
        save(wb, name, what)

    wb, ws = book()
    for i, (x, y) in enumerate(((1, 3), (2.5, 7), (4, 5), (6, 9)), start=1):
        ws.cell(i, 1, x)
        ws.cell(i, 2, y)
    ch = ScatterChart()
    series = Series(Reference(ws, min_col=2, min_row=1, max_row=4), Reference(ws, min_col=1, min_row=1, max_row=4))
    # Points only, as Calc's and Excel's default XY chart draws them.
    series.marker.symbol = "circle"
    series.marker.size = 7
    series.marker.graphicalProperties.solidFill = "4F81BD"
    series.marker.graphicalProperties.line.solidFill = "4F81BD"
    series.graphicalProperties.line.noFill = True
    ch.series.append(series)
    ch.width, ch.height = 9, 6
    ws.add_chart(ch, "D2")
    save(wb, "chart-scatter", "An XY scatter of (1,3) (2.5,7) (4,5) (6,9) as points, anchored at D2")

    # TBL-002: two named series with a bottom legend, so a visual
    # comparison covers multi-series legends — what the single-series
    # fixtures above cannot show. (No chart/axis titles: neither these
    # fixtures nor the grid's chart drawing carry them.)
    from openpyxl.chart.series import SeriesLabel
    wb, ws = book()
    for i, (q, a, b) in enumerate((("Q1", 3, 2), ("Q2", 7, 5), ("Q3", 5, 6), ("Q4", 9, 4)), start=1):
        ws.cell(i, 1, q)
        ws.cell(i, 2, a)
        ws.cell(i, 3, b)
    ch = BarChart()
    ch.legend.position = "b"
    ch.add_data(Reference(ws, min_col=2, min_row=1, max_row=4), titles_from_data=False)
    ch.series[0].title = SeriesLabel(v="Sales")
    ch.add_data(Reference(ws, min_col=3, min_row=1, max_row=4), titles_from_data=False)
    ch.series[1].title = SeriesLabel(v="Plan")
    ch.set_categories(Reference(ws, min_col=1, min_row=1, max_row=4))
    ch.width, ch.height = 9, 6
    ws.add_chart(ch, "D2")
    save(wb, "chart-multi", "A bar chart of Sales (3,7,5,9) and Plan (2,5,6,4) over Q1..Q4 with a bottom legend, anchored at D2")

    # Reduced from Schedule of musts 2026 (#1323):
    # an unheaded bordered worksheet with 4 columns (widths 13, 115, 26, 49)
    # and 16 rows enclosed by thin borders across all cells.
    # In LibreOffice Calc, headings are disabled (headings=False) so no row
    # numbers (1..N) or column letters (A..D) are printed. However, because
    # all cells have unbroken borders, lo_cell_box() finds >= 3 long lines along
    # both axes and erroneously treats row 1 and column A as heading boxes,
    # cropping them out of the LibreOffice reference image. Meanwhile,
    # Tables' on-screen capture (A-1.png) retains row 1 and column A past
    # our_cell_origin (ox=40, oy=20). When align() resamples A-1.png onto
    # the reference, row 1 is mapped to row 2 and col A to col B, causing
    # a 148pt displacement, scale 1.77×, and SSIM 0.06.
    wb, ws = book()
    ws.title = "Schedule of Musts"
    ws.page_setup.orientation = ws.ORIENTATION_LANDSCAPE
    ws.column_dimensions["A"].width = 13
    ws.column_dimensions["B"].width = 115
    ws.column_dimensions["C"].width = 26
    ws.column_dimensions["D"].width = 49

    thin_border = Border(
        left=Side(style="thin", color="000000"),
        right=Side(style="thin", color="000000"),
        top=Side(style="thin", color="000000"),
        bottom=Side(style="thin", color="000000"),
    )
    section_fill = PatternFill(start_color="D9D9D9", end_color="D9D9D9", fill_type="solid")

    ws.cell(1, 1, "Schedule of Musts 2026").font = Font(name="Arial", size=24, bold=True)
    ws.cell(1, 4, "Fully compliant").font = Font(name="Arial", size=18, bold=True)
    ws.cell(
        2,
        1,
        "These are the requirements in the Academy Trust Handbook brought together into one list: the 'musts'.",
    ).font = Font(name="Arial", size=11)
    ws.cell(
        3,
        1,
        "References to the relevant sections are included, which must be read in full.",
    ).font = Font(name="Arial", size=11)

    headers = [
        "Paragraph",
        "Requirement",
        "In Place:\n- Fully compliant\n- Working towards\n- Not in place",
        "Comments\nAction to take",
    ]
    for c_idx, h in enumerate(headers, start=1):
        cell = ws.cell(4, c_idx, h)
        cell.font = Font(name="Arial", size=11, bold=True)
        cell.alignment = Alignment(wrap_text=True, vertical="center")

    sec_title = "Top 10 ‘musts’ for chairs and other trustees"
    ws.cell(5, 1, sec_title).font = Font(name="Arial", size=11, bold=True)

    musts = [
        (
            "1.11 and 1.12",
            "Apply highest standards of conduct and ensure robust governance, comply with charitable objects, with duties as company directors, with charity law and the funding agreement.",
            "Fully compliant",
            "",
        ),
        ("2.3", "Ensure the board of trustees meets at least three times a year.", "Fully compliant", ""),
        ("2.4", "Approve a written scheme of delegation of financial powers.", "Fully compliant", ""),
        (
            "2.10",
            "Ensure the board approves a balanced budget for the financial year and minutes their approval",
            "Fully compliant",
            "",
        ),
        (
            "2.19 and 2.20",
            "Share management accounts with the chair of trustees monthly and consider when the board meets, taking action to maintain financial viability",
            "Fully compliant",
            "",
        ),
        (
            "2.31 to 2.33",
            "Ensure decisions about executive pay follow a robust evidence-based process, that is documented in an agreed pay policy, reflecting the individual's role and responsibilities, and that the approach to pay is transparent, proportionate and defensible. Does not increase at a faster rate that that of teachers.",
            "Fully compliant",
            "",
        ),
        (
            "3.6 to 3.14",
            "Appoint an audit and risk committee (either dedicated or combined with another committee) to advise on the adequacy of the trust's controls and risks.",
            "Fully compliant",
            "",
        ),
        ("4.4", "Submit audited accounts to DfE by 31 December", "Fully compliant", ""),
        (
            "4.14",
            "Ensure an appropriate, reasonable and timely response to findings by auditors, taking opportunities to strengthen financial management and control",
            "Fully compliant",
            "",
        ),
        (
            "5.36 to 5.56",
            "Manage conflicts of interest, be even-handed with related parties, and ensure goods or services provided by them are at no more than cost, beyond the limits in this handbook",
            "Fully compliant",
            "",
        ),
        ("Roles and responsibilities", "", "", ""),
    ]

    for offset, (para, req, status, comments) in enumerate(musts, start=6):
        c1 = ws.cell(offset, 1, para)
        c1.font = Font(name="Arial", size=11, bold=(offset == 16))
        c2 = ws.cell(offset, 2, req)
        c2.font = Font(name="Arial", size=11)
        c2.alignment = Alignment(wrap_text=True, vertical="center")
        c3 = ws.cell(offset, 3, status)
        c3.font = Font(name="Arial", size=11)
        c3.alignment = Alignment(horizontal="center", vertical="center")
        c4 = ws.cell(offset, 4, comments)
        c4.font = Font(name="Arial", size=11)

    for r in range(1, 17):
        for c in range(1, 5):
            ws.cell(r, c).border = thin_border
            if r in (5, 16):
                ws.cell(r, c).fill = section_fill

    save(
        wb,
        "unheaded-bordered-musts",
        "A 4-column unheaded landscape schedule with full cell borders across rows 1..16; isolates lo_cell_box line-detection false positive on unheaded bordered tables",
        headings=False,
    )


# ── Decks (pptx) ──────────────────────────────────────────────────────────
def decks(img):
    from pptx import Presentation
    from pptx.dml.color import RGBColor
    from pptx.enum.shapes import MSO_SHAPE
    from pptx.oxml.ns import qn
    from pptx.util import Inches, Pt

    d = os.path.join(OUT, "decks")
    os.makedirs(d, exist_ok=True)

    def deck():
        p = Presentation()
        p.slide_width, p.slide_height = Inches(13.333), Inches(7.5)
        return p

    def fixed_box(tf):
        # python-pptx's add_textbox writes wrap="none" + spAutoFit. On open
        # LibreOffice re-fits such a box around its centre, moving the text
        # hundreds of points; PowerPoint (and Decks) keep the stored
        # geometry. That is autofit behaviour, not what these fixtures test,
        # so their boxes wrap and don't resize: then every renderer agrees
        # on where the box is.
        from pptx.enum.text import MSO_AUTO_SIZE
        tf.word_wrap = True
        tf.auto_size = MSO_AUTO_SIZE.NONE

    def save(p, name, expect):
        path = os.path.join(d, f"{name}.pptx")
        p.save(path)
        add("decks", name, path, expect)

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[0])
    s.shapes.title.text = "Title Slide"
    s.placeholders[1].text = "Subtitle text"
    save(p, "title-layout", "Large centred title and smaller subtitle from the layout placeholders")

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[1])
    s.shapes.title.text = "Bullets"
    body = s.placeholders[1].text_frame
    body.text = "First point"
    for t, lvl in (("Second point", 0), ("Sub point", 1)):
        para = body.add_paragraph()
        para.text, para.level = t, lvl
    save(p, "bullets", "Title plus a bulleted body; 'Sub point' is indented a level")

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[1])
    s.shapes.title.text = "Autofit"
    body = s.placeholders[1].text_frame
    body.text = "Point 1"
    for i in range(2, 11):
        body.add_paragraph().text = f"Point {i}"
    # What PowerPoint records after shrinking an overflowing body: 62.5%
    # text, 20% less line spacing. Stated, not computed by the renderer.
    from lxml import etree
    fit = etree.SubElement(body._bodyPr, qn("a:normAutofit"))
    fit.set("fontScale", "62500")
    fit.set("lnSpcReduction", "20000")
    save(p, "autofit", "Ten bullets shrunk to fit the body: 62.5% text, lines 20% closer")

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    tb = s.shapes.add_textbox(Inches(1), Inches(1), Inches(8), Inches(3)).text_frame
    fixed_box(tb)
    for i, (size, rgb) in enumerate(((14, (0, 0, 0)), (32, (200, 0, 0)), (54, (0, 0, 200)))):
        para = tb.paragraphs[0] if i == 0 else tb.add_paragraph()
        r = para.add_run()
        r.text = f"{size}pt"
        r.font.size = Pt(size)
        r.font.color.rgb = RGBColor(*rgb)
    save(p, "text-styles", "Three lines: 14pt black, 32pt red, 54pt blue")

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    for kind, x, color in ((MSO_SHAPE.RECTANGLE, 1, (220, 40, 40)), (MSO_SHAPE.OVAL, 5, (40, 160, 60)), (MSO_SHAPE.ROUNDED_RECTANGLE, 9, (40, 80, 220))):
        sh = s.shapes.add_shape(kind, Inches(x), Inches(2), Inches(3), Inches(2))
        sh.fill.solid()
        sh.fill.fore_color.rgb = RGBColor(*color)
    save(p, "shapes", "Red rectangle, green ellipse, blue rounded rectangle in a row")

    # The presets drawn with BetterOffice's DrawingML outlines
    # (decks-core shape.rs, borrowed_outline): stars, polygons, arrows,
    # a donut's hole, a cube's faces, callouts. Without them each was its box.
    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    presets = ["star4", "star6", "star8", "star10", "star12",
               "heptagon", "decagon", "dodecagon", "leftRightArrow", "bentArrow",
               "corner", "foldedCorner", "noSmoking", "donut", "cube",
               "flowChartTerminator", "wedgeRoundRectCallout", "wedgeEllipseCallout", "cloudCallout", "mathMultiply"]
    for i, prst in enumerate(presets):
        sh = s.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(0.4 + (i % 5) * 2.6), Inches(0.4 + (i // 5) * 1.8), Inches(2.0), Inches(1.2))
        sh._element.spPr.find(qn("a:prstGeom")).set("prst", prst)
        sh.fill.solid()
        sh.fill.fore_color.rgb = RGBColor(40, 80, 220)
    save(p, "preset-shapes", "Twenty preset shapes in four rows: stars, polygons, arrows, a donut, a cube, callouts; none a plain box")

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    s.shapes.add_picture(img, Inches(3), Inches(2), width=Inches(5))
    save(p, "image", "The four-colour test image, 5in wide, at (3in, 2in)")

    # A cropped picture (a:srcRect): the left quarter and the bottom fifth
    # of the four-colour image are cut, the rest stretched to the box.
    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    pic = s.shapes.add_picture(img, Inches(3), Inches(2), width=Inches(5), height=Inches(3))
    pic.crop_left, pic.crop_bottom = 0.25, 0.2
    save(p, "image-crop", "The test image cropped: left quarter and bottom fifth gone, the rest stretched to a 5x3in box")

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    s.background.fill.solid()
    s.background.fill.fore_color.rgb = RGBColor(30, 30, 60)
    tb = s.shapes.add_textbox(Inches(1), Inches(1), Inches(6), Inches(1)).text_frame
    fixed_box(tb)
    tb.text = "Light text on dark background"
    tb.paragraphs[0].runs[0].font.color.rgb = RGBColor(255, 255, 255)
    save(p, "background", "Dark navy slide background with white text")

    # DCK-004, reduced from the pay centre impact assessment (#1397): its
    # title slide's background is a picture (p:bg -> a:blipFill, stretched
    # 1% past each side by a:fillRect l="-1000" r="-1000") under a white
    # title. The picture here is drawn for this fixture: navy, a light
    # blue outlined box and a white footer band, at the source's 1.8:1
    # aspect so the stretch onto 16:9 is measurable.
    from lxml import etree
    from pptx.oxml.ns import qn

    bg_path = os.path.join(d, "pay-centre-background.png")
    W, H = 1800, 1000
    pic = Image.new("RGB", (W, H), (23, 42, 69))
    draw = ImageDraw.Draw(pic)
    draw.rectangle([0, 870, W - 1, H - 1], fill=(255, 255, 255))
    draw.rectangle([60, 880 - 220, 120 + 640, 880 - 120], outline=(0, 170, 230), width=4)
    draw.rectangle([1500, 905, 1740, 965], fill=(30, 30, 30))
    pic.save(bg_path)
    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    _, rid = s.part.get_or_add_image_part(bg_path)
    bg = etree.fromstring(
        '<p:bg xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" '
        'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        '<p:bgPr><a:blipFill dpi="0" rotWithShape="1">'
        f'<a:blip r:embed="{rid}"><a:lum/></a:blip><a:srcRect/>'
        '<a:stretch><a:fillRect l="-1000" r="-1000"/></a:stretch></a:blipFill>'
        '<a:effectLst/></p:bgPr></p:bg>'
    )
    c_sld = s._element.find(qn("p:cSld"))
    c_sld.insert(0, bg)
    tb = s.shapes.add_textbox(Inches(0.6), Inches(2.8), Inches(10), Inches(1.5)).text_frame
    fixed_box(tb)
    tb.text = "Picture background under a white title"
    run = tb.paragraphs[0].runs[0]
    run.font.size, run.font.name = Pt(40), "Arial"
    run.font.color.rgb = RGBColor(255, 255, 255)
    save(p, "pay-centre-reduced-picture-background",
         "A navy picture fills the slide (light blue box, white footer band) under a white 40pt title")

    # DCK-004, reduced from the same title (#1397): one paragraph broken
    # over three lines with a:br. Dropping the breaks welds "and" to the
    # next line's first word and rewraps the paragraph. Measured amber
    # (run 38699284598): words 1.0, lost_lines 0, SSIM 0.86, but disp
    # 7.1pt > 6pt because our 100% line pitch for 36pt Arial is ~7% tighter
    # than Impress's — a line-height defect of its own, not the breaks.
    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    tb = s.shapes.add_textbox(Inches(0.6), Inches(1.5), Inches(11), Inches(3)).text_frame
    fixed_box(tb)
    para = tb.paragraphs[0]
    for i, line in enumerate(("Line breaks inside one paragraph –", "the second line ends here and", "Departmental Surcharge starts the third")):
        if i:
            para.add_line_break()
        run = para.add_run()
        run.text = line
        run.font.size, run.font.name = Pt(36), "Arial"
    save(p, "pay-centre-reduced-line-breaks",
         "One 36pt paragraph on three lines, broken after '–' and after 'and'; no words run together")

    # DCK-004, reduced from the same deck's figure tiles (#1397): a white
    # separator rule between each tile's two figures is a connector
    # (p:cxnSp, the `line` preset). The reader skipped connectors, so the
    # rules were not drawn. A diagonal one checks it is drawn corner to
    # corner, not as its box.
    from pptx.enum.shapes import MSO_CONNECTOR

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    for i in range(3):
        left = Inches(0.8 + 4 * i)
        box = s.shapes.add_shape(MSO_SHAPE.RECTANGLE, left, Inches(1.5), Inches(3.4), Inches(2.4))
        box.fill.solid()
        box.fill.fore_color.rgb = RGBColor(23, 42, 69)
        box.line.fill.background()
        rule = s.shapes.add_connector(MSO_CONNECTOR.STRAIGHT, left + Inches(0.4), Inches(2.7), left + Inches(3.0), Inches(2.7))
        rule.line.color.rgb = RGBColor(255, 255, 255)
        rule.line.width = Pt(1.5)
    slash = s.shapes.add_connector(MSO_CONNECTOR.STRAIGHT, Inches(1), Inches(4.6), Inches(12), Inches(6.6))
    slash.line.color.rgb = RGBColor(200, 30, 30)
    slash.line.width = Pt(3)
    save(p, "pay-centre-reduced-connector-rules",
         "Three navy tiles, each split by a white 1.5pt rule across its middle; one red 3pt diagonal below them")
    # DCK-004, reduced from the same deck's figure tiles (#1397): each tile
    # is a top-anchored box that opens with two empty 14pt paragraphs and
    # then the figure. Dropping the leading paragraphs drew the figure at
    # the top of the tile instead of two lines down.
    from pptx.enum.text import MSO_ANCHOR, PP_ALIGN

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    for i, figure in enumerate(("6,046", "1,288", "312")):
        box = s.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(0.8 + 4 * i), Inches(1.5), Inches(3.4), Inches(2.4))
        box.fill.solid()
        box.fill.fore_color.rgb = RGBColor(23, 42, 69)
        box.line.fill.background()
        tf = box.text_frame
        tf.vertical_anchor = MSO_ANCHOR.TOP
        tf.word_wrap = True
        for k in range(3):
            para = tf.paragraphs[0] if k == 0 else tf.add_paragraph()
            para.alignment = PP_ALIGN.CENTER
            if k < 2:
                para._p.get_or_add_endParaRPr().set("sz", "1400")
                continue
            run = para.add_run()
            run.text = figure
            run.font.size, run.font.bold, run.font.name = Pt(14), True, "Arial"
            run.font.color.rgb = RGBColor(255, 255, 255)
    save(p, "pay-centre-reduced-leading-blank-paragraphs",
         "Three navy tiles, each with a bold white 14pt figure two blank lines below the tile's top edge")

    # DCK-004, reduced from the 2024-2025 delegated authorities report: its
    # theme was saved on a machine in a dark mode, so lt1 is the system
    # "window" colour with lastClr="202020". The master's background is
    # bg1 (lt1), and the window is white: the slide must stay white.
    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    tb = s.shapes.add_textbox(Inches(1), Inches(1), Inches(8), Inches(1.5)).text_frame
    fixed_box(tb)
    tb.text = "Dark text on the theme's white window background"
    tb.paragraphs[0].runs[0].font.size = Pt(32)
    tb.paragraphs[0].runs[0].font.color.rgb = RGBColor(0x54, 0x57, 0x5A)
    themes = [part for part in p.part.package.iter_parts() if str(part.partname).startswith("/ppt/theme/")]
    assert themes, "python-pptx's template has a theme part"
    import re
    for part in themes:
        xml = part.blob.decode("utf-8")
        dark = re.sub(r"<a:lt1>.*?</a:lt1>", '<a:lt1><a:sysClr val="window" lastClr="202020"/></a:lt1>', xml, count=1, flags=re.S)
        assert dark != xml, "the template's theme names lt1"
        part._blob = dark.encode("utf-8")
    save(p, "delegated-authorities-reduced-dark-mode-window",
         "A white slide (theme lt1 = system window, saved with lastClr 202020) with grey 32pt text")

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    rows, cols = 3, 3
    t = s.shapes.add_table(rows, cols, Inches(1), Inches(1), Inches(8), Inches(3)).table
    for r in range(rows):
        for c in range(cols):
            t.cell(r, c).text = f"R{r + 1}C{c + 1}"
    save(p, "table", "A 3x3 table with a styled header row")

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    sh = s.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(5), Inches(2.5), Inches(3), Inches(2))
    sh.rotation = 30
    save(p, "rotation", "A rectangle rotated 30 degrees clockwise")


def letters_odt_page_break_spacing():
    """Reduce real-explanatory-note-2's 24pt loss at a master switch.

    Keep identical page geometry and text looks so the report measures
    paragraph spacing, without the source's lists, headers and footers.
    """
    from zipfile import ZIP_STORED, ZipFile

    directory = os.path.join(OUT, "letters")
    os.makedirs(directory, exist_ok=True)
    path = os.path.join(directory, "odt-page-break-spacing.odt")
    namespaces = (
        'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
        'xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" '
        'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
        'xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" '
        'xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" '
        'office:version="1.3"'
    )
    content = f'''<?xml version="1.0" encoding="UTF-8"?>
<office:document-content {namespaces}>
  <office:automatic-styles>
    <style:style style:name="First" style:family="paragraph"
                 style:parent-style-name="Spaced" style:master-page-name="One"/>
    <style:style style:name="Break" style:family="paragraph" style:parent-style-name="Spaced">
      <style:paragraph-properties fo:break-before="page"/>
    </style:style>
    <style:style style:name="Switch" style:family="paragraph"
                 style:parent-style-name="Spaced" style:master-page-name="Two">
      <style:paragraph-properties fo:break-before="page"/>
    </style:style>
  </office:automatic-styles>
  <office:body><office:text>
    <text:p text:style-name="First">First page: 24pt before.</text:p>
    <text:p text:style-name="Break">Explicit page break: 24pt before.</text:p>
    <text:p text:style-name="Switch">New page master: 24pt before.</text:p>
  </office:text></office:body>
</office:document-content>'''
    styles = f'''<?xml version="1.0" encoding="UTF-8"?>
<office:document-styles {namespaces}>
  <office:font-face-decls>
    <style:font-face style:name="Liberation Serif" svg:font-family="Liberation Serif"/>
  </office:font-face-decls>
  <office:styles>
    <style:style style:name="Spaced" style:family="paragraph">
      <style:paragraph-properties fo:margin-top="24pt" fo:margin-bottom="0pt"/>
      <style:text-properties style:font-name="Liberation Serif" fo:font-size="12pt"/>
    </style:style>
  </office:styles>
  <office:automatic-styles>
    <style:page-layout style:name="Page">
      <style:page-layout-properties fo:page-width="432pt" fo:page-height="432pt"
                                   fo:margin="72pt"/>
    </style:page-layout>
  </office:automatic-styles>
  <office:master-styles>
    <style:master-page style:name="One" style:page-layout-name="Page"/>
    <style:master-page style:name="Two" style:page-layout-name="Page"/>
  </office:master-styles>
</office:document-styles>'''
    manifest = '''<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"
                   manifest:version="1.3">
  <manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/>
  <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
  <manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>
</manifest:manifest>'''
    with ZipFile(path, "w") as package:
        package.writestr("mimetype", "application/vnd.oasis.opendocument.text", compress_type=ZIP_STORED)
        package.writestr("META-INF/manifest.xml", manifest)
        package.writestr("content.xml", content)
        package.writestr("styles.xml", styles)
    add("letters", "odt-page-break-spacing", path,
        "Three pages with 24pt paragraph space before: document start, explicit break, then a new page master")


def main():
    os.makedirs(OUT, exist_ok=True)
    img = test_image(os.path.join(OUT, "test-image.png"))
    letters(img)
    letters_odt_page_break_spacing()
    letters_floating_image_gap(img)
    tables(img)
    decks(img)
    with open(os.path.join(OUT, "manifest.json"), "w") as f:
        json.dump(MANIFEST, f, indent=2)
    print(f"{len(MANIFEST)} fixtures -> {OUT}")


if __name__ == "__main__":
    main()
