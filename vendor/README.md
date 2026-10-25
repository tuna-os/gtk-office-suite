# Collaboration dependency backports (#1043)

These are unpacked crates.io releases, patched locally so the optional product
collaboration features need no RustSec exceptions for `im`, `sized-chunks`,
`bitmaps`, or `atomic-polyfill`. Their licenses remain in each crate directory.
The root workspace and `fuzz/` both patch crates.io to these checked-in sources.
The measurement spikes remain separate experiments with their own lockfiles.

| Directory | Release archive SHA-256 | Upstream revision |
| --- | --- | --- |
| `loro-internal` (1.17.1) | `7e533c1688430aa4fcdd933f00861e331c169a4158ee6d27d054e0558892d447` | `loro-dev/loro@ef83c083f19aea0d590a46db2d541b3a5daf1339` |
| `serde_columnar` (0.3.14) | `2a16e404f17b16d0273460350e29b02d76ba0d70f34afdc9a4fa034c97d6c6eb` | `loro-dev/columnar@06663cfc569c6c770b28688f015f8a87fc04e156` |

Archives are available at
`https://static.crates.io/crates/<crate>/<crate>-<version>.crate`.
All archive files are preserved except the nested `Cargo.lock` files (unused
by the product workspace). `LICENSE` was copied from each upstream revision's
repository root because the published archives omit it.

Local changes, also reflected in `Cargo.toml.orig`:

- `loro-internal`: replace `im` 15.1 with `imbl` 7.0.2, as proposed in
  [loro-dev/loro#1122](https://github.com/loro-dev/loro/pull/1122).
  `ImVersionVector` uses `GenericHashMap` to retain its Fx hasher and default
  shared-pointer kind; its iterator's type names that pointer kind explicitly.
  The DAG test uses `imbl::HashSet`. No CRDT or encoding algorithms change.
- `serde_columnar`: disable Postcard's default features while retaining `alloc`.
  The unused `heapless-cas` default was the path through `heapless` 0.7 to
  `atomic-polyfill`; columnar uses Postcard's allocation-backed serializers.
  Loro already requests Postcard with defaults disabled and `use-std` enabled.

## How to remove

Upgrade to published Loro/serde_columnar releases carrying these changes, then
remove both patch entries from the root and fuzz manifests and delete the
corresponding vendor directories. Regenerate both workspace lockfiles and run:

```sh
cargo test -p tables-core -p decks-core --features tables-core/collab,decks-core/collab collab
cargo deny --all-features check advisories
python3 -m unittest discover -s tests -p 'test_patched_dependencies.py'
```

Keep the #1043 advisory ignores removed. Check the resolved graph, including
all features, before dropping either backport: fixing `im` alone does not
remove `atomic-polyfill`.
