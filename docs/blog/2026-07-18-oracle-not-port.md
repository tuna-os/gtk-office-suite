# Oracle, not port: building a Rust office suite that proves its parity on every commit

*TunaOS project, July 2026 — draft for review before posting*

We're building a GNOME-native office suite in Rust — **Letters** (writing),
**Tables** (spreadsheets), **Decks** (presentations) — GTK4/libadwaita,
shipped as Flatpaks. This post is about the part we think other projects
might want to steal: how a small codebase competes with thirty years of
office-suite engineering without porting a line of it.

## The problem with "compatible with Word"

Every alternative office suite makes a compatibility claim, and almost none
can say what it means. LibreOffice earned its claim with three decades of
accumulated test documents and bug archaeology. We had eleven thousand lines
of Rust and a test suite that — we discovered on day one — had literally
never run: the CI badge was green because `pytest || true` swallowed the
fact that pytest wasn't installed. Under that badge, both the spreadsheet
and the presentation app had shipped in a state where they could not launch
at all, the word processor's save shortcut was a silent no-op, and speaker
notes were discarded on every save.

So this is first a story about honest CI. But red tests only tell you what
you already thought to check. The interesting question is: how do you check
against *reality* — the files people actually have?

## Let LibreOffice grade the homework

Our answer: **run LibreOffice headless in CI as an oracle, and never port
its code, tests, or data.**

Three mechanisms, all ratcheted (a pass count that CI refuses to let
regress; raising it is the definition of progress):

1. **LO-authored corpora.** Test scenarios are written as HTML; headless
   Writer converts them to .docx *at test time*; our engine must extract
   the same text and styles from what LibreOffice wrote. Nothing is
   vendored — the corpus regenerates on every run. 109 scenarios for
   Letters, currently 109/109. For Decks, where there's no cheap authoring
   input, scenarios go *through* the oracle: we write .pptx, Impress
   imports and re-exports it in its own grammar, our reader reads
   LibreOffice's version back. 9/9, including styled runs and speaker notes.

2. **Round-trip oracles.** Every file our engines write must open in
   Writer/Calc/Impress and survive conversion with identical content.
   Both directions gate every commit.

3. **Vendored permissive corpora.** The CommonMark spec's 652 examples run
   as a round-trip-idempotence torture test for the document model
   (630/652 — target met; the remaining 22 are escape/entity/autolink edge
   cases), and 107 table-driven cases keyed to ODF OpenFormula measure the
   spreadsheet engine — now 107/107.

The corpus pays for itself constantly. It caught table text being silently
dropped by our DOCX reader, speaker notes that had never once survived a
save, and — our favorite — a regression *we introduced while adding image
support*, flagged by the ratchet twenty minutes after writing the bug.

## A document engine is smaller than you think

The scary part of a word processor is supposedly the document engine.
It turned out to be roughly 2,500 lines of Rust, because the big costs
live elsewhere and Rust's ecosystem or the platform already pays them:

- **Text layout is Pango's job.** Line breaking, shaping, bidi — the
  platform does it, the same as every GNOME app. LibreOffice built its own
  because it predates usable system text stacks. We refuse to.
- **Format plumbing is a library.** OOXML packaging/XML lives in
  [rdocx](https://github.com/tensorbee/rdocx) (we contributed the read
  getters and hyperlink/image write support our fidelity tests demanded),
  spreadsheet evaluation in [IronCalc](https://github.com/ironcalc/ironcalc),
  Markdown in pulldown-cmark, PDF export in Typst-as-a-library.
- **What's left is the actual engine**: a paragraphs-of-styled-runs model
  with enforced invariants, offset addressing deliberately identical to
  GtkTextBuffer's (so the widget bridge is a thin adapter), and format
  converters whose honesty is measured by the machinery above. That model
  is shared: Decks' text boxes carry the same `Run`/`RunStyle` types as
  Letters.

Explicitly out of scope until each item earns an architecture decision:
fields, macros, mail merge, change tracking, frames with text flow. The
target is the documents people actually make, with fidelity you can read
off a scoreboard instead of taking on faith.

## The scoreboard, today

| Measure | Value |
|---|---|
| LibreOffice-authored parity — Letters | 109/109 |
| LibreOffice-authored parity — Decks | 9/9 |
| OpenFormula conformance — Tables | 107/107 |
| CommonMark round-trip idempotence | 630/652 (target met) |

Every number prints into the CI job summary on every push, and none of
them is allowed to go down. Full feature-by-feature detail, including the
soffice-oracle and DOCX round-trip instruments, is in
[docs/PARITY.md](../PARITY.md).

## Steal this

If you maintain anything that reads or writes someone else's file format,
the pattern is portable: find the reference implementation, run it headless
in CI, make it author your corpus, and ratchet the pass count. It's a few
hundred lines of test harness, and it converts "we aim to be compatible"
into a number that moves.

*Code: [tuna-os/gtk-office-suite](https://github.com/tuna-os/gtk-office-suite)
(GPL-3.0-or-later). Three of its core crates — suite-common-core,
suite-export, tables-core — are already on crates.io; `letters-core` isn't
yet, though the git-pinned dependencies that used to block it are gone
(`rdocx` and `ironcalc_base` are now ordinary crates.io versions).*
