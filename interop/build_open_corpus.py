#!/usr/bin/env python3
"""Generate real files with LibreOffice for app-level open/edit/save journeys.

Sources are authored for this project (GPL-3.0-or-later). Converted fixtures
record the installed LibreOffice version; they are not Microsoft-authored.
Never writes into an existing output directory or a user's office documents.
"""

import argparse
import csv
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
from xml.sax.saxutils import escape
import zipfile

UNICODE = "Café e\u0301 日本語 हिन्दी שלום مرحبا 😀"
EXPORT_FILTERS = {
    "docx": "Office Open XML Text", "odt": "writer8",
    "xlsx": "Calc MS Excel 2007 XML", "xls": "MS Excel 97", "ods": "calc8",
    "pptx": "Impress MS PowerPoint 2007 XML", "odp": "impress8",
}
NS = ('xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
      'xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" '
      'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
      'xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" '
      'xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" '
      'xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" '
      'xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" '
      'xmlns:of="urn:oasis:names:tc:opendocument:xmlns:of:1.2"')


def flat_document(kind, body):
    return (f'<?xml version="1.0" encoding="UTF-8"?><office:document {NS} '
            f'office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.{kind}">'
            f'<office:body><office:{kind}>{body}</office:{kind}></office:body></office:document>')


def spreadsheet(sheets):
    result = []
    for name, rows in sheets:
        cells = []
        for row in rows:
            values = []
            for value in row:
                if isinstance(value, int):
                    values.append(f'<table:table-cell office:value-type="float" office:value="{value}"><text:p>{value}</text:p></table:table-cell>')
                else:
                    values.append(f'<table:table-cell office:value-type="string"><text:p>{escape(value)}</text:p></table:table-cell>')
            cells.append('<table:table-row>' + ''.join(values) + '</table:table-row>')
        result.append(f'<table:table table:name="{escape(name)}">' + ''.join(cells) + '</table:table>')
    return flat_document("spreadsheet", ''.join(result))


def presentation(texts, shapes=False):
    slides = []
    for index, text in enumerate(texts):
        shape = ('<draw:rect svg:x="2cm" svg:y="5cm" svg:width="4cm" svg:height="2cm"/>'
                 '<draw:ellipse svg:x="8cm" svg:y="5cm" svg:width="3cm" svg:height="3cm"/>') if shapes else ''
        slides.append(f'<draw:page draw:name="Slide {index + 1}">'
                      '<draw:frame svg:x="2cm" svg:y="2cm" svg:width="20cm" svg:height="2cm">'
                      f'<draw:text-box><text:p>{escape(text)}</text:p></draw:text-box></draw:frame>'
                      f'{shape}</draw:page>')
    return flat_document("presentation", ''.join(slides))


def source_cases():
    cases = []
    for name, content in (
        ("plain", "<p>Letters plain fixture</p>"),
        ("unicode", f"<p>Letters unicode fixture</p><p>{UNICODE}</p>"),
        ("rich", '<h1>Letters rich fixture</h1><p><b>bold</b> <i>italic</i> <u>underline</u></p><ul><li>first</li><li>second</li></ul>'),
        ("table", '<p>Letters table fixture</p><table border="1"><tr><td>Name</td><td>Value</td></tr><tr><td>Budget</td><td>42</td></tr></table>'),
        ("long", '<p>Letters long fixture</p>' + '<p>A long document paragraph with several words.</p>' * 200),
    ):
        marker = f"Letters {name} fixture"
        cases.append(dict(id=f"letters-{name}", app="letters", source_ext="html",
                          source=f'<html><head><meta charset="utf-8"></head><body>{content}</body></html>',
                          formats=["odt", "docx"], markers=[marker]))
    for name, sheets in (
        ("numbers", [("Budget", [["Tables numbers fixture", "Value"], ["Income", 4200], ["Cost", 1700]])]),
        ("unicode", [("Budget", [["Tables unicode fixture", UNICODE], ["Line", "First\nSecond"]])]),
        ("multisheet", [("Budget", [["Tables multisheet fixture", 42]]), ("Forecast", [["Second sheet", 84]])]),
    ):
        cases.append(dict(id=f"tables-{name}", app="tables", source_ext="fods",
                          source=spreadsheet(sheets), formats=["xlsx", "ods", "xls"],
                          markers=[f"Tables {name} fixture"], sheet_names=[name for name, _ in sheets]))
    for name, texts, shapes in (
        ("plain", ["Decks plain fixture"], False),
        ("unicode", [f"Decks unicode fixture {UNICODE}"], False),
        ("multislide", ["Decks multislide fixture", "Second slide", "Third slide"], False),
        ("shapes", ["Decks shapes fixture"], True),
    ):
        cases.append(dict(id=f"decks-{name}", app="decks", source_ext="fodp",
                          source=presentation(texts, shapes), formats=["pptx", "odp"],
                          markers=[f"Decks {name} fixture"], slide_count=len(texts)))
    return cases


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build(output, soffice="soffice", timeout=60, flatpak=False):
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    command = [soffice]
    if flatpak:
        command = ["flatpak", "run", f"--filesystem={output}",
                   "--command=/app/libreoffice/program/soffice", "org.libreoffice.LibreOffice"]
    version = subprocess.check_output([*command, "--version"], text=True, timeout=timeout).strip()
    fixtures = []
    files = output / "files"
    files.mkdir()
    sources = output / "sources"
    sources.mkdir()

    def record(path, app, markers, **metadata):
        fixtures.append(dict(id=path.stem + "-" + path.suffix[1:], app=app,
                             path=path.relative_to(output).as_posix(), sha256=digest(path),
                             format=path.suffix[1:].lower(), markers=markers,
                             expected="open", license="GPL-3.0-or-later", **metadata))

    # Keep the profile beside the files so sandboxed LibreOffice wrappers
    # can see both when the output is in a shared workspace (host /tmp may
    # not be the application's /tmp).
    with tempfile.TemporaryDirectory(prefix=".lo-profile-", dir=output) as profile:
        for case in source_cases():
            source = sources / f"{case['id']}.{case['source_ext']}"
            source.write_text(case["source"], encoding="utf-8")
            for extension in case["formats"]:
                target = files / f"{case['id']}.{extension}"
                result = subprocess.run(
                    [*command, "--headless", f"-env:UserInstallation={Path(profile).as_uri()}",
                     "--convert-to", f"{extension}:{EXPORT_FILTERS[extension]}",
                     "--outdir", str(files), str(source)],
                    capture_output=True, text=True, timeout=timeout,
                )
                (sources / f"{case['id']}-{extension}.log").write_text(result.stdout + result.stderr)
                if result.returncode or not target.is_file() or target.stat().st_size == 0:
                    raise RuntimeError(f"LibreOffice failed to create {target}: {result.stdout} {result.stderr}")
                # A converter exit code alone is not evidence of a valid package.
                if extension != "xls":
                    with zipfile.ZipFile(target) as package:
                        if package.testzip() is not None:
                            raise RuntimeError(f"Corrupt generated package: {target}")
                metadata = {key: case[key] for key in ("sheet_names", "slide_count") if key in case}
                record(target, case["app"], case["markers"], authoring=version,
                       source=source.relative_to(output).as_posix(), source_sha256=digest(source), **metadata)

    for name, extension, content in (
        ("letters-plain", "txt", "Letters text fixture\n" + UNICODE),
        ("letters-rich", "md", "# Letters markdown fixture\n\n**bold** and *italic*\n\n" + UNICODE),
        ("letters-html", "html", '<html><body><p>Letters HTML fixture</p></body></html>'),
    ):
        target = files / f"{name}.{extension}"
        target.write_text(content, encoding="utf-8")
        record(target, "letters", [f"Letters {'text' if extension == 'txt' else 'markdown' if extension == 'md' else 'HTML'} fixture"], authoring="project text fixture")

    for name, delimiter, extension, rows in (
        ("quoted", ",", "csv", [["Tables CSV fixture", "comma,value"], ["quoted", 'a "quote"'], ["multiline", "first\nsecond"]]),
        ("unicode", ",", "csv", [["Tables Unicode CSV fixture", UNICODE], ["value", "42"]]),
        ("tabs", "\t", "tsv", [["Tables TSV fixture", "value"], ["row", "42"]]),
    ):
        text = io.StringIO(newline="")
        csv.writer(text, delimiter=delimiter).writerows(rows)
        target = files / f"tables-{name}.{extension}"
        target.write_bytes(text.getvalue().encode("utf-8"))
        record(target, "tables", [rows[0][0]], authoring="project CSV/TSV fixture")

    # Same content under path variants isolates detection/path bugs from parsing.
    for app, extension in (("letters", "docx"), ("tables", "xlsx"), ("decks", "pptx")):
        source = next(item for item in fixtures if item["app"] == app and item["format"] == extension)
        target = files / f"{app} résumé 日本語 with spaces.{extension.upper()}"
        shutil.copyfile(output / source["path"], target)
        record(target, app, source["markers"], authoring=source["authoring"], variant_of=source["id"])
        for variant, payload in (("empty", b""), ("truncated", b"PK\x03\x04broken")):
            target = files / f"{app}-{variant}.{extension}"
            target.write_bytes(payload)
            record(target, app, [], authoring="project malformed fixture")
            fixtures[-1]["expected"] = "error"
    manifest = dict(schema_version=1, producer=version, fixtures=fixtures)
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
    print(f"Created {len(fixtures)} real files at {output}; producer: {version}")
    return manifest


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--soffice", default="soffice")
    parser.add_argument("--flatpak", action="store_true", help="use LibreOffice Flatpak with explicit corpus access")
    args = parser.parse_args()
    build(args.output, args.soffice, flatpak=args.flatpak)
