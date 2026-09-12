# Patched dependencies

This project normally takes dependencies straight from crates.io. One entry in
the workspace `[patch.crates-io]` section is an exception, recorded here so it
does not become permanent by being forgotten.

## `oxml-layout` — fontdb feature unification

**Why.** `oxml-layout` is a non-optional dependency of `rdocx` (through
`oxml-chart`), and its `font_data_for_face` matches `fontdb::Source`
exhaustively:

```rust
match &face.source {
    fontdb::Source::Binary(data) => ...,
    #[cfg(feature = "system-fonts")]
    fontdb::Source::File(path) => ...,
}
```

fontdb's variants are gated on **fontdb's own** features — `File` on `fs`,
`SharedFile` on `fs` + `memmap` — and a dependent crate cannot see those
through its own `cfg(feature = ...)`. So that match stops compiling as soon as
anything else in the consumer's graph enables another fontdb feature. In this
workspace `krilla-svg` (via `typst-pdf`, via `suite-export`) enables `memmap`,
which adds `Source::SharedFile`:

```
error[E0004]: non-exhaustive patterns: `&fontdb::Source::SharedFile(_, _)` not covered
  --> oxml-layout-0.11.0/src/font.rs:1885:11
```

This is why #283 (rdocx 0.7 → 0.13) sat on `hold` for a week.

**What was tried first, and why it does not work.**

| attempt | outcome |
|---|---|
| pin `fontdb` back to 0.22 | `typst-kit` requires `^0.23`, and that is our PDF export |
| `rdocx` with `default-features = false` | worse: the `File` arm is gated out too, so *two* variants go unhandled |
| wait for upstream | `oxml-layout` 0.11.0 (2026-09-06) is still the newest release, against a prior cadence of one every one-to-two days, and nobody had reported it |

**The patch.** A wildcard arm, on a fork at
[tuna-os/rdocx](https://github.com/tuna-os/rdocx), branch
`fix/oxml-layout-sharedfile-arm`. Naming `SharedFile` explicitly would fail to
compile whenever `memmap` is *absent*; a wildcard is the only form that
survives feature unification, and callers already treat `None` as "no byte data
for this face" and fall back.

It is pinned by **`rev`, never a branch** — a moving ref would make the build
irreproducible, which is the opposite of what the release gate is for.

**How to remove it.** When upstream publishes an `oxml-layout` that handles the
variant (or adds its own wildcard):

1. drop the `[patch.crates-io]` section from the workspace `Cargo.toml`;
2. `cargo update -p oxml-layout`;
3. `cargo check -p letters` — the E0004 above is the thing that must stay gone;
4. delete this section.

`tests/test_patched_dependencies.py` checks that every patch entry here is
pinned by `rev` and documented, so a patch cannot quietly turn into a moving
branch ref or an undocumented fork.
