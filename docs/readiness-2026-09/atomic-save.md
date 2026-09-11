# [P0] Shared saves: exclusive temporary files, permission preservation, and durable commit

Audited on `e7e4df6`; closed out on `902d9d8`.

## Reproduced defects
`suite-common-core/src/atomic_save.rs` created `.<name>.tmp-<pid>` with
`File::create`. A pre-existing symlink at that name caused an unrelated file
to be truncated. Replacing a mode-0600 destination also changed it to the
process default permissions (observed 0644). Parallel writes in one process
shared the temporary name. The parent directory was not synced after rename.

The same defect then turned up a second time, in the one place that had not
gone through this module: `OpaquePackage::append_to` in
`suite-common-core/src/interop.rs` wrote its own temporary at a fully
predictable name (`report.odt` → `report.odt-opaque-tmp`) with
`File::create`. That is the last step of every opaque-preserving save —
`letters-core/src/odt.rs`, `letters-core/src/docx.rs`,
`tables-core/src/io/save.rs`, `decks-core/src/lib.rs` — so a planted symlink
there destroyed an unrelated file on an ordinary save. It now assembles the
package in memory and hands it to `atomic_write_bytes` like every other
writer, which is what the module header asked for all along.

## Design
Use an exclusively created random temporary file in the destination
directory with owned cleanup. Preserve existing file permissions; new files
default to private permissions. Write and sync data, atomically persist,
then sync the containing directory on Unix. A post-rename durability failure
must be reported without claiming the original bytes remain. Document
destination-symlink, hard-link, ownership/ACL and portal-filesystem policies
rather than silently promising metadata preservation.

## Acceptance
- [x] Stale temporary symlink cannot alter unrelated bytes or be removed by
      the save — `save_does_not_follow_a_stale_temporary_symlink`, and
      `appending_opaque_parts_does_not_follow_a_stale_temporary_symlink` for
      the package path.
- [x] Existing private mode remains private —
      `save_preserves_existing_file_permissions`.
- [x] Concurrent writers never produce mixed/truncated bytes or interfere
      with temporary ownership —
      `concurrent_savers_never_expose_a_partial_or_blended_file`: six
      threads write distinct 256 KiB payloads to one path, 40 rounds each,
      while a reader watches; every observation must be one whole payload,
      and the directory must hold nothing but the destination afterwards.
      Swapping the writer for `fs::write` fails it, so it detects the
      failure mode rather than merely passing.
- [x] Create/write/commit failure preserves the destination where commit has
      not occurred and leaves no owned temp —
      `every_pre_commit_boundary_leaves_the_original_intact` sweeps all six
      injected boundaries, `failed_commit_preserves_destination_and_cleans_temporary_file`.
- [x] Relative/non-UTF8 paths and invalid destinations return correct
      results — `a_bare_relative_file_name_saves_into_the_working_directory`
      (bare name and `sub/nested.txt`),
      `a_non_utf8_path_saves_to_that_exact_path` (a `0xFF` byte in the file
      name, saved and overwritten, with nothing written beside it — a lossy
      conversion would land on a neighbouring U+FFFD name),
      `path_without_file_name_errors_before_any_write`,
      `rename_over_directory_fails_and_cleans_up_temp_file`.
- [x] Directory sync semantics and caller error handling are
      documented/tested as supported — the `SyncDir` boundary is injectable
      and `a_failure_after_the_rename_says_so_rather_than_claiming_a_rollback`
      asserts the caller is told the file *was* replaced.
- [x] All format writers and recovery paths are inventoried for bypasses —
      six format writers, PDF export and autosave all route through
      `atomic_write_bytes`; `append_to` was the only bypass and no longer is.
- [x] Shared-core suite, relevant persistence tests, lint and a GUI
      save-failure journey pass — `test_failed_save_retains_edits_and_close_guard`
      in `tests/gui/test_smoke.py` is the GUI journey and runs in CI.

This is a narrow prerequisite for the save lifecycle and #322, not a request
to replace the format engines.
