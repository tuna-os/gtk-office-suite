#!/usr/bin/env python3
"""Keep exactly one GitHub issue per non-green render-parity fixture.

This turns the render lab into a work queue that agents (the hive) and
humans can pick from. Each issue has one acceptance criterion that CI
checks: "fixture <app>/<feature> is green". Issues are:

- opened when a fixture is red/amber/missing and has no open issue;
- updated in place when its verdict or metrics change;
- closed automatically when the fixture turns green;
- deduplicated by a hidden marker, never by title, so re-runs and
  concurrent runs can't create duplicates.

Blocked fixtures (manifest "needs") get the `blocked` label and point at
the architecture item they wait on, so they sort after unblocked work.

Usage: sync_issues.py <summary.json> [--repo OWNER/REPO] [--run-url URL] [--dry-run] [--mode render|export|real]
Requires `gh` authenticated with issues:write.
"""

import argparse
import json
import os
import subprocess
import sys

LABEL = "render-parity"
MARK = "<!-- render-parity:{} -->"
LABEL_EXPORT = "export-parity"
MARK_EXPORT = "<!-- export-parity:{} -->"
# The real-document corpus (real_corpus/, #1200): one issue per document
# that is not green, under its own label, closing when it turns green.
LABEL_REAL = "render-real"
MARK_REAL = "<!-- render-real:{} -->"
REAL_CORPUS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "real_corpus", "manifest.json")
RANK = {"missing": 0, "red": 1, "amber": 2, "green": 3}
NEEDS_TEXT = {
    "letters-page-layout": "Phase 1 Letters page layout engine: a render tree that lays out pages, used for screen, print and PDF",
    "tables-cell-style-model": "Phase 1 Tables cell-style model: font, fill, alignment and wrap in tables-core, with xlsx/ods I/O",
    "decks-shape-style-model": "Phase 1 Decks shape-style model: fill, stroke, placeholders and inherited text styles in decks-core",
}
WHERE = {
    "letters": "`letters/src/bridge.rs` (model to buffer tags), `letters/src/page_container.rs`, `letters/src/layout.rs`",
    "tables": "`tables/src/grid_render.rs` (draw_grid), `tables-core/src/sheet.rs` (model), the xlsx/ods readers in tables-core",
    "decks": "`decks/src/canvas.rs` (draw_slide_multi), `decks-core/src/engine/model.rs`, `decks-core/src/engine/parse.rs`",
}


def gh(*args, input=None):
    r = subprocess.run(["gh", *args], capture_output=True, text=True, input=input)
    if r.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args[:3])}: {r.stderr.strip()}")
    return r.stdout


def worst(verdicts):
    return min(verdicts.values(), key=lambda v: RANK[v]) if verdicts else "missing"


def fmt(v, spec):
    return "–" if v is None else spec.format(v)


def body(fx, run_url, mode="render"):
    key = fx["fixture"]
    rows = "\n".join(
        f"| {t} | {m['verdict']} | {fmt(m.get('ink'), '{:.2f}')} | {fmt(m.get('words'), '{:.0%}')} | "
        f"{fmt(m.get('disp_pt'), '{:.1f}pt')} | {fmt(m.get('colors'), '{:.0%}')} | {fmt(m.get('ssim'), '{:.2f}')} | "
        f"{fmt(m.get('scale'), '{:.2f}×')} |"
        for t, m in sorted(fx["metrics"].items())
    ) or "| – | missing | | | | | | |"
    blocked = ""
    if fx.get("needs"):
        blocked = (
            f"\n> [!IMPORTANT]\n> **Blocked by:** {NEEDS_TEXT.get(fx['needs'], fx['needs'])}. "
            "Don't work around it in widget code. Build the architecture item first "
            "(docs/RENDER-PARITY-ROADMAP.md, Phase 1).\n"
        )
    run = f"\nLatest CI run with the side-by-side report: {run_url}\n" if run_url else ""
    if mode == "real":
        return real_body(fx, rows, run)
    if mode == "export":
        return f"""{MARK_EXPORT.format(key)}
Our editor's PDF export of the `{key}` fixture does not match LibreOffice's PDF of the same file.
{blocked}
**Must be visible:** {fx['expect']}

| tier | verdict | ink | words found | Δ position | colours | SSIM | scale |
|---|---|---|---|---|---|---|---|
{rows}
{run}
### Acceptance (checked by CI, not by hand)

- The `Render parity` workflow's export job reports `{key}` as **green** for the export tier.
- `tools/render-lab/baseline-export.json` is updated in the same PR (`tools/render-lab/run-export.sh --app {fx['app']} --update-baseline`).
- No other export fixture regresses (the export ratchet fails the PR otherwise).

This issue closes itself when the fixture's export turns green on `main`.

### Reproduce

```bash
podman build -t gui-test tests/gui/container && podman build -t render-lab tools/render-lab
tools/render-lab/run-export.sh --app {fx['app']}
xdg-open render-lab-out/export-report.html   # look at {key}
```

Fixture source: `tools/render-lab/fixtures.py` (`{fx['file']}`). Likely code: {WHERE.get(fx['app'], '')}.
"""
    return f"""{MARK.format(key)}
Our editor's on-screen rendering of the `{key}` fixture does not match LibreOffice's rendering of the same file.
{blocked}
**Must be visible:** {fx['expect']}

| tier | verdict | ink | words found | Δ position | colours | SSIM | scale |
|---|---|---|---|---|---|---|---|
{rows}
{run}
### Acceptance (checked by CI, not by hand)

- The `Render parity` workflow reports `{key}` as **green** for tiers A and B.
- `tools/render-lab/baseline.json` is updated in the same PR (`tools/render-lab/run.sh --app {fx['app']} --update-baseline`).
- No other fixture regresses (the ratchet fails the PR otherwise).

This issue closes itself when the fixture turns green on `main`.

### Reproduce

```bash
podman build -t gui-test tests/gui/container && podman build -t render-lab tools/render-lab
tools/render-lab/run.sh --app {fx['app']}
xdg-open render-lab-out/report.html   # look at {key}
```

Fixture source: `tools/render-lab/fixtures.py` (`{fx['file']}`). Likely code: {WHERE.get(fx['app'], '')}.
"""


def real_source(fx):
    """The corpus entry (source, licence, publisher) for a real document."""
    try:
        corpus = json.load(open(REAL_CORPUS))
    except OSError:
        return {}
    return next((e for e in corpus if e["app"] == fx["app"] and e["feature"] == fx["feature"]), {})


def real_body(fx, rows, run):
    key, src = fx["fixture"], real_source(fx)
    origin = (
        f"[{src.get('expect', fx['expect'])}]({src['page']}) ({src.get('source')}, {src.get('license')}"
        + (f", {src['publisher'].split(' | ')[0]}" if src.get("publisher") else "")
        + f"; [file]({src['url']}))"
        if src.get("page") else f"`{fx['file']}`"
    )
    return f"""{MARK_REAL.format(key)}
A published document from the real-document corpus does not render like LibreOffice in our editor.

**Document:** {origin}

| tier | verdict | ink | words found | Δ position | colours | SSIM | scale |
|---|---|---|---|---|---|---|---|
{rows}
{run}
A real document mixes many features, so first find which one is wrong in the side-by-side report and
reduce it to a single-feature fixture in `tools/render-lab/fixtures.py`: that fixture gets its own
`render-parity` issue, and fixing it is how this document moves.

### Acceptance (checked by CI, not by hand)

- The `Render parity (real documents)` workflow reports `{key}` as **green** for tiers A and B.
- `tools/render-lab/baseline-real.json` is updated in the same PR
  (`RENDER_LAB_CORPUS=real tools/render-lab/run.sh --app {fx['app']} --update-baseline`).
- No other real document regresses (the ratchet fails the PR otherwise).

This issue closes itself when the document turns green on `main`.

### Reproduce

```bash
podman build -t gui-test tests/gui/container && podman build -t render-lab tools/render-lab
RENDER_LAB_CORPUS=real tools/render-lab/run.sh --app {fx['app']}
xdg-open render-lab-out/report.html   # look at {key}
```
"""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("summary")
    ap.add_argument("--repo")
    ap.add_argument("--run-url")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--mode", choices=("render", "export", "real"), default="render")
    args = ap.parse_args()
    repo = ["--repo", args.repo] if args.repo else []
    label = {"export": LABEL_EXPORT, "real": LABEL_REAL}.get(args.mode, LABEL)
    mark = {"export": MARK_EXPORT, "real": MARK_REAL}.get(args.mode, MARK)

    summary = json.load(open(args.summary))
    if not args.dry_run:
        for name, color in ((label, "d93f0b"), ("blocked", "cccccc"), ("app:letters", "1d76db"), ("app:tables", "0e8a16"), ("app:decks", "fbca04")):
            subprocess.run(["gh", "label", "create", name, "--color", color, "--force", *repo], capture_output=True)
        existing = json.loads(gh("issue", "list", *repo, "--label", label, "--state", "open", "--limit", "500", "--json", "number,body"))
    else:
        existing = []
    by_key = {}
    for issue in existing:
        for fx in summary["fixtures"]:
            if mark.format(fx["fixture"]) in (issue["body"] or ""):
                by_key[fx["fixture"]] = issue["number"]

    opened = updated = closed = 0
    for fx in summary["fixtures"]:
        key, v = fx["fixture"], worst(fx["verdicts"])
        num = by_key.get(key)
        if v == "green":
            if num:
                print(f"close #{num} {key}")
                if not args.dry_run:
                    gh("issue", "close", str(num), *repo, "--comment", f"`{key}` is green on main. Closing. {args.run_url or ''}")
                closed += 1
            continue
        title = f"[{label}] {key} is {v}: {fx['expect'][:70]}"
        labels = [label, f"app:{fx['app']}"] + (["blocked"] if fx.get("needs") else [])
        text = body(fx, args.run_url, args.mode)
        if num:
            print(f"update #{num} {key} ({v})")
            if not args.dry_run:
                gh("issue", "edit", str(num), *repo, "--title", title, "--body-file", "-", input=text)
            updated += 1
        else:
            print(f"open {key} ({v})")
            if not args.dry_run:
                gh("issue", "create", *repo, "--title", title, "--label", ",".join(labels), "--body-file", "-", input=text)
            opened += 1
    print(f"{label} issues: {opened} opened, {updated} updated, {closed} closed")


if __name__ == "__main__":
    try:
        main()
    except RuntimeError as e:
        sys.exit(str(e))
