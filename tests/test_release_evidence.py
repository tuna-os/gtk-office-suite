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


def lock(tmp_path):
    path = tmp_path / "Cargo.lock"
    path.write_text("version = 4\n")
    return str(path)


def test_every_kind_is_listed_with_its_digest(tmp_path):
    src, out = tmp_path / "in", tmp_path / "out"
    fill(src, evidence.KINDS)
    assert evidence.main(["evidence.py", "abc123", "v3.0.0", str(src), str(out), lock(tmp_path)]) == 0
    manifest = json.loads((out / "evidence.json").read_text())
    assert (manifest["revision"], manifest["tag"], manifest["missing"]) == ("abc123", "v3.0.0", [])
    entry = manifest["kinds"]["junit"]["files"][0]
    assert entry["path"] == "junit/sub/junit.txt"
    assert entry["sha256"] == hashlib.sha256(b"junit").hexdigest()
    assert (out / "junit" / "sub" / "junit.txt").read_text() == "junit"


def test_a_missing_kind_fails_and_is_named(tmp_path):
    src, out = tmp_path / "in", tmp_path / "out"
    fill(src, [k for k in evidence.KINDS if k != "visual"])
    assert evidence.main(["evidence.py", "abc123", "v3.0.0", str(src), str(out), lock(tmp_path)]) == 1
    assert json.loads((out / "evidence.json").read_text())["missing"] == ["visual"]


def test_the_bundle_names_the_lock_it_was_built_from(tmp_path):
    """release.md: evidence identifies the exact commit *and* packaged lock."""
    src, out = tmp_path / "in", tmp_path / "out"
    fill(src, evidence.KINDS)
    assert evidence.main(["evidence.py", "abc123", "v3.0.0", str(src), str(out), lock(tmp_path)]) == 0
    manifest = json.loads((out / "evidence.json").read_text())
    assert manifest["locks"] == {"Cargo.lock": hashlib.sha256(b"version = 4\n").hexdigest()}


def test_a_bundle_without_the_lock_fails(tmp_path):
    src, out = tmp_path / "in", tmp_path / "out"
    fill(src, evidence.KINDS)
    assert evidence.main(["evidence.py", "abc123", "v3.0.0", str(src), str(out)]) == 1
    assert json.loads((out / "evidence.json").read_text())["missing"] == ["Cargo.lock"]


def test_the_workflow_passes_the_tags_lock():
    with open(os.path.join(REPO, ".github", "workflows", "release-evidence.yml")) as f:
        workflow = f.read()
    assert 'git show "$rev:Cargo.lock"' in workflow
    assert "locks/*" in workflow


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


def test_a_published_release_gathers_its_evidence():
    """publish-flatpak.yml publishes the release with GITHUB_TOKEN, which
    raises no `release: published` event, so release-evidence.yml never ran
    for v2.1.62 or v2.1.63 (#1209). The release job has to dispatch it on the
    tag, with attach on, and needs `actions: write` to."""
    import yaml

    path = os.path.join(REPO, ".github", "workflows", "publish-flatpak.yml")
    with open(path) as f:
        release = yaml.safe_load(f)["jobs"]["release"]
    assert release["permissions"].get("actions") == "write"
    script = "\n".join(step.get("run", "") for step in release["steps"])
    dispatch = script[script.index("gh workflow run release-evidence.yml"):]
    dispatch = dispatch[: dispatch.index("\n", dispatch.index("attach"))]
    assert '--ref "$GITHUB_REF_NAME"' in dispatch
    assert '-f tag="$GITHUB_REF_NAME"' in dispatch
    assert "-f attach=true" in dispatch
    # Only after the release is public: release-evidence attaches to it.
    assert script.index("--draft=false") < script.index("gh workflow run release-evidence.yml")
