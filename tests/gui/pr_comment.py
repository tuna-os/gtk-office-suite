#!/usr/bin/env python3
"""Render the pull-request evidence block for a recorded verification run.

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
END_MARKER = "<!-- /gui-feature-verification -->"

#: Clips embedded beyond the lead one. A full smoke run records ~48
#: journeys; a body holding all of them is a body nobody reads.
EMBED_LIMIT = 5


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
            shown = [e for e in entries if e.get("gif")]
            # Failures first, so the cap below can only ever drop passing
            # journeys. A dropped failure is the one thing this must not do.
            shown.sort(key=lambda e: e.get("outcome") != "failed")
            # One clip plays without a click. A body where every clip is
            # behind a <details> is a body where nobody watches any of
            # them, which defeats the point of recording them. A failure
            # is the one worth seeing first; otherwise the first journey.
            lead = next((e for e in shown if e.get("outcome") == "failed"),
                        shown[0] if shown else None)
            if lead is not None:
                lines += [
                    f"**`{lead['test']}`** — {lead.get('outcome', 'recorded')}",
                    "",
                    f"![{lead['test']}]({base}/{lead['gif']})",
                    "",
                    f"[Full-resolution video]({base}/{lead['video']})",
                    "",
                ]
            rest = [e for e in shown if e is not lead]
            # Embedding every clip of a full smoke run means ~48 of them,
            # which buries the description the clips are meant to support.
            # The table above already accounts for all of them, and the
            # artifacts hold every file; what goes inline is a sample.
            embedded, overflow = rest[:EMBED_LIMIT], rest[EMBED_LIMIT:]
            if embedded:
                lines += [f"<details><summary>{len(embedded)} more "
                          f"recorded journey(s)</summary>", ""]
                for e in embedded:
                    lines += [
                        f"**`{e['test']}`** — {e.get('outcome', 'recorded')}",
                        "",
                        f"![{e['test']}]({base}/{e['gif']})",
                        "",
                        f"[Full-resolution video]({base}/{e['video']})",
                        "",
                    ]
                lines += ["</details>", ""]
            if overflow:
                lines += [
                    f"The remaining {len(overflow)} clip(s) are in the "
                    f"workflow artifacts rather than inline — every journey "
                    f"is still listed in the table above.",
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
    # The closing marker delimits the region the workflow replaces inside a
    # pull-request body. Without it an update could only append, and the
    # body would grow a new copy of the evidence on every push.
    lines += ["", END_MARKER, ""]
    return "\n".join(lines)


def splice(current: str, block: str) -> str:
    """Put `block` into `current`, replacing any block already there.

    Only the marked region is ever rewritten, so a description someone
    edited by hand keeps its prose — and keeps the evidence wherever they
    moved it to. A start marker with no end marker is treated as "replace
    from here to the end": that is what an older run's output looks like,
    and leaving it would grow a second copy of the evidence on every push.
    """
    block = block.strip()
    start = current.find(MARKER)
    if start == -1:
        base = current.rstrip()
        separator = "\n\n---\n\n" if base else ""
        return f"{base}{separator}{block}\n"

    end = current.find(END_MARKER, start)
    if end == -1:
        return current[:start] + block + "\n"
    return current[:start] + block + current[end + len(END_MARKER):]


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence_json", nargs="?",
                        help="evidence bundle to render; omit it and pass "
                             "--block to splice an already-rendered block")
    parser.add_argument("--media-base", default="",
                        help="public base URL where the gif/mp4 files are served")
    parser.add_argument("--journey", default="", help="journey selection that was run")
    parser.add_argument("--out", default="-", help="output file, or - for stdout")
    parser.add_argument("--splice-into", default="",
                        help="existing pull-request body to update in place "
                             "(the marked region is replaced; the rest is kept)")
    parser.add_argument("--block", default="",
                        help="file holding an already-rendered block, instead "
                             "of rendering one from an evidence bundle")
    args = parser.parse_args(argv)

    if args.block:
        with open(args.block) as f:
            body = f.read()
    elif args.evidence_json:
        with open(args.evidence_json) as f:
            bundle = json.load(f)
        body = build(bundle, args.media_base, args.journey)
    else:
        parser.error("pass an evidence bundle or --block")
    if args.splice_into:
        with open(args.splice_into) as f:
            body = splice(f.read(), body)
    if args.out == "-":
        sys.stdout.write(body)
    else:
        with open(args.out, "w") as f:
            f.write(body)
    return 0


if __name__ == "__main__":
    sys.exit(main())
