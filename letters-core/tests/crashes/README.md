# Retained crash inputs — Letters

Every file in this directory is an input that once made
`letters_core::odt::read` panic. `tests/malformed_inputs.rs` replays all of
them on every pull request, so a fixed crash stays fixed.

This directory being empty means no ODT crash has been found yet — not that
nothing is watching. The replay test reports how many inputs it replayed so
that "no files" cannot be mistaken for "not wired up".

## Adding one

When the nightly libFuzzer lane (`fuzz/`) or a `seed_campaign` seed finds a
panic:

1. Minimize it — `cargo fuzz tmin` for a libFuzzer artifact, or shrink the
   mutation by hand for a campaign seed.
2. Commit the minimized bytes here with a name that says what is wrong,
   e.g. `truncated-central-directory.odt`.
3. Fix the reader in the same pull request, and reference the issue.

Do not commit a large input. If the minimized case is more than a few
kilobytes it has not been minimized.
