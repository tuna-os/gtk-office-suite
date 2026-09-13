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
      **Decks is now covered by `decks-core/tests/snapshot_fidelity.rs`.**
      Notes, backgrounds, object geometry, run styles, slide order, shape
      rotation and slide masters all survive both formats, and
      `snapshot_fidelity.rs` has no `#[ignore]` left. Rotation survived neither until
      recently: both writers emitted none at all and both readers hardcoded
      zero, so the rotate gesture's result was discarded by any save. pptx
      spells it `a:xfrm/@rot`, one attribute, and was fixed first. ODF has
      no rotation attribute — it spells the same thing as a
      `draw:transform` list, carrying rotation and position together — and
      that is now written and read too, with the convention established by
      probing Impress rather than by reading the spec:
      - ODF's angle is radians *counter-clockwise* where OOXML's `rot` is
        sixtieth-thousandths of a degree *clockwise*, so the ODF angle is
        the plain negation of the model's degrees, left negative rather than
        normalised into `[0, 2π)` — which is what Impress itself writes.
      - The matrix for `rotate (a)` turns counter-clockwise in a y-down
        space, not SVG's direction, and the terms apply left to right, so
        `rotate (a) translate (t)` maps a local point `p` to `R(a)·p + t`.
        The shape's local box starts at the origin and OOXML rotates about
        the centre, so the translate is `centre − R(a)·(w/2, h/2)`.
      Both facts are asserted offline by `the_transform_we_write_is_the_one
      _impress_wrote`, which reproduces Impress's own output byte-for-byte
      to its three decimal places of centimetres, and end-to-end by two
      oracle tests that cross formats in both directions. Crossing formats
      is the point: read our own odp back and a mirrored convention cancels
      itself out and passes, which is exactly what happens to the
      round-trip tests when the matrix is mutated to SVG's — they stay
      green and only the two Impress-grounded tests go red.
      **Masters now survive too, in both formats**, which was the last
      thing this row was waiting on. Decks read a master from an imported
      deck and rendered it — the canvas and the sidebar thumbnails both
      consult it — while neither writer emitted one, so the reader
      synthesised a white default and the deck's design was gone. Like the
      Tables bug above it was a writer/reader asymmetry pointing the other
      way, and it cost an imported deck's design on *every* save rather
      than only on recovery.
      A pptx master is three parts, not one: `p:sldMaster` carries the
      decorations, a `p:sldLayout` sits between it and the slides, and it is
      the layout a slide relates to. The reader already walked exactly that
      chain, so all three parts and their four relationship files now get
      written; decorations go on the master and the layout's shape tree is
      left empty, because the reader concatenates both and writing the
      shapes twice would double them on every save. ODF puts masters in
      `styles.xml` under `office:master-styles`, which the odp writer did
      not write at all; each `draw:page` now names its master with
      `draw:master-page-name`, escaped into a style token the way
      LibreOffice escapes it (`House_20_Style`) and unescaped on the way
      back.
      Two things about the tests, both of which were wrong first and are
      worth keeping written down:
      - Reading masters goes through the *same* walker as slides now
        (`parse_pages`, with the page tag as a parameter), because a
        master's decorations came back as nothing while that logic existed
        only for slides. Any shape the slide reader learns, the master
        reader now learns too.
      - The mapping assertion needs a slide on a master that is *not* the
        first. Losing the mapping falls back to master 0, so a deck whose
        every slide is already on master 0 cannot tell that apart from
        working — the first version of this test could not, and two
        mutations (the odp page dropping its master name, the pptx slide
        always relating to layout 1) passed under it.
      `impress_keeps_the_master_we_write` is the check our own reader cannot
      make: the pptx chain spans three parts and four relationship files,
      and a package where any link is missing still round-trips through code
      that looks where it wrote. Impress drops a master it cannot resolve,
      so handing both formats to Impress and reading back what it rewrote is
      what proves the parts are actually related.
      What keeps this row `[~]` rather than `[x]`: the odp writer still
      drops `SlideObject::Image`, which is supported content and a real
      strip. It needs a packaged media part and a manifest entry, the same
      shape of work the pptx path already does — the row cannot honestly
      say "no format strips supported content" until it does. Two smaller
      ones, recorded so they are not rediscovered as bugs: a master's
      `default_font` is not carried by either format (pptx would need a
      theme, ODF a page style this reader does not model), and a master
      decoration's run styling is not either, in both cases because
      *neither* reader fills them, so the writers emit nothing rather than
      writing something nothing reads.
      A note on how the odp rotation gap was found, because the test design
      hid it: the fidelity tests looped `for kind in FORMATS` and asserted
      inside the loop, which aborts on the first format that fails. While
      pptx was broken the odp failure was invisible, and it only surfaced
      when fixing pptx made the same assertion fail again with a different
      prefix. A loop over cases reports one case — so
      `shape_rotation_survives_a_snapshot` now collects its complaints and
      asserts once at the end, naming every format that regressed instead of
      only the first.
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
      **Cleanup errors now leave a trace.** #666 neutralised the
      *consequence* of a failed clear — `find_orphaned_snapshots` no longer
      offers a snapshot the saved file has overtaken, so already-saved work
      is not handed back as a "recovered" document on every subsequent
      launch — and deliberately did not put a warning in front of somebody
      whose save just worked, about a temporary file they cannot act on.
      That was the right call on the dialog and the wrong one on the record:
      it left a failing clear with no output at all, so whoever is asking
      why a state directory keeps filling up had nothing to read. Every
      `let _ = slot.clear()` in the three apps is now `clear_or_report()`,
      which logs the doc id and the reason on stderr and says nothing when
      the clear works — the house convention for a diagnostic nobody can act
      on mid-session. `TablesUnclearableSnapshotSmoke` is the journey: it
      replaces the app's own snapshot *file* with a directory, so
      `remove_file` fails for root as well, saves with Ctrl+S, and reads the
      app's stderr. Blocking the whole state directory instead would have
      passed while proving nothing — the snapshot would never exist, so the
      clear would succeed with nothing to do.
      Still open in this row, and why it is `[~]` rather than `[x]`: "keep
      dirty state on failed commit" is the save transaction rather than the
      snapshot — #436 and #437 own it.
- [~] Inject failures before/after each checkpoint/rename and kill the real app; verify old-or-new complete state, never a mismatched generation. The headless half is done: `atomic_save::fault` arms any of the six boundaries of a durable write (temp create, permission preservation, data write, data sync, rename, directory sync) and any arrival at one, so "fail the second commit of this transaction" is expressible. A sweep asserts that every pre-commit boundary leaves the destination byte-identical with no temporary left behind, that the one post-rename boundary reports the replacement rather than claiming a rollback, and that no boundary or arrival in a snapshot write can pair two generations. The hook is `cfg(test)` only — a release build contains no branch to take.
      **What a real kill does was then measured rather than assumed, and it
      found a defect the fault sweep structurally could not.** A SIGKILL
      during a 600 MiB `atomic_write_bytes` left the destination intact —
      the atomicity promise held — and left a 78 MiB
      `.office-save-J6zNaJ` beside it. The prefix appeared exactly once in
      the whole repository, in the line that creates it, so nothing ever
      removed one: `tempfile` cleans up on drop, which covers every *error*
      and no *kill*, because SIGKILL, an OOM kill and a power cut run no
      destructor. One hidden file the size of the document, in the user's
      own document directory, per crashed save, forever.
      That is the shape of gap this row keeps producing: "no temporary left
      behind" was asserted by a mechanism that guarantees it. A failing
      write unwinds; only a killed one strands anything, and the sweep
      cannot kill.
      `atomic_write_bytes` now sweeps stranded temporaries from the
      destination directory after a successful save. It is deliberately
      narrow, because it deletes files in a directory the user owns: only
      regular files (not symlinks, not directories), only names carrying
      this module's own prefix, and only ones at least a day old — a live
      save's temporary exists for one `write_all` plus one `fsync`, so a
      day is four orders of magnitude of margin and a concurrent window's
      temporary can never plausibly be caught. Errors are ignored
      throughout, so a read-only directory cannot fail a save that already
      landed.
      The kind check needed a mutation to become real. Dropping `is_file`
      changes nothing for a directory, because `remove_file` refuses one
      anyway — it changes what happens to a *symlink*, which `remove_file`
      will happily unlink. The test that covers it injects the clock
      instead of backdating the entries, because a symlink's own mtime
      cannot be set through `std` and a real-time sweep would skip the link
      for being fresh, which is how that test would have passed while
      testing nothing.
      Still open: killing the real app under the GUI harness. The hook is
      `cfg(test)`-only by design, so a journey cannot arm a boundary in a
      release binary, and racing a real save with SIGKILL only reaches the
      window if the document is big enough to make the write slow — which
      is not something a journey can type in. A probabilistic sweep of that
      shape belongs in the nightly stress workflow rather than in a
      deterministic journey, and the measurement above is what a first pass
      at it would have produced.
- [x] Cover multiple windows, multiple documents, renamed/missing originals, unsaved documents, duplicate recovery attempts and schema upgrades. All six have headless lifecycle tests in `suite-common-core/src/autosave.rs` and a real kill/relaunch journey in every app, which is what this row's completion note asks for.
      | scenario | Tables | Letters | Decks | journey |
      |---|---|---|---|---|
      | multiple windows | yes | yes | yes | `LiveOwnerMixin` |
      | multiple documents | yes | yes | yes | `TwoDocumentsMixin`; Letters via `LettersAutosaveSmoke` |
      | renamed/missing original | yes | yes | yes | `RenamedOriginalMixin` |
      | unsaved documents | yes | yes | yes | the autosave journeys |
      | duplicate recovery | yes | yes | yes | `*RecoveryIsItselfProtectedSmoke` |
      | schema upgrade | yes | yes | yes | `LegacySnapshotUpgradeMixin` |
      Letters reaches "multiple documents" through ordinary use rather than a
      planted pair: a window holds a document per tab, so two dirty tabs are
      two documents with two slots, and `LettersAutosaveSmoke` has asserted
      since #99 that a crash recovers both. The single-document apps are the
      ones that have to *choose* which orphan to take, so those get the
      planted pair and the ordering assertion.
      Each mixin is pinned by a mutation rather than by its own passing.
      Reversing `find_orphaned_snapshots`' comparator brings up
      `'older.xlsx (Recovered) — Tables'` and `'older.pptx (Recovered) — Decks'`.
      Making #718's legacy guard unconditional — the over-refusing direction,
      which silently discards the unsaved work of somebody who crashed on the
      old build and upgraded — fails the schema-upgrade journey in all three
      apps. Flipping `superseded_by_a_real_save`'s missing-original bias
      fails the renamed-original journey in all three while the
      stale-snapshot journey keeps passing, which is what shows that pair is
      pinning opposite sides of one decision rather than restating it.
      The planted documents are hand-built in every case — `minimal_xlsx_bytes`,
      `minimal_pptx_bytes`, and a spelled-out `Document` JSON literal for
      Letters. A snapshot the app wrote would be an envelope and would prove
      nothing about the legacy path. Letters' literal spells out every field
      because `ParaStyle` has no serde defaults: a trimmed one is rejected
      with `missing field 'alignment'`, and the journey would then fail as a
      timeout rather than as a parse error.
      Writing the schema-upgrade case is also what found #718's defect, which
      was not in the upgrade path at all — see that row above.

Completion requires headless lifecycle tests plus real kill/relaunch journeys for all three apps. Avoid promising perfect power-loss survival on filesystems whose durability guarantees have not been verified.
