#!/usr/bin/env python3
"""Turn recorded GUI journeys into a review-ready evidence bundle.

The journeys leave `<Class>.<test>.mp4` plus a small JSON sidecar in the
video directory. This renders each one to a GIF (what a pull request
comment can show inline) and a poster frame, and writes `summary.md` and
`evidence.json` describing what ran, on which revision, and how it went.

Deliberately dependency-free apart from ffmpeg, so CI can run it in the
same container without installing anything else, and a developer can run
it over a local run's output:

    python3 tests/gui/collect_evidence.py tests/gui/videos --out evidence

Exit status is 0 whenever the bundle was produced. Evidence collection
reports on a verdict, it does not issue one — the journey's own exit
status is what gates.
"""

import argparse
import hashlib
import json
import os
import shutil
import sys

# Import the recorder module directly rather than through the `framework`
# package: framework/__init__.py pulls in the AT-SPI harness (dogtail,
# mss, PIL), and collecting evidence must work anywhere ffmpeg does —
# including a plain PR check with none of the GUI dependencies installed.
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "framework"))
import recorder  # noqa: E402


#: Must match framework/base.py EVIDENCE_SCHEMA_VERSION (pinned by
#: tests/test_gui_evidence.py, which reads both).
EVIDENCE_SCHEMA_VERSION = 2

#: Outcomes the producer can write. Anything else is reported as "unknown".
KNOWN_OUTCOMES = ("passed", "failed", "skipped")

#: Mapping onto the capability ledger's result vocabulary. A recording can
#: never promote a capability: only "passed" maps to a result that could
#: support a claim, and the ledger validator stays the admission authority.
LEDGER_RESULT = {"passed": "pass", "failed": "fail", "skipped": "not-run",
                 "unknown": "unknown", "invalid": "unknown"}


def human_size(num_bytes: int) -> str:
    for unit in ("B", "KB", "MB", "GB"):
        if num_bytes < 1024 or unit == "GB":
            return f"{num_bytes:.0f} {unit}" if unit == "B" else f"{num_bytes:.1f} {unit}"
        num_bytes /= 1024.0
    return f"{num_bytes:.1f} GB"


def load_metadata(video_path: str) -> dict:
    sidecar = os.path.splitext(video_path)[0] + ".json"
    meta = {}
    if os.path.exists(sidecar):
        try:
            with open(sidecar) as f:
                meta = json.load(f)
        except (OSError, json.JSONDecodeError) as e:
            print(f"Warning: unreadable metadata for {video_path}: {e}")
    name = os.path.splitext(os.path.basename(video_path))[0]
    meta.setdefault("test", name)
    meta.setdefault("app", name.split(".")[0])
    meta.setdefault("outcome", "unknown")
    return meta


def validate_entry(meta: dict, video_path: str) -> list:
    """Reasons this sidecar cannot be trusted as a verdict (empty if fine)."""
    problems = []
    if meta.get("schema_version") != EVIDENCE_SCHEMA_VERSION:
        problems.append(f"schema_version {meta.get('schema_version')!r} "
                        f"(expected {EVIDENCE_SCHEMA_VERSION})")
    if meta.get("outcome") not in KNOWN_OUTCOMES:
        problems.append(f"outcome {meta.get('outcome')!r} is not one of "
                        f"{', '.join(KNOWN_OUTCOMES)}")
    if not meta.get("revision"):
        problems.append("no source revision recorded")
    declared = meta.get("video_sha256")
    if not declared:
        problems.append("no video hash recorded")
    elif declared != _sha256(video_path):
        problems.append("video does not match its recorded hash (stale or replaced clip)")
    return problems


def _sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def bundle_problems(entries: list) -> list:
    """Cross-entry problems: duplicate ids, mixed revisions or attempts."""
    problems = []
    seen = {}
    for e in entries:
        seen[e["test"]] = seen.get(e["test"], 0) + 1
    problems += [f"duplicate journey id {t!r}" for t, n in sorted(seen.items()) if n > 1]
    for key in ("revision", "run_id", "attempt"):
        values = {e.get(key, "") for e in entries}
        if len(values) > 1:
            problems.append(f"mixed {key}s across clips: {sorted(values)}")
    return problems


def collect(video_dir: str, out_dir: str, title: str, gif: bool = True) -> dict:
    os.makedirs(out_dir, exist_ok=True)
    videos = sorted(
        os.path.join(video_dir, f)
        for f in os.listdir(video_dir) if f.endswith(".mp4")
    ) if os.path.isdir(video_dir) else []

    entries = []
    for video in videos:
        meta = load_metadata(video)
        base = os.path.splitext(os.path.basename(video))[0]
        dest_video = os.path.join(out_dir, base + ".mp4")
        if os.path.abspath(video) != os.path.abspath(dest_video):
            shutil.copy(video, dest_video)

        problems = validate_entry(meta, video)
        outcome = meta["outcome"]
        if problems and outcome in ("passed", "skipped"):
            # An untrustworthy sidecar can never carry a passing verdict.
            outcome = "invalid"
        elif outcome not in KNOWN_OUTCOMES and outcome != "invalid":
            outcome = "unknown"
        entry = {
            "test": meta["test"],
            "app": meta["app"],
            "outcome": outcome,
            "declared_outcome": meta["outcome"],
            "ledger_result": LEDGER_RESULT.get(outcome, "unknown"),
            "problems": problems,
            "revision": meta.get("revision", ""),
            "run_id": meta.get("run_id", ""),
            "attempt": meta.get("attempt", ""),
            "video_sha256": meta.get("video_sha256", ""),
            "trust": meta.get("trust", "unknown"),
            "video": os.path.basename(dest_video),
            "video_bytes": os.path.getsize(dest_video),
            "duration_seconds": recorder.duration_seconds(dest_video),
        }
        poster = recorder.to_poster(dest_video)
        if poster:
            entry["poster"] = os.path.basename(poster)
        if gif:
            gif_path = recorder.to_gif(dest_video)
            if gif_path:
                entry["gif"] = os.path.basename(gif_path)
                entry["gif_bytes"] = os.path.getsize(gif_path)
        entries.append(entry)

    problems = bundle_problems(entries)
    # The revision is what the clips themselves say, never the collector's
    # environment. Mixed revisions leave it empty and are listed as a problem.
    revisions = {e["revision"] for e in entries if e["revision"]}
    bundle = {
        "title": title,
        "schema_version": EVIDENCE_SCHEMA_VERSION,
        "revision": revisions.pop() if len(revisions) == 1 else "",
        "run_url": run_url(),
        "recorded": len(entries),
        "failed": sum(1 for e in entries if e["outcome"] == "failed"),
        "passed": sum(1 for e in entries if e["outcome"] == "passed"),
        "skipped": sum(1 for e in entries if e["outcome"] == "skipped"),
        "unverified": sum(1 for e in entries if e["outcome"] in ("unknown", "invalid")),
        "bundle_problems": problems,
        "all_passed": bool(entries) and not problems
                      and all(e["outcome"] == "passed" for e in entries),
        "entries": entries,
        "ffmpeg": bool(recorder.available()),
    }
    with open(os.path.join(out_dir, "evidence.json"), "w") as f:
        json.dump(bundle, f, indent=2)
    with open(os.path.join(out_dir, "summary.md"), "w") as f:
        f.write(render_markdown(bundle))
    return bundle


def run_url() -> str:
    server = os.environ.get("GITHUB_SERVER_URL", "https://github.com")
    repo = os.environ.get("GITHUB_REPOSITORY")
    run_id = os.environ.get("GITHUB_RUN_ID")
    return f"{server}/{repo}/actions/runs/{run_id}" if repo and run_id else ""


def render_markdown(bundle: dict) -> str:
    lines = [f"## {bundle['title']}", ""]
    if not bundle["entries"]:
        lines += [
            "No journey recordings were produced.",
            "",
            ("`ffmpeg` was not available in this environment, so nothing could be "
             "recorded." if not bundle["ffmpeg"] else
             "The run finished without recording a journey — check the job log; "
             "this is a harness problem, not a verdict about the feature."),
            "",
        ]
        return "\n".join(lines)

    if bundle.get("all_passed"):
        status = "all recorded journeys passed"
    elif bundle["failed"]:
        status = f"{bundle['failed']} of {bundle['recorded']} recorded journeys failed"
    else:
        status = (f"not every recorded journey passed: "
                  f"{bundle.get('passed', 0)} passed, {bundle.get('skipped', 0)} skipped, "
                  f"{bundle.get('unverified', 0)} unverified of {bundle['recorded']}")
    lines += [f"{status}.", ""]
    for p in bundle.get("bundle_problems", []):
        lines.append(f"- ⚠️ {p}")
    if bundle.get("bundle_problems"):
        lines.append("")
    lines += ["| Journey | App | Result | Length | Video |",
              "|---|---|---|---|---|"]
    for e in bundle["entries"]:
        mark = {"passed": "✅ passed", "failed": "❌ failed",
                "skipped": "⏭ skipped", "invalid": "⚠️ unverified",
                "unknown": "⚠️ unverified"}.get(e["outcome"], "⚠️ unverified")
        length = f"{e['duration_seconds']:.0f}s" if e.get("duration_seconds") else "—"
        lines.append(f"| `{e['test']}` | {e['app']} | {mark} | {length} | "
                     f"{e['video']} ({human_size(e['video_bytes'])}) |")
    lines.append("")
    if bundle["revision"]:
        lines.append(f"Recorded from the real applications at `{bundle['revision'][:12]}`.")
    if bundle["run_url"]:
        lines.append(f"Full videos: [workflow run artifacts]({bundle['run_url']}).")
    lines.append("")
    return "\n".join(lines)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("video_dir", help="directory containing recorded .mp4 journeys")
    parser.add_argument("--out", default="evidence", help="output directory")
    parser.add_argument("--title", default="Feature verification", help="report heading")
    parser.add_argument("--no-gif", action="store_true",
                        help="skip GIF rendering (faster; no inline preview)")
    args = parser.parse_args(argv)

    if not recorder.available():
        print("Warning: ffmpeg not found; GIFs and poster frames will be skipped")
    bundle = collect(args.video_dir, args.out, args.title, gif=not args.no_gif)
    print(f"{bundle['recorded']} recording(s) collected into {args.out}")
    if not shutil.which("ffprobe"):
        print("Note: ffprobe not found; durations omitted")
    return 0


if __name__ == "__main__":
    sys.exit(main())
