## Concrete persistence architecture and acceptance tests

September audit at `e7e4df6`; retain this issue as recovery owner. Depends on #437 (durable writes), #436 (save outcome) and #354 (fault-injection journeys).

Use one document-session lifecycle with an explicit revision/savepoint and outcomes for save/cancel/failure. Recovery is a separate checkpoint, never a successful user save. Keep the last verified checkpoint until a newer checkpoint commits, a real save succeeds, or the user explicitly discards it.

AutosaveSlot used to store bytes and metadata in separate atomic writes. Each write was atomic, which is not the same as the pair being atomic: fault injection at every boundary of the write (`suite-common-core/src/atomic_save.rs::fault`) read back `"generation two"` paired with `/tmp/first.md`, kind `md` — after a Save As, recovery would have offered the new content under the old path and format. That is a silent wrong-file restore, and it is now one atomic write of one versioned envelope. Tables and Decks no longer stop at the first snapshot, and the window autosave callbacks no longer ignore errors — see the rows below for what each of those does and does not now cover.

- [x] Versioned single-generation envelope (or atomic manifest pointing to immutable generation files) binds bytes, format, identity, revision and checksum — `suite-common-core/src/autosave.rs::envelope`, one atomic write, CRC-32 over the payload, magic whose last byte is the version. Snapshots written by the previous two-file build are still read so an upgrade mid-crash does not discard unsaved work. A revision counter is **not** in the envelope: nothing in the suite has one yet, and a field always written zero would look like coverage.
- [x] Correctly round-trip newline/non-UTF8 paths or explicitly reject unsupported identities without corrupting metadata — the path is stored as its native OS bytes on Unix, so a newline or invalid UTF-8 round-trips exactly; off Unix a non-Unicode path is rejected at write time rather than mangled. The previous newline-delimited text metadata truncated the first at the newline and `to_string_lossy`-mangled the second into a path that does not exist.
- [x] Distinguish active sessions from abandoned snapshots using safe ownership/locking; never recover another live window's state. Each `AutosaveSlot` claims `<doc-id>.lock` with `flock(2)` for as long as its window lives, and `find_orphaned_snapshots` skips any slot whose lock it cannot take. "Orphaned" used to mean only "not the document being opened", so a second window opened while the first was still editing was offered the first window's **in-progress** autosave — accept it and two windows own one document, with the recovering window's content overwriting a session that never crashed.
      `flock` rather than a pid or a flag file because the failure mode
      recovery exists for is SIGKILL: a process that dies running no cleanup
      at all. Anything the writer has to remember to clear is
      indistinguishable from a stale copy of itself after one. The kernel
      releases an `flock` on process death and never attaches it to a file
      sitting on disk, and because the lock is per open file description
      rather than per process, a second descriptor conflicts with the first
      even inside one process — which is how the unit tests exercise it
      without forking.
      An unusable lock path (unreadable directory, a regular file where the
      lock should go) is biased toward **offering** the snapshot: failing to
      prove a window is alive is not evidence that one is, and the costs are
      not symmetric — a spurious recovery prompt is a dialog, a suppressed
      one is lost work. Wired into all three apps: Tables and Decks hold the
      claim as a window field, Letters on `DocumentSession` so a per-tab
      document gets a per-tab lock.
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
- [~] Preserve imported non-buffer metadata and model state; no recovery format silently strips supported content. **Tables is done and was badly wrong.** Its snapshot is an xlsx package from `save_sheets_to_xlsx_bytes`, read back by the same `load_workbook` a plain Open uses — and that reader parsed no column widths, no row heights, no frozen panes and no merged ranges. All four were written correctly and silently dropped on the way back in, so the loss was never specific to recovery: any save-then-reopen lost them too, and recovered work inherited that.
      The gap survived because the tests that covered it were about the
      wrong program. `soffice_oracle.rs` has
      `column_widths_survive_calc_rewrite`, `frozen_panes_survive_calc_rewrite`
      and `merged_cells_survive_calc_rewrite`, each asserting that the *XML*
      still carries the feature after LibreOffice rewrites the file. They
      prove LibreOffice preserves what Tables writes. None of them asks
      whether Tables can read its own output back, so they passed for as long
      as the reader ignored all four. A round-trip claim needs both halves to
      be *ours*; an oracle comparison is a different claim wearing the same
      words. `tables-core/tests/snapshot_fidelity.rs` closes it through the
      byte path a snapshot actually uses, with the negative controls that a
      careless fix would trip (a default sheet must not come back carrying
      "explicit" defaults, and sheet two must not inherit sheet one's layout).
      **Letters checks out.** Its snapshot is the whole `Document` as JSON
      with nothing `serde(skip)`, and the four fields that do not live in the
      `GtkTextBuffer` — header, footer, page geometry, footnotes — ride on
      sidecar data attached to the buffer: `capture_from_buffer` reads them
      and `render_to_buffer` reinstalls them through `set_buffer_sidecars`,
      so the recovery path closes the loop. Round-trip tests already cover
      header, footer and page geometry.
      **Decks is now covered by `decks-core/tests/snapshot_fidelity.rs`, and
      two gaps remain**, which is why this row is partial rather than done.
      Notes, backgrounds, object geometry, run styles and slide order all
      survive both formats, and shape *rotation* now survives pptx — it
      previously survived neither, because both writers emitted no rotation
      at all and both readers hardcoded zero, so the rotate gesture's result
      was discarded by any save. The two that still strip content:
      - **Rotation in odp.** pptx spells it `a:xfrm/@rot`, one attribute,
        fixed. ODF spells the same thing as
        `draw:transform="rotate(θ) translate(x y)"`, entangling rotation with
        position in one attribute that LibreOffice emits in several shapes.
        Larger than a fix, so not pretended to be one.
      - **Masters, in both formats.** Decks reads a master from an imported
        deck and renders it — the canvas and the sidebar thumbnails both
        consult it — but neither writer emits one, so the reader synthesises
        a white default and the deck's design is gone. Like the Tables bug
        above this is a writer/reader asymmetry, pointing the other way, and
        it costs an imported deck's design on *every* save rather than only
        on recovery. `masters_survive_a_snapshot` specifies the fix and is
        `#[ignore]`d with that reason, so it appears in every run's skip
        report instead of being a comment nobody reads; unignore it when a
        writer starts emitting masters.
      A note on how the odp rotation gap was found, because the test design
      hid it: the fidelity tests loop `for kind in FORMATS` and assert inside
      the loop, which aborts on the first format that fails. While pptx was
      broken the odp failure was invisible, and it only surfaced when fixing
      pptx made the same assertion fail again with a different prefix. A loop
      over cases reports one case.
- [x] Restart after recovery and before the next autosave does not lose the recovered checkpoint.
      All three apps used to clear the orphan slot as soon as the recovered
      content was in memory and leave the next autosave tick to write a
      replacement — up to a minute later at the shipped 60-second interval,
      and never at all in a Letters that shipped with its timer switched
      off. A crash inside that window lost work that had just survived a
      crash, which is the one thing recovery must not do.
      The order is inverted now: `AutosaveSlot::adopt_recovered` writes the
      recovered content to the new window's own slot and reports whether the
      orphan may be dropped, so the work is covered continuously. A failed
      write keeps the orphan, so content whose replacement could not be
      written is still offered next launch; the cost is that a clean save
      this session will not clear that orphan — the close path clears the
      window's own slot, not the one it recovered from — so the same content
      can be offered once more. Work offered twice is recoverable; work
      silently dropped is not.
      `{Tables,Decks,Letters}RecoveryIsItselfProtectedSmoke` crash, recover,
      then crash again with no autosave in between and require the content
      to still be there; each fails against the old order. The two branches
      of `adopt_recovered` are unit-tested in `suite-common-core`, the
      failure branch included, since "keep the orphan when the write fails"
      was otherwise only an argument in a comment.
      Worth recording what this cost in test terms: three existing journeys
      asserted `snapshot_files() == []` after recovery — nothing on disk —
      as a proxy for "the orphan is not offered twice". Zero files also
      describes *unprotected work*, so that spelling was quietly asserting
      the defect. They now assert the intent directly and more strictly: the
      recovered orphan is gone, and the recovered document is itself covered.
      The rewritten assertions still fail against the old code.
- [~] Surface snapshot/write/cleanup errors; keep dirty state on failed commit.
      **Snapshot write failures now reach the user.** Every autosave write
      site in the three apps read `let _ = slot.write(&bytes, &meta);` — five
      of them. A snapshot write fails for ordinary reasons (a read-only home,
      a full disk, a sandbox denying the state directory) and when it did,
      autosave did nothing for the rest of the session while the user went on
      believing their unsaved work was protected. They found out at the
      crash, which is the one moment the feature exists for.
      The decision of when to speak is
      `suite_common_core::autosave::AutosaveNotices`, which is GTK-free and
      tested: the first failure of a streak, a reminder every tenth failed
      attempt after that (about ten minutes at the 60-second timer Tables and
      Decks ship), and one notice when it starts working again. Reporting every
      failure would raise a notice 120 times an hour, which is a notice
      nobody reads. `suite_common::autosave_notice::AutosaveNotifier` turns
      those answers into toasts, so each call site gained one line rather
      than a copy of the policy.
      **Tables had nowhere to put a notice at all.** It carried no
      `AdwToastOverlay` and no `add_toast` call anywhere in the crate: the
      one `adw::Toast` in the file — "Invalid input — value rejected" — was
      built, given a timeout, and dropped un-shown, so a rejected cell value
      told the user nothing. It has an overlay now and that toast is
      actually posted.
      Verified in the real apps rather than only in unit tests:
      `TablesAutosaveFailureSmoke` and `LettersAutosaveFailureSmoke` make the
      write fail by pointing `XDG_STATE_HOME` at a regular file (ENOTDIR,
      which fails for root too — a permission bit would not, and
      `crash-stress.md` records a previous test that chmod-ed to 0555 and
      therefore asserted nothing), then assert the notice is on screen. Both
      fail against the unwired code.
      Two defects were found while checking that figure, both since fixed.
      **Letters shipped with its autosave timer switched off** (#659). Its
      `auto-save-interval` default was `0` where Tables and Decks ship `60`,
      and `register_autosave` installs a timer only `if interval > 0`, so a
      shipped Letters never snapshotted on its own: every crash-recovery
      guarantee this file makes for it held only when something invoked the
      `autosave-now` action, which every Letters autosave journey did
      explicitly. They proved the snapshot machinery worked and asserted
      nothing about whether it ever ran. The default is `60` across all
      three apps now, `tests/test_autosave_defaults.py` compares them so the
      next divergence fails a check, and
      `{Letters,Tables,Decks}UnattendedAutosaveSmoke` wait for a snapshot
      without triggering anything. The range still permits `0`, so a user
      who chose to switch autosave off keeps that: a changed default reaches
      only installs that never set the key.
      **Recovery left a window with no protection at all** — the row two
      above this one, fixed there: all three apps cleared the orphan slot as
      soon as the content was in memory and relied on the next timer tick
      for a replacement, so a crash in that window lost work that had
      already survived one crash.
      **Cleanup errors are neutralised rather than surfaced** (#666), which
      is why this row is still `[~]`. `clear_tab_autosave` and the
      `slot.clear()` calls in Tables and Decks do still drop their errors, so
      a snapshot whose clear fails is still left on disk. What changed is the
      consequence: `find_orphaned_snapshots` no longer offers a snapshot the
      saved file has overtaken, so already-saved work is not handed back as a
      "recovered" document on every subsequent launch.
      That was a deliberate choice over the obvious one. A failed clear
      happens at the moment of a *successful* save, about a temporary file
      the user cannot act on, so reporting it would put a warning in front of
      someone whose save just worked. The false recovery offer is what they
      actually experience, so that is what was removed. The trade-off is that
      a failing clear now leaves no trace at all for anyone diagnosing a full
      or read-only state directory — arguably a log line's job rather than a
      toast's, and not done either way.
      Also still open in this row: "keep dirty state on failed commit" is the
      save transaction rather than the snapshot — #436 and #437 own it.
- [~] Inject failures before/after each checkpoint/rename and kill the real app; verify old-or-new complete state, never a mismatched generation. The headless half is done: `atomic_save::fault` arms any of the six boundaries of a durable write (temp create, permission preservation, data write, data sync, rename, directory sync) and any arrival at one, so "fail the second commit of this transaction" is expressible. A sweep asserts that every pre-commit boundary leaves the destination byte-identical with no temporary left behind, that the one post-rename boundary reports the replacement rather than claiming a rollback, and that no boundary or arrival in a snapshot write can pair two generations. The hook is `cfg(test)` only — a release build contains no branch to take. Killing the real app under the GUI harness is still open.
- [ ] Cover multiple windows, multiple documents, renamed/missing originals, unsaved documents, duplicate recovery attempts and schema upgrades.

Completion requires headless lifecycle tests plus real kill/relaunch journeys for all three apps. Avoid promising perfect power-loss survival on filesystems whose durability guarantees have not been verified.
