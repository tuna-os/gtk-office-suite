#!/usr/bin/env python3
"""Compare our PDF export against LibreOffice's PDF of the same fixture.

For every manifest entry with `"export": true` (docs/EXPORT-PARITY-SPEC.md),
scores `ours-<n>.png` (export_render.py: our --export-pdf rasterized with
the same `pdftoppm -r 96` as the reference) against `lo-<n>.png` with the
existing screenshot-parity metrics and budgets — same thresholds, same
gates. Letters pages and Decks slides share a page box with LibreOffice's
PDF, so no new alignment: compare.align resamples ours onto the reference.

Writes export-report.html + scorecard-export.json. The verdicts ratchet
against the SEPARATE tools/render-lab/baseline-export.json (never
baseline.json), so export and screenshot parity move independently.

Usage:
    export_compare.py <fixtures-dir> <out-dir> [--baseline baseline-export.json]
                      [--update-baseline] [--app APP]
"""

import argparse
import glob
import html
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import compare

TIER = "export"
VERDICT_RANK = compare.VERDICT_RANK


def score_export(app, d, ref_words_cache):
    """Ours-PDF vs LibreOffice-PDF for one fixture directory, or None."""
    lo_pages = sorted(glob.glob(os.path.join(d, "lo-[0-9]*.png")), key=compare.page_no)
    ours_pages = sorted(glob.glob(os.path.join(d, "ours-[0-9]*.png")), key=compare.page_no)
    if not lo_pages or not ours_pages:
        return None
    per_page = []
    for i, lo in enumerate(lo_pages):
        if i >= len(ours_pages):
            per_page.append(
                {
                    "ink": 0.0,
                    "words": 0.0,
                    "lost_lines": None,
                    "disp_pt": None,
                    "colors": 0.0,
                    "ssim": 0.0,
                    "ref_words": None,
                }
            )
            continue
        ref, ours, scale = compare.align(app, lo, ours_pages[i])
        key = (lo, ref.size)
        if key not in ref_words_cache:
            ref_words_cache[key] = compare.ocr_words(ref, app == "tables")
        per_page.append(dict(compare.compare_page(app, ref, ours, ref_words_cache[key]), scale=scale))
    m = {k: compare.mean([p.get(k) for p in per_page]) for k in compare.METRICS}
    m["ref_words"] = sum(p["ref_words"] or 0 for p in per_page)
    lost = [p["lost_lines"] for p in per_page if p.get("lost_lines") is not None]
    m["lost_lines"] = sum(lost) if lost else None
    m["pages_lo"], m["pages_ours"] = len(lo_pages), len(ours_pages)
    # Paper fidelity: the export must paginate exactly like the reference.
    m["page_count_match"] = len(lo_pages) == len(ours_pages)
    m["verdict"] = compare.verdict(m)
    return m


def write_export_report(out, manifest, results):
    rows = []
    counts = {v: 0 for v in VERDICT_RANK}
    for fx in manifest:
        key = f"{fx['app']}/{fx['feature']}"
        d = os.path.join(out, fx["app"], fx["feature"])
        m = results.get(key)
        counts[(m or {"verdict": "missing"})["verdict"]] += 1
        imgs = []
        lo1 = os.path.join(d, "lo-1.png")
        ours1 = os.path.join(d, "ours-1.png")
        if os.path.exists(lo1):
            imgs.append(("LibreOffice PDF", os.path.relpath(lo1, out)))
        if os.path.exists(ours1):
            imgs.append(("Our PDF export", os.path.relpath(ours1, out)))
        if os.path.exists(lo1) and os.path.exists(ours1):
            dp = os.path.join(d, "diff-export-1.png")
            compare.diff_overlay(fx["app"], lo1, ours1, dp)
            imgs.append(("diff", os.path.relpath(dp, out)))
        cells = "".join(
            f'<figure><a href="{html.escape(src)}"><img loading="lazy" src="{html.escape(src)}"></a>'
            f"<figcaption>{html.escape(label)}</figcaption></figure>"
            for label, src in imgs
        )
        metrics = (
            f"<tr><td>{TIER}</td><td><span class=\"v {m['verdict']}\">{m['verdict']}</span></td>"
            f"<td>{compare.fmt(m['ink'])}</td><td>{compare.fmt(m['words'], '{:.0%}')}</td>"
            f"<td>{compare.fmt(m['disp_pt'], '{:.1f}pt')}</td><td>{compare.fmt(m['colors'], '{:.0%}')}</td>"
            f"<td>{compare.fmt(m['ssim'])}</td><td>{m['pages_ours']}/{m['pages_lo']}</td></tr>"
            if m
            else "<tr><td colspan=8>no export captures</td></tr>"
        )
        worst = (m or {"verdict": "missing"})["verdict"]
        rows.append(
            f'<section class="fx"><h2><span class="v {worst}">{worst}</span> {html.escape(key)}</h2>'
            f'<p class="expect">Expect: {html.escape(fx["expect"])}</p>'
            f"<table><tr><th>tier</th><th>verdict</th><th>ink</th><th>words</th><th>Δpos</th>"
            f"<th>colours</th><th>SSIM</th><th>pages</th></tr>{metrics}</table>"
            f'<div class="imgs">{cells}</div></section>'
        )
    summary = "<tr><td>export</td>" + "".join(
        f'<td class="{v}">{counts[v]}</td>' for v in ("green", "amber", "red", "missing")
    )
    summary += "</tr>"
    page = f"""<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Export Parity Report</title>
<style>
:root{{--bg:#fafafa;--fg:#1d1d1f;--muted:#666;--card:#fff;--line:#ddd;
--green:#1a7f37;--amber:#b35900;--red:#c62828;--missing:#777}}
body{{background:var(--bg);color:var(--fg);font:14px/1.45 system-ui,sans-serif;margin:0;padding:16px;max-width:1400px;margin-inline:auto}}
h1{{font-size:22px}} h2{{font-size:16px;margin:0 0 4px}}
.fx{{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:12px;margin:12px 0}}
.expect{{color:var(--muted);margin:0 0 8px}}
table{{border-collapse:collapse;font-variant-numeric:tabular-nums}} td,th{{padding:2px 10px;text-align:left;border-bottom:1px solid var(--line)}}
.v{{font-weight:600;text-transform:uppercase;font-size:12px}}
.green{{color:var(--green)}}.amber{{color:var(--amber)}}.red{{color:var(--red)}}.missing{{color:var(--missing)}}
.imgs{{display:flex;gap:8px;overflow-x:auto;margin-top:8px}}
figure{{margin:0;flex:0 0 auto}} figure img{{max-height:300px;max-width:420px;border:1px solid var(--line);background:#fff}}
figcaption{{color:var(--muted);font-size:12px}}
</style></head><body>
<h1>Export parity: our PDF vs LibreOffice PDF</h1>
<p>Diff overlay: <b class="red">red</b> = LibreOffice drew it and our export didn't,
<b style="color:#1e5ae6">blue</b> = our export drew something LibreOffice didn't.
Same metrics and budgets as screenshot parity (docs/RENDER-PARITY-ROADMAP.md).</p>
<table><tr><th></th><th class="green">green</th><th class="amber">amber</th><th class="red">red</th><th class="missing">missing</th></tr>{summary}</table>
{''.join(rows)}
</body></html>"""
    with open(os.path.join(out, "export-report.html"), "w") as f:
        f.write(page)
    return counts


def ratchet(args, manifest, card):
    """Same contract as compare.ratchet for the single export tier: exit 1
    when a verdict regressed, or improved without --update-baseline."""
    now = {k: {t: m["verdict"] for t, m in v.items()} for k, v in card.items()}
    base = {}
    if args.baseline and os.path.exists(args.baseline):
        base = json.load(open(args.baseline))
    regressed, improved = [], []
    for fx in manifest:
        key = f"{fx['app']}/{fx['feature']}"
        b = base.get(key, {}).get(TIER)
        n = now.get(key, {}).get(TIER)
        if n is None:
            # No capture. If the baseline holds a measured verdict, that
            # evidence is lost, not clean: report it rather than skip it.
            # Skipping is how every Tables export fixture sat green in the
            # baseline while CI never exported a Tables PDF (#1194).
            if b is not None and b != "missing":
                regressed.append({"fixture": key, "tier": TIER, "from": b, "to": "missing"})
            continue
        if b is None:
            if base:
                improved.append({"fixture": key, "tier": TIER, "from": "missing", "to": n})
            continue
        if VERDICT_RANK[n] < VERDICT_RANK[b]:
            regressed.append({"fixture": key, "tier": TIER, "from": b, "to": n})
        elif VERDICT_RANK[n] > VERDICT_RANK[b]:
            improved.append({"fixture": key, "tier": TIER, "from": b, "to": n})

    counts = {v: 0 for v in VERDICT_RANK}
    for tiers in now.values():
        for v in tiers.values():
            counts[v] += 1
    summary = {
        "counts": {TIER: counts},
        "regressed": regressed,
        "improved": improved,
        "fixtures": [
            {
                "fixture": f"{fx['app']}/{fx['feature']}",
                "app": fx["app"],
                "feature": fx["feature"],
                "file": fx["file"],
                "expect": fx["expect"],
                "needs": fx.get("needs"),
                "verdicts": now.get(f"{fx['app']}/{fx['feature']}", {}),
                "metrics": card.get(f"{fx['app']}/{fx['feature']}", {}),
            }
            for fx in manifest
        ],
    }
    with open(os.path.join(args.out, "summary-export.json"), "w") as f:
        json.dump(summary, f, indent=2, sort_keys=True)

    icon = {"green": "🟢", "amber": "🟠", "red": "🔴", "missing": "⚪"}
    md = ["## Export parity: our PDF vs LibreOffice PDF", ""]
    md.append("| tier | " + " | ".join(f"{icon[v]} {v}" for v in ("green", "amber", "red", "missing")) + " |")
    md.append("|---|---|---|---|---|")
    md.append(f"| {TIER} | " + " | ".join(str(counts[v]) for v in ("green", "amber", "red", "missing")) + " |")
    for title, rows in (("Regressed", regressed), ("Improved", improved)):
        if rows:
            md += ["", f"**{title}:**", ""]
            md += [f"- `{r['fixture']}` tier {r['tier']}: {icon[r['from']]} {r['from']} → {icon[r['to']]} {r['to']}" for r in rows]
    if not regressed and not improved:
        md += ["", "No verdict changes against the committed export baseline."]
    with open(os.path.join(args.out, "summary-export.md"), "w") as f:
        f.write("\n".join(md) + "\n")

    if args.update_baseline:
        with open(args.baseline, "w") as f:
            json.dump({**base, **now}, f, indent=2, sort_keys=True)
            f.write("\n")
        print(f"export baseline written: {args.baseline}")
        return
    if not args.baseline:
        return
    if regressed:
        print("EXPORT RATCHET FAILED: export got worse:\n  " + "\n  ".join(f"{r['fixture']}: {r['from']} -> {r['to']}" for r in regressed), file=sys.stderr)
    if improved:
        print("EXPORT RATCHET STALE: improved or new fixtures are not in the baseline; lock them in with\n  tools/render-lab/run-export.sh --update-baseline\nand commit tools/render-lab/baseline-export.json:\n  " + "\n  ".join(f"{r['fixture']}: {r['from']} -> {r['to']}" for r in improved), file=sys.stderr)
    if regressed or improved:
        sys.exit(1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("fixtures")
    ap.add_argument("out")
    ap.add_argument("--baseline")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--app")
    args = ap.parse_args()

    manifest = json.load(open(os.path.join(args.fixtures, "manifest.json")))
    manifest = [fx for fx in manifest if fx.get("export")]
    if args.app:
        manifest = [fx for fx in manifest if fx["app"] == args.app]
    results, cache = {}, {}
    for fx in manifest:
        key = f"{fx['app']}/{fx['feature']}"
        d = os.path.join(args.out, fx["app"], fx["feature"])
        results[key] = score_export(fx["app"], d, cache)
        m = results[key]
        print(f"{key:32} export:{(m or {'verdict': '-'})['verdict']}")

    counts = write_export_report(args.out, manifest, results)
    # Nothing exported at all is a broken pipeline, not a clean result.
    if manifest and not any(results.values()):
        print("export_compare: no export captures found for any fixture; the lab produced nothing to judge", file=sys.stderr)
        sys.exit(2)
    card = {
        k: {TIER: {"verdict": m["verdict"], "lost_lines": m.get("lost_lines"), **{x: m.get(x) for x in compare.METRICS}}}
        for k, m in results.items()
        if m
    }
    with open(os.path.join(args.out, "scorecard-export.json"), "w") as f:
        json.dump(card, f, indent=2, sort_keys=True)
    print("Export: " + ", ".join(f"{counts[v]} {v}" for v in VERDICT_RANK if counts[v] or v == "missing"))
    ratchet(args, manifest, card)


if __name__ == "__main__":
    main()
