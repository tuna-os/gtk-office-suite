## Concrete persistence architecture and acceptance tests

September audit at `e7e4df6`; retain this issue as recovery owner. Depends on #437 (durable writes), #436 (save outcome) and #354 (fault-injection journeys).

Use one document-session lifecycle with an explicit revision/savepoint and outcomes for save/cancel/failure. Recovery is a separate checkpoint, never a successful user save. Keep the last verified checkpoint until a newer checkpoint commits, a real save succeeds, or the user explicitly discards it.

AutosaveSlot used to store bytes and metadata in separate atomic writes. Each write was atomic, which is not the same as the pair being atomic: fault injection at every boundary of the write (`suite-common-core/src/atomic_save.rs::fault`) read back `"generation two"` paired with `/tmp/first.md`, kind `md` — after a Save As, recovery would have offered the new content under the old path and format. That is a silent wrong-file restore, and it is now one atomic write of one versioned envelope. Tables/Decks still select only the first snapshot, and window autosave callbacks still ignore errors.

- [x] Versioned single-generation envelope (or atomic manifest pointing to immutable generation files) binds bytes, format, identity, revision and checksum — `suite-common-core/src/autosave.rs::envelope`, one atomic write, CRC-32 over the payload, magic whose last byte is the version. Snapshots written by the previous two-file build are still read so an upgrade mid-crash does not discard unsaved work. A revision counter is **not** in the envelope: nothing in the suite has one yet, and a field always written zero would look like coverage.
- [x] Correctly round-trip newline/non-UTF8 paths or explicitly reject unsupported identities without corrupting metadata — the path is stored as its native OS bytes on Unix, so a newline or invalid UTF-8 round-trips exactly; off Unix a non-Unicode path is rejected at write time rather than mangled. The previous newline-delimited text metadata truncated the first at the newline and `to_string_lossy`-mangled the second into a path that does not exist.
- [ ] Distinguish active sessions from abandoned snapshots using safe ownership/locking; never recover another live window's state.
- [~] Enumerate all recoverable documents deterministically; corrupt/incomplete candidates do not hide valid later ones. `find_orphaned_snapshots` now lists a snapshot only if it reads back whole — a truncated or checksum-failing envelope, and a legacy data file whose metadata never landed, are skipped rather than offered — so one damaged candidate no longer hides a valid one. It costs one read per candidate at launch.
      **Deterministic ordering is done.** The function used to return
      `read_dir` order — whatever the filesystem handed back — and its own
      test had to sort the result to assert anything, which was the tell that
      there was no order to assert. It now returns the newest snapshot first,
      breaking mtime ties on `doc_id` so the order is total rather than
      merely usually-stable: a crash writes snapshots milliseconds apart, so
      two sharing one filesystem timestamp is the likely case, and without
      the tiebreak the nondeterminism would just move from the directory
      listing to the clock. That matters because a caller holding one
      document per window is choosing *which* unsaved work the user gets
      back, and "whichever the directory listed first" is not a choice
      anybody made.
      **A candidate that fails to load no longer hides the later ones
      either.** Reading back whole and *loading* are different things — an
      xlsx-shaped snapshot the importer rejects, a temp file that cannot be
      written — and Tables and Decks gave up on the first candidate that
      failed. Since a snapshot is only cleared once recovered, the failing
      one won again on every subsequent launch: one unloadable snapshot could
      bury a user's recoverable work indefinitely. Both now try each
      candidate in turn.
      **Still open: the single-document windows recover one snapshot per
      launch.** Tables and Decks hold one workbook or deck per window, so
      with two valid orphans they reopen the newest and leave the rest on
      disk for the next launch rather than discarding them. Presenting
      several at once needs the multiple-window/multiple-document work in the
      last row below; Letters already does it per tab.
- [ ] Preserve imported non-buffer metadata and model state; no recovery format silently strips supported content.
- [ ] Restart after recovery and before the next autosave does not lose the recovered checkpoint.
- [ ] Surface snapshot/write/cleanup errors; keep dirty state on failed commit.
- [~] Inject failures before/after each checkpoint/rename and kill the real app; verify old-or-new complete state, never a mismatched generation. The headless half is done: `atomic_save::fault` arms any of the six boundaries of a durable write (temp create, permission preservation, data write, data sync, rename, directory sync) and any arrival at one, so "fail the second commit of this transaction" is expressible. A sweep asserts that every pre-commit boundary leaves the destination byte-identical with no temporary left behind, that the one post-rename boundary reports the replacement rather than claiming a rollback, and that no boundary or arrival in a snapshot write can pair two generations. The hook is `cfg(test)` only — a release build contains no branch to take. Killing the real app under the GUI harness is still open.
- [ ] Cover multiple windows, multiple documents, renamed/missing originals, unsaved documents, duplicate recovery attempts and schema upgrades.

Completion requires headless lifecycle tests plus real kill/relaunch journeys for all three apps. Avoid promising perfect power-loss survival on filesystems whose durability guarantees have not been verified.
