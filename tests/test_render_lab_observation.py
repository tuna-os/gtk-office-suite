"""Required OCR and visual evidence fails closed (#1808): the observation
contract in tools/render-lab/observation.py. Stdlib only; tesseract is
injected, so no engine, language data or display is needed."""

import os
import subprocess
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "render-lab"))

import observation as ob  # noqa: E402

HEADER = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext"


def tsv(*words):
    rows = [HEADER]
    for i, (text, left, top, conf) in enumerate(words):
        rows.append(f"5\t1\t1\t1\t1\t{i + 1}\t{left}\t{top}\t40\t12\t{conf}\t{text}")
    return "\n".join(rows) + "\n"


class Proc:
    def __init__(self, out="", err="", rc=0):
        self.stdout, self.stderr, self.returncode = out, err, rc


def run(runner, which=lambda _: "/usr/bin/tesseract"):
    return ob.run_tesseract("x.png", timeout=2, runner=runner, which=which)


class EngineFailures(unittest.TestCase):
    def reason(self, runner, **kw):
        with self.assertRaises(ob.ObservationError) as cm:
            run(runner, **kw)
        self.assertEqual(cm.exception.attempt["status"], ob.BLOCKED)
        return cm.exception.reason

    def test_missing_engine(self):
        self.assertEqual(self.reason(lambda *a, **k: Proc(), which=lambda _: None), "missing_engine")

    def test_missing_language_data(self):
        r = lambda *a, **k: Proc(err="Failed loading language 'deu'", rc=1)  # noqa: E731
        self.assertEqual(self.reason(r), "missing_language_data")

    def test_nonzero_exit(self):
        self.assertEqual(self.reason(lambda *a, **k: Proc(err="boom", rc=3)), "nonzero_exit")

    def test_hang_times_out(self):
        def hang(*a, **k):
            self.assertEqual(k["timeout"], 2)
            raise subprocess.TimeoutExpired("tesseract", 2)

        self.assertEqual(self.reason(hang), "timeout")

    def test_malformed_tsv(self):
        self.assertEqual(self.reason(lambda *a, **k: Proc(out="garbage\n")), "malformed_output")
        bad = HEADER + "\n5\t1\t1\t1\t1\t1\tx\ty\tz\tw\tNaNo\tword\n"
        self.assertEqual(self.reason(lambda *a, **k: Proc(out=bad)), "malformed_output")

    def test_success_keeps_attempt(self):
        out, attempt = run(lambda *a, **k: Proc(out=tsv(("Save", 5, 5, 90))))
        self.assertEqual(attempt["status"], ob.PASS)
        self.assertIn("Save", out)

    def test_frame_problems(self):
        self.assertEqual(ob.frame_problem(None), "missing_frame")
        self.assertEqual(ob.frame_problem(b""), "missing_frame")
        self.assertEqual(ob.frame_problem(b"x" * 10), "capture_failure")
        frame = b"x" * 100
        self.assertEqual(ob.frame_problem(frame, reference_hash=ob.sha256(frame)), "stale_frame")
        self.assertIsNone(ob.frame_problem(frame, reference_hash="other"))
        self.assertTrue(ob.blank_frame([255] * 9))


class TextMatching(unittest.TestCase):
    def words(self, *w):
        return ob.parse_tsv(tsv(*w))

    def test_expected_phrase_passes(self):
        w = self.words(("Save", 100, 100, 95), ("As", 150, 100, 95), ("Copy", 190, 100, 95))
        r = ob.match_text(w, expected=["save as"], roi=(80, 80, 300, 60))
        self.assertEqual(r["status"], ob.PASS)

    def test_wrong_expected_fails(self):
        w = self.words(("Open", 100, 100, 95))
        r = ob.match_text(w, expected=["Save"])
        self.assertEqual((r["status"], r["missing"]), (ob.FAIL, ["Save"]))

    def test_forbidden_fails(self):
        w = self.words(("Save", 5, 5, 95), ("Error", 60, 5, 95))
        r = ob.match_text(w, expected=["Save"], forbidden=["error"])
        self.assertEqual((r["status"], r["forbidden_present"]), (ob.FAIL, ["error"]))

    def test_titlebar_only_match_fails(self):
        # "Save" appears only in the titlebar, outside the named crop.
        w = self.words(("Save", 10, 4, 95), ("Name", 100, 200, 95))
        r = ob.match_text(w, expected=["Save"], roi=(0, 100, 400, 200))
        self.assertEqual(r["status"], ob.FAIL)

    def test_empty_assertion_is_not_a_pass(self):
        with self.assertRaises(ValueError):
            ob.match_text([], expected=[])

    def test_short_label_mixed_case(self):
        r = ob.match_text(self.words(("OK", 5, 5, 90)), expected=["ok"])
        self.assertEqual(r["status"], ob.PASS)

    def test_non_ascii_policy(self):
        w = self.words(("Café", 5, 5, 90), ("日本語", 60, 5, 90))
        self.assertEqual(ob.match_text(w, expected=["café", "日本語"])["status"], ob.PASS)
        # Accents are kept: "cafe" is not "café".
        self.assertEqual(ob.match_text(w, expected=["cafe"])["status"], ob.FAIL)
        # The legacy ASCII policy erases non-Latin text, and a phrase that
        # normalises to nothing is refused rather than matched vacuously.
        with self.assertRaises(ValueError):
            ob.match_text(w, expected=["日本語"], policy="ascii")

    def test_sparse_numeric_cells(self):
        w = self.words(("1/2", 5, 5, 80), ("3.5", 60, 5, 80), ("###", 120, 5, 80))
        self.assertEqual(ob.match_text(w, expected=["1/2", "3.5"])["status"], ob.PASS)
        self.assertEqual(ob.match_text(w, expected=["7"])["status"], ob.FAIL)

    def test_low_confidence_dropped(self):
        self.assertEqual(ob.parse_tsv(tsv(("ghost", 5, 5, 10))), [])


class Verdict(unittest.TestCase):
    def preds(self):
        return [ob.declare_predicate("label", "text"), ob.declare_predicate("mark", "image")]

    def test_high_ssim_cannot_green_a_blocked_required_text(self):
        obs = {"mark": {"status": ob.PASS}, "label": ob.blocked("text", "timeout")}
        v = ob.fixture_verdict(self.preds(), obs)
        self.assertFalse(v["passed"])

    def test_absent_and_not_run_fail_required(self):
        self.assertFalse(ob.fixture_verdict(self.preds(), {"mark": {"status": ob.PASS}})["passed"])

    def test_all_pass(self):
        obs = {"mark": {"status": ob.PASS}, "label": {"status": ob.PASS}}
        v = ob.fixture_verdict(self.preds(), obs)
        self.assertTrue(v["passed"])
        self.assertEqual(v["coverage"], {"text": True, "image": True})

    def test_text_free_fixture_declares_unsupported(self):
        preds = [ob.declare_predicate("label", "text"), ob.declare_predicate("mark", "image")]
        obs = {"mark": {"status": ob.PASS}, "label": {"status": ob.UNSUPPORTED}}
        v = ob.fixture_verdict(preds, obs, unsupported_kinds=("text",))
        self.assertTrue(v["passed"])
        self.assertFalse(v["coverage"]["text"])

    def test_unsupported_never_inferred(self):
        obs = {"mark": {"status": ob.PASS}, "label": {"status": ob.UNSUPPORTED}}
        self.assertFalse(ob.fixture_verdict(self.preds(), obs)["passed"])

    def test_optional_may_be_blocked_but_not_fail(self):
        p = [ob.declare_predicate("label", "text", required=False)]
        self.assertTrue(ob.fixture_verdict(p, {"label": ob.blocked("text", "timeout")})["passed"])
        self.assertFalse(ob.fixture_verdict(p, {"label": {"status": ob.FAIL}})["passed"])


class Identity(unittest.TestCase):
    base = dict(raw=b"px", roi=(0, 0, 10, 10), reference=b"ref", engine="tesseract 5.3", tessdata="eng@1",
                preprocessing="2x", locale="C.UTF-8", fonts="dejavu", resolution=96, scale=1, policy="unicode",
                lang="eng")

    def key(self, **over):
        return ob.evidence_identity(**{**self.base, **over})["key"]

    def test_each_input_invalidates(self):
        base = self.key()
        self.assertEqual(base, self.key())
        for change in (dict(raw=b"py"), dict(roi=(0, 0, 10, 11)), dict(reference=b"ref2"), dict(locale="de_DE"),
                       dict(engine="tesseract 5.4"), dict(tessdata="eng@2"), dict(scale=2), dict(policy="ascii")):
            self.assertNotEqual(base, self.key(**change), change)

    def test_cache(self):
        c = ob.EvidenceCache()
        ident = ob.evidence_identity(**self.base)
        c.put(ident, {"status": ob.PASS})
        self.assertIsNotNone(c.get(ident))
        self.assertIsNone(c.get(ob.evidence_identity(**{**self.base, "raw": b"new"})))


class Schema(unittest.TestCase):
    def test_rejects_drift(self):
        ok = {"schema": 1, "status": ob.PASS, "kind": "text"}
        ob.validate_observation(ok)
        for bad in ({**ok, "schema": 2}, {**ok, "status": "green"}, {**ok, "kind": "audio"},
                    {**ok, "status": ob.BLOCKED}, None):
            with self.assertRaises(ValueError):
                ob.validate_observation(bad)


if __name__ == "__main__":
    unittest.main()
