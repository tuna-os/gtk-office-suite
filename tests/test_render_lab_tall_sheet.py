"""The HMT Core Tables reduction isolates vertical multipage split and unprinted headings."""

import importlib.util
from pathlib import Path

import pytest

pytest.importorskip("PIL")
openpyxl = pytest.importorskip("openpyxl")

ROOT = Path(__file__).resolve().parents[1]


def test_tall_unprinted_sheet_fixture_is_a_multipage_unheaded_sheet(tmp_path):
    spec = importlib.util.spec_from_file_location("render_fixtures", ROOT / "tools/render-lab/fixtures.py")
    fixtures = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixtures)
    fixtures.OUT = str(tmp_path)
    fixtures.tables()

    entry = next(e for e in fixtures.MANIFEST if e["feature"] == "tall-unprinted-sheet")
    assert entry["app"] == "tables"
    assert entry["file"] == "tables/tall-unprinted-sheet.xlsx"
    workbook = openpyxl.load_workbook(tmp_path / entry["file"])
    assert len(workbook.worksheets) == 1
    sheet = workbook.active
    assert sheet.print_options.headings is False
    assert sheet.print_options.gridLines is False
    assert sheet.max_row == 74
    assert sheet.max_column == 6
    assert sheet.cell(1, 1).value == "HM Treasury Core Financial Summary"
    assert sheet.cell(3, 1).value == "Category"
    assert sheet.cell(3, 6).value == "2025-26"
    assert sheet.cell(4, 1).value == "Programme expenditure line 01"
    assert sheet.cell(74, 1).value == "Programme expenditure line 71"
