# Patched dependencies

This project normally takes dependencies straight from crates.io. Any entry in
the workspace `[patch.crates-io]` section is an exception, and is recorded
here so it does not become permanent by being forgotten: why it exists, what
was tried first, and how to remove it.

## Collaboration security backports (#1043)

The latest published Loro 1.17.1 still depends on `im`. Its maintained
replacement, `imbl`, is proposed in
[loro-dev/loro#1122](https://github.com/loro-dev/loro/pull/1122), which has not
merged. We vendor the released **loro-internal** crate with that small
migration rather than keep the `im`, `sized-chunks`, and `bitmaps` advisory
exceptions. We also vendor **serde_columnar** 0.3.14 to disable its unused
Postcard default features, removing `heapless` 0.7 and `atomic-polyfill`.
Collaboration remains optional and off by default.

[Vendor provenance and changes](../third_party/README.md) records release checksums,
upstream commits, licenses, and validation commands. How to remove: upgrade to
published releases with both fixes, remove the corresponding patches from the
root, fuzz and `collab/headless` manifests, delete the vendored sources, and
regenerate those lockfiles. Run the collaboration tests and the all-features advisory gate;
none of the #1043 exceptions should return.

## typst's CSL parser on a fixed quick-xml (#225)

PDF export links typst, whose `typst-library` always depends on hayagriva and
through it on **citationberg** 0.7.0, the latest release. That release asks for
quick-xml `^0.38.1`, and the 0.38 line has no fix for RUSTSEC-2026-0194 or
RUSTSEC-2026-0195; only quick-xml 0.41 and later are fixed. We waited for an
upstream release first and kept both advisories in `deny.toml` meanwhile.

We now vendor citationberg 0.7.0 with one change: its quick-xml requirement
becomes 0.41.0, the version the workspace already resolves. The crate compiles
without source changes, and its full test suite passes, including round-trips
of every style in the CSL styles repository and every CSL locale. The quick-xml
0.38.4 entry is gone from all three lockfiles, and so are the two advisory
ignores.

How to remove: once a citationberg release accepts quick-xml 0.41 or later,
and the hayagriva and typst releases we use allow it, remove the
`citationberg` patch from the root, fuzz and `collab/headless` manifests,
delete `third_party/citationberg`, and regenerate those lockfiles. Then check
that `cargo tree -i quick-xml@0.38.4` finds nothing and that `cargo deny check
advisories` passes without the two ignores.

## Patch policy

A patch must be mirrored in `fuzz/Cargo.toml` and `collab/headless/Cargo.toml`
(separate workspaces the root's patches do not reach, #883).
Git patches must pin a **`rev`, never a branch**;
vendored patches must use checked-in sources under `third_party/`, preserve their
licenses, and document provenance and local changes. `tests/test_patched_dependencies.py`
checks the source pins, documentation, and matching fuzz patches.

The previous `oxml-layout` fontdb feature-unification fork (`tuna-os/rdocx`,
#283, #943) was dropped with rdocx 0.16: upstream `oxml-layout` 0.14 reads any
`fontdb::Source` it does not match explicitly through fontdb itself, so
`memmap`'s `SharedFile` no longer breaks the build.
