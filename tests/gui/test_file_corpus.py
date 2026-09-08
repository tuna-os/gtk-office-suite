"""Real file → open → edit → undo/redo → Save As → restart/reopen journeys.

Run explicitly after interop/build_open_corpus.py with GUI_FILE_CORPUS pointing
at manifest.json. Missing or modified fixtures fail collection, never skip.
"""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

from framework import BaseGUITestCase

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "interop"))
from open_corpus import load_manifest

MANIFEST = Path(os.environ["GUI_FILE_CORPUS"]).resolve()
CORPUS = load_manifest(MANIFEST)


def semantics(app, snapshot):
    """Opening/editing evidence; full style/geometry fidelity has separate oracles."""
    if app == "letters":
        return {"paragraphs": ["".join(run["text"] for run in para["runs"])
                               for para in snapshot["paragraphs"]]}
    if app == "tables":
        return {"sheets": snapshot["sheet_names"],
                "cells": snapshot["sheet"]["cells"]}
    return {"slide_count": snapshot["slide_count"],
            "objects": [[{key: obj[key] for key in ("kind", "text", "x", "y")}
                         for obj in slide["objects"]] for slide in snapshot["slides"]]}


class FileCorpusJourney(BaseGUITestCase):
    __test__ = False
    fixture = None

    def setUp(self):
        self._root = Path(self.temp_dir("office-file-journey-"))
        self._source = MANIFEST.parent / self.fixture["path"]
        self._input = self._root / self._source.name
        shutil.copyfile(self._source, self._input)
        self.launch_args = [str(self._input)]
        self.isolate_xdg()
        self.isolate_gsettings()
        self.isolate_snapshot()
        self._stage = "launch"
        self.addCleanup(self._retain_journey_files)
        super().setUp()

    def _retain_journey_files(self):
        # Runs after failure capture but before temporary-directory deletion.
        if getattr(self, "_test_failed", True):
            base = Path(os.environ.get("GUI_TEST_ARTIFACT_DIR", str(Path(__file__).parent / "failure_artifacts")))
            evidence = base / self.fixture["id"]
            evidence.mkdir(parents=True, exist_ok=True)
            for path in self._root.iterdir():
                if path.is_file():
                    shutil.copyfile(path, evidence / path.name)
            (evidence / "fixture.json").write_text(json.dumps(
                dict(self.fixture, failed_stage=self._stage), indent=2, ensure_ascii=False) + "\n")

    def snapshot(self):
        self.assertIsNone(self.process.poll(), f"{self._stage}: app crashed opening {self.fixture['id']}")
        return self.trigger_snapshot(f"org.tunaos.{self.app_name}")

    def test_open_edit_save_reopen(self):
        from dogtail import rawinput, tree
        aid = f"org.tunaos.{self.app_name}"
        self._stage = "open"
        if self.fixture["expected"] == "error":
            self.wait_for_condition(
                lambda: self.app.findChildren(lambda node: any(
                    phrase in node.name.lower() for phrase in ("could not", "cannot open", "failed to", "unable to", "unsupported", "invalid file", "error opening"))),
                description="visible file-open error",
            )
            self.assertIsNone(self.process.poll(), "malformed file crashed the app")
            self.assertEqual(hashlib.sha256(self._input.read_bytes()).hexdigest(), self.fixture["sha256"])
            return

        def loaded():
            current = self.snapshot()
            text = json.dumps(semantics(self.app_name, current), ensure_ascii=False)
            return current if all(marker in text for marker in self.fixture["markers"]) else None
        opened = self.wait_for_condition(loaded, description=f"loaded content of {self.fixture['id']}")
        if "sheet_names" in self.fixture:
            self.assertEqual(opened["sheet_names"], self.fixture["sheet_names"])
        if "slide_count" in self.fixture:
            self.assertEqual(opened["slide_count"], self.fixture["slide_count"])
        (self._root / "opened.json").write_text(json.dumps(opened, indent=2))
        self._stage = "edit"
        before = semantics(self.app_name, opened)
        if self.app_name == "letters":
            rawinput.keyCombo("<Control>End")
            rawinput.typeText(" corpus edit sentinel")
        elif self.app_name == "tables":
            # The name-box focus change is asynchronous after the initial
            # snapshot; give GTK one frame before sending the reference.
            time.sleep(0.3)
            rawinput.keyCombo("<Control>g")
            rawinput.typeText("A10")
            rawinput.keyCombo("Return")
            self.wait_for_condition(lambda: self.snapshot()["sheet"]["selection"] == [9, 0, 9, 0],
                                    description="A10 selected for edit")
            rawinput.typeText("corpus edit sentinel")
            rawinput.keyCombo("Return")
        else:
            self.gapplication_action(aid, "add-text-box")

        def changed():
            current = semantics(self.app_name, self.snapshot())
            return current if current != before else None
        edited = self.wait_for_condition(changed, description="document edit")
        if self.app_name != "decks":
            self.wait_for_condition(lambda: "corpus edit sentinel" in json.dumps(semantics(self.app_name, self.snapshot())),
                                    description="complete typed edit")
            edited = semantics(self.app_name, self.snapshot())
        self._stage = "undo-redo"
        self.gapplication_action(aid, "undo")
        self.wait_for_condition(lambda: semantics(self.app_name, self.snapshot()) != edited, description="undo changed content")
        self.gapplication_action(aid, "redo")
        self.wait_for_condition(lambda: semantics(self.app_name, self.snapshot()) == edited, description="redo restored content")

        self._stage = "save-as"
        extension = {"letters": "docx", "tables": "xlsx", "decks": "pptx"}[self.app_name]
        if self.fixture["format"] in ("odt", "odp"):
            extension = self.fixture["format"]
        destination = self._root / f"edited.{extension}"
        self.gapplication_action(aid, "save-file-as")
        entry = self.wait_for_condition(lambda: tree.root.findChild(
            lambda node: node.name == "Name:" and node.roleName == "text"), description="Save As filename")
        entry.text = str(destination)
        button = tree.root.findChild(lambda node: node.name == "Save" and node.roleName == "push button")
        button.do_action(0)
        self.wait_for_file(str(destination))
        # A processed snapshot action is a main-loop barrier after synchronous save;
        # file existence alone can precede completion of the writer.
        self.assertEqual(semantics(self.app_name, self.snapshot()), edited)
        self.assertEqual(hashlib.sha256(self._source.read_bytes()).hexdigest(), self.fixture["sha256"])
        self.assertEqual(hashlib.sha256(self._input.read_bytes()).hexdigest(), self.fixture["sha256"])

        self._stage = "reopen"
        self._stop_process()
        self.process = subprocess.Popen([self.bin_path, str(destination)], env=self.launch_env,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.app = self.wait_for_app(self.app_name)
        self._activate_window()
        self.wait_for_condition(lambda: semantics(self.app_name, self.snapshot()) == edited,
                                description="reopened edited and untouched content")
        (self._root / "reopened.json").write_text(json.dumps(self.snapshot(), indent=2))


for fixture in CORPUS["fixtures"]:
    class_name = fixture["app"].capitalize() + "File_" + re.sub(r"\W", "_", fixture["id"])
    if class_name in globals():
        raise ValueError(f"Fixture names collide after normalization: {class_name}")
    globals()[class_name] = type(class_name, (FileCorpusJourney,),
                                {"__test__": True, "app_name": fixture["app"], "fixture": fixture})
