"""The release evidence bundle (tools/release/evidence.py) lists every
file with its digest, and refuses a bundle with a kind missing."""

import hashlib
import json
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "tools", "release"))

import evidence  # noqa: E402


def fill(root, kinds):
    for kind in kinds:
        os.makedirs(os.path.join(root, kind, "sub"), exist_ok=True)
        with open(os.path.join(root, kind, "sub", f"{kind}.txt"), "w") as f:
            f.write(kind)


def test_every_kind_is_listed_with_its_digest(tmp_path):
    src, out = tmp_path / "in", tmp_path / "out"
    fill(src, evidence.KINDS)
    assert evidence.main(["evidence.py", "abc123", "v3.0.0", str(src), str(out)]) == 0
    manifest = json.loads((out / "evidence.json").read_text())
    assert (manifest["revision"], manifest["tag"], manifest["missing"]) == ("abc123", "v3.0.0", [])
    entry = manifest["kinds"]["junit"]["files"][0]
    assert entry["path"] == "junit/sub/junit.txt"
    assert entry["sha256"] == hashlib.sha256(b"junit").hexdigest()
    assert (out / "junit" / "sub" / "junit.txt").read_text() == "junit"


def test_a_missing_kind_fails_and_is_named(tmp_path):
    src, out = tmp_path / "in", tmp_path / "out"
    fill(src, [k for k in evidence.KINDS if k != "visual"])
    assert evidence.main(["evidence.py", "abc123", "v3.0.0", str(src), str(out)]) == 1
    assert json.loads((out / "evidence.json").read_text())["missing"] == ["visual"]


def test_the_kinds_are_the_ones_release_md_names():
    release = open(os.path.join(REPO, "docs", "readiness-2026-09", "release.md")).read()
    row = next(line for line in release.splitlines() if "machine-readable capability matrix" in line)
    for words in ("capability matrix", "JUnit", "independent-reader", "visual/a11y", "performance"):
        assert words in row
    assert set(evidence.KINDS) == {"capabilities", "junit", "independent-readers", "visual", "performance"}


def test_the_workflow_does_not_swallow_a_missing_kind():
    """The step that runs evidence.py pipes it into tee; without pipefail
    the step took tee's status, and a bundle with gaps passed (#1209)."""
    with open(os.path.join(REPO, ".github", "workflows", "release-evidence.yml")) as f:
        workflow = f.read()
    step = workflow[workflow.index("- name: Assemble the bundle"):]
    step = step[: step.index("- uses:")]
    assert "evidence.py" in step and "| tee" in step
    assert "set -o pipefail" in step, "the evidence.py | tee pipe must fail when evidence.py does"
