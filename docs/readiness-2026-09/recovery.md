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
- [x] Preserve imported non-buffer metadata and model state; no recovery format silently strips supported content. **Tables is done and was badly wrong.** Its snapshot is an xlsx package from `save_sheets_to_xlsx_bytes`, read back by the same `load_workbook` a plain Open uses — and that reader parsed no column widths, no row heights, no frozen panes and no merged ranges. All four were written correctly and silently dropped on the way back in, so the loss was never specific to recovery: any save-then-reopen lost them too, and recovered work inherited that.
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
      rotation, slide masters and embedded pictures all survive both
      formats, and `snapshot_fidelity.rs` has no `#[ignore]` left. Rotation survived neither until
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
      **Embedded pictures now survive an odp save too**, which was the
      last strip in this row. The odp writer dropped `SlideObject::Image`
      outright while the pptx writer carried it, so saving a deck as odp
      lost every picture in it — the same asymmetry as the Tables bug
      above, costing the content on any save-and-reopen rather than only on
      recovery. Each picture is now a `Pictures/imageN.<ext>` part, stored
      rather than deflated (a PNG is already compressed), declared in the
      manifest with the media type its extension implies, and referenced by
      `draw:image xlink:href` inside the frame. The reader unpacks it to a
      fresh temporary file, as the pptx reader does.
      Three things here are worth keeping written down, because each is a
      test that would otherwise have passed while proving nothing:
      - **The manifest entry is invisible to our own reader.** Our reader
        goes straight to the `xlink:href`, so a package whose manifest
        never declares the picture round-trips perfectly through it — while
        a conforming reader never opens the part. Mutating the manifest
        entry away leaves `images_survive_a_snapshot` green and reddens
        only the manifest unit test and
        `impress_finds_the_picture_we_write_into_an_odp`. That is what the
        oracle is for.
      - **The byte comparison is the assertion.** Both readers hand back a
        temp-file path, so the path that comes out cannot be the one that
        went in and a test demanding it would assert the wrong thing.
        Asserting merely that *an* `Image` came back passes on a writer
        that packages an empty file, which is exactly what one of the
        mutations does.
      - **The `../` href check is not the security boundary**, and the test
        that looked like it checked one could not fail: a zip lookup is a
        name lookup, so `../` resolves to "no such entry" with or without
        the guard. What keeps this safe is that the *destination* is a
        `NamedTempFile` and the href only selects which archive entry to
        read, so that is what
        `an_unpacked_picture_lands_where_the_document_cannot_choose`
        asserts — and it reddens against a reader that rebuilds the gh-268
        `/tmp/<document-supplied>` path.
      A missing source file fails the save rather than writing a deck with
      a picture-shaped hole, matching the pptx writer. That is the right
      answer for an explicit save and a debatable one for an autosave
      snapshot, where it means one deleted image file costs all the unsaved
      work; both formats behave the same way, so it is recorded here rather
      than fixed in one of them.
      Two things kept this row `[~]` once the strips above were gone: a
      master's `default_font` and a master decoration's run styling, carried
      by neither format — in both cases because *neither* reader filled
      them, so the writers emitted nothing rather than writing something
      nothing reads. Symmetric omissions rather than silent strips, but the
      row's claim is about supported content either way. **Both are now
      carried**; what each cost to close is below, and the run-styling half
      turned up three genuine strips that nothing in the suite could see.
      On `default_font` the note above was true and incomplete, in a way
      worth recording because it nearly produced the mistake this row keeps
      catching. The field was not merely unfilled by the readers: it was
      **read nowhere in the workspace** — written at all eight construction
      sites, consumed at none, while the renderer hardcoded `"Sans"` for
      every text box. So "carry `default_font` through the pptx and odp
      writers" would have been a round-trip that preserved a value with no
      effect, and a round-trip test over it would have passed while the
      font it named was never applied once. That is the same defect shape
      as every other entry in this row, arrived at from the opposite
      direction: not a writer dropping supported content, but content that
      was never supported being treated as though it were.
      So the renderer honours it first. Slide text and the master's own text
      shapes both draw through it; application chrome (the `<image>`
      placeholder, the "Slide N" indicator) keeps its own face on purpose,
      since a deck asking for a display face should not restyle the
      furniture. A blank-but-present font falls back rather than asking
      pango for a family with no name, which is what an `unwrap_or` alone
      would have done. That policy lives on `MasterSlide::font_family` and
      nowhere else, because the renderer and both writers have to agree:
      a font carried into a package the canvas would not have drawn is a
      round-trip of something nothing honours.
      Both formats now carry it, and **they do not carry it equally.** The
      asymmetry is a property of the formats, was measured rather than
      assumed, and is pinned by
      `masters_keep_their_own_font_in_pptx_but_share_one_in_odp`:
        * **pptx** gets a theme part per master
          (`a:fontScheme/a:minorFont/a:latin`), so two masters keep two
          fonts. Both the major (heading) and minor (body) fonts are
          written from the one the model has; writing only the minor would
          leave a reader that consults headings falling back elsewhere.
          The theme is also **required** rather than optional — ECMA-376
          gives every `p:sldMaster` exactly one theme relationship, and
          until now this writer emitted masters with none. Impress opened
          those packages regardless, which is why nothing caught it.
        * **odp** gets one `style:default-style[@style:family="graphic"]`
          for the whole document. That is where a real consumer puts it:
          converting a pptx carrying a per-master theme font through
          Impress and reading its `.odp` back shows exactly that element,
          and nothing per-master. So a deck whose masters name *different*
          fonts keeps the first master's and the rest read back as that
          one. Writing per-master presentation styles was the alternative
          and was not taken: nothing was found that reads them back, so it
          would have round-tripped through this reader alone.
      One measurement worth keeping, because it first looked like a bug
      here. An oracle test asserting odp → Impress → **pptx** failed with
      `["Arial"]`. Impress is not dropping what this writer emits: the same
      file converted odp → **odp** comes back with the font intact, so
      Impress reads our `style:default-style` and writes it straight out.
      What loses it is Impress's *pptx exporter*, which does not derive a
      theme `a:fontScheme` from the document's default graphic font. That
      is a gap in its export path, and the test now asserts the direction
      that is a claim about this package rather than about someone else's
      bug.
      A second limitation of the same kind, found the same way: deleting
      the theme part's `[Content_Types].xml` override leaves every
      round-trip test in `snapshot_fidelity.rs` green **and all 35 oracle
      tests green**, because LibreOffice opens the package anyway. ECMA-376
      still requires the override and a stricter consumer may reject the
      part, so `the_pptx_declares_the_theme_part_it_ships` asserts the
      package bytes directly. That is the weakest kind of check in that
      file and is used only where no behavioural one can exist — the same
      situation as #727's manifest entry, except there the oracle could
      see it.
      Its first version was itself too weak, which is worth recording: it
      checked that the rels part contained `rId1` and that it contained
      `slideLayout1.xml`, as two independent substrings. Both hold
      whichever order the relationships are written in, so it passed
      against a mutation that swapped them — and that swap matters,
      because `master_part_xml` names rId1 as the layout in fixed text, so
      a master would point at its own theme as though it were the layout.
      It now reads out rId1's actual target.
      **Run styling in a master decoration** was the other half, and it
      could not be carried honestly until something drew it. The canvas
      drew a decoration with cairo's toy text API: one weight for the whole
      box, hardcoded to bold — so every unemphasised decoration was drawn
      as though the design asked for bold — and `show_text` has no concept
      of a line break, so a multi-line decoration was silently drawn as one
      line. Filling `runs` without fixing that would have round-tripped
      styling nothing drew, which is the trap the `default_font` note above
      describes. So the decoration now goes through pango and through the
      same `set_styled_text` the slides use, and the call site that passes
      the runs is itself reachable from a test — closing the gap the
      `default_font` work recorded and could not close, by asserting over
      the layout the master path builds rather than the pixels it produces.
      Three real strips turned up on the way, none of which any test in the
      suite could see, and all three were invisible for the same reason:
      **our writer and our reader shared a convention, so a round trip
      cancelled it out.**
        * The pptx reader recorded one entry per `a:t` and joined them with
          `\n`. Runs are the pieces of *one* line and `a:p` is what ends a
          line, so a decoration reading "Plain **Bold**" came back as two
          lines — and, since a master is written back from that text, the
          break was then saved into the file. `text` is now derived from
          the runs rather than accumulated beside them, which turns the
          model's "concatenated run text equals `text`" from a thing each
          walker has to remember into a thing neither can get wrong.
        * Trimming hid a second one underneath it. `a:t` content is
          significant, and the reader trimmed it, so "Plain " + "Bold"
          became "PlainBold" the moment the spurious newline stopped
          separating them. The odp walker had already been reading its text
          untrimmed for this exact reason.
        * The odp writer's *styled* path emitted one `text:p` with a
          literal newline inside, and ODF collapses that to a space.
          Impress read our two-line box back as one line reading "Bold one
          plain two". Our own reader could not see it, because it split
          that single paragraph back on the newline it had itself written.
          The runs-empty path had always split correctly; only the styled
          one did not. The pptx writer had the same defect — a newline
          inside `a:t` rather than a second `a:p` — and there LibreOffice
          happens to read the newline as a break, so it round-tripped
          either way and only the emitted markup was wrong. Both write one
          element per paragraph now.
      Fixing the writers exposed the reader's other half: odp kept a box's
      runs only when it had a single paragraph, "matching the pptx reader's
      behavior" — which is to say both readers lost a styled multi-line
      box's styling, one by discarding it and the other by never reading
      it. Writing the break correctly without that would have traded a lost
      line break for lost styling. Both readers now carry the paragraph
      break *inside* the runs, which is the one convention that keeps the
      invariant and the styling at the same time.
      Three notes on the tests, because each marks a fixture that could not
      have failed:
        * `run_styles_survive_a_snapshot` has covered run styling since the
          beginning and saw none of this, because it uses a **single** run —
          and one run cannot reveal how two are joined. The same shape as
          #725's master-mapping test needing a slide on a master that is
          not the first.
        * The odp style-collision test uses bold on the slide and italic on
          the master deliberately. `content.xml` and `styles.xml` each
          number their automatic styles from 1 and the reader merges both
          into one map by name, so a master's `T1` would quietly redefine a
          slide's; with one style shared between them the collision would
          overwrite a value with itself and pass. Master styles are now
          named per master (`MT1_`, `MT2_`).
        * The odp reader builds a text box at two places, and only one of
          them is reachable from our own writer: `draw:custom-shape` is
          what Impress and PowerPoint's exporters emit for a shape carrying
          text. It kept its own copy of the drop-the-runs rule, and the
          Impress-grounded test did not catch it because Impress writes the
          other shape. That one needs a crafted package, since our writer
          cannot produce one.
      One case is import-only and worth naming: an empty `<a:p/>` between
      two paragraphs is a blank line, and it arrives as a single Empty
      event rather than Start+End. Paragraph closes are counted rather than
      flagged so two consecutive ones cannot collapse into one. Our own
      writer never produces that shape, so this is reachable by imported
      documents alone.
      A trailing empty paragraph is still dropped in pptx and kept in odp,
      which is a real asymmetry and is left alone deliberately: the pptx
      side treats it as noise, and no reading of either format makes one
      obviously right.
      **The audit that paragraph asks for found one straight away, and it
      is worse than anything above.** Both readers converted coordinates
      with a fixed constant — 9525 EMU per model unit in pptx, one point
      per unit in odp — when `960x540` are *model units* (ADR 0004) and a
      coordinate only means something relative to the slide it sits on.
      Those two are the same thing only for a slide of exactly the size our
      own writer emits. PowerPoint and Impress both default a 16:9 deck to
      13.333in x 7.5in (`sldSz cx="12192000" cy="6858000"`), and on one of
      those a full-bleed shape read back as **1280x720 in a 960x540
      space** — a third too large, running off the canvas, on every import
      of a current PowerPoint file. The two writers disagreed with each
      other too: pptx declares a 10in slide and odp a 13.333in page, so a
      deck taken pptx -> Impress -> odp came back at 0.75x and the reverse
      at 1.33x.
      Nothing could see it, and the reason is worth stating exactly,
      because it is a *sharper* version of the round-trip trap:
      `positions_approx_survive_impress_rewrite` and
      `odp_geometry_survives_impress_rewrite` both send a file through real
      LibreOffice, so they look like exactly the foreign-reader check this
      row keeps asking for. They are not. **A same-format rewrite cannot
      detect a wrong unit**: Impress reads whatever we wrote and writes the
      same value back, so our constant cancels out on the way in and out.
      Only crossing formats forces a real conversion — Impress turning EMU
      into centimetres by the true ratio — and no test crossed them. An
      Impress-grounded test is not automatically a test of the thing it
      appears to ground.
      Both readers now normalise against the size the document declares,
      falling back to exactly the old constant when it declares none, so no
      document written before this shifts. Two things the fix had to get
      right that the first attempt did not:
        * A document has several page layouts, and Impress writes the notes
          one as **A4 portrait**. Taking the first `style:page-layout` read
          a landscape slide as 1.21x wider and 0.48x shorter — non-uniformly
          wrong, where the bug being fixed was at least uniformly wrong. The
          layout named by the first `style:master-page` is the slides'.
        * Each axis is normalised against its own extent. Every fixture
          here is 16:9, where `960/cx` and `540/cy` are equal, so a
          mutation using the width factor for both axes passed everything
          until a 4:3 slide was added — the same fixture trap as the
          single-run styling test and the master-mapping test before it. A
          4:3 deck cannot be represented faithfully in a fixed 16:9 model
          space; normalising per axis stretches it but keeps a full-bleed
          shape full-bleed and everything on-slide, where the old
          behaviour pushed content off the bottom.
      The same audit, continued across the remaining fields, found a
      second one: **speaker notes were dropped from every deck that had
      been through Impress.** Impress's pptx exporter writes the notes into
      a shape with *no placeholder at all* — one `p:sp` whose `p:nvPr` is
      empty — and `extract_notes_text` only read text inside a
      `p:ph type="body"`, which is what our own writer emits and therefore
      the only shape any test ever presented it with.
      How it was diagnosed is worth keeping, because the obvious reading
      was wrong and #733 had trained the wrong reflex. A notes loss on
      odp -> Impress -> pptx looks exactly like the font case, where
      Impress's exporter genuinely does not write the part. It was not:
      an odp -> odp rewrite kept the notes, proving Impress reads what we
      write, and its pptx *did* contain `notesSlide1.xml` carrying the
      text. Checking which side actually dropped it is what separated our
      bug from theirs — the same two-step that cleared us in #733 and
      convicted us here.
      The fallback widens only to shapes that declare **no** placeholder.
      A shape declaring some other one — a slide number, a date, a footer —
      is still never notes, because reading a page number as a speaker note
      would be worse than losing the note.
      Four other fields were audited the same way and came back clean in
      all four directions (self and cross-format, both ways): rotation,
      slide background, the slide's master mapping, and that master's own
      background. Run styling's six carried fields were checked in #783's
      wake and are clean too.
      Finishing the sweep found a third: **a slide's name was destroyed by
      every .pptx save.** `Slide::title` is the slide's name —
      `draw:page/@draw:name` in ODF, `p:cSld/@name` in OOXML. The odp
      writer had always written it. The pptx writer wrote the slide's
      `p:cSld` *bare* while setting `name` on the master's and the layout's
      a few lines above, and the pptx reader synthesised
      `format!("Slide {n}")` without ever looking for the real one. So
      slide names survived a .odp save and were destroyed by a .pptx one —
      and pptx is the format an unsaved deck is snapshotted in, which makes
      it this row's business exactly: crash recovery lost every slide name.
      The fixture is why nothing saw it, and it is the fourth time:
      `slide_of` sets `title: String::new()`, so every test in
      `snapshot_fidelity.rs` was asking whether an *empty* name survived —
      a question with the same answer whether the writer carries names or
      not. (The others: the single-run styling test, #725's master mapping,
      and the 16:9-only geometry fixtures.) The positional fallback is what
      made it invisible rather than obvious — the reader returned a
      plausible "Slide 1", so a round trip produced a title that merely
      looked right.
      Two further defects fell out of writing the tests for it, both found
      by the tests rather than by reading:
        * **Names were never unescaped.** `parse_c_sld_name` and the odp
          `attr` helper both returned the raw attribute bytes, so a page
          called `R&D <draft>` came back as `R&amp;D &lt;draft&gt;`. Latent
          for every other attribute either reader touches — style names,
          numbers, colours, where an entity never appears — and live for
          names. Both now normalise, which is what the rest of the
          codebase already did for `type` and rotation attributes.
        * **An unnamed page came back nameless in odp and labelled in
          pptx.** Both writers now omit the attribute rather than writing
          it empty, and both readers fall back to the same positional
          label. The odp fallback is applied where slides are assembled,
          not in `parse_pages`, because that walker also reads master pages
          and a master's title is its name — "Slide 3" is not a master.
      Only the pptx -> odp direction is asserted against Impress, and that
      choice was measured. Coming back the other way its pptx exporter
      writes `<p:cSld>` with no name and the string appears nowhere in
      `slide1.xml`, while an odp -> odp rewrite keeps it: Impress reads our
      `draw:name` correctly and simply does not carry a page name into
      pptx. Its export gap, the same as the master font in #733 — and the
      opposite verdict to the speaker notes above, where the part was
      present and we were the ones not reading it. Which side dropped it is
      worth checking every time; the symptom looks identical.
      That completes the sweep this row asked for. Every field of the model
      has now been checked in all four directions: `Deck.slides` and
      `.masters`, `Slide.title`, `.background`, `.notes`, `.master_idx`,
      every `SlideObject` variant's geometry and rotation, a `Circle`'s
      radius, an `Image`'s bytes and frame (including a frame stretched
      against the image's own aspect, which must not be "corrected"), a
      `TextBox`'s text and runs, `MasterSlide.name`, `.background`,
      `.default_font` and `.shapes`, and `RunStyle`'s six carried fields.
      Three strips were found and fixed; everything else was verified
      rather than assumed.
      **Why this row is now `[x]`, and what that claim does and does not
      cover.** It was held at `[~]` after its two named gaps closed, on the
      grounds that carrying the run styling had turned up three strips —
      a reader splitting one line in two, a reader eating the space between
      two runs, a writer emitting a break ODF collapses — every one of them
      live while the whole suite, including thirty-five Impress-grounded
      oracle tests, was green. What they had in common is that our writer
      and our reader agreed with each other, so a round trip proved
      nothing. Three strips found by probing rather than by a test failing
      is not evidence for "no format silently strips supported content",
      and the condition set for flipping was a pass over *every* field a
      round trip could confirm while both halves share a convention.
      That pass is the one recorded above. It covered every field of the
      model in all four directions, found three more strips — the
      coordinate scale, the speaker notes, the slide name — fixed each, and
      verified the rest rather than assuming it. So the claim now rests on
      measurement instead of on the absence of a failing test, which is
      what the row was waiting for.
      What it does not cover, stated so the `[x]` is not read wider than it
      is: content the model does not represent at all — transitions,
      animations, charts — is an opaque-preservation concern (ADR 0004),
      not "supported content", and nothing here measured it. Two
      asymmetries are known and deliberate: a trailing empty paragraph is
      dropped in pptx and kept in odp, and a 4:3 deck is stretched into the
      16:9 model space. And two losses belong to Impress's exporters rather
      than to us — the master font (#733) and the page name — which no
      change here can fix.
      One limitation, stated because a mutation found it rather than
      because it is comfortable: `document_font_description` and
      `master_for` are covered — five mutations of the resolver, the
      builder, and the index handling all redden — but the *call* inside
      `draw_slide_multi` is not. Replacing its argument with `None` leaves
      the whole decks suite green. Closing that needs an assertion over
      rendered output, and the only headless one available compares pixels
      between two font families, which passes or fails on which fonts the
      runner happens to have installed. Given the afternoon this repository
      just had with an environment-dependent test, that trade is not worth
      making; the call site is one expression, verified by reading.
      Unifying the three hand-rolled master lookups in that function into
      `master_for` shrinks what the untested expression can get wrong: two
      of them used `masters.get(mi)` and the third a hand-written
      `mi < masters.len()`, so one slide's background and its text could
      have disagreed about which master it had.
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
      destination directory after a successful save, and whether one is
      stranded is *asked* rather than guessed. Every live save holds an
      advisory `flock` on its temporary for as long as it is writing, and
      the kernel releases that lock when the process dies however it dies —
      so a temporary nobody can lock is one nobody owns. That is the same
      mechanism `AutosaveSlot::claim` uses to tell a crashed window's
      snapshot from a live one's, which is why it is the mechanism used
      here rather than a second invention.
      The first version of this used an age proxy instead — "older than a
      day" standing in for "nobody owns it" — and it was worse in two ways
      that are worth recording. It left a crash's leftovers for a day, and
      it could only ever be a guess: a proxy for liveness cannot
      distinguish a stranded temporary from one a very slow save is still
      filling. Asking the kernel is both exact and immediate.
      A short race floor survives from that version, doing a different and
      real job: a save creates its temporary and *then* locks it, and in
      the microseconds between, the file is on disk holding no lock, which
      is exactly what a stranded one looks like. A minute is enormous
      beside two syscalls, and it only delays cleaning up after a crash by
      however long the next save takes to arrive.
      Otherwise deliberately narrow, because this deletes files in a
      directory the user owns: only regular files (not symlinks, not
      directories), only names carrying this module's own prefix, only ones
      nothing holds a lock on. Anything it cannot establish it leaves —
      including a lock it could not ask about, which is the opposite bias
      to `has_a_live_owner`'s and for the same reason: failing to prove
      something is safe is not proof that it is, and here the consequence
      being gated is a deletion.
      Verified end to end against a real kill rather than a planted file:
      SIGKILL during a 600 MiB save left the destination intact and one
      stranded temporary; a save immediately afterwards kept it (the race
      floor); a save once it was past the floor removed it.
      Two of these tests only became real under mutation, both in the same
      way — they were passing on something other than what they named:
      - The kind check. Dropping `is_file` changes nothing for a directory,
        because `remove_file` refuses one anyway; it changes what happens
        to a *symlink*, which `remove_file` will happily unlink. So the
        test covers a symlink, and injects the clock rather than backdating
        the entries, because a symlink's own mtime cannot be set through
        `std` and a real-time sweep would skip the link for being fresh.
      - The live-save check. A test that plants a *fresh* unlocked
        temporary and watches it survive is passing on the race floor, not
        on the lock, and would protect nothing a minute later. So the test
        backdates the file past the floor and holds a real lock on it,
        which leaves the lock as the only thing between the sweep and that
        file.
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
