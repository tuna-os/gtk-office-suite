"""Decks render-lab fixtures (pptx).

Split out of fixtures.py (a merge hotspot). Shared state (OUT, MANIFEST,
add, test_image) lives in fixtures.py; it binds itself here as `core` when
it loads, so setting fixtures.OUT still redirects every fixture.
"""

import os

from PIL import Image, ImageDraw

core = None  # the fixtures module; bound by fixtures.py at import


# ── Decks (pptx) ──────────────────────────────────────────────────────────
def decks(img):
    from pptx import Presentation
    from pptx.dml.color import RGBColor
    from pptx.enum.shapes import MSO_SHAPE
    from pptx.oxml.ns import qn
    from pptx.util import Inches, Pt

    d = os.path.join(core.OUT, "decks")
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
        core.add("decks", name, path, expect)

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

    # Line spacing (#2353): Office's themes set text at 90%; a paragraph may
    # set its own. Three wrapped paragraphs at 90%, 100% and 150%, side by
    # side, so each column's lines sit at their own pitch.
    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    words = "Line spacing sets how far apart the lines of one paragraph sit, from tight to open."
    for i, spacing in enumerate((0.9, 1.0, 1.5)):
        tf = s.shapes.add_textbox(Inches(0.6 + i * 4.2), Inches(1.5), Inches(3.8), Inches(4.5)).text_frame
        fixed_box(tf)
        para = tf.paragraphs[0]
        para.line_spacing = spacing
        r = para.add_run()
        r.text = words
        r.font.size = Pt(24)
    save(p, "line-spacing", "Three 24pt paragraphs wrapped over several lines at 90%, 100% and 150% line spacing")

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    for kind, x, color in ((MSO_SHAPE.RECTANGLE, 1, (220, 40, 40)), (MSO_SHAPE.OVAL, 5, (40, 160, 60)), (MSO_SHAPE.ROUNDED_RECTANGLE, 9, (40, 80, 220))):
        sh = s.shapes.add_shape(kind, Inches(x), Inches(2), Inches(3), Inches(2))
        sh.fill.solid()
        sh.fill.fore_color.rgb = RGBColor(*color)
    save(p, "shapes", "Red rectangle, green ellipse, blue rounded rectangle in a row")

    # Shape effects (#2332): an outer shadow straight down-right, a blurred
    # one, a shadow fixed to the slide under a turned shape, and a glow.
    # python-pptx has no effects API, so each a:effectLst is written into
    # the shape's spPr, after its fill, where CT_ShapeProperties puts it.
    from pptx.oxml import parse_xml

    a_ns = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"'
    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    effects = (
        (MSO_SHAPE.RECTANGLE, 1.0, 0, '<a:outerShdw dist="152400" dir="2700000" algn="ctr" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="60000"/></a:srgbClr></a:outerShdw>'),
        (MSO_SHAPE.OVAL, 4.2, 0, '<a:outerShdw blurRad="190500" dist="114300" dir="5400000" algn="ctr" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="50000"/></a:srgbClr></a:outerShdw>'),
        (MSO_SHAPE.RECTANGLE, 7.4, 30, '<a:outerShdw dist="152400" dir="2700000" algn="ctr" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="60000"/></a:srgbClr></a:outerShdw>'),
        (MSO_SHAPE.OVAL, 10.6, 0, '<a:glow rad="190500"><a:srgbClr val="00B0F0"><a:alpha val="60000"/></a:srgbClr></a:glow>'),
    )
    for kind, x, turn, effect in effects:
        sh = s.shapes.add_shape(kind, Inches(x), Inches(2.5), Inches(2.2), Inches(2.0))
        sh.fill.solid()
        sh.fill.fore_color.rgb = RGBColor(40, 80, 220)
        sh.line.fill.background()
        sh.rotation = turn
        sh._element.spPr.append(parse_xml(f"<a:effectLst {a_ns}>{effect}</a:effectLst>"))
    save(p, "shape-effects", "Blue shapes with a shadow, a blurred shadow, a turned shape's slide-fixed shadow, and a glow")

    # Text on shapes (the pay-centre innovation deck): an oval's text wraps
    # in its inscribed rectangle, in the white its style's fontRef gives it
    # (python-pptx's autoshapes carry `a:fontRef idx="minor"` with lt1),
    # and Wingdings bullets with no margin to hang in (marL 0, indent
    # -285750) draw as the squares and diamonds they show, beside the text.
    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    for x, words in ((6.8, "Since January 2024, the team processed 22,478 cases using bulk processing and the tool."),
                     (10.0, "Total of approximately 2,938 hours of manual work avoided")):
        sh = s.shapes.add_shape(MSO_SHAPE.OVAL, Inches(x), Inches(2.2), Inches(3.0), Inches(2.4))
        sh.fill.solid()
        sh.fill.fore_color.rgb = RGBColor(0x1F, 0x38, 0x64)
        sh.text_frame.word_wrap = True
        r = sh.text_frame.paragraphs[0].add_run()
        r.text = words
        r.font.size = Pt(16)
    box = s.shapes.add_textbox(Inches(0.8), Inches(2.2), Inches(5.6), Inches(2.4))
    box.text_frame.word_wrap = True
    for i, (char, words) in enumerate((("§", "14,634 Cases - Inside Service Standard"), ("§", "15,573 Cases - Aged Intake"),
                                       ("§", "42,898 Cases - Backlog over a year"), ("v", "Reduction of 21,675 cases"))):
        para = box.text_frame.paragraphs[0] if i == 0 else box.text_frame.add_paragraph()
        para._p.insert(0, parse_xml(
            f'<a:pPr {a_ns} indent="-285750"><a:buFont typeface="Wingdings" pitchFamily="2" charset="2"/><a:buChar char="{char}"/></a:pPr>'))
        r = para.add_run()
        r.text = words
        r.font.size = Pt(18)
    save(p, "shape-text", "White text wrapped inside two navy ovals; four Wingdings-bulleted lines (three squares, a diamond) beside their text")

    # Translucent fills (#2350): 79 shape fills across 12 real decks carry
    # a:alpha, as the CNSC roundtable's boxes at 75% over a navy slide. Two
    # overlapping boxes at 50% and 75% over a navy band, so both the band
    # and each other show through.
    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    band = s.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(0), Inches(1.5), Inches(13.333), Inches(4.5))
    band.fill.solid()
    band.fill.fore_color.rgb = RGBColor(0x16, 0x26, 0x3E)
    band.line.fill.background()
    for x, rgb, alpha in ((1.5, "C0504D", 50000), (4.5, "F2C14E", 75000)):
        sh = s.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(x), Inches(0.8), Inches(4.5), Inches(4.0))
        sh.line.fill.background()
        # After the geometry, before the outline, as CT_ShapeProperties orders them.
        sh._element.spPr.find(qn("a:prstGeom")).addnext(parse_xml(f'<a:solidFill {a_ns}><a:srgbClr val="{rgb}"><a:alpha val="{alpha}"/></a:srgbClr></a:solidFill>'))
    # python-pptx's p:style names the theme's shadow (effectRef idx 2),
    # which LibreOffice draws; the fills are what this fixture is about.
    for sh in s.shapes:
        sh._element.remove(sh._element.find(qn("p:style")))
    save(p, "translucent-fill", "A red box at 50% and a gold box at 75% overlap each other and a navy band that shows through them")

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
    # DCK-004, reduced from the HR and reporting decks: the rule under every
    # slide's title is on the layout, as a connector (p:cxnSp) or as a
    # `line` preset p:sp with zero height. The master/layout reader took
    # neither, so the rule was missing from every slide.
    from lxml import etree

    p = deck()
    layout = p.slide_layouts[6]
    ns = 'xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"'
    rules = [
        f"""<p:cxnSp {ns}><p:nvCxnSpPr><p:cNvPr id="90" name="Title rule"/><p:cNvCxnSpPr/><p:nvPr userDrawn="1"/></p:nvCxnSpPr>
        <p:spPr><a:xfrm><a:off x="731520" y="1371600"/><a:ext cx="10728960" cy="0"/></a:xfrm><a:prstGeom prst="line"><a:avLst/></a:prstGeom>
        <a:ln w="38100"><a:solidFill><a:srgbClr val="1F4E79"/></a:solidFill></a:ln></p:spPr></p:cxnSp>""",
        f"""<p:sp {ns}><p:nvSpPr><p:cNvPr id="91" name="Footer rule"/><p:cNvSpPr/><p:nvPr userDrawn="1"/></p:nvSpPr>
        <p:spPr><a:xfrm><a:off x="731520" y="6126480"/><a:ext cx="10728960" cy="0"/></a:xfrm><a:prstGeom prst="line"><a:avLst/></a:prstGeom>
        <a:ln w="19050"><a:solidFill><a:srgbClr val="C00000"/></a:solidFill></a:ln></p:spPr></p:sp>""",
    ]
    tree = layout.shapes._spTree
    for xml in rules:
        tree.append(etree.fromstring(xml))
    s = p.slides.add_slide(layout)
    tb = s.shapes.add_textbox(Inches(0.8), Inches(0.5), Inches(10), Inches(0.9)).text_frame
    fixed_box(tb)
    tb.text = "Layout rules"
    tb.paragraphs[0].runs[0].font.size = Pt(32)
    save(p, "layout-rules",
         "A 32pt title over a navy 3pt rule across the slide, and a red 1.5pt rule near the bottom; both rules come from the layout")
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

    # DCK-004, reduced from the children's nature survey's side charts: a
    # horizontal bar chart (first category at the top) on a dark panel, its
    # bars and text in a theme colour (bg1, white), labelled as
    # percentages. Decks drew vertical columns in the default accent with
    # raw fractions (0.72) for labels and black text.
    from pptx.chart.data import CategoryChartData
    from pptx.enum.chart import XL_CHART_TYPE, XL_LABEL_POSITION
    from pptx.enum.dml import MSO_THEME_COLOR
    from lxml import etree

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    panel = s.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(5), 0, Inches(5), p.slide_height)
    panel.fill.solid()
    panel.fill.fore_color.rgb = RGBColor(0x2E, 0x5E, 0x2E)
    panel.line.fill.background()
    data = CategoryChartData()
    data.categories = ["Garden", "Park", "Woods", "Beach", "Other"]
    data.add_series("Share", (0.72, 0.64, 0.27, 0.26, 0.01), number_format="0%")
    chart = s.shapes.add_chart(XL_CHART_TYPE.BAR_CLUSTERED, Inches(5.3), Inches(0.8), Inches(4.4), Inches(5.6), data).chart
    chart.has_legend = False
    chart.category_axis.reverse_order = True
    chart.value_axis.visible = False
    chart.value_axis.has_major_gridlines = False
    plot = chart.plots[0]
    plot.has_data_labels = True
    plot.data_labels.number_format = "0%"
    plot.data_labels.number_format_is_linked = False
    plot.data_labels.position = XL_LABEL_POSITION.OUTSIDE_END
    fill = plot.series[0].format.fill
    fill.solid()
    fill.fore_color.theme_color = MSO_THEME_COLOR.BACKGROUND_1
    chart.font.size = Pt(12)
    chart.font.color.theme_color = MSO_THEME_COLOR.BACKGROUND_1
    chart.has_title = False
    # No chart background, as the survey's: the panel shows through.
    space = chart._chartSpace
    space.chart.addnext(etree.fromstring(
        '<c:spPr xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" '
        'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr>'))
    save(p, "chart-horizontal-percent",
         "On a green panel, five white horizontal bars, Garden at the top to Other at the bottom, each labelled in white with its percentage (72% to 1%)")

    # DCK-004, reduced from the adults' nature survey: its illustration of
    # people walking in a park is mirrored (a:xfrm flipH="1" on the
    # picture). Decks drew it the way the file stores it, facing away.
    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    plain = s.shapes.add_picture(img, Inches(0.5), Inches(1.5), Inches(4), Inches(2.4))
    flipped = s.shapes.add_picture(img, Inches(5.5), Inches(1.5), Inches(4), Inches(2.4))
    flipped._element.spPr.get_or_add_xfrm().set("flipH", "1")
    save(p, "picture-flip",
         "The test picture twice: as stored on the left, mirrored left to right on the right")

    # DCK-004, reduced from the HR and pay monthly status report: the
    # master carries a plain text box (not a placeholder) holding a
    # slide-number field, cached as "‹#›". Decks drew that cache, grey and
    # faint, on every slide instead of the slide's number.
    from lxml import etree

    p = deck()
    master = p.slide_master
    master.shapes._spTree.append(etree.fromstring(
        '<p:sp xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" '
        'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">'
        '<p:nvSpPr><p:cNvPr id="90" name="Page number"/><p:cNvSpPr txBox="1"/><p:nvPr userDrawn="1"/></p:nvSpPr>'
        '<p:spPr><a:xfrm><a:off x="8001000" y="6172200"/><a:ext cx="914400" cy="369332"/></a:xfrm>'
        '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr>'
        '<p:txBody><a:bodyPr wrap="none"><a:spAutoFit/></a:bodyPr><a:lstStyle/><a:p><a:pPr algn="r"/>'
        '<a:fld id="{8670F7E3-4535-4F13-B622-75272AD0E151}" type="slidenum"><a:rPr lang="en-CA" sz="1800" b="1">'
        '<a:solidFill><a:srgbClr val="1F4E79"/></a:solidFill></a:rPr><a:t>\u2039#\u203a</a:t></a:fld></a:p></p:txBody></p:sp>'))
    s = p.slides.add_slide(p.slide_layouts[6])
    tb = s.shapes.add_textbox(Inches(1), Inches(1), Inches(8), Inches(1)).text_frame
    fixed_box(tb)
    tb.text = "A slide numbered by its master"
    tb.paragraphs[0].runs[0].font.size = Pt(28)
    save(p, "master-slide-number",
         "28pt text at the top left; a bold navy 18pt '1' at the bottom right, the master's slide-number field")

    # DCK-004, reduced from the CCRM deck's closing slide: its section
    # layout overrides the master's colour map (bg1 = dk1, tx1 = lt1), so
    # the slide is navy with white text, and its icon is saved only as SVG
    # (asvg:svgBlip, no PNG on the blip). Decks drew a white slide with
    # dark text and no icon.
    import re
    from zipfile import ZIP_DEFLATED, ZipFile

    p = deck()
    layout = p.slide_layouts[6]
    s = p.slides.add_slide(layout)
    tb = s.shapes.add_textbox(Inches(1), Inches(0.6), Inches(8), Inches(1.2)).text_frame
    fixed_box(tb)
    tb.text = "Questions"
    run = tb.paragraphs[0].runs[0]
    run.font.size, run.font.bold = Pt(40), True
    rpr = run._r.get_or_add_rPr()
    rpr.append(etree.fromstring('<a:solidFill xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:schemeClr val="tx1"/></a:solidFill>'))
    s.shapes.add_picture(img, Inches(3.5), Inches(2.2), Inches(3), Inches(3))
    path = os.path.join(d, "section-colour-map-svg.pptx")
    p.save(path)
    icon = (
        '<?xml version="1.0" encoding="UTF-8"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 96 96">'
        '<rect x="30" y="8" width="58" height="40" rx="4" fill="#00c0f0"/><circle cx="30" cy="58" r="12" fill="#00c0f0"/>'
        '<path d="M8 92a22 18 0 0 1 44 0z" fill="#00c0f0"/></svg>'
    )
    with ZipFile(path) as zin:
        items = [(i, zin.read(i.filename)) for i in zin.infolist()]
    media = next(i.filename for i, _ in items if i.filename.startswith("ppt/media/"))
    svg_name = re.sub(r"\.\w+$", ".svg", media)
    layout_part = "ppt/slideLayouts/slideLayout7.xml"
    with ZipFile(path, "w", ZIP_DEFLATED) as zout:
        for info, data in items:
            name = info.filename
            if name == media:
                zout.writestr(svg_name, icon)
                continue
            if name.endswith(".rels") or name.endswith(".xml"):
                text = data.decode("utf-8")
                text = text.replace(media.split("/")[-1], svg_name.split("/")[-1])
                if name == "[Content_Types].xml" and 'Extension="svg"' not in text:
                    text = text.replace("<Default ", '<Default Extension="svg" ContentType="image/svg+xml"/><Default ', 1)
                if name.startswith("ppt/slides/slide") and name.endswith(".xml"):
                    text = re.sub(
                        r'<a:blip r:embed="(\w+)"\s*/>',
                        r'<a:blip><a:extLst><a:ext uri="{96DAC541-7B7A-43D3-8B79-37D633B846F1}"><asvg:svgBlip xmlns:asvg="http://schemas.microsoft.com/office/drawing/2016/SVG/main" r:embed="\1"/></a:ext></a:extLst></a:blip>',
                        text,
                    )
                if name == layout_part:
                    text = re.sub(r"(<p:cSld[^>]*>)", r'\1<p:bg><p:bgPr><a:solidFill><a:schemeClr val="bg1"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>', text, count=1)
                    text = re.sub(
                        r"<p:clrMapOvr>.*?</p:clrMapOvr>",
                        '<p:clrMapOvr><a:overrideClrMapping bg1="dk1" tx1="lt1" bg2="dk2" tx2="lt2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:clrMapOvr>',
                        text,
                        flags=re.S,
                    )
                data = text.encode("utf-8")
            zout.writestr(info, data)
    core.add("decks", "section-colour-map-svg", path,
        "A dark slide (its layout maps bg1 to dk1) with a white bold 40pt 'Questions' and a cyan SVG speech-bubble icon below it")

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

    # A one-cell table as a text panel (the HR delegations deck's
    # objectives): its style draws the first row as a white bold header,
    # but the runs state black and regular, one phrase bold pink, and the
    # cell states its own wide margins. Each run's own colour and weight,
    # and the cell's margins, win over the style.
    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    panel = s.shapes.add_table(1, 1, Inches(1.3), Inches(1.7), Inches(10.5), Inches(4.2)).table
    cell = panel.cell(0, 0)
    cell.margin_left, cell.margin_top = Inches(0.7), Inches(0.5)
    cell.fill.solid()
    cell.fill.fore_color.rgb = RGBColor(0xD9, 0xD9, 0xD9)
    tf = cell.text_frame
    for i, (lead, key, tail) in enumerate((("Review the ", "laws and policies", " relevant to the role"),
                                           ("Discuss ", "emerging priorities", " in management"))):
        para = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
        for text, highlight in ((lead, False), (key, True), (tail, False)):
            r = para.add_run()
            r.text = text
            r.font.size = Pt(28)
            r.font.bold = highlight
            r.font.color.rgb = RGBColor(0xD5, 0x00, 0x57) if highlight else RGBColor(0, 0, 0)
    save(p, "table-text-panel", "A grey one-cell table: two lines of black regular 28pt text, each with a bold pink phrase, inset 0.7 in")

    p = deck()
    s = p.slides.add_slide(p.slide_layouts[6])
    sh = s.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(5), Inches(2.5), Inches(3), Inches(2))
    sh.rotation = 30
    save(p, "rotation", "A rectangle rotated 30 degrees clockwise")
