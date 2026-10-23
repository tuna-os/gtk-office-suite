# Patched dependencies

This project normally takes dependencies straight from crates.io. Any entry in
the workspace `[patch.crates-io]` section is an exception, and is recorded
here so it does not become permanent by being forgotten: why it exists, what
was tried first, and how to remove it.

## citationberg

The root and fuzz workspaces pin `citationberg` to upstream commit
`06a591e2f237d25e1dfdedac3f3d1494c496c52d` (typst/citationberg#44). The
crates.io `citationberg` 0.7.0 release requires `quick-xml ^0.38.1`, whose
latest compatible version, 0.38.4, is affected by RUSTSEC-2026-0194 and
RUSTSEC-2026-0195. Upstream's pinned commit changes that requirement to the
patched `quick-xml 0.41`, but upstream declined a semver-incompatible 0.7.x
release and has not published a compatible successor.

A direct `quick-xml` lockfile update cannot cross citationberg's 0.x caret
constraint. Remove both patch entries and this section once hayagriva and
typst accept a published citationberg release using `quick-xml >= 0.41`.

## Previous patches

The last patch, the `oxml-layout` fontdb feature-unification fork
(`tuna-os/rdocx`, #283, #943), was dropped with rdocx 0.16: upstream
`oxml-layout` 0.14 reads any `fontdb::Source` it does not match explicitly
through fontdb itself, so `memmap`'s `SharedFile` no longer breaks the build.

A patch must be mirrored in `fuzz/Cargo.toml` (a separate workspace the
root's patches do not reach, #883) and pinned by **`rev`, never a branch** — a
moving ref would make the build irreproducible, which is the opposite of what
the release gate is for. `tests/test_patched_dependencies.py` checks that
every patch entry is pinned by `rev` and documented here.
