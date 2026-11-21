"""lo_render.unscaled_copy prints a Tables reference at its cells' own size:
an xlsx sheet's print scale and fit to page, and an ods page layout's
scale-to, come out of the copy LibreOffice prints."""

import importlib.util
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def _lo_render():
    spec = importlib.util.spec_from_file_location("lo_render", ROOT / "tools/render-lab/lo_render.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _package(path, parts):
    with zipfile.ZipFile(path, "w") as z:
        for name, text in parts.items():
            z.writestr(name, text)
    return str(path)


def _read(path, name):
    with zipfile.ZipFile(path) as z:
        return z.read(name).decode()


def test_an_xlsx_sheets_print_scale_and_fit_go(tmp_path):
    src = _package(tmp_path / "s.xlsx", {
        "xl/worksheets/sheet1.xml": '<worksheet><sheetPr><pageSetUpPr fitToPage="1"/></sheetPr>'
        '<pageSetup paperSize="9" scale="46" fitToHeight="0" orientation="landscape"/></worksheet>',
    })
    out = _lo_render().unscaled_copy(src, str(tmp_path))
    sheet = _read(out, "xl/worksheets/sheet1.xml")
    assert "scale=" not in sheet and "fitTo" not in sheet
    assert 'orientation="landscape"' in sheet, "the rest of the page setup stays"


def test_an_ods_page_layouts_scale_goes(tmp_path):
    src = _package(tmp_path / "s.ods", {
        "mimetype": "application/vnd.oasis.opendocument.spreadsheet",
        "styles.xml": '<style:page-layout style:name="pm1"><style:page-layout-properties '
        'style:print-orientation="portrait" style:scale-to="80%"/></style:page-layout>'
        '<style:page-layout style:name="pm2"><style:page-layout-properties style:scale-to-X="1" style:scale-to-Y="2"/></style:page-layout>',
    })
    out = _lo_render().unscaled_copy(src, str(tmp_path))
    styles = _read(out, "styles.xml")
    assert "scale-to" not in styles
    assert 'style:print-orientation="portrait"' in styles


def test_a_workbook_that_prints_at_its_own_size_is_used_as_is(tmp_path):
    src = _package(tmp_path / "s.ods", {"styles.xml": "<style:page-layout-properties style:print-orientation=\"portrait\"/>"})
    assert _lo_render().unscaled_copy(src, str(tmp_path)) == src
