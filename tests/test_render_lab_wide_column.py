"""The SFI26 reduction keeps the oversized column and print settings."""

import importlib.util
from pathlib import Path

import pytest

pytest.importorskip("PIL")
openpyxl = pytest.importorskip("openpyxl")

ROOT = Path(__file__).resolve().parents[1]


def test_wide_column_fixture_is_a_single_oversized_unheaded_sheet(tmp_path):
    spec = importlib.util.spec_from_file_location("render_fixtures", ROOT / "tools/render-lab/fixtures.py")
    fixtures = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixtures)
    fixtures.OUT = str(tmp_path)
    fixtures.tables()

    entry = next(e for e in fixtures.MANIFEST if e["feature"] == "wide-column-no-headings")
    assert entry["app"] == "tables"
    assert entry["file"] == "tables/wide-column-no-headings.xlsx"
    workbook = openpyxl.load_workbook(tmp_path / entry["file"])
    assert len(workbook.worksheets) == 1
    sheet = workbook.active
    assert sheet.column_dimensions["A"].width == 200
    assert sheet.print_options.headings is False
    assert sheet.print_options.gridLines is False
    assert (sheet.max_row, sheet.max_column) == (20, 1)
    assert all(len(sheet.cell(row, 1).value) > 120 for row in range(1, 21))
