# `suite-convert`: headless document conversion CLI

**Status**: specification draft, not accepted · **Tracking**: [#579]

## Provenance

This document consolidates eight independently generated drafts of the same
specification: #530, #536, #551, #585, #650, #655, #664 and #683, written in
parallel over 2026-09-11/12 into six different file paths. One of them (#655)
claimed the RFC number `0002`, which is also claimed by three drafts of the
unrelated extension-architecture proposal.

They agree on what the tool is. They disagree on its command-line surface, and
on three mutually incompatible exit-code tables. Those disagreements are
recorded in [Open questions](#open-questions) rather than settled by fiat.

## What this is

A GTK-free command-line binary that converts documents using the same core
crates the desktop apps use — `suite-common-core`, `letters-core`,
`tables-core`, `decks-core` — with no GTK, no libadwaita, no display server and
no Xvfb.

## Why it is worth building

Three reasons the drafts agree on, in the order they actually matter:

1. **It enforces the architecture rule.** `AGENTS.md` requires business logic to
   live outside widget code. A binary that links the core crates and *cannot*
   link GTK turns that rule from a convention into a build error. This is the
   strongest argument in any of the drafts and it is independent of whether
   anyone ever runs the tool in production.
2. **It makes parity testing cheap.** The `PARITY.md` corpora can be exercised
   without a GUI session, which is faster and removes a whole class of flake
   from the interop gates.
3. **It replaces `soffice --headless`** for server-side and CI conversion, at a
   fraction of the startup cost and memory footprint.

## Scope

| Category | In | Out |
|---|---|---|
| Text | `.md` (CommonMark), `.odt`, `.docx` | `.odt`, `.docx`, `.md`, `.html`, `.pdf` |
| Spreadsheets | `.csv`, `.ods`, `.xlsx` | `.ods`, `.csv`, `.html`, `.pdf` |
| Presentations | `.odp`, `.pptx` | `.odp`, `.pptx`, `.pdf`, `.png` (slide snapshots) |

Every conversion goes through the same engine the GUI export path uses. If the
CLI and the GUI can produce different output for the same document, that is a
bug in the layering, not a feature of the CLI.

## Loss budgets

The tool reports what a conversion lost, in the same vocabulary the interop
work already uses, and can fail when the loss exceeds a caller-specified
threshold. This is the feature that makes it useful as a CI gate rather than
just a converter, and all eight drafts include some form of it.

Agreed: loss is reported as structured JSON; there is a flag to fail on loss
above a severity; severities are ordered (roughly `none` < `minor` < `major` <
`critical`). The flag's name and the resulting exit code are not agreed — see
below.

## Interface sketch

Not a decision. This is the shape the drafts most nearly converge on, recorded
so the disagreements below have something to point at.

```
suite-convert [OPTIONS] <INPUT> [OUTPUT]

  -f, --from <FORMAT>     Input format; inferred from the extension if omitted
  -t, --to <FORMAT>       Output format; inferred from OUTPUT if omitted
  -o, --output <PATH>     Output path; '-' for stdout
      --batch <PATTERN>   Convert every file matching a glob
      --report-loss       Emit a structured loss report
      --fail-on-loss <L>  Exit non-zero when loss exceeds L
  -v, --verbose
```

`-` for stdin/stdout, so the tool composes in a shell pipeline. Several drafts
call this out and none contradicts it.

## Open questions

1. **Exit codes.** Three drafts specify three incompatible tables:

   | Code | #585 | #664 | #683 |
   |---|---|---|---|
   | 1 | parse/syntax error | bad args or input missing | — |
   | 2 | loss threshold exceeded | parse failure | — |
   | 3 | I/O error | engine failure | loss threshold exceeded |
   | 4 | — | strict-mode violation | — |

   Exit codes are an API. Scripts will branch on these, so this has to be
   decided once, before the first release, and not changed after.

2. **One binary or four?** `suite-convert` alone, versus additionally
   `letters-convert` / `tables-convert` / `decks-convert` (#530, #536). Four
   binaries mean four things to install and document; one means format routing
   is the tool's job. The subcommand form `suite-convert batch ...` (#650) is a
   third option nobody compared against the flag form.

3. **Positional or flag arguments?** `suite-convert in.odt out.pdf` (#585,
   #683) versus `--input`/`--output` (#551, #655, #664). Positional is more
   UNIX-idiomatic; flags are harder to get wrong in a batch script.

4. **The loss-failure flag's name.** `--fail-on-loss` (#585, #683),
   `--strict` (#664), `--strict-loss-budget` (#655). One name.

5. **Does `--batch` take a directory or a glob?** #585 takes `--batch <DIR>`
   plus a separate `--glob`; #683 takes a glob directly. These interact with
   shell expansion differently and the difference is user-visible.

6. **PDF rendering.** Every draft lists PDF as an output and none says what
   renders it. The core crates do layout; producing a PDF page needs a
   rasteriser or a PDF writer, and whether that dependency is GTK-free is the
   single biggest open risk to the "no GTK" premise. #642 and #614 propose a
   print/render architecture and should be reconciled with this before phase 2.

## Phasing

| Phase | Deliverable | Done when |
|---|---|---|
| 1 | `suite-convert` crate, argument parsing, format routing | Builds in the workspace with no GTK in the dependency tree — asserted by a test, not by inspection |
| 2 | Letters formats wired | Round-trips the `LO-Letters` corpus with results identical to the GUI export path |
| 3 | Tables and Decks wired | Same, against their corpora |
| 4 | Distribution | Shipped alongside the Flatpaks; container image if there is demand |

Phase 1's success criterion is the load-bearing one. A test that fails the
build if `libgtk-4` appears in `suite-convert`'s dependency graph is what makes
this specification enforce something.

## Relationship to the readiness plan

Nothing here precedes [Roadmap to dependable daily use](readiness-2026-09/README.md)
([#443]). Phase 1 is cheap and sharpens an existing architectural rule; the rest
waits.

[#579]: https://github.com/tuna-os/gtk-office-suite/issues/579
[#443]: https://github.com/tuna-os/gtk-office-suite/issues/443
