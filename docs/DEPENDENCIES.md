# Patched dependencies

This project normally takes dependencies straight from crates.io. Any entry in
the workspace `[patch.crates-io]` section is an exception, and is recorded
here so it does not become permanent by being forgotten: why it exists, what
was tried first, and how to remove it.

## rdocx: permit the patched Markdown parser

The product and every separate workspace that uses `letters-core` patch
the shared rdocx/OOXML family to the immutable fork revision
`0c3d3a03ce7be1ec02c0ad602c666d1a94f5b343` in
[`tuna-os/rdocx`](https://github.com/tuna-os/rdocx/commit/0c3d3a03ce7be1ec02c0ad602c666d1a94f5b343).
Published rdocx through 0.16.4 and upstream main still pinned
`pulldown-cmark =0.13.4` when this repair was made. That prevents resolving
the patched `>=0.13.6` required by
[RUSTSEC-2026-0433](https://rustsec.org/advisories/RUSTSEC-2026-0433.html).
The earlier parser waiver is not a dependency fix.

The fork starts at `57f4d7438483e109cb8e45097f3498fe1c223b23`, the upstream
revision recorded in the published rdocx 0.16.2 archive. Its archive SHA-256
is `5bfe2eb1107ef1293cde0824ec8dde8fc60cacfd40a7281c712e76dbaf8a479d`.
The Rust sources compare identically to that archive. The original monorepo
manifest is retained, changing only its parser constraint to `0.13.6`.
The complete shared family is pinned together so Cargo does not introduce
registry/git duplicate types:

- `rdocx`, `rdocx-html`, `rdocx-layout`, `rdocx-oxml`: 0.16.2.
- `oxml-chart`, `oxml-core`, `oxml-drawing`, `oxml-layout`, `oxml-media`,
  `oxml-opc`, `oxml-pdf`, `oxml-sml`: 0.14.2.

All twelve used Rust source trees compare identically with their published
archives. These are the versions already used by the product lockfile, not
an unrelated family upgrade. This is a downstream compatibility patch, not
upstream certification.

The patch set is mirrored in `fuzz`, `collab/headless`, and `tools/crdt-spike`.
The last workspace's obsolete `oxml-layout`-only fork was replaced by this
released-source family: upstream oxml-layout 0.14 already handles fontdb's
`SharedFile` source.

### How to remove

Once a published rdocx release permits `pulldown-cmark >=0.13.6`, remove the
family patch set from all four manifests and resolve all four lockfiles against
that release. Run the advisory gate, a bounded nested-image-wikilink parser
smoke with `ENABLE_WIKILINKS`, the Markdown corpus, and the affected DOCX
compatibility tests. Keep the direct parser dependencies' patched minimum.

A patch must be mirrored in `fuzz/Cargo.toml` (a separate workspace the
root's patches do not reach, #883) and pinned by **`rev`, never a branch** — a
moving ref would make the build irreproducible, which is the opposite of what
the release gate is for. `tests/test_patched_dependencies.py` checks that
every patch entry is pinned by `rev` and documented here.
