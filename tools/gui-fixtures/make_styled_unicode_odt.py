#!/usr/bin/env python3
"""Writes tests/gui/fixtures/styled-unicode.odt (#1681).

Hand-authored ODF, not made by Letters: CJK, combining marks, emoji, RTL and
mixed Latin, run styles (bold, italic, colour and underline), paragraph styles
(centred, indented, 150% line height), a footnote, bullet and numbered lists,
a table, an inline image, a comment, a tracked insertion, a table of contents,
a header and footer, and an A4 page with its own margins. The last paragraph
is the one the Save As journey edits.

styled-unicode.docx is LibreOffice Writer's own conversion of this file:
  soffice --headless --convert-to docx:"MS Word 2007 XML" styled-unicode.odt

Usage: make_styled_unicode_odt.py OUT.odt
"""
import zipfile, base64, sys
PNG=base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAYAAACp8Z5+AAAAFElEQVR4nGP8z8Dwn4EIwDiqEF8AAFTrAxO3gaHsAAAAAElFTkSuQmCC")
NS=('xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" '
 'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" '
 'xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" '
 'xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" '
 'xmlns:loext="urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0"')
styles=f'''<?xml version="1.0" encoding="UTF-8"?><office:document-styles {NS} office:version="1.3">
<office:font-face-decls><style:font-face style:name="Liberation Sans" svg:font-family="'Liberation Sans'"/></office:font-face-decls>
<office:styles>
<style:default-style style:family="paragraph"><style:text-properties style:font-name="Liberation Sans" fo:font-size="11pt"/></style:default-style>
<style:style style:name="Standard" style:family="paragraph" style:class="text"/>
<style:style style:name="Heading_20_1" style:display-name="Heading 1" style:family="paragraph" style:parent-style-name="Standard" style:default-outline-level="1"><style:paragraph-properties fo:margin-top="0.17in" fo:margin-bottom="0.08in"/><style:text-properties fo:font-size="16pt" fo:font-weight="bold"/></style:style>
<style:style style:name="Heading_20_2" style:display-name="Heading 2" style:family="paragraph" style:parent-style-name="Standard" style:default-outline-level="2"><style:text-properties fo:font-size="13pt" fo:font-weight="bold"/></style:style>
<style:style style:name="Header" style:family="paragraph" style:parent-style-name="Standard"/>
<style:style style:name="Footer" style:family="paragraph" style:parent-style-name="Standard"/>
</office:styles>
<office:automatic-styles><style:page-layout style:name="pm1"><style:page-layout-properties fo:page-width="8.2681in" fo:page-height="11.6929in" fo:margin-top="0.7874in" fo:margin-bottom="0.7874in" fo:margin-left="0.9in" fo:margin-right="0.9in"/><style:header-style/><style:footer-style/></style:page-layout></office:automatic-styles>
<office:master-styles><style:master-page style:name="Standard" style:page-layout-name="pm1"><style:header><text:p text:style-name="Header">Kopfzeile ✓ שלום</text:p></style:header><style:footer><text:p text:style-name="Footer">Fußzeile 見出し</text:p></style:footer></style:master-page></office:master-styles></office:document-styles>'''
content=f'''<?xml version="1.0" encoding="UTF-8"?><office:document-content {NS} office:version="1.3">
<office:automatic-styles>
<style:style style:name="B" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
<style:style style:name="I" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>
<style:style style:name="Red" style:family="text"><style:text-properties fo:color="#cc0000" style:text-underline-style="solid" style:text-underline-width="auto" style:text-underline-color="font-color"/></style:style>
<style:style style:name="Ctr" style:family="paragraph" style:parent-style-name="Standard"><style:paragraph-properties fo:text-align="center" fo:margin-top="0.1in" fo:margin-bottom="0.1in"/></style:style>
<style:style style:name="Ind" style:family="paragraph" style:parent-style-name="Standard"><style:paragraph-properties fo:margin-left="0.5in" fo:text-indent="0.2in" fo:line-height="150%"/></style:style>
<style:style style:name="Fr" style:family="graphic"><style:graphic-properties style:wrap="none" style:vertical-pos="top" style:horizontal-pos="center" text:anchor-type="as-char"/></style:style>
<style:style style:name="Tbl" style:family="table"><style:table-properties style:width="5.5in" table:align="left"/></style:style>
<style:style style:name="Cell" style:family="table-cell"><style:table-cell-properties fo:padding="0.04in" fo:border="0.5pt solid #000000"/></style:style>
<text:list-style style:name="Bul"><text:list-level-style-bullet text:level="1" text:bullet-char="•"><style:list-level-properties text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment text:label-followed-by="listtab" text:list-tab-stop-position="0.5in" fo:text-indent="-0.25in" fo:margin-left="0.5in"/></style:list-level-properties></text:list-level-style-bullet></text:list-style>
<text:list-style style:name="Num"><text:list-level-style-number text:level="1" style:num-suffix="." style:num-format="1"><style:list-level-properties text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment text:label-followed-by="listtab" text:list-tab-stop-position="0.5in" fo:text-indent="-0.25in" fo:margin-left="0.5in"/></style:list-level-properties></text:list-level-style-number></text:list-style>
</office:automatic-styles>
<office:body><office:text>
<text:tracked-changes text:track-changes="false"><text:changed-region text:id="ct1"><text:insertion><office:change-info><dc:creator>Alice</dc:creator><dc:date>2026-09-01T10:00:00</dc:date></office:change-info></text:insertion></text:changed-region></text:tracked-changes>
<text:h text:style-name="Heading_20_1" text:outline-level="1">Überschrift 見出し</text:h>
<text:p text:style-name="Standard">Plain, <text:span text:style-name="B">bold café</text:span>, <text:span text:style-name="I">italic e&#x301;</text:span>, <text:span text:style-name="Red">red underlined</text:span> 😀 emoji 👩&#x200d;💻.</text:p>
<text:p text:style-name="Standard">שלום עולם and مرحبا بالعالم mixed with Latin text.</text:p>
<text:p text:style-name="Ctr">A centred paragraph with 日本語のテキスト.</text:p>
<text:p text:style-name="Ind">An indented paragraph with 150% line height and a footnote<text:note text:id="ftn1" text:note-class="footnote"><text:note-citation>1</text:note-citation><text:note-body><text:p text:style-name="Standard">The note says naïve café 😀.</text:p></text:note-body></text:note> inside it.</text:p>
<text:list text:style-name="Bul"><text:list-item><text:p text:style-name="Standard">First bullet</text:p></text:list-item><text:list-item><text:p text:style-name="Standard">Second bullet שלום</text:p></text:list-item></text:list>
<text:list text:style-name="Num"><text:list-item><text:p text:style-name="Standard">First number</text:p></text:list-item><text:list-item><text:p text:style-name="Standard">Second number</text:p></text:list-item></text:list>
<table:table table:name="T1" table:style-name="Tbl"><table:table-column table:number-columns-repeated="2"/>
<table:table-row><table:table-cell table:style-name="Cell" office:value-type="string"><text:p text:style-name="Standard">Cell A1 üñí</text:p></table:table-cell><table:table-cell table:style-name="Cell" office:value-type="string"><text:p text:style-name="Standard">Cell B1 😀</text:p></table:table-cell></table:table-row>
<table:table-row><table:table-cell table:style-name="Cell" office:value-type="string"><text:p text:style-name="Standard">Cell A2 日本</text:p></table:table-cell><table:table-cell table:style-name="Cell" office:value-type="string"><text:p text:style-name="Standard">Cell B2 שלום</text:p></table:table-cell></table:table-row></table:table>
<text:p text:style-name="Standard"><draw:frame draw:style-name="Fr" draw:name="Image1" text:anchor-type="as-char" svg:width="0.5in" svg:height="0.5in"><draw:image xlink:href="Pictures/p.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/></draw:frame> An inline image.</text:p>
<text:p text:style-name="Standard">A <office:annotation office:name="c1"><dc:creator>Bob</dc:creator><dc:date>2026-09-02T09:30:00</dc:date><text:p>Check this sentence 😀</text:p></office:annotation>commented stretch<office:annotation-end office:name="c1"/> and a <text:change-start text:change-id="ct1"/>tracked insertion<text:change-end text:change-id="ct1"/>.</text:p>
<text:table-of-content text:name="TOC1" text:protected="true"><text:table-of-content-source text:outline-level="3"><text:index-title-template text:style-name="Standard">Contents</text:index-title-template></text:table-of-content-source><text:index-body><text:index-title text:name="TOC1_Head"><text:p text:style-name="Standard">Contents</text:p></text:index-title><text:p text:style-name="Standard">Überschrift 見出し<text:tab/>1</text:p></text:index-body></text:table-of-content>
<text:h text:style-name="Heading_20_2" text:outline-level="2">Second heading</text:h>
<text:p text:style-name="Standard">The edit target paragraph.</text:p>
</office:text></office:body></office:document-content>'''
man='''<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="Pictures/p.png" manifest:media-type="image/png"/></manifest:manifest>'''
out=sys.argv[1]
with zipfile.ZipFile(out,'w') as z:
    z.writestr(zipfile.ZipInfo('mimetype'),'application/vnd.oasis.opendocument.text')
    for n,d in (('content.xml',content),('styles.xml',styles),('META-INF/manifest.xml',man)): z.writestr(n,d,zipfile.ZIP_DEFLATED)
    z.writestr('Pictures/p.png',PNG)
