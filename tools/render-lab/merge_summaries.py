#!/usr/bin/env python3
"""Merge the render lab's summary.json files from several runs into one.

CI splits the real corpus across jobs (`run.sh --shard K/N`), each judging
its own documents against the baseline. This puts their verdicts back
together, so a run is still read in one place:

    merge_summaries.py OUT_DIR SUMMARY.json...

writes OUT_DIR/summary.json (counts summed; regressions, improvements,
disagreements and fixtures joined) and OUT_DIR/summary.md. A shard with
no summary at all (a job that died before comparing) is named in the
markdown, so a merged summary that is short of documents says so.
"""

import json
import os
import sys

VERDICTS = ("green", "amber", "red", "missing")
ICON = {"green": "🟢", "amber": "🟠", "red": "🔴", "missing": "⚪"}


def merge(summaries):
    out = {"counts": {}, "regressed": [], "improved": [], "tier_disagreements": [],
           "print_disagreements": [], "fixtures": []}
    for s in summaries:
        for tier, counts in s.get("counts", {}).items():
            into = out["counts"].setdefault(tier, {v: 0 for v in VERDICTS})
            for v, n in counts.items():
                into[v] = into.get(v, 0) + n
        for key in ("regressed", "improved", "tier_disagreements", "print_disagreements", "fixtures"):
            out[key] += s.get(key, [])
    out["fixtures"].sort(key=lambda f: f.get("fixture", ""))
    for key in ("tier_disagreements", "print_disagreements"):
        out[key] = sorted(set(out[key]))
    return out


def markdown(merged, missing):
    md = ["## Render parity vs LibreOffice (real documents)", ""]
    md.append("| tier | " + " | ".join(f"{ICON[v]} {v}" for v in VERDICTS) + " |")
    md.append("|---|---|---|---|---|")
    for tier, c in sorted(merged["counts"].items()):
        md.append(f"| {tier} | " + " | ".join(str(c.get(v, 0)) for v in VERDICTS) + " |")
    for title, rows in (("Regressed", merged["regressed"]), ("Improved", merged["improved"])):
        if rows:
            md += ["", f"**{title}:**", ""]
            md += [f"- `{r['fixture']}` tier {r['tier']}: {ICON[r['from']]} {r['from']} → {ICON[r['to']]} {r['to']}" for r in rows]
    if not merged["regressed"] and not merged["improved"]:
        md += ["", "No verdict changes against the committed baseline."]
    if missing:
        md += ["", "**Shards with no summary** (their documents are not counted above):", ""]
        md += [f"- `{m}`" for m in missing]
    return "\n".join(md) + "\n"


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    out_dir, paths = argv[0], argv[1:]
    summaries, missing = [], []
    for p in paths:
        try:
            with open(p) as f:
                summaries.append(json.load(f))
        except (OSError, ValueError):
            missing.append(p)
    merged = merge(summaries)
    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "summary.json"), "w") as f:
        json.dump(merged, f, indent=2, sort_keys=True)
    with open(os.path.join(out_dir, "summary.md"), "w") as f:
        f.write(markdown(merged, missing))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
