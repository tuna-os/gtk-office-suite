# letters-core — document engine design

The document model Letters has been missing: today formatting lives only in
GtkTextBuffer tags and the "model" is a Markdown string, so anything Markdown
can't express is silently dropped on save. letters-core owns the truth;
the GtkTextBuffer becomes a *view* of it.

## Model

```
Document   { paragraphs: Vec<Paragraph> }        // always ≥ 1 paragraph
Paragraph  { style: ParaStyle, runs: Vec<Run> }  // runs may be empty (blank line)
Run        { text: String, style: RunStyle }     // text never empty
RunStyle   { bold, italic, underline, strikethrough, highlight, link: Option<String> }
ParaStyle  { heading: Option<u8 1..=6>, alignment, list: ListKind, line_spacing }
```

### Invariants (normalization)

1. A `Run.text` is never empty.
2. Adjacent runs with identical `RunStyle` are merged.
3. A document always has at least one paragraph.

Every mutating operation restores these before returning; property-style
tests assert them after each op.

## Addressing

Offsets are **global character offsets** counting each paragraph break as
one character (`\n`) — deliberately identical to GtkTextBuffer's offset
model, so the GTK bridge is offset-for-offset with no translation layer.
`char` means Unicode scalar (Rust `char`), matching gtk_text_iter offsets.

## Operations (all pure, all unit-tested)

- `insert_text(offset, &str)` — `\n` in input splits paragraphs; inserted
  text inherits the style at the insertion point.
- `delete_range(start, end)` — deleting across a boundary merges paragraphs.
- `apply_run_style(start, end, patch)` / `style_at(offset)` — patch toggles
  individual attributes; splitting runs as needed.
- `set_para_style(range, patch)` — headings, alignment, lists, spacing.
- `to_plain_text()` / `from_plain_text()` / `char_len()`.

## Review state

Tracked changes and comments are part of `Document`, as marks on the text
they cover: `RunStyle::revision` (`track`) and `RunStyle::comments`
(`comments`, one mark key per thread, so threads overlap), with the comments
themselves in `Document::comments`. Every review step is an op with an exact
inverse, so it undoes (and would replicate) like any edit; nothing needs
rebasing after an edit. See docs/LETTERS-REVIEW-WORKFLOWS.md for why the
first design, a `ReviewState` sidecar of offsets, was replaced.

`table_of_contents()` derives entries in paragraph order from heading levels,
`Title`/`Subtitle`, and `Heading 1` through `Heading 6` named styles. The
`base_direction()` helper resolves the first strong Unicode character and has
an explicit fallback, so caret, selection, list-marker, and print layers can
share the same paragraph direction decision.

.docx and .odt keep both; the other formats warn on save
(`save::compatibility_report`) rather than drop them silently.

## I/O layers (separate modules, model stays format-agnostic)

- `markdown`: `Document ⇄ Markdown` (pulldown-cmark for parse). Documented
  lossiness: highlight, underline, alignment, line-spacing have no Markdown
  form — they survive model round-trips but not a Markdown export. Corpus
  test: CommonMark spec inputs must round-trip *idempotently*
  (md → Doc → md → Doc gives equal Docs).
- `docx` (next): `Document ⇄ DOCX` via rdocx, replacing letters'
  `docx_bridge`. Golden fixtures under `tests/fixtures/`.

## Collaboration (`collab` feature, off by default)

`collab.rs` (RFC-0001 Phase 5, RFC-0005 P5) holds the document as one Loro
`Text` with Peritext marks: the edit sequence is the text, character
formatting is marks with the expand rules of `edit::MarkKey::expand`, an
inline object is its `OBJECT` char with an `obj` mark, a paragraph's style
is a `p` mark on the break that starts it. Its `Replica` has the API of
`tables_core::controller::collab::Replica`: `export_snapshot` /
`from_snapshot` to join, `version_bytes` / `export_since` for deltas, and an
`import_updates` that applies an update to a scratch copy first and leaves
the live document untouched when it fails.

Not replicated, and refused with `CollabError::Unsupported` (in a document,
in an op, and in a peer's update): tables, footnotes, headers and footers,
sections, comments, and character formatting that no mark carries. The
offsets are the model's sequence offsets; mapping them to a
`GtkTextBuffer` is the shell's, and is not done or tested here.

Undo stays local (ADR 0011). `ImportReport::rebase` moves the stored undo
and redo steps over what a peer did to the text (`History::rebase`):
offsets shift, a range skips text the peer inserted inside it, a step with
nothing left is dropped, that step alone.

### The live session (`collab/live.rs`)

`Shared` is a `Replica` plus what has been sent: `record` puts one user
action's ops into the replica all or none (`Replica::record_all`: a refused
op leaves nothing half shared), `outgoing` is the delta to publish, and
`import` dry-runs a peer's update and returns the document the replica now
holds, with the `ImportReport` that moves the undo history. What a peer sent
is never in `outgoing`, so it is never echoed back. `map_offset` and
`map_selection` move a caret and a selection over the peer's `TextEdit`s, as
offsets into the edit sequence; a peer's insert at exactly the caret goes
before it (`Bias::After`), as in `Rebaser`, and a selection keeps text typed
at its edges outside. Mapping buffer offsets to sequence offsets (list markers,
a chip's label, a footnote's `[n]`) is the shell's (`letters::live`).

## GTK bridge (lives in `letters`, not here)

`render_to_buffer(doc, buffer)` writes text + tags; `capture_from_buffer`
rebuilds a Document from buffer + tags. Tag names map 1:1 to RunStyle
fields / ParaStyle values. Buffer edits route through the model via the
insert/delete signal handlers, keeping model and buffer in lockstep.
