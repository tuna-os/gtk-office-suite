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

        entry = {
            "test": meta["test"],
            "app": meta["app"],
            "outcome": meta["outcome"],
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

    bundle = {
        "title": title,
        "revision": os.environ.get("GITHUB_SHA", ""),
        "run_url": run_url(),
        "recorded": len(entries),
        "failed": sum(1 for e in entries if e["outcome"] == "failed"),
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

    status = ("all recorded journeys passed" if not bundle["failed"]
              else f"{bundle['failed']} of {bundle['recorded']} recorded journeys failed")
    lines += [f"{status}.", "", "| Journey | App | Result | Length | Video |",
              "|---|---|---|---|---|"]
    for e in bundle["entries"]:
        mark = {"passed": "✅ passed", "failed": "❌ failed"}.get(e["outcome"], "· recorded")
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
