"""Letters render-lab fixtures (docx and odt).

Split out of fixtures.py (a merge hotspot). Shared state (OUT, MANIFEST,
add, test_image) lives in fixtures.py; it binds itself here as `core` when
it loads, so setting fixtures.OUT still redirects every fixture.
"""

import os
import re

core = None  # the fixtures module; bound by fixtures.py at import


# ── Letters (docx) ────────────────────────────────────────────────────────
def letters(img):
    from docx import Document
    from docx.enum.section import WD_ORIENT
    from docx.enum.text import WD_ALIGN_PARAGRAPH, WD_BREAK
    from docx.oxml import parse_xml
    from docx.shared import Inches, Pt, RGBColor

    d = os.path.join(core.OUT, "letters")
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
        core.add("letters", name, path, expect)

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
    path = os.path.join(core.OUT, "letters", "floating-image-gap.odt")
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
    core.add("letters", "floating-image-gap", path,
        "Three lines touch a floating image's right edge (zero clearance); the fourth starts at the left margin")


def letters_odt_page_break_spacing():
    """Reduce real-explanatory-note-2's 24pt loss at a master switch.

    Keep identical page geometry and text looks so the report measures
    paragraph spacing, without the source's lists, headers and footers.
    """
    from zipfile import ZIP_STORED, ZipFile

    directory = os.path.join(core.OUT, "letters")
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
    core.add("letters", "odt-page-break-spacing", path,
        "Three pages with 24pt paragraph space before: document start, explicit break, then a new page master")


def letters_odt_list_hanging():
    """A list level whose own hanging is not Letters' 18pt (#1630).

    Word's "1.1" numbering hangs its number 0.5in before text at 0.5in,
    kept on the level's label alignment rather than on the paragraph.
    Long items wrap, so the text edge and the marker are both measured.
    """
    from zipfile import ZIP_STORED, ZipFile

    directory = os.path.join(core.OUT, "letters")
    os.makedirs(directory, exist_ok=True)
    path = os.path.join(directory, "odt-list-hanging.odt")
    namespaces = (
        'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
        'xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" '
        'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
        'xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" '
        'xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" '
        'office:version="1.3"'
    )
    item = "Resolved that the committee notes the report and the auditors' findings, " * 3
    content = f'''<?xml version="1.0" encoding="UTF-8"?>
<office:document-content {namespaces}>
  <office:automatic-styles>
    <text:list-style style:name="L1">
      <text:list-level-style-number text:level="1" style:num-format="1" style:num-suffix=".">
        <style:list-level-properties text:list-level-position-and-space-mode="label-alignment">
          <style:list-level-label-alignment text:label-followed-by="listtab"
            text:list-tab-stop-position="0.5in" fo:margin-left="0.5in" fo:text-indent="-0.5in"/>
        </style:list-level-properties>
      </text:list-level-style-number>
    </text:list-style>
  </office:automatic-styles>
  <office:body><office:text>
    <text:list text:style-name="L1">
      <text:list-item><text:p text:style-name="Body">{item}</text:p></text:list-item>
      <text:list-item><text:p text:style-name="Body">{item}</text:p></text:list-item>
    </text:list>
  </office:text></office:body>
</office:document-content>'''
    styles = f'''<?xml version="1.0" encoding="UTF-8"?>
<office:document-styles {namespaces}>
  <office:font-face-decls>
    <style:font-face style:name="Liberation Serif" svg:font-family="Liberation Serif"/>
  </office:font-face-decls>
  <office:styles>
    <style:style style:name="Body" style:family="paragraph">
      <style:paragraph-properties fo:margin-top="0pt" fo:margin-bottom="12pt"/>
      <style:text-properties style:font-name="Liberation Serif" fo:font-size="12pt"/>
    </style:style>
  </office:styles>
  <office:automatic-styles>
    <style:page-layout style:name="Page">
      <style:page-layout-properties fo:page-width="8.5in" fo:page-height="11in" fo:margin="1in"/>
    </style:page-layout>
  </office:automatic-styles>
  <office:master-styles>
    <style:master-page style:name="Standard" style:page-layout-name="Page"/>
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
    core.add("letters", "odt-list-hanging", path,
        "Two numbered items whose numbers sit at the left margin, their text and wrapped lines 0.5in in")
