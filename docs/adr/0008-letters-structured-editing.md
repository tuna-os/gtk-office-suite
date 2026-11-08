# ADR 0008: Structured editing remains model-owned

> Renumbered from 0006 to 0008 on 2026-10-01 (#1205): 0006 was also taken by another ADR. The decision is unchanged.


Letters keeps tables and paragraph structure in `letters-core`; the GTK text
buffer is a rendered projection of it. Table cells remain tagged paragraphs so
global cursor offsets, undo, and clipboard fragments continue to work. Table
row/column insertion, deletion, and row-major navigation are model operations,
not string-prefix rewrites.

List nesting and numbering restarts are explicit paragraph state. Indentation,
spacing, first-line offsets, and page columns are also persisted model state,
with ODT paragraph/page styles carrying the supported values. DOCX numbering
levels are retained on import/export. Constructs outside those mappings remain
covered by the existing interoperability report and opaque-package warning
path rather than being silently flattened.

The GTK bridge renders list markers from the model level. It used to capture
them back from four-space indentation, reading the document out of the buffer
on save; since #1202 and LET-003 the live model (`letters/src/live.rs`) is the
only authority: saving, find, spelling and word count read it, and the buffer
is kept as the input-method and caret projection. The buffer-to-model readers
in `letters/src/bridge.rs` remain for tests only (`#[cfg(test)]`), which check
that the projection loses nothing. This keeps keyboard continuation and paste
journeys readable while preserving structured state for round-trips.

`letters_core::StructuredEditor` is the GTK-free controller seam for those
commands. It owns cursor/selection state and delegates replacement, formatting,
paragraph/list commands, table navigation, and structural edits to `Document`.
GTK views and keyboard journeys should call this surface rather than mutate
paragraph vectors directly, keeping the same command semantics available to
DOCX/ODT round-trip tests and GUI automation.
