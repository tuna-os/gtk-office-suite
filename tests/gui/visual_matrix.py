#!/usr/bin/env python3
"""Run the visual matrix (#1284): every app in four states at three widths,
three themes and two scales, compared with the committed expected images.

    tests/gui/visual_matrix.py                 # compare every config
    tests/gui/visual_matrix.py --only w800     # configs whose name has w800
    tests/gui/visual_matrix.py --update        # rewrite the expected images

Each config gets its own harness run (its own Xvfb at that screen size),
running test_visual_matrix.py. Evidence lands in --out (default
tests/gui/visual/out): per cell the actual screenshot, the app's state
snapshot, the comparison result and, on a mismatch, expected and diff;
and over everything report.json and report.md. Exits non-zero when any
cell failed or any config's run did.
"""

import argparse
import glob
import json
import os
import subprocess
import sys

GUI_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, GUI_DIR)
import visual_compare  # noqa: E402

RUNNER = os.path.join(GUI_DIR, "run_gui_tests.sh")


def run_config(name, env, out, update, extra):
    full = {**os.environ, **env, "GUI_VISUAL_OUT": out}
    if update:
        full["GUI_VISUAL_UPDATE"] = "1"
    os.makedirs(os.path.join(out, name), exist_ok=True)
    log = os.path.join(out, name, "run.log")
    with open(log, "w") as f:
        code = subprocess.call([RUNNER, "test_visual_matrix.py", *extra], env=full, stdout=f, stderr=subprocess.STDOUT)
    return code, log


def collect(out, name, subset):
    """Every cell's result for one config; a cell with no result file never
    got as far as a screenshot. A run narrowed with pytest arguments
    (`subset`) reports only the cells it produced."""
    found = {}
    for path in glob.glob(os.path.join(out, name, "*.result.json")):
        cell = os.path.basename(path).removesuffix(".result.json")
        with open(path) as f:
            found[cell] = json.load(f)
    if subset:
        return found
    return {cell: found.get(cell, {"ok": False, "message": "no screenshot (see run.log)"})
            for cell in visual_compare.cells()}


def report(out, results):
    failed = [(c, cell, r) for c, cells in results.items() for cell, r in cells.items() if not r["ok"]]
    total = sum(len(cells) for cells in results.values())
    with open(os.path.join(out, "report.json"), "w") as f:
        json.dump(results, f, indent=1, sort_keys=True)
    lines = [f"# Visual matrix: {total - len(failed)}/{total} cells match", ""]
    lines += ["| Config | Cells matching | Failed cells |", "| --- | --- | --- |"]
    for config, cells in results.items():
        bad = [cell for cell, r in cells.items() if not r["ok"]]
        lines.append(f"| {config} | {len(cells) - len(bad)}/{len(cells)} | {', '.join(bad) or '—'} |")
    if failed:
        lines += ["", "## Failures", ""]
        lines += [f"- `{config}/{cell}`: {r['message']}" for config, cell, r in failed]
    with open(os.path.join(out, "report.md"), "w") as f:
        f.write("\n".join(lines) + "\n")
    return failed


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--only", default="", help="run configs whose name contains this")
    parser.add_argument("--update", action="store_true", help="rewrite the expected images")
    parser.add_argument("--out", default=os.path.join(visual_compare.VISUAL_DIR, "out"))
    parser.add_argument("pytest_args", nargs="*", help="passed to the harness (e.g. -k Tables)")
    args = parser.parse_args()
    out = os.path.abspath(args.out)
    results, broken = {}, []
    for name, env in visual_compare.configs():
        if args.only not in name:
            continue
        # Results from an earlier run must not be read as this one's.
        for stale in glob.glob(os.path.join(out, name, "*.result.json")):
            os.remove(stale)
        code, log = run_config(name, env, out, args.update, args.pytest_args)
        results[name] = collect(out, name, bool(args.pytest_args))
        ok = sum(r["ok"] for r in results[name].values())
        print(f"{name}: {ok}/{len(results[name])} cells match (harness exit {code}; {log})", flush=True)
        if code not in (0, 1):  # 1 is pytest's "some tests failed"
            broken.append(name)
    failed = report(out, results)
    print(open(os.path.join(out, "report.md")).read())
    if broken:
        print(f"harness runs that broke: {', '.join(broken)}")
    return 1 if failed or broken else 0


if __name__ == "__main__":
    sys.exit(main())
