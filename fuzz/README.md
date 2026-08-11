# Importer fuzzing

The fuzz crate targets the ZIP/XML entry points for each document family. The
targets intentionally discard parser errors: malformed documents are expected,
while panics, sanitizer findings, timeouts, and excessive allocations are bugs.

Install the runner with `cargo install cargo-fuzz`, then run a target locally:

```sh
cargo fuzz run letters_docx -- -max_len=1048576 -timeout=10 -rss_limit_mb=512
cargo fuzz run tables_xlsx -- -max_len=1048576 -timeout=10 -rss_limit_mb=512
cargo fuzz run decks_pptx -- -max_len=1048576 -timeout=10 -rss_limit_mb=512
```

Nightly CI performs a bounded smoke run for every target. Minimized reproducers
are retained as the `fuzz-artifacts` workflow artifact for 30 days.
