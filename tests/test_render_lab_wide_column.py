"""The SFI26 reduction keeps the oversized column and print settings."""

import importlib.util
from pathlib import Path

import pytest

pytest.importorskip("PIL")
openpyxl = pytest.importorskip("openpyxl")

ROOT = Path(__file__).resolve().parents[1]


def _load_module(name, rel_path):
    spec = importlib.util.spec_from_file_location(name, ROOT / rel_path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def test_wide_column_fixture_is_a_single_oversized_unheaded_sheet(tmp_path):
    fixtures = _load_module("render_fixtures", "tools/render-lab/fixtures.py")
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
    assert all(sheet.cell(row, 1).font.name == "Liberation Sans" for row in range(1, 21))


def test_align_clips_wide_unheaded_sheet_to_shared_width_and_vertical_scale(tmp_path):
    import numpy as np
    from PIL import Image

    compare = _load_module("render_compare", "tools/render-lab/compare.py")

    # Build a synthetic unheaded wide-column pair:
    # - LibreOffice reference (794x1123) has text lines starting at (68, 80)
    #   and running to the right edge of the page (x=793, width=726, height=400).
    # - Our capture (1100x430) has screen headers (40px wide, 20px tall) and
    #   the same text lines starting at (40, 26) and running to the right edge
    #   of the viewport (x=1099, width=1060, height=400).
    ref_img = np.full((1123, 794, 3), 255, dtype=np.uint8)
    ours_img = np.full((430, 1100, 3), 255, dtype=np.uint8)
    ours_img[:20, :] = 242
    ours_img[:, :40] = 242

    for row in range(10):
        y_ref = 80 + row * 40
        y_ours = 26 + row * 40
        for col_block in range(0, 1050, 25):
            x_end_ours = min(1100, 40 + col_block + 15)
            ours_img[y_ours : y_ours + 12, 40 + col_block : x_end_ours] = 20
            if 68 + col_block < 794:
                x_end_ref = min(794, 68 + col_block + 15)
                ref_img[y_ref : y_ref + 12, 68 + col_block : x_end_ref] = 20
        # Ensure ink reaches within 4px of the right edge on both sides.
        ref_img[y_ref : y_ref + 12, 791:794] = 20
        ours_img[y_ours : y_ours + 12, 1097:1100] = 20

    ref_path = tmp_path / "lo-1.png"
    ours_path = tmp_path / "A-1.png"
    Image.fromarray(ref_img).save(ref_path)
    Image.fromarray(ours_img).save(ours_path)

    ref_aligned, ours_aligned, scale = compare.align("tables", str(ref_path), str(ours_path))
    assert abs(scale - 1.0) <= 0.02
    assert ref_aligned.size == ours_aligned.size
    assert compare.ssim(ref_aligned, ours_aligned) >= 0.95


def test_align_preserves_unclipped_or_ruled_sheets(tmp_path):
    import numpy as np
    from PIL import Image

    compare = _load_module("render_compare", "tools/render-lab/compare.py")

    # When the capture is not horizontally clipped at the viewport edge,
    # align() preserves its full width ratio.
    ref_img = np.full((300, 794, 3), 255, dtype=np.uint8)
    ref_img[40:120, 68:368] = 20
    ours_img = np.full((140, 500, 3), 255, dtype=np.uint8)
    ours_img[:20, :] = 242
    ours_img[:, :40] = 242
    ours_img[25:105, 45:345] = 20

    ref_path = tmp_path / "lo-unclipped.png"
    ours_path = tmp_path / "A-unclipped.png"
    Image.fromarray(ref_img).save(ref_path)
    Image.fromarray(ours_img).save(ours_path)

    _, _, scale = compare.align("tables", str(ref_path), str(ours_path))
    expected_scale = (500 / 300 + 140 / 80) / 2
    assert abs(scale - expected_scale) < 1e-6
