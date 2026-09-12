"""A failure before the test body still has to leave evidence behind.

unittest skips `tearDown` when `setUp` raises, and the harness launches the
app *in* setUp — so for the whole class of startup failures, a capture
called only from tearDown retains nothing. Measured before this was fixed:
a binary that panicked on launch left exactly one file, `journey.mp4`, and
its stderr — a panic naming a file and line — reached neither the retained
artifacts nor the run log, because nothing reads the process pipes except
the capture that never ran. What a reader saw was "Timed out after 15.0s
waiting for application 'letters' in the AT-SPI registry" over a silent
video.

That is the failure mode #442 asks to close, and it is self-concealing: the
journey reports its own failure correctly, so only someone trying to debug
it discovers there is nothing to debug with.

These journeys drive a deliberately broken launch and assert on what was
kept. They need no built app — a stub that prints to stderr and exits
non-zero reproduces a startup crash exactly, and is faster and more
specific than waiting for a real one.
"""

import json
import os
import shutil
import tempfile
import unittest

from framework.base import BaseGUITestCase

GUI_DIR = os.path.dirname(os.path.abspath(__file__))
ARTIFACTS_ROOT = os.path.join(GUI_DIR, "failure_artifacts")

STUB = """#!/bin/sh
echo "stub: starting"
echo "thread 'main' panicked at src/main.rs:1: {message}" >&2
exit 101
"""


def _run_in_isolation(case_class):
    """Run one TestCase class and return its result, without failing us.

    The subject is *supposed* to fail; what is under test is what it left
    behind. Running it through unittest directly rather than as a nested
    pytest keeps the assertions about artifacts in one process, and
    buffering keeps the subject's own output off this run's report.
    """
    suite = unittest.TestLoader().loadTestsFromTestCase(case_class)
    result = unittest.TestResult()
    suite.run(result)
    return result


class StartupFailureArtifacts(unittest.TestCase):
    """Not a journey: it runs a journey that fails on purpose."""

    def setUp(self):
        self.stub_dir = tempfile.mkdtemp(prefix="artifact-probe-")
        self.addCleanup(shutil.rmtree, self.stub_dir, ignore_errors=True)
        os.makedirs(os.path.join(self.stub_dir, "debug"))
        self.message = "simulated startup crash"
        stub = os.path.join(self.stub_dir, "debug", "letters")
        with open(stub, "w") as handle:
            handle.write(STUB.format(message=self.message))
        os.chmod(stub, 0o755)

        # The subject reads CARGO_TARGET_DIR in its own setUp.
        self._saved = os.environ.get("CARGO_TARGET_DIR")
        os.environ["CARGO_TARGET_DIR"] = self.stub_dir
        self.addCleanup(self._restore_target_dir)
        # Keep the probe's wait short: the stub is already dead, so the
        # only thing this budget buys is how long we sit watching it.
        self._saved_ready = os.environ.get("GUI_TEST_READY_SECONDS")
        os.environ["GUI_TEST_READY_SECONDS"] = "5"
        self.addCleanup(self._restore_ready)

    def _restore_target_dir(self):
        if self._saved is None:
            os.environ.pop("CARGO_TARGET_DIR", None)
        else:
            os.environ["CARGO_TARGET_DIR"] = self._saved

    def _restore_ready(self):
        if self._saved_ready is None:
            os.environ.pop("GUI_TEST_READY_SECONDS", None)
        else:
            os.environ["GUI_TEST_READY_SECONDS"] = self._saved_ready

    def _artifacts_for(self, case_class, method="test_body_is_never_reached"):
        directory = os.path.join(ARTIFACTS_ROOT, f"{case_class.__name__}.{method}")
        self.addCleanup(shutil.rmtree, directory, ignore_errors=True)
        return directory

    def test_a_crash_before_the_test_body_still_retains_the_apps_stderr(self):
        """The one artifact that explains a startup crash, and the one that was lost."""

        class CrashesOnLaunch(BaseGUITestCase):
            app_name = "letters"

            def test_body_is_never_reached(self):
                raise AssertionError("unreachable: setUp must have failed first")

        directory = self._artifacts_for(CrashesOnLaunch)
        result = _run_in_isolation(CrashesOnLaunch)

        self.assertFalse(result.wasSuccessful(),
                         "the probe was supposed to fail in setUp")
        self.assertTrue(os.path.isdir(directory),
                        f"no artifacts directory at {directory}")

        log = os.path.join(directory, "app.log")
        self.assertTrue(os.path.exists(log), "the app log was not retained")
        with open(log) as handle:
            contents = handle.read()
        self.assertIn(self.message, contents,
                      "the app's stderr is missing from the retained log — this is "
                      "exactly what a startup crash leaves nothing else to diagnose")

    def test_the_other_artifacts_are_retained_too(self):
        class CrashesOnLaunchAgain(BaseGUITestCase):
            app_name = "letters"

            def test_body_is_never_reached(self):
                raise AssertionError("unreachable")

        directory = self._artifacts_for(CrashesOnLaunchAgain)
        _run_in_isolation(CrashesOnLaunchAgain)

        for name in ("screenshot.png", "atspi_tree.txt", "input_trace.json",
                     "captured.json"):
            with self.subTest(artifact=name):
                path = os.path.join(directory, name)
                self.assertTrue(os.path.exists(path), f"{name} was not retained")

    def test_the_manifest_says_what_was_and_was_not_captured(self):
        """A guarded capture can retain nothing quietly; the manifest is how that shows."""

        class CrashesAndIsAudited(BaseGUITestCase):
            app_name = "letters"

            def test_body_is_never_reached(self):
                raise AssertionError("unreachable")

        directory = self._artifacts_for(CrashesAndIsAudited)
        _run_in_isolation(CrashesAndIsAudited)

        with open(os.path.join(directory, "captured.json")) as handle:
            manifest = json.load(handle)

        self.assertIn("app_log", manifest["artifacts"])
        self.assertTrue(manifest["artifacts"]["app_log"]["captured"])
        # This journey sets no snapshot path, so the snapshot is recorded as
        # not applicable rather than as a missing artifact — otherwise every
        # failure would report a gap that is not one, and a real gap would
        # be noise.
        snapshot = manifest["artifacts"]["state_snapshot"]
        self.assertFalse(snapshot["captured"])
        self.assertFalse(snapshot["applicable"])
        self.assertEqual(manifest["missing"], [],
                         "nothing applicable should be missing on this path")

    def test_a_capture_that_produces_nothing_is_recorded_as_missing(self):
        """Mutation: break one capture and the manifest must name it.

        Without this the manifest could be a constant that always reports a
        full set, which is the failure mode it was added to prevent.
        """

        class CrashesWithABrokenDump(BaseGUITestCase):
            app_name = "letters"

            def _dump_atspi_tree(self, max_depth: int = 12) -> str:
                raise RuntimeError("AT-SPI registry is not answering")

            def test_body_is_never_reached(self):
                raise AssertionError("unreachable")

        directory = self._artifacts_for(CrashesWithABrokenDump)
        _run_in_isolation(CrashesWithABrokenDump)

        with open(os.path.join(directory, "captured.json")) as handle:
            manifest = json.load(handle)

        self.assertIn("atspi_tree", manifest["missing"])
        self.assertIn("not answering", manifest["artifacts"]["atspi_tree"]["reason"])
        # The rest must survive one broken capture: losing the app log
        # because a tree dump failed is how a single flaky artifact costs
        # you the evidence that mattered.
        self.assertTrue(manifest["artifacts"]["app_log"]["captured"])


class PassingJourneysLeaveNothing(unittest.TestCase):
    """The capture now runs as a cleanup, which runs on success too.

    So the thing most at risk from this change is a clean run filling the
    artifacts directory with evidence of nothing. This needs a real app: a
    stub cannot reach the AT-SPI tree, so only a built binary can produce a
    journey that actually passes.
    """

    def test_a_passing_journey_retains_no_artifacts(self):
        class LaunchesAndPasses(BaseGUITestCase):
            app_name = "letters"

            def test_launch_then_nothing(self):
                self.assertIsNotNone(self.app)

        directory = os.path.join(ARTIFACTS_ROOT,
                                 "LaunchesAndPasses.test_launch_then_nothing")
        self.addCleanup(shutil.rmtree, directory, ignore_errors=True)
        result = _run_in_isolation(LaunchesAndPasses)

        self.assertTrue(
            result.wasSuccessful(),
            "the probe journey itself failed, so it cannot say anything about "
            f"the success path: {result.errors or result.failures}",
        )
        self.assertFalse(os.path.isdir(directory),
                         "a passing journey retained failure artifacts")


class SkipsAreNotFailures(unittest.TestCase):
    """`SkipTest` derives from `Exception`, which is easy to catch by accident.

    The harness records a verdict by catching exceptions out of the test
    method, so a journey that skipped itself was recorded as failed: the
    clip was kept and labelled `"outcome": "failed"`, and once capture
    became a cleanup it would have collected a full artifact set for every
    skip. `skipTest("GEMINI_API_KEY not set")` in the VLM helpers is the
    live case, so this is not hypothetical.
    """

    def test_a_skipped_journey_retains_nothing(self):
        class SkipsItself(BaseGUITestCase):
            app_name = "letters"

            def test_skips(self):
                self.skipTest("standing in for a missing API key")

        directory = os.path.join(ARTIFACTS_ROOT, "SkipsItself.test_skips")
        self.addCleanup(shutil.rmtree, directory, ignore_errors=True)
        result = _run_in_isolation(SkipsItself)

        self.assertEqual(len(result.skipped), 1,
                         f"the probe was supposed to skip: {result.errors}")
        self.assertFalse(os.path.isdir(directory),
                         "a skipped journey retained failure artifacts")

    def test_a_skipped_journey_is_not_recorded_as_a_failed_one(self):
        """The verdict itself, not just the artifacts: the clip's label."""

        class SkipsItselfAgain(BaseGUITestCase):
            app_name = "letters"

            def test_skips(self):
                self.skipTest("standing in for a missing API key")

        video_dir = tempfile.mkdtemp(prefix="skip-video-")
        self.addCleanup(shutil.rmtree, video_dir, ignore_errors=True)
        saved = os.environ.get("GUI_TEST_VIDEO"), os.environ.get("GUI_TEST_VIDEO_DIR")

        def restore():
            for key, value in zip(("GUI_TEST_VIDEO", "GUI_TEST_VIDEO_DIR"), saved):
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value

        self.addCleanup(restore)
        # "failures" is what CI uses: record everything, keep only the
        # clips of journeys that failed. A skip must not qualify.
        os.environ["GUI_TEST_VIDEO"] = "failures"
        os.environ["GUI_TEST_VIDEO_DIR"] = video_dir

        _run_in_isolation(SkipsItselfAgain)

        kept = sorted(os.listdir(video_dir))
        self.assertEqual(kept, [],
                         f"a skipped journey kept video evidence: {kept}")


if __name__ == "__main__":
    unittest.main()
