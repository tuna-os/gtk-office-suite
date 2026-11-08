"""Wrap evidence measures pixels and row growth without choosing word breaks."""

import importlib.util
from pathlib import Path
from unittest import mock

import pytest

pytest.importorskip("numpy")
pytest.importorskip("PIL")
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("wrap_compare", ROOT / "tools/render-lab/compare.py")
compare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compare)


def capture(path, *, reference=False, height=80, lines=(5, 23, 41), widths=(7, 5, 6)):
    image = Image.new("RGB", (120, height), "white")
    draw = ImageDraw.Draw(image)
    if reference:
        # Calc's printed headings and outer frame.
        for x in (0, 20, 119):
            draw.line((x, 0, x, height - 1), fill="black")
        for y in (0, 20, height - 1):
            draw.line((0, y, 119, y), fill="black")
    else:
        draw.rectangle((0, 0, 119, 19), fill=(240, 240, 240))
        draw.rectangle((0, 0, 19, height - 1), fill=(240, 240, 240))
    for i, y in enumerate(lines):
        for glyph in range(widths[i % len(widths)]):
            draw.rectangle((23 + glyph * 10, 21 + y, 28 + glyph * 10, 27 + y), fill="black")
    image.save(path)


@pytest.fixture
def pictures(tmp_path):
    capture(tmp_path / "lo-1.png", reference=True)
    for tier in ("A", "B"):
        # Same three lines, different word breaks/line widths.
        capture(tmp_path / f"{tier}-1.png", widths=(7, 4, 7))
    return tmp_path


def good_metrics():
    return dict(ink=1.14, words=1.0, lost_lines=0, disp_pt=3.8, colors=None, ssim=0.43, ref_words=8)


def score(pictures, tier="A", app="tables", feature="wrap-text", metrics=None):
    with mock.patch.object(compare, "reference_words", return_value=[]), mock.patch.object(
        compare, "compare_page", return_value=metrics or good_metrics()
    ):
        return compare.score_fixture(app, str(pictures), tier, {}, feature)


@pytest.mark.parametrize("tier", ["A", "B"])
def test_three_lines_and_grown_row_replace_only_ssim(pictures, tier):
    result = score(pictures, tier)
    assert result["verdict"] == "green"
    assert result["ssim"] == 0.43
    assert result["wrap_structure"]["passed"]
    assert result["wrap_structure"]["ours"]["lines"] == 3
    assert result["wrap_structure"]["ours"]["row_height"] == 60


@pytest.mark.parametrize(
    "changes",
    [dict(lines=()), dict(lines=(5,)), dict(lines=(5, 23)), dict(lines=(2, 15, 28, 41)),
     dict(height=60, lines=(1, 13, 25)), dict(lines=(5, 23, 52))],
)
def test_missing_extra_clipped_lines_or_ungrown_row_fail_even_with_high_ssim(pictures, changes):
    capture(pictures / "A-1.png", **changes)
    result = score(pictures, metrics=dict(good_metrics(), ssim=1.0))
    assert not result["wrap_structure"]["passed"]
    assert result["verdict"] == "amber"


def test_reference_must_also_show_wrap(pictures):
    capture(pictures / "lo-1.png", reference=True, lines=(5,))
    assert not score(pictures)["wrap_structure"]["passed"]


def test_missing_framing_fails_closed(pictures):
    Image.new("RGB", (120, 80), "white").save(pictures / "A-1.png")
    assert not score(pictures)["wrap_structure"]["passed"]


def test_absent_capture_remains_missing(pictures):
    (pictures / "A-1.png").unlink()
    assert score(pictures) is None


@pytest.mark.parametrize("changes", [dict(words=0.6), dict(lost_lines=1), dict(disp_pt=9), dict(colors=0.1), dict(ink=0.01)])
def test_structural_success_keeps_other_metric_gates(pictures, changes):
    result = score(pictures, metrics=dict(good_metrics(), **changes))
    assert result["wrap_structure"]["passed"]
    assert result["verdict"] != "green"


@pytest.mark.parametrize("app,feature", [("tables", "alignment"), ("letters", "wrap-text"), ("tables", None)])
def test_other_fixtures_still_require_ssim(pictures, app, feature):
    result = score(pictures, app=app, feature=feature)
    assert "wrap_structure" not in result
    assert result["verdict"] == "amber"


def test_small_gaps_within_glyphs_are_one_line(pictures):
    path = pictures / "A-1.png"
    image = Image.open(path)
    ImageDraw.Draw(image).line((20, 47, 119, 47), fill="white")
    image.save(path)
    assert score(pictures)["wrap_structure"]["passed"]


def test_main_publishes_structural_evidence_in_report_and_summary(pictures, monkeypatch):
    import json
    import shutil

    out = pictures / "run"
    destination = out / "tables" / "wrap-text"
    destination.mkdir(parents=True)
    for path in pictures.glob("*.png"):
        shutil.copy(path, destination / path.name)
    fixtures = pictures / "fixtures"
    fixtures.mkdir()
    (fixtures / "manifest.json").write_text(json.dumps([
        dict(app="tables", feature="wrap-text", file="tables/wrap-text.xlsx", expect="Three lines; row grows")
    ]))
    monkeypatch.setattr(compare, "reference_words", lambda *args: [])
    monkeypatch.setattr(compare, "compare_page", lambda *args: good_metrics())
    monkeypatch.setattr("sys.argv", ["compare.py", str(fixtures), str(out)])
    compare.main()
    card = json.loads((out / "scorecard.json").read_text())
    summary = json.loads((out / "summary.json").read_text())
    for tier in ("A", "B"):
        assert card["tables/wrap-text"][tier]["wrap_structure"]["passed"]
        assert summary["fixtures"][0]["metrics"][tier]["wrap_structure"]["ours"]["lines"] == 3
        assert f"Tier {tier} wrap check: pass" in (out / "report.html").read_text()


def test_structural_success_does_not_hide_scale_regression(pictures):
    align = compare.align

    def oversized(*args):
        ref, ours, _ = align(*args)
        return ref, ours, 1.5

    with mock.patch.object(compare, "align", oversized):
        result = score(pictures)
    assert result["wrap_structure"]["passed"]
    assert result["verdict"] == "amber"
