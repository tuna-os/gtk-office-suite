#!/usr/bin/env python3
"""Render the pull-request comment for a recorded verification run.

Reads the bundle written by collect_evidence.py and prints Markdown. Kept
out of the workflow YAML so it can be unit tested and changed without
re-running CI to find a typo.

    python3 tests/gui/pr_comment.py evidence/evidence.json \\
        --media-base https://raw.githubusercontent.com/o/r/ci-evidence/pr-1/42

Without --media-base the comment still lists what ran and how it went; it
just cannot show the clips inline, because GitHub only renders images it
can fetch from a URL.
"""

import argparse
import json
import sys

MARKER = "<!-- gui-feature-verification -->"


def build(bundle: dict, media_base: str = "", journey: str = "") -> str:
    entries = bundle.get("entries", [])
    failed = [e for e in entries if e.get("outcome") == "failed"]

    if not entries:
        headline = "⚠️ No journey was recorded"
    elif failed:
        headline = f"❌ {len(failed)} of {len(entries)} recorded journeys failed"
    else:
        headline = f"✅ {len(entries)} journey(s) verified on video"

    lines = [MARKER, f"### Feature verification — {headline}", ""]
    if journey:
        lines += [f"Journey selection: `{journey}`", ""]

    if not entries:
        lines += [
            "The run produced no recordings. That is a harness result, not a "
            "verdict about the change — see the workflow log.",
            "",
        ]
    else:
        lines += ["| Journey | Result | Length |", "|---|---|---|"]
        for e in entries:
            mark = {"passed": "✅", "failed": "❌"}.get(e.get("outcome"), "·")
            length = (f"{e['duration_seconds']:.0f}s"
                      if e.get("duration_seconds") else "—")
            lines.append(f"| `{e['test']}` | {mark} {e.get('outcome', 'recorded')} | {length} |")
        lines.append("")

        if media_base:
            base = media_base.rstrip("/")
            for e in entries:
                if not e.get("gif"):
                    continue
                lines += [
                    f"<details><summary><code>{e['test']}</code> — "
                    f"{e.get('outcome', 'recorded')}</summary>",
                    "",
                    f"![{e['test']}]({base}/{e['gif']})",
                    "",
                    f"[Full-resolution video]({base}/{e['video']})",
                    "",
                    "</details>",
                    "",
                ]
        else:
            lines += [
                "Videos are attached to the workflow run as artifacts "
                "(inline previews need a published media branch).",
                "",
            ]

    revision = bundle.get("revision", "")
    if revision:
        lines.append(f"Recorded from the real applications built at `{revision[:12]}`.")
    if bundle.get("run_url"):
        lines.append(f"[Workflow run and full artifacts]({bundle['run_url']})")
    lines.append("")
    return "\n".join(lines)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence_json")
    parser.add_argument("--media-base", default="",
                        help="public base URL where the gif/mp4 files are served")
    parser.add_argument("--journey", default="", help="journey selection that was run")
    parser.add_argument("--out", default="-", help="output file, or - for stdout")
    args = parser.parse_args(argv)

    with open(args.evidence_json) as f:
        bundle = json.load(f)
    body = build(bundle, args.media_base, args.journey)
    if args.out == "-":
        sys.stdout.write(body)
    else:
        with open(args.out, "w") as f:
            f.write(body)
    return 0


if __name__ == "__main__":
    sys.exit(main())
