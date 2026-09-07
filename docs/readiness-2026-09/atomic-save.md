# [P0] Shared saves: exclusive temporary files, permission preservation, and durable commit

Audited on `e7e4df6`. Implementation started locally; not merged.

## Reproduced defects
`suite-common-core/src/atomic_save.rs` creates `.<name>.tmp-<pid>` with `File::create`. A pre-existing symlink at that name causes an unrelated file to be truncated. Replacing a mode-0600 destination also changes it to the process default permissions (observed 0644). Parallel writes in one process share the temporary name. The parent directory is not synced after rename.

Two added regression tests reproduced the symlink overwrite and permission change. The initial fix passed all 76 shared-core tests on the original checkout; it must be revalidated after rebasing to current main.

## Design
Use an exclusively created random temporary file in the destination directory with owned cleanup. Preserve existing file permissions; new files default to private permissions. Write and sync data, atomically persist, then sync the containing directory on Unix. A post-rename durability failure must be reported without claiming the original bytes remain. Document destination-symlink, hard-link, ownership/ACL and portal-filesystem policies rather than silently promising metadata preservation.

## Acceptance
- [ ] Stale temporary symlink cannot alter unrelated bytes or be removed by the save.
- [ ] Existing private mode remains private.
- [ ] Concurrent writers never produce mixed/truncated bytes or interfere with temporary ownership.
- [ ] Create/write/commit failure preserves the destination where commit has not occurred and leaves no owned temp.
- [ ] Relative/non-UTF8 paths and invalid destinations return correct results.
- [ ] Directory sync semantics and caller error handling are documented/tested as supported.
- [ ] All format writers and recovery paths are inventoried for bypasses.
- [ ] Shared-core suite, relevant persistence tests, lint and a GUI save-failure journey pass.

This is a narrow prerequisite for the save lifecycle and #322, not a request to replace the format engines.

