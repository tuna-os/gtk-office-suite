## Concrete persistence architecture and acceptance tests

September audit at `e7e4df6`; retain this issue as recovery owner. Depends on #437 (durable writes), #436 (save outcome) and #354 (fault-injection journeys).

Use one document-session lifecycle with an explicit revision/savepoint and outcomes for save/cancel/failure. Recovery is a separate checkpoint, never a successful user save. Keep the last verified checkpoint until a newer checkpoint commits, a real save succeeds, or the user explicitly discards it.

Current AutosaveSlot stores bytes and metadata in separate atomic writes. A crash between them can pair different generations; metadata uses newline-delimited paths. Recovery scanning treats mere file presence as orphan evidence, and Tables/Decks select only the first snapshot. Window autosave callbacks ignore errors. These need concrete failure tests rather than a journal chosen in advance.

- [ ] Versioned single-generation envelope (or atomic manifest pointing to immutable generation files) binds bytes, format, identity, revision and checksum.
- [ ] Correctly round-trip newline/non-UTF8 paths or explicitly reject unsupported identities without corrupting metadata.
- [ ] Distinguish active sessions from abandoned snapshots using safe ownership/locking; never recover another live window's state.
- [ ] Enumerate all recoverable documents deterministically; corrupt/incomplete candidates do not hide valid later ones.
- [ ] Preserve imported non-buffer metadata and model state; no recovery format silently strips supported content.
- [ ] Restart after recovery and before the next autosave does not lose the recovered checkpoint.
- [ ] Surface snapshot/write/cleanup errors; keep dirty state on failed commit.
- [ ] Inject failures before/after each checkpoint/rename and kill the real app; verify old-or-new complete state, never a mismatched generation.
- [ ] Cover multiple windows, multiple documents, renamed/missing originals, unsaved documents, duplicate recovery attempts and schema upgrades.

Completion requires headless lifecycle tests plus real kill/relaunch journeys for all three apps. Avoid promising perfect power-loss survival on filesystems whose durability guarantees have not been verified.
