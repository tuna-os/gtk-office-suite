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
import sys

from PIL import Image, ImageDraw

OUT = sys.argv[1] if len(sys.argv) > 1 else "render-lab-out/fixtures"
MANIFEST = []

# Fixtures that cannot go green until a Phase 1 architecture item exists
# (docs/RENDER-PARITY-ROADMAP.md). Their issues are labelled blocked so
# agents pick unblocked work first instead of hacking around the gap.
NEEDS = {
    "letters/page-margins": "letters-page-layout",
    "letters/landscape": "letters-page-layout",
    "letters/header-footer": "letters-page-layout",
    "letters/page-break": "letters-page-layout",
    "letters/pagination": "letters-page-layout",
    "letters/table": "letters-page-layout",
    "tables/cell-fonts": "tables-cell-style-model",
    "tables/fills": "tables-cell-style-model",
    "tables/alignment": "tables-cell-style-model",
    "tables/wrap-text": "tables-cell-style-model",
    "decks/shapes": "decks-shape-style-model",
    "decks/title-layout": "decks-shape-style-model",
    "decks/bullets": "decks-shape-style-model",
    "decks/table": "decks-shape-style-model",
}


def add(app, feature, path, expect):
    MANIFEST.append(
        {
            "app": app,
            "feature": feature,
            "file": os.path.relpath(path, OUT),
            "expect": expect,
            "needs": NEEDS.get(f"{app}/{feature}"),
        }
    )


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

    x = doc()
    t = x.add_table(rows=3, cols=3)
    t.style = "Table Grid"
    for r in range(3):
        for c in range(3):
            t.cell(r, c).text = f"R{r + 1}C{c + 1}"
    save(x, "table", "A 3x3 table with visible grid borders and cell text")

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
    for i in range(24):
        x.add_paragraph(f"Filler line {i + 1}.")
    p = x.add_paragraph()
    p.add_run("Chapter Two").bold = True
    p.paragraph_format.keep_with_next = True
    # One line: a longer follower brings in orphan control, where
    # LibreOffice gives up the keep (see the roadmap's known differences).
    x.add_paragraph("The chapter begins here.")
    save(x, "keep-with-next", "The bold 'Chapter Two' line is kept with the paragraph after it: both start page 2, page 1 ends at 'Filler line 24.' (25 lines fit a page with the template's paragraph spacing)")

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


# ── Tables (xlsx) ─────────────────────────────────────────────────────────
def tables():
    from openpyxl import Workbook
    from openpyxl.formatting.rule import CellIsRule
    from openpyxl.styles import Alignment, Border, Font, PatternFill, Side

    d = os.path.join(OUT, "tables")
    os.makedirs(d, exist_ok=True)

    def book():
        wb = Workbook()
        return wb, wb.active

    def save(wb, name, expect):
        # Print headings so LibreOffice's PDF shows the same cells as our
        # grid, but not gridlines: they are view furniture, which Calc
        # prints black and screens draw faint, and a printed gridline hides
        # a thin cell border drawn on top of it. capture.py turns Tables'
        # "Show gridlines" off to match, so both sides show the cells'
        # own borders, fills and text.
        for sheet in wb.worksheets:
            sheet.print_options.gridLines = False
            sheet.print_options.headings = True
        path = os.path.join(d, f"{name}.xlsx")
        wb.save(path)
        add("tables", name, path, expect)

    wb, ws = book()
    for r in range(1, 6):
        for c in range(1, 5):
            ws.cell(r, c, r * 10 + c)
    save(wb, "values", "A 5x4 block of integers, right-aligned in their cells")

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


# ── Decks (pptx) ──────────────────────────────────────────────────────────
def decks(img):
    from pptx import Presentation
    from pptx.dml.color import RGBColor
    from pptx.enum.shapes import MSO_SHAPE
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
    from pptx.oxml.ns import qn
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

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    s.shapes.add_picture(img, Inches(3), Inches(2), width=Inches(5))
    save(p, "image", "The four-colour test image, 5in wide, at (3in, 2in)")

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    s.background.fill.solid()
    s.background.fill.fore_color.rgb = RGBColor(30, 30, 60)
    tb = s.shapes.add_textbox(Inches(1), Inches(1), Inches(6), Inches(1)).text_frame
    fixed_box(tb)
    tb.text = "Light text on dark background"
    tb.paragraphs[0].runs[0].font.color.rgb = RGBColor(255, 255, 255)
    save(p, "background", "Dark navy slide background with white text")

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


def main():
    os.makedirs(OUT, exist_ok=True)
    img = test_image(os.path.join(OUT, "test-image.png"))
    letters(img)
    tables()
    decks(img)
    with open(os.path.join(OUT, "manifest.json"), "w") as f:
        json.dump(MANIFEST, f, indent=2)
    print(f"{len(MANIFEST)} fixtures -> {OUT}")


if __name__ == "__main__":
    main()
