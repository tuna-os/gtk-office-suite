# ADR 0006: Structured editing remains model-owned

Letters keeps tables and paragraph structure in `letters-core`; the GTK text
buffer is a rendered/editable view. Table cells remain tagged paragraphs so
global cursor offsets, undo, and clipboard fragments continue to work. Table
row/column insertion, deletion, and row-major navigation are model operations,
not string-prefix rewrites.

List nesting and numbering restarts are explicit paragraph state. Indentation,
spacing, first-line offsets, and page columns are also persisted model state,
with ODT paragraph/page styles carrying the supported values. DOCX numbering
levels are retained on import/export. Constructs outside those mappings remain
covered by the existing interoperability report and opaque-package warning
path rather than being silently flattened.

The GTK bridge renders list markers from the model level and captures them
back from four-space indentation. This keeps keyboard continuation and paste
journeys readable while preserving structured state for round-trips.

`letters_core::StructuredEditor` is the GTK-free controller seam for those
commands. It owns cursor/selection state and delegates replacement, formatting,
paragraph/list commands, table navigation, and structural edits to `Document`.
GTK views and keyboard journeys should call this surface rather than mutate
paragraph vectors directly, keeping the same command semantics available to
DOCX/ODT round-trip tests and GUI automation.
