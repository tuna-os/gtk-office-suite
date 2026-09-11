#!/usr/bin/env python3
"""Tests for the GUI evidence pipeline's dependency-free parts.

collect_evidence.py and pr_comment.py decide what a reviewer sees about a
change. They run once per verification run, in CI, where a mistake shows
up as a wrong or empty comment long after the interesting run is gone —
so the report-building logic is tested here, in the fast PR lane, with no
display, no ffmpeg and no GUI dependencies.

The recorder's own ffmpeg handling is not tested here: it is exercised for
real by every recorded journey, and by the container image's build check.
"""

import json
import os
import sys
import tempfile
import unittest

GUI_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                       "tests", "gui")
sys.path.insert(0, GUI_DIR)

import collect_evidence  # noqa: E402
import pr_comment  # noqa: E402


class HumanSizeTests(unittest.TestCase):
    def test_bytes_have_no_decimal_point(self):
        self.assertEqual(collect_evidence.human_size(512), "512 B")

    def test_scales_up(self):
        self.assertEqual(collect_evidence.human_size(1536), "1.5 KB")
        self.assertEqual(collect_evidence.human_size(5 * 1024 * 1024), "5.0 MB")


class MetadataTests(unittest.TestCase):
    def test_sidecar_is_used_when_present(self):
        with tempfile.TemporaryDirectory() as d:
            video = os.path.join(d, "LettersSmoke.test_launch.mp4")
            open(video, "wb").close()
            with open(os.path.join(d, "LettersSmoke.test_launch.json"), "w") as f:
                json.dump({"test": "LettersSmoke.test_launch", "app": "letters",
                           "outcome": "failed"}, f)
            meta = collect_evidence.load_metadata(video)
        self.assertEqual(meta["outcome"], "failed")
        self.assertEqual(meta["app"], "letters")

    def test_missing_sidecar_falls_back_to_the_filename(self):
        """A clip without metadata is still reported — as unknown, never as
        a pass. Reporting an unlabelled recording as green would be the
        one failure mode that matters here."""
        with tempfile.TemporaryDirectory() as d:
            video = os.path.join(d, "TablesSmoke.test_edit.mp4")
            open(video, "wb").close()
            meta = collect_evidence.load_metadata(video)
        self.assertEqual(meta["test"], "TablesSmoke.test_edit")
        self.assertEqual(meta["app"], "TablesSmoke")
        self.assertEqual(meta["outcome"], "unknown")

    def test_corrupt_sidecar_does_not_raise(self):
        with tempfile.TemporaryDirectory() as d:
            video = os.path.join(d, "DecksSmoke.test_slide.mp4")
            open(video, "wb").close()
            with open(os.path.join(d, "DecksSmoke.test_slide.json"), "w") as f:
                f.write("{not json")
            meta = collect_evidence.load_metadata(video)
        self.assertEqual(meta["outcome"], "unknown")


class SummaryMarkdownTests(unittest.TestCase):
    def bundle(self, entries, **kwargs):
        base = {"title": "Feature verification", "revision": "abcdef1234567890",
                "run_url": "https://example.test/run/1", "entries": entries,
                "recorded": len(entries),
                "failed": sum(1 for e in entries if e["outcome"] == "failed"),
                "ffmpeg": True}
        base.update(kwargs)
        return base

    def test_failure_is_visible_in_the_summary(self):
        md = collect_evidence.render_markdown(self.bundle([
            {"test": "A.test_one", "app": "letters", "outcome": "passed",
             "video": "a.mp4", "video_bytes": 1024, "duration_seconds": 3.2},
            {"test": "B.test_two", "app": "letters", "outcome": "failed",
             "video": "b.mp4", "video_bytes": 2048, "duration_seconds": 9.9},
        ]))
        self.assertIn("1 of 2 recorded journeys failed", md)
        self.assertIn("❌ failed", md)
        self.assertIn("`abcdef123456`", md)

    def test_empty_bundle_explains_a_missing_encoder(self):
        md = collect_evidence.render_markdown(self.bundle([], ffmpeg=False))
        self.assertIn("ffmpeg", md)
        self.assertNotIn("| Journey |", md)


class CommentTests(unittest.TestCase):
    def bundle(self, entries):
        return {"revision": "0123456789ab", "run_url": "https://example.test/run/2",
                "entries": entries}

    def passing_entry(self):
        return {"test": "LettersSmoke.test_launch", "outcome": "passed",
                "video": "clip.mp4", "gif": "clip.gif", "duration_seconds": 4.0}

    def test_marker_is_first_so_the_comment_can_be_found_and_updated(self):
        body = pr_comment.build(self.bundle([self.passing_entry()]))
        self.assertTrue(body.startswith(pr_comment.MARKER))

    def test_media_base_produces_inline_images(self):
        body = pr_comment.build(self.bundle([self.passing_entry()]),
                                media_base="https://media.test/pr-1/9/")
        self.assertIn("![LettersSmoke.test_launch](https://media.test/pr-1/9/clip.gif)", body)
        self.assertIn("https://media.test/pr-1/9/clip.mp4", body)

    def test_without_media_base_it_points_at_the_artifacts(self):
        body = pr_comment.build(self.bundle([self.passing_entry()]))
        self.assertNotIn("![", body)
        self.assertIn("artifacts", body)

    def test_failure_headline_states_the_count(self):
        entries = [self.passing_entry(),
                   {"test": "X.test_y", "outcome": "failed", "video": "x.mp4",
                    "gif": "x.gif", "duration_seconds": 1.0}]
        body = pr_comment.build(self.bundle(entries))
        self.assertIn("❌ 1 of 2 recorded journeys failed", body)

    def test_no_recordings_is_not_reported_as_success(self):
        body = pr_comment.build(self.bundle([]))
        self.assertIn("⚠️", body)
        self.assertNotIn("✅", body)


if __name__ == "__main__":
    unittest.main()
