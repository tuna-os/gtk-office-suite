# Vendored conformance corpora

## commonmark-spec.json

The 652 embedded examples from the CommonMark specification, v0.31.2.

- Source: https://spec.commonmark.org/0.31.2/spec.json
- License: the CommonMark spec is CC-BY-SA 4.0 (John MacFarlane).
- Used by `tests/markdown_corpus.rs` — **not** as HTML-output conformance
  (we don't render HTML) but as a battle-hardened input corpus: every
  example's Markdown must round-trip *idempotently* through the Document
  model (parse → serialize → parse yields an equal Document).

The pass count is ratcheted by `roundtrip-baseline.txt`: CI fails if the
count drops below the baseline; raise the baseline when you make it climb.
