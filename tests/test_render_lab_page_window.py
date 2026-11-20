"""compare.window_to_page cuts a Tables capture to the printed page it is
scored against, placed by the words both sides read."""

import importlib.util
from pathlib import Path

import pytest

pytest.importorskip("numpy")
Image = pytest.importorskip("PIL.Image")

ROOT = Path(__file__).resolve().parents[1]


def _compare():
    spec = importlib.util.spec_from_file_location("render_compare", ROOT / "tools/render-lab/compare.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def test_capture_holding_more_than_the_page_is_cut_to_it(monkeypatch):
    compare = _compare()
    ref = Image.new("RGB", (100, 400), "white")
    ours = Image.new("RGB", (600, 900), "white")
    # The page sits at (10, 20) in ours, drawn 1.05x as large.
    monkeypatch.setattr(compare, "page_in_capture", lambda r, o: (10, 20, 115, 440, 1.05))
    _, cut, scale = compare.window_to_page(ref, ours, 4.2)
    assert cut.size == (105, 420)
    assert scale == pytest.approx(1.05)


def test_capture_no_larger_than_the_page_is_scored_whole(monkeypatch):
    compare = _compare()
    ref = Image.new("RGB", (500, 400), "white")
    ours = Image.new("RGB", (540, 420), "white")
    monkeypatch.setattr(compare, "page_in_capture", lambda r, o: (0, 0, 520, 410, 1.03))
    _, same, scale = compare.window_to_page(ref, ours, 1.04)
    assert same is ours and scale == 1.04


def test_without_paired_words_the_capture_is_scored_whole(monkeypatch):
    compare = _compare()
    ref = Image.new("RGB", (100, 100), "white")
    ours = Image.new("RGB", (900, 900), "white")
    monkeypatch.setattr(compare, "page_in_capture", lambda r, o: None)
    _, same, scale = compare.window_to_page(ref, ours, 9.0)
    assert same is ours and scale == 9.0
