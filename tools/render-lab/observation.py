"""Required-observation contract for the render lab's text and image checks.

An unavailable or failed *required* measurement must never be silently left
out of a green result (#1808). This module is stdlib-only and pure: capture,
input and application state probes live elsewhere and hand it bytes, TSV
text and numbers.

Statuses (SCHEMA_VERSION 1):
    pass         measured, and the predicate held
    fail         measured, and the predicate did not hold
    blocked      the measurement could not be taken (engine, language data,
                 timeout, malformed output, missing/blank/stale frame)
    not_run      nothing attempted
    unsupported  the fixture declares this kind of measurement does not apply

A *required* predicate is satisfied only by `pass`. `unsupported` is honoured
only when the fixture declared that kind unsupported up front; it is never
inferred from an engine returning no data, and it never counts as coverage.
"""

import hashlib
import json
import re
import shutil
import subprocess
import tempfile
import unicodedata

SCHEMA_VERSION = 1

PASS, FAIL, BLOCKED, NOT_RUN, UNSUPPORTED = "pass", "fail", "blocked", "not_run", "unsupported"
STATUSES = (PASS, FAIL, BLOCKED, NOT_RUN, UNSUPPORTED)
KINDS = ("text", "image")

DEFAULT_TIMEOUT_S = 60.0
MIN_CONFIDENCE = 30.0

# Normalisation policies, recorded with every text observation.
#   ascii    legacy compare.py: lower-case, keep [0-9a-z]. Non-Latin text is
#            erased, so it is NOT usable for non-ASCII assertions.
#   unicode  NFKC, casefold, keep letters and digits of every script; accents
#            are kept ("é" != "e").
POLICIES = ("ascii", "unicode")


class ObservationError(Exception):
    """A measurement could not be taken. `reason` is a stable slug."""

    def __init__(self, reason, detail=""):
        super().__init__(f"{reason}: {detail}" if detail else reason)
        self.reason = reason
        self.detail = detail


# ── normalisation and TSV ────────────────────────────────────────────────
def normalize(text, policy="unicode"):
    if policy == "ascii":
        return re.sub(r"[^0-9a-z]", "", text.lower())
    if policy == "unicode":
        t = unicodedata.normalize("NFKC", text).casefold()
        return "".join(c for c in t if unicodedata.category(c)[0] in "LN" or unicodedata.category(c) == "Mn")
    raise ValueError(f"unknown normalisation policy {policy!r}")


TSV_COLUMNS = 12


def parse_tsv(text, policy="unicode", min_conf=MIN_CONFIDENCE):
    """Word dicts {text, raw, conf, box:(l,t,w,h)} from tesseract TSV.
    Raises ObservationError('malformed_output') when the header or a data
    row is not tesseract's TSV, rather than reading it as 'no words'."""
    lines = text.splitlines()
    if not lines or not lines[0].startswith("level\t") or len(lines[0].split("\t")) != TSV_COLUMNS:
        raise ObservationError("malformed_output", "missing TSV header")
    words = []
    for n, line in enumerate(lines[1:], start=2):
        if not line.strip():
            continue
        f = line.split("\t")
        if len(f) < TSV_COLUMNS - 1:
            raise ObservationError("malformed_output", f"row {n} has {len(f)} fields")
        try:
            conf = float(f[10])
            box = tuple(float(v) for v in f[6:10])
        except ValueError as e:
            raise ObservationError("malformed_output", f"row {n}: {e}") from None
        raw = f[11].strip() if len(f) >= TSV_COLUMNS else ""
        norm = normalize(raw, policy)
        if conf < min_conf or not norm:
            continue
        words.append({"text": norm, "raw": raw, "conf": conf, "box": box})
    return words


# ── bounded tesseract run ────────────────────────────────────────────────
def run_tesseract(image_path, psm="3", lang="eng", timeout=DEFAULT_TIMEOUT_S, runner=None, which=None):
    """One bounded tesseract TSV run -> (tsv_text, attempt). Raises
    ObservationError on any failure; the attempt record (also attached as
    `.attempt`) is kept either way so every bounded attempt is retained."""
    runner = runner or subprocess.run
    which = which or shutil.which
    attempt = {"engine": "tesseract", "psm": psm, "lang": lang, "timeout_s": timeout}

    def fail(reason, detail=""):
        attempt.update(status=BLOCKED, reason=reason, detail=detail[:500])
        err = ObservationError(reason, detail)
        err.attempt = attempt
        raise err

    if not which("tesseract"):
        fail("missing_engine", "tesseract is not on PATH")
    try:
        r = runner(
            ["tesseract", image_path, "-", "--psm", str(psm), "-l", lang, "tsv"],
            capture_output=True, text=True, timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        fail("timeout", f"no result within {timeout}s")
    except OSError as e:
        fail("missing_engine", str(e))
    if r.returncode != 0:
        err = (r.stderr or "").strip()
        low = err.lower()
        if "failed loading language" in low or "could not initialize tesseract" in low or "traineddata" in low:
            fail("missing_language_data", err)
        fail("nonzero_exit", f"exit {r.returncode}: {err}")
    try:
        tsv = r.stdout
        parse_tsv(tsv)  # validate only
    except ObservationError as e:
        fail(e.reason, e.detail)
    attempt.update(status=PASS, reason="ok")
    return tsv, attempt


# ── frame checks ─────────────────────────────────────────────────────────
def frame_problem(data, min_bytes=64, reference_hash=None):
    """Why a captured frame cannot be measured, or None. `data` is the raw
    bytes (None/empty = missing); `reference_hash` is the hash of the
    previous frame, so an unchanged capture reads as stale."""
    if not data:
        return "missing_frame"
    if len(data) < min_bytes:
        return "capture_failure"
    if reference_hash is not None and sha256(data) == reference_hash:
        return "stale_frame"
    return None


def blank_frame(pixels):
    """True when a flat sequence of grey values is one constant level."""
    return len(set(pixels)) <= 1


# ── text matching (pure) ─────────────────────────────────────────────────
def words_in_roi(words, roi):
    """Words whose centre lies inside roi=(left, top, width, height); None
    means the whole frame. This is what keeps a titlebar out of a crop."""
    if roi is None:
        return list(words)
    left, top, w, h = roi
    out = []
    for word in words:
        bl, bt, bw, bh = word["box"]
        cx, cy = bl + bw / 2, bt + bh / 2
        if left <= cx <= left + w and top <= cy <= top + h:
            out.append(word)
    return out


def contains_phrase(words, phrase, policy="unicode"):
    """True when `phrase` equals a run of consecutive words (any count,
    including a single short label) once both are normalised and joined."""
    target = normalize(phrase, policy)
    if not target:
        raise ValueError(f"phrase {phrase!r} is empty under policy {policy!r}")
    toks = [w["text"] for w in words]
    for i in range(len(toks)):
        joined = ""
        for j in range(i, len(toks)):
            joined += toks[j]
            if joined == target:
                return True
            if not target.startswith(joined):
                break
    return False


def match_text(words, expected=(), forbidden=(), roi=None, policy="unicode"):
    """Text observation for a named crop. Status is `fail` on any missing
    expected or any present forbidden phrase, `pass` otherwise; an empty
    assertion (nothing expected, nothing forbidden) is a ValueError, not a
    pass."""
    if not expected and not forbidden:
        raise ValueError("a text predicate needs expected or forbidden phrases")
    crop = words_in_roi(words, roi)
    missing = [p for p in expected if not contains_phrase(crop, p, policy)]
    present = [p for p in forbidden if contains_phrase(crop, p, policy)]
    return {
        "kind": "text", "status": FAIL if missing or present else PASS,
        "missing": missing, "forbidden_present": present,
        "policy": policy, "roi": list(roi) if roi else None,
        "words": [w["raw"] for w in crop],
    }


def blocked(kind, reason, detail="", attempts=()):
    return {"kind": kind, "status": BLOCKED, "reason": reason, "detail": detail, "attempts": list(attempts)}


# ── required / optional verdict ──────────────────────────────────────────
def declare_predicate(name, kind, required=True, **params):
    if kind not in KINDS:
        raise ValueError(f"unknown predicate kind {kind!r}")
    return {"name": name, "kind": kind, "required": bool(required), **params}


def fixture_verdict(predicates, observations, unsupported_kinds=()):
    """Fold per-predicate observations into one fixture result.

    `observations` maps predicate name -> observation dict (or absent).
    A required predicate passes only on measured `pass`; absent, blocked,
    not_run and fail all fail it. `unsupported` satisfies a required
    predicate only when its kind is in `unsupported_kinds` (declared by the
    fixture); that fixture then reports no text coverage."""
    results = {}
    for p in predicates:
        obs = observations.get(p["name"])
        status = obs["status"] if obs else NOT_RUN
        if status not in STATUSES:
            raise ValueError(f"unknown status {status!r} for {p['name']}")
        if p["kind"] in unsupported_kinds:
            # Declared unsupported up front: the only honest outcome.
            ok = status == UNSUPPORTED
        elif p["required"]:
            ok = status == PASS
        else:
            # Optional: may be absent or blocked, never a measured failure.
            ok = status != FAIL
        results[p["name"]] = {"status": status, "required": p["required"], "ok": ok}
    passed = all(r["ok"] for r in results.values())
    covered = {
        k: any(p["kind"] == k and p["required"] and results[p["name"]]["status"] == PASS for p in predicates)
        for k in KINDS
    }
    return {"schema": SCHEMA_VERSION, "passed": passed, "predicates": results, "coverage": covered}


# ── identity and cache ───────────────────────────────────────────────────
def sha256(data):
    return hashlib.sha256(data).hexdigest()


def evidence_identity(raw, roi=None, reference=None, engine=None, tessdata=None, preprocessing=None,
                      locale=None, fonts=None, resolution=None, scale=None, policy=None, lang=None):
    """Content/config identity of one measurement: every input that can
    change the answer. Filenames are deliberately not part of it."""
    ident = {
        "schema": SCHEMA_VERSION,
        "raw_sha256": sha256(raw) if raw is not None else None,
        "reference_sha256": sha256(reference) if reference is not None else None,
        "roi": list(roi) if roi else None,
        "engine": engine, "tessdata": tessdata, "preprocessing": preprocessing,
        "locale": locale, "fonts": fonts, "resolution": resolution, "scale": scale,
        "policy": policy, "lang": lang,
    }
    ident["key"] = sha256(json.dumps(ident, sort_keys=True).encode())
    return ident


class EvidenceCache:
    """In-memory cache keyed by evidence identity; any changed pixel, crop,
    reference, locale or tool/config version misses."""

    def __init__(self):
        self._items = {}

    def get(self, ident):
        return self._items.get(ident["key"])

    def put(self, ident, observation):
        self._items[ident["key"]] = {"identity": ident, "observation": observation}


# ── schema check ─────────────────────────────────────────────────────────
def validate_observation(doc):
    """Reject observation documents a consumer cannot safely read."""
    if not isinstance(doc, dict) or doc.get("schema") != SCHEMA_VERSION:
        raise ValueError(f"unsupported observation schema {doc.get('schema') if isinstance(doc, dict) else doc!r}")
    if doc.get("status") not in STATUSES:
        raise ValueError(f"unknown status {doc.get('status')!r}")
    if doc.get("kind") not in KINDS:
        raise ValueError(f"unknown kind {doc.get('kind')!r}")
    if doc["status"] == BLOCKED and not doc.get("reason"):
        raise ValueError("a blocked observation must carry a reason")
    return doc
