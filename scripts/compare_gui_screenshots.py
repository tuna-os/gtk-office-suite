#!/usr/bin/env python3
"""Compare the journey screenshots of two GUI test runs.

CI's GUI Tests workflow keeps a screenshot of every journey as it ended, in
the run's `gui-screenshots` artifact (tests/gui/framework/base.py,
`GUI_TEST_SCREENSHOT_DIR`). This pulls two runs' and says which journeys
look different:

    scripts/compare_gui_screenshots.py BEFORE AFTER [--out DIR] [--repo OWNER/NAME]

BEFORE and AFTER are each a workflow run id (downloaded with $GITHUB_TOKEN)
or a directory of screenshots. DIR (default gui-screenshot-diff) gets
report.html: per journey that changed, the two screens and their
difference, the most changed first; plus the journeys only one run has.
Exits 0 whatever it finds: a changed screen is something to look at, not a
verdict.
"""

import argparse
import html
import io
import json
import os
import sys
import urllib.request
import zipfile

from PIL import Image, ImageChops

ARTIFACT = "gui-screenshots"
# Mean absolute difference, in grey levels, below which two screens are
# the same: a caret's blink or a clock is a few pixels, not a level.
SAME = 0.5


def _get(url, token):
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {token}",
                                               "Accept": "application/vnd.github+json"})
    with urllib.request.urlopen(req) as r:
        return r.read()


def fetch(spec, repo, into):
    """A directory of screenshots for `spec`: itself, or run `spec`'s
    artifact, downloaded into `into`."""
    if os.path.isdir(spec):
        return spec
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if not token:
        sys.exit(f"{spec} is not a directory, and no GITHUB_TOKEN to download run {spec}'s screenshots")
    api = f"https://api.github.com/repos/{repo}/actions/runs/{spec}/artifacts?per_page=100"
    artifacts = json.loads(_get(api, token))["artifacts"]
    found = [a for a in artifacts if a["name"] == ARTIFACT]
    if not found:
        sys.exit(f"run {spec} has no {ARTIFACT} artifact (names: {sorted(a['name'] for a in artifacts)})")
    out = os.path.join(into, str(spec))
    os.makedirs(out, exist_ok=True)
    with zipfile.ZipFile(io.BytesIO(_get(found[0]["archive_download_url"], token))) as z:
        z.extractall(out)
    return out


def journeys(directory):
    """Journey id -> screenshot path; a failed journey's `.failed` is part of
    the file name, not of the id, so a pass and a fail still pair up."""
    out = {}
    for root, _, files in os.walk(directory):
        for f in files:
            if f.endswith(".png"):
                out[f.removesuffix(".png").removesuffix(".failed")] = os.path.join(root, f)
    return out


def difference(a, b):
    """(mean grey-level difference, diff image) of two screenshots, the
    smaller padded to the larger's size."""
    a, b = Image.open(a).convert("RGB"), Image.open(b).convert("RGB")
    size = (max(a.width, b.width), max(a.height, b.height))
    pa, pb = Image.new("RGB", size, "white"), Image.new("RGB", size, "white")
    pa.paste(a)
    pb.paste(b)
    diff = ImageChops.difference(pa, pb).convert("L")
    mean = sum(i * n for i, n in enumerate(diff.histogram())) / (size[0] * size[1])
    return mean, diff.point(lambda v: 255 if v > 16 else 0)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("before")
    ap.add_argument("after")
    ap.add_argument("--out", default="gui-screenshot-diff")
    ap.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", "tuna-os/gtk-office-suite"))
    args = ap.parse_args(argv)

    os.makedirs(args.out, exist_ok=True)
    before = journeys(fetch(args.before, args.repo, os.path.join(args.out, "runs")))
    after = journeys(fetch(args.after, args.repo, os.path.join(args.out, "runs")))
    changed = []
    for name in sorted(before.keys() & after.keys()):
        mean, diff = difference(before[name], after[name])
        if mean >= SAME:
            path = os.path.join(args.out, "diff", name + ".png")
            os.makedirs(os.path.dirname(path), exist_ok=True)
            diff.save(path)
            changed.append((mean, name, path))
    changed.sort(reverse=True)

    rel = lambda p: html.escape(os.path.relpath(p, args.out))  # noqa: E731
    rows = "".join(
        f"<h3>{html.escape(n)} <small>{m:.1f}</small></h3><div class=row>"
        f"<img src='{rel(before[n])}'><img src='{rel(after[n])}'><img src='{rel(d)}'></div>"
        for m, n, d in changed)
    only = lambda keys, which: "".join(f"<li>{html.escape(k)} ({which} only)</li>" for k in sorted(keys))  # noqa: E731
    page = (
        "<!doctype html><meta charset=utf-8><title>GUI screenshot diff</title>"
        "<style>body{font:14px sans-serif;margin:16px}.row{display:flex;gap:8px}"
        ".row img{width:32%;border:1px solid #ccc}</style>"
        f"<h1>{len(changed)} of {len(before.keys() & after.keys())} journeys look different</h1>"
        f"<p>before: {html.escape(args.before)} · after: {html.escape(args.after)} · "
        "columns: before, after, difference; number: mean grey-level difference</p>"
        f"<ul>{only(before.keys() - after.keys(), 'before')}{only(after.keys() - before.keys(), 'after')}</ul>"
        + rows)
    with open(os.path.join(args.out, "report.html"), "w") as f:
        f.write(page)
    for m, n, _ in changed:
        print(f"{m:6.1f}  {n}")
    print(f"{len(changed)} changed of {len(before.keys() & after.keys())}; report: {os.path.join(args.out, 'report.html')}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
