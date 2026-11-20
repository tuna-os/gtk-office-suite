"""tools/release/check_oci_ref.py refuses an image filed under another app.

ghcr.io/tuna-os/tables held Decks for x86_64 in v2.1.62 and v2.1.63: three
apps' images shared one artifact name in the publish run. publish-flatpak.yml
now checks each image's org.flatpak.ref before pushing it."""

import hashlib
import json
import os
import subprocess
import sys

import yaml

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TOOL = os.path.join(REPO, "tools", "release", "check_oci_ref.py")
PUBLISH = os.path.join(REPO, ".github", "workflows", "publish-flatpak.yml")


def put(layout, data):
    raw = json.dumps(data).encode()
    digest = hashlib.sha256(raw).hexdigest()
    (layout / "blobs" / "sha256").mkdir(parents=True, exist_ok=True)
    (layout / "blobs" / "sha256" / digest).write_bytes(raw)
    return f"sha256:{digest}"


def layout_for(tmp_path, ref):
    layout = tmp_path / "oci"
    labels = {"org.flatpak.ref": ref} if ref else {}
    config = put(layout, {"architecture": "amd64", "config": {"Labels": labels}})
    manifest = put(layout, {"schemaVersion": 2, "config": {"digest": config}, "layers": []})
    (layout / "index.json").write_text(json.dumps({"schemaVersion": 2, "manifests": [{"digest": manifest}]}))
    return layout


def run(layout, prefix):
    return subprocess.run([sys.executable, TOOL, str(layout), prefix], capture_output=True, text=True)


def test_the_right_app_passes(tmp_path):
    result = run(layout_for(tmp_path, "app/org.tunaos.tables/x86_64/master"), "app/org.tunaos.tables/x86_64/")
    assert result.returncode == 0, result.stderr


def test_another_apps_image_is_refused(tmp_path):
    result = run(layout_for(tmp_path, "app/org.tunaos.decks/x86_64/master"), "app/org.tunaos.tables/x86_64/")
    assert result.returncode == 1
    assert "org.tunaos.decks" in result.stderr


def test_another_architecture_is_refused(tmp_path):
    result = run(layout_for(tmp_path, "app/org.tunaos.tables/aarch64/master"), "app/org.tunaos.tables/x86_64/")
    assert result.returncode == 1


def test_an_image_without_the_label_is_refused(tmp_path):
    assert run(layout_for(tmp_path, None), "app/org.tunaos.tables/x86_64/").returncode == 1


def test_every_artifact_name_carries_the_app():
    """The cause: artifacts named after the architecture alone."""
    jobs = yaml.safe_load(open(PUBLISH))["jobs"]
    for job in jobs.values():
        for step in job.get("steps", []):
            uses = str(step.get("uses", ""))
            if uses.startswith(("actions/upload-artifact@", "actions/download-artifact@")):
                name = str((step.get("with") or {}).get("name", ""))
                if name:
                    assert "matrix.app" in name, f"artifact {name!r} does not name the app"


def test_the_image_is_checked_before_it_is_pushed():
    publish = yaml.safe_load(open(PUBLISH))["jobs"]["publish"]
    runs = [str(step.get("run", "")) for step in publish["steps"]]
    check = next(i for i, r in enumerate(runs) if "check_oci_ref.py" in r)
    push = next(i for i, r in enumerate(runs) if "skopeo copy" in r)
    assert check < push
