"""Unit tests for conformance/validate_parity.py — the E1–E4 parity ratchet.

All pure functions; no repo-state or network needed. These pin the claim
ratchet semantics that gate the conformance scorecard (#96): green rows
need evidence, evidence must cite real paths, journey-gated features need
journey proof, and green↔non-green transitions require evidence changes.
"""

import os
import sys
from pathlib import Path

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'conformance'))

import validate_parity as vp  # noqa: E402


# ── strip_markers / norm ─────────────────────────────────────────────────

def test_strip_markers_removes_all():
    # strip_markers removes the markers but leaves the surrounding spaces;
    # collapsing whitespace is norm()'s job.
    assert vp.strip_markers("✅ a ⚠️ b ❓ c") == "a  b  c"
    assert vp.norm("✅ a ⚠️ b ❓ c") == "a b c"


def test_norm_lowercases_and_collapses():
    assert vp.norm("  **Hello**   World  ") == "hello world"


# ── parse_parity ─────────────────────────────────────────────────────────

PARITY_SAMPLE = """# TunaOS office suite parity

## letters — suite-common migration

### Tier 1
| Feature | Evidence | State |
|---|---|---|
| undo/redo | ✅ I6 | ✅ |
| save | ✅ tests/gui/test_letters.py | ✅ |
| export | ❌ | ❌ |

### Tier 3
| Feature | Evidence |
|---|---|
| legacy shim | — |
"""


def test_parse_parity_yields_rows(tmp_path):
    p = tmp_path / "PARITY.md"
    p.write_text(PARITY_SAMPLE)
    rows = vp.parse_parity(p)
    assert len(rows) == 4, rows  # header row + 3 data rows
    # rows[0] is the table header (parsed like any | line); data starts at [1].
    app, tier, feature, _, _ = rows[1]
    assert app == "letters"
    assert tier == 1
    assert feature == "undo/redo"


def test_parse_parity_skips_tier3(tmp_path):
    p = tmp_path / "PARITY.md"
    p.write_text(PARITY_SAMPLE)
    rows = vp.parse_parity(p)
    assert all(tier != 3 for _, tier, *_ in rows)


# ── has_marker / path_tokens ─────────────────────────────────────────────

def test_has_marker():
    assert vp.has_marker("✅ done")
    assert vp.has_marker("⚠️ partial")
    assert not vp.has_marker("plain text")


def test_path_tokens_strips_punctuation():
    # PATH_RE is rooted in KNOWN_DIRS; a trailing / is part of the token.
    toks = list(vp.path_tokens("see letters-core/tests/corpus/, and letters-core/tests/corpus.rs."))
    assert "letters-core/tests/corpus/" in toks
    assert "letters-core/tests/corpus.rs" in toks


# ── evidence_exists ──────────────────────────────────────────────────────

def test_evidence_exists_finds_real_path(tmp_path):
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "PARITY.md").write_text("x")
    missing = vp.evidence_exists("see docs/PARITY.md", tmp_path)
    assert missing == []


def test_evidence_exists_prefix_allowed(tmp_path):
    (tmp_path / "letters-core").mkdir()
    (tmp_path / "letters-core" / "tests").mkdir()
    (tmp_path / "letters-core" / "tests" / "corpus").mkdir()
    missing = vp.evidence_exists("letters-core/tests/corpus", tmp_path)
    assert missing == []


def test_evidence_exists_reports_missing(tmp_path):
    # Only KNOWN_DIRS-rooted tokens are checked; non-root prose is ignored.
    missing = vp.evidence_exists("docs/nope.md", tmp_path)
    assert missing == ["docs/nope.md"]


# ── gated_feature ────────────────────────────────────────────────────────

def test_gated_feature():
    assert vp.gated_feature("undo/redo")
    assert vp.gated_feature("multi-sheet (2 sheets)")
    assert vp.gated_feature("preferences")
    assert not vp.gated_feature("cell borders")


# ── check_evidence (E1/E2/E3) ────────────────────────────────────────────

def _row(feature, evidence, marker="✅"):
    return ("letters", 1, feature, f"| {feature} | {evidence} | {marker} |",
            [feature, evidence, marker])


def test_e1_empty_evidence_on_green(tmp_path):
    errors, warnings = [], []
    vp.check_evidence(_row("save", ""), tmp_path, errors, warnings)
    assert any("E1" in e and "empty" in e for e in errors)


def test_e1_evidence_without_test_or_path(tmp_path):
    errors, warnings = [], []
    # "manually verified" cites no test/instrument/path (note: the word
    # "tested" alone WOULD satisfy EVIDENCE_RE, so use neutral wording).
    vp.check_evidence(_row("save", "verified by hand"), tmp_path, errors, warnings)
    assert any("cites no test" in e for e in errors)


def test_e1_keyword_evidence_accepted(tmp_path):
    errors, warnings = [], []
    vp.check_evidence(_row("save", "verified by hand"), tmp_path, errors, warnings)
    assert any("cites no test" in e for e in errors)
    errors.clear()
    vp.check_evidence(_row("save", "covered by round-trip fixture"), tmp_path, errors, warnings)
    assert not errors


def test_e2_missing_path(tmp_path):
    errors, warnings = [], []
    vp.check_evidence(_row("save", "see docs/nope.md"), tmp_path, errors, warnings)
    assert any("E2" in e and "does not exist" in e for e in errors)


def test_e3_journey_gated_without_journey(tmp_path):
    errors, warnings = [], []
    vp.check_evidence(_row("undo/redo", "I1 model"), tmp_path, errors, warnings)
    assert any("E3" in e and "I6" in e for e in errors)


def test_e3_journey_gated_with_journey_ok(tmp_path):
    errors, warnings = [], []
    vp.check_evidence(_row("undo/redo", "✅ I6"), tmp_path, errors, warnings)
    assert not errors


def test_non_green_row_not_checked(tmp_path):
    errors, warnings = [], []
    vp.check_evidence(_row("save", "", "❌"), tmp_path, errors, warnings)
    assert errors == []


# ── check_transitions (E4) ───────────────────────────────────────────────

def _parity_md(tmp_path, rows, name="PARITY.md"):
    p = tmp_path / name
    lines = ["## letters — x", "", "### Tier 1", "| Feature | Evidence | State |", "|---|---|---|"]
    lines += [f"| {f} | {e} | {s} |" for f, e, s in rows]
    p.write_text("\n".join(lines))
    return p


def test_e4_green_regressed_without_evidence_change(tmp_path):
    base = _parity_md(tmp_path, [("save", "I1", "✅")], "BASE.md")
    head = _parity_md(tmp_path, [("save", "I1", "❌")], "HEAD.md")
    errors, warnings = [], []
    vp.check_transitions(base, vp.parse_parity(head), errors)
    assert any("E4" in e and "regressed" in e for e in errors)


def test_e4_jumped_green_without_new_evidence(tmp_path):
    base = _parity_md(tmp_path, [("save", "I1", "❌")], "BASE.md")
    head = _parity_md(tmp_path, [("save", "I1", "✅")], "HEAD.md")
    errors, warnings = [], []
    vp.check_transitions(base, vp.parse_parity(head), errors)
    assert any("E4" in e and "jumped to green" in e for e in errors)


def test_e4_evidence_change_allows_transition(tmp_path):
    base = _parity_md(tmp_path, [("save", "I1", "❌")], "BASE.md")
    head = _parity_md(tmp_path, [("save", "I1 + I6", "✅")], "HEAD.md")
    errors, warnings = [], []
    vp.check_transitions(base, vp.parse_parity(head), errors)
    assert errors == []


# ── validate() entry point ───────────────────────────────────────────────

def test_validate_missing_file_returns_2(tmp_path):
    assert vp.validate(tmp_path / "nope.md") == 2


def test_validate_clean_file_returns_0(tmp_path):
    p = tmp_path / "PARITY.md"
    # Every green row carries evidence citing a test/instrument; non-green
    # rows are not checked.
    clean = """## letters — x

### Tier 1
| Feature | Evidence | State |
|---|---|---|
| undo/redo | I6 journey | ✅ |
| save | I1 round-trip fixture | ✅ |
| export | ❌ | ❌ |
"""
    p.write_text(clean)
    assert vp.validate(p, repo_root=tmp_path) == 0
