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

## Collaboration byte paths

What other collaborators send arrives over the network, not from a file the
user chose to open, so it is fuzzed too (#1995):

- `collab_codec` (this crate): the sidecar protocol lines the apps and
  `collabd` exchange, through `LineReader`, `decode_request` and
  `decode_message`. A line that decodes must also survive its own encoder.
- `dcp_update_ingest` and `dcp_check_package` (`collab/fuzz`, a crate of its
  own because `dcp-core` lives in the `collab/` workspace): update ingest
  through the update schema and `Receiver::ingest`, which must apply an update
  at most once and nothing on a second read of the same log, and the ZIP
  directory walk in `integrity::check_package`.

```sh
cargo fuzz run collab_codec -- -max_len=65536 -timeout=10
cargo fuzz run --fuzz-dir collab/fuzz dcp_update_ingest -- -max_len=65536 -timeout=10
```

Nightly CI performs a bounded smoke run for every target. Minimized reproducers
are retained as the `fuzz-artifacts` workflow artifact for 30 days.
