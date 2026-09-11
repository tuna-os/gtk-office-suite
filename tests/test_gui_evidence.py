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


class TheLeadClipPlaysWithoutAClick(unittest.TestCase):
    """A body where every clip is behind a <details> is a body where
    nobody watches any of them."""

    def bundle(self, entries):
        return {"entries": entries, "revision": "abc123def4567890",
                "run_url": "https://example/run/1"}

    def entry(self, name, outcome="passed"):
        return {"test": name, "outcome": outcome, "video": f"{name}.mp4",
                "gif": f"{name}.gif", "duration_seconds": 3.0}

    def test_the_first_clip_is_not_collapsed(self):
        body = pr_comment.build(
            self.bundle([self.entry("A"), self.entry("B")]),
            media_base="https://media.test/x")
        before_details = body.split("<details>")[0]
        self.assertIn("![A](https://media.test/x/A.gif)", before_details)

    def test_the_remaining_clips_are_collapsed(self):
        body = pr_comment.build(
            self.bundle([self.entry("A"), self.entry("B")]),
            media_base="https://media.test/x")
        self.assertIn("<details>", body)
        after_details = body.split("<details>")[1]
        self.assertIn("![B](https://media.test/x/B.gif)", after_details)

    def test_a_failure_is_the_clip_shown_first(self):
        body = pr_comment.build(
            self.bundle([self.entry("A"), self.entry("B", "failed")]),
            media_base="https://media.test/x")
        before_details = body.split("<details>")[0]
        self.assertIn("![B](https://media.test/x/B.gif)", before_details)
        self.assertNotIn("![A]", before_details)

    def test_a_single_journey_adds_no_details_block_at_all(self):
        body = pr_comment.build(self.bundle([self.entry("A")]),
                                media_base="https://media.test/x")
        self.assertNotIn("<details>", body)
        self.assertIn("![A](https://media.test/x/A.gif)", body)


class SpliceIntoPullRequestBody(unittest.TestCase):
    """The evidence goes into the pull-request description, and the
    description is something a person also writes in. Only the marked
    region may ever be rewritten."""

    BLOCK = f"{pr_comment.MARKER}\nEVIDENCE v1\n{pr_comment.END_MARKER}"
    NEWER = f"{pr_comment.MARKER}\nEVIDENCE v2\n{pr_comment.END_MARKER}"

    def test_a_body_with_no_block_gets_one_appended(self):
        out = pr_comment.splice("My description.", self.BLOCK)
        self.assertTrue(out.startswith("My description."))
        self.assertIn("EVIDENCE v1", out)

    def test_a_second_run_replaces_rather_than_duplicates(self):
        once = pr_comment.splice("My description.", self.BLOCK)
        twice = pr_comment.splice(once, self.NEWER)
        self.assertEqual(twice.count(pr_comment.MARKER), 1)
        self.assertNotIn("EVIDENCE v1", twice)
        self.assertIn("EVIDENCE v2", twice)

    def test_prose_on_both_sides_of_the_block_survives(self):
        body = f"Intro.\n\n{self.BLOCK}\n\nTrailing note."
        out = pr_comment.splice(body, self.NEWER)
        self.assertIn("Intro.", out)
        self.assertIn("Trailing note.", out)
        self.assertNotIn("EVIDENCE v1", out)

    def test_a_start_marker_with_no_end_is_replaced_to_the_end(self):
        # What an older run's output looks like. Left alone it would grow a
        # second copy of the evidence on every push.
        body = f"Intro.\n\n{pr_comment.MARKER}\nsprawl\nmore sprawl\n"
        out = pr_comment.splice(body, self.NEWER)
        self.assertEqual(out.count(pr_comment.MARKER), 1)
        self.assertNotIn("sprawl", out)
        self.assertTrue(out.startswith("Intro."))

    def test_an_empty_body_does_not_get_a_leading_rule(self):
        out = pr_comment.splice("", self.BLOCK)
        self.assertTrue(out.startswith(pr_comment.MARKER), out)

    def test_build_emits_both_markers_so_splice_can_find_them(self):
        body = pr_comment.build({"entries": []})
        self.assertIn(pr_comment.MARKER, body)
        self.assertIn(pr_comment.END_MARKER, body)


if __name__ == "__main__":
    unittest.main()
