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

[Vendor provenance and changes](../vendor/README.md) records release checksums,
upstream commits, licenses, and validation commands. How to remove: upgrade to
published releases with both fixes, remove the corresponding patches from the
root and fuzz manifests, delete the vendored sources, and regenerate both
lockfiles. Run the collaboration tests and the all-features advisory gate;
none of the #1043 exceptions should return.

## Patch policy

A patch must be mirrored in `fuzz/Cargo.toml` (a separate workspace the root's
patches do not reach, #883). Git patches must pin a **`rev`, never a branch**;
vendored patches must use checked-in sources under `vendor/`, preserve their
licenses, and document provenance and local changes. `tests/test_patched_dependencies.py`
checks the source pins, documentation, and matching fuzz patches.

The previous `oxml-layout` fontdb feature-unification fork (`tuna-os/rdocx`,
#283, #943) was dropped with rdocx 0.16: upstream `oxml-layout` 0.14 reads any
`fontdb::Source` it does not match explicitly through fontdb itself, so
`memmap`'s `SharedFile` no longer breaks the build.
