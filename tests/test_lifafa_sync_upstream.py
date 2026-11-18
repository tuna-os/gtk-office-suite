"""lifafa/fork/sync-upstream.sh: upstream Hylki's changes applied under lifafa/.

Lifafa was imported squashed, so the script applies upstream's own diff from
lifafa/fork/UPSTREAM_BASE to the new upstream commit, three-way. These run it
against a throwaway "upstream" repository and a throwaway suite checkout.

lifafa/fork/check-touchpoints.sh is tested here too: it compares lifafa/ with
the same UPSTREAM_BASE commit, so it shares the throwaway repositories.
"""

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "lifafa" / "fork" / "sync-upstream.sh"
CHECK = ROOT / "lifafa" / "fork" / "check-touchpoints.sh"
ENV = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.org",
       "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.org",
       "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull}


def git(cwd, *args):
    return subprocess.run(["git", *args], cwd=cwd, env=ENV, check=True,
                          capture_output=True, text=True).stdout.strip()


class SyncUpstream(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir)
        # Upstream: a file the fork changes and one it does not.
        self.up = self.dir / "hylki"
        self.up.mkdir()
        git(self.up, "init", "-q", "-b", "main")
        git(self.up, "config", "uploadpack.allowReachableSHA1InWant", "true")
        (self.up / "a.txt").write_text("one\ntwo\nthree\n")
        (self.up / "b.txt").write_text("upstream b\n")
        git(self.up, "add", "-A")
        git(self.up, "commit", "-q", "-m", "base")
        self.base = git(self.up, "rev-parse", "HEAD")
        # The suite: lifafa/ is upstream's base plus a fork change to a.txt.
        self.suite = self.dir / "suite"
        (self.suite / "lifafa" / "fork").mkdir(parents=True)
        shutil.copy(SCRIPT, self.suite / "lifafa" / "fork" / "sync-upstream.sh")
        (self.suite / "lifafa" / "a.txt").write_text("one\ntwo (fork)\nthree\n")
        (self.suite / "lifafa" / "b.txt").write_text("upstream b\n")
        (self.suite / "lifafa" / "fork" / "UPSTREAM_BASE").write_text(self.base + "\n")
        git(self.suite, "init", "-q", "-b", "main")
        git(self.suite, "add", "-A")
        git(self.suite, "commit", "-q", "-m", "import")

    def upstream_commit(self, files):
        for name, text in files.items():
            (self.up / name).write_text(text)
        git(self.up, "commit", "-q", "-am", "upstream change")
        return git(self.up, "rev-parse", "HEAD")

    def sync(self):
        env = {**ENV, "UPSTREAM_URL": f"file://{self.up}"}
        return subprocess.run(["bash", "lifafa/fork/sync-upstream.sh", "main"], cwd=self.suite,
                               env=env, capture_output=True, text=True)

    def test_an_upstream_change_the_fork_did_not_touch_applies_cleanly(self):
        new = self.upstream_commit({"b.txt": "upstream b, improved\n"})
        result = self.sync()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.suite / "lifafa" / "b.txt").read_text(), "upstream b, improved\n")
        # The fork's own change is kept.
        self.assertIn("two (fork)", (self.suite / "lifafa" / "a.txt").read_text())
        self.assertEqual((self.suite / "lifafa" / "fork" / "UPSTREAM_BASE").read_text().strip(), new)

    def test_a_change_to_lines_the_fork_changed_is_left_as_a_conflict(self):
        self.upstream_commit({"a.txt": "one\ntwo (upstream)\nthree\n"})
        result = self.sync()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        text = (self.suite / "lifafa" / "a.txt").read_text()
        self.assertIn("<<<<<<<", text)
        self.assertIn("two (fork)", text)
        self.assertIn("two (upstream)", text)
        self.assertIn("lifafa/a.txt", result.stderr)

    def test_nothing_new_changes_nothing(self):
        result = self.sync()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("already on", result.stdout)
        self.assertEqual(git(self.suite, "status", "--porcelain"), "")

    def test_uncommitted_work_in_lifafa_is_refused(self):
        self.upstream_commit({"b.txt": "changed\n"})
        (self.suite / "lifafa" / "b.txt").write_text("local edit\n")
        result = self.sync()
        self.assertEqual(result.returncode, 2)
        self.assertIn("uncommitted", result.stderr)


class CheckTouchpoints(unittest.TestCase):
    """The suite has no merge base with upstream, so the check reads UPSTREAM_BASE."""

    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir)
        self.up = self.dir / "hylki"
        self.up.mkdir()
        git(self.up, "init", "-q", "-b", "main")
        git(self.up, "config", "uploadpack.allowReachableSHA1InWant", "true")
        (self.up / "a.txt").write_text("upstream a\n")
        (self.up / "b.txt").write_text("upstream b\n")
        git(self.up, "add", "-A")
        git(self.up, "commit", "-q", "-m", "base")
        self.base = git(self.up, "rev-parse", "HEAD")
        # The suite: lifafa/ changes a.txt and adds a fork-only file. The
        # suite is its own repository and never fetched upstream, as in CI.
        self.suite = self.dir / "suite"
        fork = self.suite / "lifafa" / "fork"
        fork.mkdir(parents=True)
        shutil.copy(CHECK, fork / "check-touchpoints.sh")
        (fork / "UPSTREAM_BASE").write_text(self.base + "\n")
        (self.suite / "lifafa" / "a.txt").write_text("upstream a, fork edit\n")
        (self.suite / "lifafa" / "b.txt").write_text("upstream b\n")
        (self.suite / "lifafa" / "fork_only.txt").write_text("fork\n")
        git(self.suite, "init", "-q", "-b", "main")

    def check(self, listed, edits=None):
        (self.suite / "lifafa" / "fork" / "touchpoints.txt").write_text(
            "# comment\n" + "".join(f"{name}\n" for name in listed))
        for name, text in (edits or {}).items():
            (self.suite / "lifafa" / name).write_text(text)
        git(self.suite, "add", "-A")
        git(self.suite, "commit", "-q", "--allow-empty", "-m", "state")
        env = {**ENV, "UPSTREAM_URL": f"file://{self.up}"}
        # From a subdirectory: the script finds lifafa/ from the suite root.
        return subprocess.run(["bash", "fork/check-touchpoints.sh"], cwd=self.suite / "lifafa",
                              env=env, capture_output=True, text=True)

    def test_a_listed_edit_passes_and_a_fork_only_file_is_not_a_touchpoint(self):
        result = self.check(["a.txt"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("1 upstream files modified, all listed", result.stdout)

    def test_an_unlisted_edit_fails_and_names_the_file(self):
        result = self.check(["a.txt"], {"b.txt": "upstream b, fork edit\n"})
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("  b.txt", result.stdout)
        self.assertNotIn("fork_only.txt", result.stdout)

    def test_an_entry_whose_file_matches_upstream_again_is_reported(self):
        result = self.check(["a.txt", "b.txt"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("no longer modified", result.stdout)
        self.assertIn("  b.txt", result.stdout)

    def test_uncommitted_edits_do_not_count(self):
        self.check(["a.txt"])
        (self.suite / "lifafa" / "b.txt").write_text("not committed\n")
        env = {**ENV, "UPSTREAM_URL": f"file://{self.up}"}
        result = subprocess.run(["bash", "lifafa/fork/check-touchpoints.sh"], cwd=self.suite,
                                env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
