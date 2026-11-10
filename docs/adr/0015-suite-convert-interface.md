# ADR 0015: The `suite-convert` command-line interface

Date: 2026-11-10 · Status: **accepted** (decided on the owner's standing instruction to decide open questions; the exit-code table is the part that is expensive to change later, so it is the part to review) · Relates to: [HEADLESS-CONVERSION-SPEC](../HEADLESS-CONVERSION-SPEC.md), tracking [#579](https://github.com/tuna-os/gtk-office-suite/issues/579), [INTEGRATION-PLAN](../INTEGRATION-PLAN.md) B2, [ADR 0001](0001-core-shell-split.md)

## Context

`HEADLESS-CONVERSION-SPEC.md` consolidated eight drafts of a GTK-free conversion tool and left six questions open. Nothing could be built, and B2 (thumbnails for DOCX, XLSX, PPTX, ODT, ODS, ODP in Files) is blocked on the tool existing, because a `.thumbnailer` entry is a call to a headless render. The spec's own phasing makes phase 1 (the crate, argument parsing and routing, with a test that fails if GTK enters its dependency graph) cheap. This ADR answers the six questions so phase 1 can start. It changes no code.

## Decisions

1. **One binary, `suite-convert`.** Format routing is the tool's job. Four per-app binaries would be four things to install and document. Subcommands are not used: `suite-convert` has one job.
2. **Positional arguments, with flags that override.** `suite-convert <INPUT> [OUTPUT]` is the common form and the one a `.thumbnailer` file can write. `-o/--output`, `-f/--from` and `-t/--to` exist for scripts and are never required when the extension says it. `-` means stdin or stdout.
3. **The loss flag is `--fail-on-loss <LEVEL>`**, with levels `none`, `minor`, `major`, `critical` in that order, and `--report-loss` for the structured JSON report. One name, the one most drafts used, and it says what it does.
4. **`--batch` takes a glob, quoted** (`--batch '*.docx'`), expanded by the tool, so behaviour does not depend on the caller's shell. A directory is accepted as shorthand for `<DIR>/*`. A separate `--glob` flag is not added.
5. **Exit codes, once:**

   | Code | Meaning |
   |---|---|
   | 0 | converted, within any loss budget |
   | 1 | bad arguments, or the input does not exist |
   | 2 | the input could not be read as the format it claims (parse failure) |
   | 3 | the conversion or writing the output failed (engine or I/O error) |
   | 4 | converted, but the loss exceeds `--fail-on-loss` |

   These are an API from the first release; a code is added, never renumbered. Code 4 means the output exists and is usable, which is what a CI gate that wants to keep it needs to tell apart from 3.
6. **PDF comes from `suite-export`** (the GTK-free Typst path, `compile_pdf`), not from the GUI print dialog. The GUI has two PDF paths, and the spec's rule "the CLI and the GUI must not differ" is met against the one the GUI labels *Export as PDF With Typst (Approximate)*; the CLI's PDF is called that too, in its `--help` and in its loss report, rather than implied to match print output.

## Thumbnails (B2)

A thumbnail is the first page as a PNG: `suite-convert --to png --page 1 --size 256 <INPUT> <OUTPUT>`, built on decision 6 (first page of the Typst PDF, rasterised). Which GTK-free rasteriser to use is **not decided here**: it needs a short spike that renders a few corpus documents, looks at the result and measures the dependency weight, because the choice decides whether the tool stays small. Until the spike, the `.thumbnailer` entry is not written.

## Consequences

- Phase 1 of the spec can start: a `suite-convert` crate with this argument surface, format routing, and a test that fails if `gtk4` or `libadwaita` appears in `cargo tree`.
- B2 is unblocked in principle and gated on the rasteriser spike, not on a specification.
- The spec's status changes from "draft, not accepted" to "interface accepted by ADR 0015; implementation not started".

## What would reverse it

A script or packaging constraint that needs a flag-only form (decision 2) or a different loss-failure code (decision 5). Decision 5 is the one to be sure of before the first release, because it cannot change after.
