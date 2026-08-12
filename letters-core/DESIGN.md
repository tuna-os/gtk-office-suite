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

`review::ReviewState` is a serializable sidecar to the canonical document.
Comments and revisions have monotonic IDs, global character ranges, author
metadata, and deterministic comment navigation. Insert revisions cover their
visible text; delete revisions retain the removed text at a zero-width anchor.
Accepting removes a pending revision while rejecting applies the inverse edit
to `Document`. Callers must call `rebase_after_edit()` for every buffer edit.

`table_of_contents()` derives entries in paragraph order from heading levels,
`Title`/`Subtitle`, and `Heading 1` through `Heading 6` named styles. The
`base_direction()` helper resolves the first strong Unicode character and has
an explicit fallback, so caret, selection, list-marker, and print layers can
share the same paragraph direction decision.

Review state is intentionally separate from `Document` until DOCX/ODT
revision/comment parts are fully mapped. This prevents a save from silently
pretending that a pending review annotation was preserved.

## I/O layers (separate modules, model stays format-agnostic)

- `markdown`: `Document ⇄ Markdown` (pulldown-cmark for parse). Documented
  lossiness: highlight, underline, alignment, line-spacing have no Markdown
  form — they survive model round-trips but not a Markdown export. Corpus
  test: CommonMark spec inputs must round-trip *idempotently*
  (md → Doc → md → Doc gives equal Docs).
- `docx` (next): `Document ⇄ DOCX` via rdocx, replacing letters'
  `docx_bridge`. Golden fixtures under `tests/fixtures/`.

## GTK bridge (lives in `letters`, not here)

`render_to_buffer(doc, buffer)` writes text + tags; `capture_from_buffer`
rebuilds a Document from buffer + tags. Tag names map 1:1 to RunStyle
fields / ParaStyle values. Buffer edits route through the model via the
insert/delete signal handlers, keeping model and buffer in lockstep.
