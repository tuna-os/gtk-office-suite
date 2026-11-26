"""Report writing and the verdict ratchet for compare.py."""

import html
import json
import os
import sys

TIERS = ("A", "B", "X", "C")
VERDICT_RANK = {"missing": 0, "red": 1, "amber": 2, "green": 3}
TIER_AGREE = 3.0  # grey levels; measured 0.0-0.3 when the tiers agree


# ── report ────────────────────────────────────────────────────────────────
def fmt(v, spec="{:.2f}"):
    return "–" if v is None else spec.format(v)


def write_report(out, manifest, results, agreement, diff_overlay):
    rows = []
    totals = {t: {k: 0 for k in VERDICT_RANK} for t in TIERS}
    for fx in manifest:
        key = f"{fx['app']}/{fx['feature']}"
        d = os.path.join(out, fx["app"], fx["feature"])
        r = results.get(key, {})
        for t in TIERS:
            totals[t][(r.get(t) or {"verdict": "missing"})["verdict"]] += 1
        imgs = []
        lo1 = os.path.join(d, "lo-1.png")
        if os.path.exists(lo1):
            imgs.append(("LibreOffice", os.path.relpath(lo1, out)))
        for t in TIERS:
            p = os.path.join(d, f"{t}-1.png")
            if os.path.exists(p):
                imgs.append((f"Tier {t}", os.path.relpath(p, out)))
                if os.path.exists(lo1):
                    dp = os.path.join(d, f"diff-{t}-1.png")
                    diff_overlay(fx["app"], lo1, p, dp)
                    imgs.append((f"diff {t}", os.path.relpath(dp, out)))
        cells = "".join(
            f'<figure><a href="{html.escape(src)}"><img loading="lazy" src="{html.escape(src)}"></a>'
            f"<figcaption>{html.escape(label)}</figcaption></figure>"
            for label, src in imgs
        )
        tiers = ""
        checks = []
        for t in TIERS:
            m = r.get(t)
            if not m:
                continue
            if "wrap_structure" in m:
                evidence = m["wrap_structure"]
                detail = "; ".join(
                    f"{side}: {v['lines']} lines, row {v['row_height']}px / heading {v['heading_height']}px, "
                    f"wholly visible: {v['unclipped']}"
                    for side, v in evidence.items()
                    if side != "passed"
                )
                checks.append(f"Tier {t} wrap check: {'pass' if evidence['passed'] else 'fail'} ({detail}).")
            tiers += (
                f'<tr><td>{t}</td><td><span class="v {m["verdict"]}">{m["verdict"]}</span></td>'
                f"<td>{fmt(m['ink'])}</td><td>{fmt(m['words'], '{:.0%}')}</td>"
                f"<td>{fmt(m['disp_pt'], '{:.1f}pt')}</td><td>{fmt(m['colors'], '{:.0%}')}</td>"
                f"<td>{fmt(m['ssim'])}</td><td>{fmt(m.get('scale'), '{:.2f}×')}</td><td>{m['pages_ours']}/{m['pages_lo']}</td></tr>"
            )
        worst = min(
            [(r[t]["verdict"]) for t in TIERS if r.get(t)] or ["missing"],
            key=lambda v: VERDICT_RANK[v],
        )
        agree = agreement.get(key)
        agree_html = (
            ""
            if agree is None
            else f'<p class="expect">Tier A↔B: {agree:.2f} grey levels '
            + (
                "(agree)"
                if agree <= TIER_AGREE
                else '<b class="red">DISAGREE: pixels changed between GTK and the browser</b>'
            )
            + "</p>"
        )
        rows.append(
            f'<section class="fx" data-app="{fx["app"]}" data-verdict="{worst}">'
            f'<h2><span class="v {worst}">{worst}</span> {html.escape(key)}</h2>'
            f'<p class="expect">Expect: {html.escape(fx["expect"])}</p>{agree_html}'
            + "".join(f'<p class="expect">{html.escape(check)}</p>' for check in checks)
            + f"<table><tr><th>tier</th><th>verdict</th><th>ink</th><th>words</th><th>Δpos</th>"
            f"<th>colours</th><th>SSIM</th><th>scale</th><th>pages</th></tr>{tiers or '<tr><td colspan=9>no captures</td></tr>'}</table>"
            f'<div class="imgs">{cells}</div></section>'
        )
    summary = "".join(
        f"<tr><td>Tier {t}</td>"
        + "".join(f'<td class="{k}">{totals[t][k]}</td>' for k in ("green", "amber", "red", "missing"))
        + "</tr>"
        for t in TIERS
        if any(totals[t][k] for k in ("green", "amber", "red"))
    )
    page = f"""<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Render Parity Report</title>
<style>
:root{{--bg:#fafafa;--fg:#1d1d1f;--muted:#666;--card:#fff;--line:#ddd;
--green:#1a7f37;--amber:#b35900;--red:#c62828;--missing:#777}}
@media (prefers-color-scheme:dark){{:root{{--bg:#1b1b1d;--fg:#eee;--muted:#aaa;--card:#26262a;--line:#3a3a40;
--green:#4ac26b;--amber:#f0a040;--red:#ff6b6b;--missing:#999}}}}
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
.filters button{{margin-right:6px}}
</style></head><body>
<h1>Render parity: our apps vs LibreOffice</h1>
<p>Each fixture isolates one feature. Diff overlay: <b class="red">red</b> = LibreOffice drew it and we didn't,
<b style="color:#1e5ae6">blue</b> = we drew something LibreOffice didn't. Budgets: docs/RENDER-PARITY-ROADMAP.md.</p>
<table><tr><th></th><th class="green">green</th><th class="amber">amber</th><th class="red">red</th><th class="missing">missing</th></tr>{summary}</table>
<p class="filters">Show: <button data-f="all">all</button><button data-f="letters">letters</button><button data-f="tables">tables</button>
<button data-f="decks">decks</button><button data-f="red">red only</button></p>
{"".join(rows)}
<script>
document.querySelectorAll('.filters button').forEach(b=>b.onclick=()=>{{const f=b.dataset.f;
document.querySelectorAll('.fx').forEach(s=>{{s.style.display=(f==='all'||s.dataset.app===f||s.dataset.verdict===f)?'':'none'}})}});
</script></body></html>"""
    with open(os.path.join(out, "report.html"), "w") as f:
        f.write(page)
    return totals


def ratchet(args, manifest, card, agreement, printed=None):
    printed = printed or {}
    """Compare verdicts with the committed baseline, write the machine-
    readable summary (summary.json, summary.md) that CI, PR comments and
    the hive consume, and exit non-zero on any change the PR didn't lock in.

    The baseline stores verdicts only, not metrics: verdicts are stable
    across runs, raw floats are not. Two ways to fail:
      regressed  a fixture's verdict got worse than the baseline;
      stale      it got better but the baseline wasn't updated, so the
                 gain isn't locked in (re-run with --update-baseline and
                 commit tools/render-lab/baseline.json).
    """
    now = {k: {t: m["verdict"] for t, m in v.items()} for k, v in card.items()}
    base = {}
    if args.baseline and os.path.exists(args.baseline):
        base = json.load(open(args.baseline))
    regressed, improved = [], []
    # A tier counts as run when any fixture was measured in it. Within a run
    # tier, a baselined fixture with no measurement lost its evidence -- an
    # app whose CI job never uploaded, say -- and is reported, not skipped.
    # A tier nobody ran (a local `--tier A`) stays out of it entirely.
    ran = {t for tiers in now.values() for t in tiers}
    for fx in manifest:
        key = f"{fx['app']}/{fx['feature']}"
        for t in TIERS:
            b = base.get(key, {}).get(t)
            n = now.get(key, {}).get(t)
            if n is None:
                if t in ran and b is not None and b != "missing":
                    regressed.append({"fixture": key, "tier": t, "from": b, "to": "missing"})
                continue
            if b is None:
                if base:  # a baseline exists but doesn't know this fixture yet
                    improved.append({"fixture": key, "tier": t, "from": "missing", "to": n})
                continue
            if VERDICT_RANK[n] < VERDICT_RANK[b]:
                regressed.append({"fixture": key, "tier": t, "from": b, "to": n})
            elif VERDICT_RANK[n] > VERDICT_RANK[b]:
                improved.append({"fixture": key, "tier": t, "from": b, "to": n})

    counts = {t: {v: 0 for v in VERDICT_RANK} for t in TIERS}
    for tiers in now.values():
        for t, v in tiers.items():
            counts[t][v] += 1
    counts = {t: c for t, c in counts.items() if sum(c.values())}
    fixtures = [
        {
            "fixture": f"{fx['app']}/{fx['feature']}",
            "app": fx["app"],
            "feature": fx["feature"],
            "file": fx["file"],
            "expect": fx["expect"],
            "needs": fx.get("needs"),
            "verdicts": now.get(f"{fx['app']}/{fx['feature']}", {}),
            "metrics": card.get(f"{fx['app']}/{fx['feature']}", {}),
            "tier_agreement": agreement.get(f"{fx['app']}/{fx['feature']}"),
            "print_agreement": printed.get(f"{fx['app']}/{fx['feature']}"),
        }
        for fx in manifest
    ]
    disagree = sorted(k for k, v in agreement.items() if v is not None and v > TIER_AGREE)
    summary = {
        "counts": counts,
        "regressed": regressed,
        "improved": improved,
        "tier_disagreements": disagree,
        "print_disagreements": sorted(k for k, v in printed.items() if v is not None and v > TIER_AGREE),
        "fixtures": fixtures,
    }
    with open(os.path.join(args.out, "summary.json"), "w") as f:
        json.dump(summary, f, indent=2, sort_keys=True)

    icon = {"green": "🟢", "amber": "🟠", "red": "🔴", "missing": "⚪"}
    md = ["## Render parity vs LibreOffice", ""]
    md.append("| tier | " + " | ".join(f"{icon[v]} {v}" for v in ("green", "amber", "red", "missing")) + " |")
    md.append("|---|---|---|---|---|")
    for t, c in counts.items():
        md.append(f"| {t} | " + " | ".join(str(c[v]) for v in ("green", "amber", "red", "missing")) + " |")
    for title, rows in (("Regressed", regressed), ("Improved", improved)):
        if rows:
            md += ["", f"**{title}:**", ""]
            md += [
                f"- `{r['fixture']}` tier {r['tier']}: {icon[r['from']]} {r['from']} → {icon[r['to']]} {r['to']}"
                for r in rows
            ]
    if not regressed and not improved:
        md += ["", "No verdict changes against the committed baseline."]
    if disagree:
        md += [
            "",
            f"**Tier A/B disagree** (> {TIER_AGREE:g} grey levels between the app's own render and the browser):",
            "",
        ]
        md += [f"- `{k}`: {agreement[k]:.1f}" for k in disagree]
    with open(os.path.join(args.out, "summary.md"), "w") as f:
        f.write("\n".join(md) + "\n")

    if args.update_baseline:
        # Merge per fixture *and* tier, so a single-app run (--app) keeps
        # the other apps, and a run of one tier (the nightly Tier C job)
        # keeps each fixture's verdicts in the tiers it did not run.
        merged = {k: {**base.get(k, {}), **now.get(k, {})} for k in {*base, *now}}
        with open(args.baseline, "w") as f:
            json.dump(merged, f, indent=2, sort_keys=True)
            f.write("\n")
        print(f"baseline written: {args.baseline}")
        # README's status table is generated from this file (#1211);
        # rewrite it in the same step so the two are committed together.
        sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
        import readme_status

        if os.path.abspath(args.baseline) in (readme_status.BASELINE, readme_status.BASELINE_REAL) and os.path.exists(
            readme_status.README
        ):
            readme_status.main(["--write"])
            print(f"README status table rewritten: {readme_status.README}")
        return
    if not args.baseline:
        return
    if regressed:
        print(
            "RATCHET FAILED: rendering got worse:\n  "
            + "\n  ".join(f"{r['fixture']} {r['tier']}: {r['from']} -> {r['to']}" for r in regressed),
            file=sys.stderr,
        )
    if improved:
        print(
            f"RATCHET STALE: improved or new fixtures are not in the baseline; lock them in with\n  tools/render-lab/run.sh --update-baseline\nand commit tools/render-lab/{os.path.basename(args.baseline)}:\n  "
            + "\n  ".join(f"{r['fixture']} {r['tier']}: {r['from']} -> {r['to']}" for r in improved),
            file=sys.stderr,
        )
    if regressed or improved:
        sys.exit(1)
