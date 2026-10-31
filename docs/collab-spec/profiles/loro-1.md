# DCP update profile `loro/1` with document models `tables/1` and `letters/1`

Status: **draft 1**. This is the algorithm profile that [DCP draft 1, section 7](../../DOCUMENT-COLLABORATION-SPEC.md#7-update-profile-l2)
says "none exists yet" for. It fixes what section 7 leaves to a profile: the envelope, the
meaning of `base_version`, the genesis, determinism, and what a receiver does when an update
fails its dry run. It was written for the Tables workbook and has a second document model,
`letters/1`; a different document model under the same algorithm (`decks/1`) differs only in
section 8 and in its own `doc_model` string. Where this text says "workbook", "cell" or "row" for
an example, a `letters/1` document has its paragraphs and characters.

The key words MUST, MUST NOT, SHOULD and MAY are used as in RFC 2119.

## 1. Identifiers

| Field | Value |
|---|---|
| `algorithm` | `loro/1`: Loro (the `loro` crate, major version 1) as the CRDT, its binary export formats as the payload |
| `doc_model` | `tables/1`: a workbook, or `letters/1`: a word-processor document, each mapped onto Loro containers as in section 8 |
| `meta.algorithms` | MUST contain `loro/1` when `meta.levels` contains `L2` |

A receiver that does not carry both strings applies nothing (spec 7 rule 1) and uses versions.

## 2. The update envelope

An update is a DCP update object ([`update.schema.json`](../schemas/update.schema.json)) with:

| Field | Value |
|---|---|
| `algorithm`, `doc_model` | `loro/1`, and `tables/1` or `letters/1` (section 8) |
| `seq` | 0, 1, 2, ... per `(principal, device)` stream (spec 4.4). A writer MUST take a number for every update it means to send and MUST send it, in order; a number it takes and does not send is a lost update (spec 7 rule 2) that stops every reader's view of that stream. |
| `base_version` | The **session base**, section 3. Never the head. |
| `payload` | Loro bytes (section 5). Inline `base64` when at most 34,000 bytes, otherwise `file` (spec 7 rule 5). The genesis is always `file` (section 4). |

## 3. The base version (the ruling)

DCP section 4.4 says `base_version` is "the version the update applies to", and the reference
receiver (`dcp_core::updates::Receiver`) applies an update only if its `base_version` is the
version the receiver loaded. For an algorithm whose updates are *instructions against a version*
that is right. For a CRDT it is not: a Loro update applies to every replica that shares the
genesis of the session, whichever versions other people have saved since, so a receiver that
re-based on every new head would silently drop the valid edit of a person who wrote before they
saw it.

For `loro/1`, `base_version` therefore names **the version the session's replica started from**:

1. The **genesis version**: the root version (`parents: []`) published when the document was
   shared (section 4); or
2. after a *restart* (rule 5), the version the replica restarted from.

and these rules hold:

1. A writer MUST set `base_version` of every update it publishes in a session to the session
   base, and to nothing else. It MUST NOT set it to the current head, to the version it last
   saved, or to the newest version it has seen.
2. A receiver MUST accept an update whose `base_version` equals the session base it loaded,
   whatever versions have been published since and whatever the head is now. A new version,
   from anybody, MUST NOT change the session base and MUST NOT cause an update to be skipped.
3. A receiver MUST skip an update whose `base_version` is any other value, and the skipped update
   still counts toward `seq` (spec 7 rule 2): it is not a gap. (It belongs to a session this
   replica is not part of.)
4. A save is a version like any other (spec 4.2). A writer's saves are children of the version
   it started from or last saved (version rule 4), independently of the session base.
5. The **only** way a session's base changes is a restart: a program loads a version whose
   `covers.algorithm` is `loro/1` (spec 7 rule 4), which holds everything the updates up to
   `covers.through` held, and starts a replica from it. A receiver restarts when its program
   does; from then on the session base is that version and only updates based on it count.
   The reference implementation is `dcp_core::updates::CrdtSession::restart_from`, which
   refuses a version that does not cover `loro/1`. No program restarts a session yet (RFC-0005
   2.3 point 7: compaction is out of scope), so in practice the session base is the genesis
   version for the life of the document. The versions a live session writes with `covers`
   (section 8.3) are claims about a prefix of the log, not restart points: a program MUST NOT
   restart from one unless every writer of the session does too.

The reference implementation is `dcp_core::updates::CrdtSession`: its writer has no way to stamp
anything but the session base, and its receiver is a `Receiver` that is never `reload`ed on a
head change. The test
`a_head_that_moved_does_not_drop_a_valid_update` in that module is the rule, and
`a_receiver_that_follows_the_head_drops_valid_updates` is the failure it prevents.

## 4. The genesis

Two replicas that each open the same file and build a Loro document from it do **not** share a
genesis: the identities of the rows, columns and sheets are made per peer, so merging the two
would hold both sets of lines. A session therefore has exactly one document that all replicas
start from, and it is published, not rebuilt:

1. The sharer publishes the saved file as the **root version** (`parents: []`, spec version rule 7).
2. The sharer publishes **update `seq 0`** of its stream with `base_version` = that root
   version's identifier and a `payload` of `encoding: "file"` whose file holds a Loro export of
   the sharer's replica (a snapshot, or all updates: any export that an empty Loro document
   imports to give the sharer's state). It is a `file` payload **whatever its size**: a genesis
   is a snapshot of the whole workbook, about four to five times the size of the `.xlsx`
   (RFC-0005 3.7), so it is over the inline limit for any real workbook, and one rule is
   simpler than a size test that every receiver would have to agree on.
3. Everyone else *joins*: it reads the root version and the genesis, creates a replica by
   importing the genesis, **replaces its workbook with the one rebuilt from the replica**, and
   from then on is an ordinary receiver. Its first rebuild SHOULD be checked against the root
   version's file (the cells and formulas of the file equal the rebuilt workbook), and the join
   fails visibly when they differ.

A receiver identifies the genesis as the first update, in log order, that has `seq` 0, a `file`
payload, and a `base_version` naming a root version that was created by the same
`(principal, device)` that wrote the update. That makes the genesis the sharer's, whoever else
publishes a `seq 0` later. The session base is that update's `base_version`.

A genesis is applied before any other update of the session; a receiver that is handed other
updates first MUST hold them until it has the genesis. (Loro keeps an update whose causal
dependencies it lacks pending, so holding is for the replica to *exist*, not for correctness.)
Re-importing a genesis into a replica that already has it is a no-op, so a program that restarts
its connection MAY be handed it again.

## 5. The payload

A payload is bytes produced by Loro's export of **the operations since a version vector**, as
they were at the end of one user action (one undo step in the application), or a snapshot for the
genesis. Properties a receiver may rely on, all of which are Loro's:

* Importing is **idempotent**: an update seen twice changes nothing.
* Importing is **commutative** across streams: the result does not depend on the order in which
  the updates of different `(principal, device)` streams arrive, provided each stream is
  applied in `seq` order (spec 7 rule 2).
* An update whose dependencies have not arrived is kept pending and applies when they do.

A writer SHOULD export from the version vector of what it last had **accepted by the binding**
(the event that answers a publish), not of what it last tried to send, so that a publish the
binding refused is re-sent as part of the next delta; one the sidecar has queued is not refused
(it keeps every update it accepted from the shell, in order, and sends it when the server is
back) and needs no re-send. A delta of one cell is about 100 bytes (RFC-0005 3.7).

## 6. Order and gaps

Spec 7 rule 2 applies unchanged. A gap in `seq` of one `(principal, device)` stream means a
lost update: the receiver stops applying that stream, tells the user that what it shows may be
incomplete, and recovers from the latest version. How a program recovers is its own (it shows the
state it has and says it may be incomplete; it does not restart the session from a version of
section 8.3, which is not a restart point, rule 5 of section 3). A dropped update (section 7)
counts toward `seq`, so a drop is not a gap.

## 7. Determinism

Spec 7 rule 6 binds this profile in full: two programs that apply the same updates in different
orders MUST compute byte-identical stored values. For `loro/1` that has two parts, and a program
that claims the profile MUST test both.

1. **The documents are equal.** Two replicas that have imported the same set of updates have
   the same state. This is Loro's guarantee, and the test is a replay: merging the replicas,
   and importing every update of both into a fresh one, give the same workbook
   (`tables-core` `collab_tests`: `two_peers_editing_at_once_converge_and_equal_a_replay`).
2. **The values computed from equal documents are equal.** Only *inputs* are replicated, never
   computed values (section 8), so every program computes the values itself, and the same inputs
   MUST give the same bits, text and errors on every peer. The test is
   `tables-core/tests/determinism.rs`. Functions whose result differs between two evaluations
   (`RAND`, `NOW`, `TODAY`) are outside the guarantee until a program pins them.

Two edits to **one cell** at once are two writes of one register, and Loro picks one by its own
rule (the later by Lamport timestamp, then by peer identifier), the same on every replica; that
is deterministic, and the other write is lost, as in any last-writer-wins register. Which of the
two survives is not for a program to choose.

## 8. The document models

### 8.1 `tables/1`

The mapping of a workbook onto Loro containers is the one documented at the top of
`tables-core/src/controller/collab.rs` (the `Replica` of the `collab` feature): sheets as a
movable list with delete-wins tombstones, rows and columns as lists of stable line identifiers,
cells as registers keyed `<row id>|<col id>|<field>`, sizes and names as maps. Only inputs are
replicated. A change to that layout that an older program would misread is a new document model,
`tables/2`, not a change of `tables/1`; and a program that reads a `tables/1` container it does
not know MUST ignore it, not fail.

### 8.2 `letters/1`

The mapping of a word-processor document onto Loro is the one documented at the top of
`letters-core/src/collab.rs` (the `Replica` of the `collab` feature):

* One Loro `Text`, `body`, holds exactly the edit sequence: the text of the paragraphs, one
  `OBJECT` character for every inline object (image, text box, smart chip) and one `\n` between
  two paragraphs. An offset is an offset into it, counted in Unicode scalar values.
* Character formatting is Peritext marks, one Loro mark key per character attribute (bold, italic,
  size, colour, link, ...), each with the expand rule the attribute has (bold, italic, size and
  colour continue when text is typed at their end; a link, inline code, raw HTML and a tracked
  change do not).
* An inline object is its `OBJECT` character with an `obj` mark holding the whole run.
* A paragraph's style is a `p` mark on the `\n` that starts the paragraph; the first paragraph's
  is `first` in the `doc` map. Deleting a break joins two paragraphs, and the joined one keeps the
  first one's style.
* The page (size, margins) is `page` in the `doc` map, one last-writer-wins value.

What the encoding does not replicate is **refused**, never silently left out, by the program that
shares a document and by the dry run of section 9 (an update that would put it into a replica
fails it): tables, footnotes, headers and footers, sections and comments, and character
formatting that has no mark. A document that holds any of them is not shared as `letters/1`; a
change to the layout that an older program would misread is a new document model, `letters/2`.
The genesis (section 4) is a Loro snapshot of the sharer's replica, which the sharer built from
the document it saved as the root version; section 4 step 3 applies with the paragraphs of the
document in place of the cells of a workbook.

### 8.3 `covers` for `loro/1`

A version's `covers` (spec 4.2, 7 rule 4) is `{algorithm: "loro/1", through: <id of an update>}`.
For `loro/1` it is a claim by the writer and means exactly this:

> The file of this version holds every update of `loro/1` up to and including `through`, in the
> order the binding lists the update log (`DocumentStore::updates`).

An update that every receiver drops or skips (section 9, section 3 rule 3) is part of no replica;
it holds nothing for the claim to name, so the claim neither includes nor needs it.

**What a writer may claim.** A live session (the `collabd` sidecar, for a program that sent
`publish_update`) knows which updates its program's replica holds: those it published itself,
those the program answered `applied`, and those it will never have (the program answered
`rejected`, or the receiver dropped or skipped them). The **applied prefix** is the longest prefix
of the log, in the order the binding lists it, in which every update is one of those. A version
saved from the live replica (the `publish_version` command, with or without `resolve`) carries
`covers` naming the last update of the applied prefix, and none when the prefix is empty. That is
true by construction: the replica the file was written from holds the whole prefix. It is not
the end of the log: an update that has arrived and that the program has not applied yet is not
named, and neither is anything after the first such update, even when the program has applied
later ones. The program MUST send the `publish_version` of a save in the turn it serialises the
file, with no `publish_update` or `applied` between the two that the file does not contain.
The order of the log is a stable append in both stores `collabd` carries: the in-memory reference
store lists updates in publish order; Durable Streams lists them epoch by epoch and, within an
epoch, in the order the server appended them to its `upd` stream. A binding whose order can
change (Matrix) MUST NOT be used for this claim before it documents its own.

A version written with `covers` has no resume hint (the Durable Streams `hint`): a reader that
started after the prefix would skip updates the file does not hold.

**What a reader may conclude.** A sidecar tells the program, for every head, `mine` (this
session published it) and `covered` (it is `mine`, or the update its `covers.through` names is in
the applied prefix). A head that is `covered` holds nothing that the program's replica does not
hold: the replica was merged with it already, by the updates, and what the file adds to those is
only what the saving program made of them. Therefore:

1. Two heads that are both `covered` are **not a conflict**: nothing is lost by showing the
   document the replica has, and the program SHOULD NOT ask the user. The `heads` event's
   `conflict` is true only when there are two heads and at least one is not `covered`: a head
   whose writer had no live session (no `covers`), or whose `through` this replica has not
   applied (it may hold edits this replica lacks). That is a real second version of the document
   (spec 6, 4.2 rule 5) and is shown.
2. An ordinary save (no `resolve`) is a child of the version the program started from **and of
   every head that is `covered`**, minus any of them that another descends from (the antichain,
   `dcp_core::graph::antichain`). That is what stops two live editors that save a moment apart
   from showing each other a spurious second head: each one's save already has the other's as a
   parent once it has seen it. A head that is not `covered` is never made a parent by a plain save:
   the user has not seen what it holds. A save with `resolve` has exactly the parents the user
   chose (spec 6.3).
3. `covered` is a statement about this replica **now**: it changes when updates arrive or the
   program answers `applied`, and the sidecar sends `heads` again whenever the heads, any head's
   `covered` or `conflict` change.

**What a reader must not conclude.** A version with `covers` is not a restart point (section 3
rule 5): it holds a prefix, other writers go on with the session base and a replica started from
it would skip their later updates. The claim also says nothing about the order or the authors
of what it holds, and nothing about versions: a head that is `covered` can still be a different
text than the replica's (two saves of one replica; the user may have edited since).

## 9. Validation: the dry run, and what a receiver does when it fails

Spec 7 rule 3: a receiver MUST validate the result of applying an update before changing its live
state. For `loro/1` the program that owns the replica does it, because only it has Loro:

1. Import the bytes into a **fork** of the replica. A failure to import, or a fork that no longer
   rebuilds into a workbook, fails the dry run.
2. For a genesis, rebuild the whole workbook from the fork (about 400 ms at 100,000 cells), and
   check it as in section 4. For other updates it is enough that what the update touches still
   decodes; a full rebuild of every update is not required.
3. Only then import into the live replica.

When the dry run fails the receiver:

* MUST NOT import the update into the live replica, and leaves the live replica as it was;
* MUST drop the update and report it (spec 7 rule 3): to its user ("an update from <name> could
  not be applied"), and to the sidecar or binding, which reports it with the other drops;
* MUST keep applying the later updates of the same stream: a dropped update counts toward
  `seq` (section 6), so the stream is not stopped;
* SHOULD say that what it shows may be incomplete when later updates of the stream depend on
  the dropped one (Loro keeps those pending), and SHOULD offer to reload from the latest
  version; and
* MUST NOT retry the same update: the bytes will not change.

In the `collabd` sidecar the shell answers every `update` event with `applied` or `rejected`; a
`rejected` becomes a `dropped` event (`kind: update`), and an update the sidecar cannot read
(not base64, a size that does not match, a payload file that does not match its hash, or a record
its binding refuses: wrong signature, will not decrypt, an author whose role may not write) is
dropped by the sidecar before the shell sees it. The sidecar does not parse Loro bytes, so its
own dry run (`Target::dry_run`) is a pass-through and the real one is the shell's.

Both answers (and a drop by the sidecar) end the update's wait: an update the program rejected,
or the receiver dropped or skipped, will never be in the replica and so counts as applied for the
applied prefix of section 8.3; without that, one bad update would keep every later version from
covering anything.

## 10. Roles

A principal with a role below `editor` MUST NOT publish updates, and a receiver MUST drop an
update published by a principal that was not an editor in the epoch or roster it was written in
(spec 5, spec 11). A viewer's program therefore opens the document read-only.

*Does the Tables controller need a read-only mode for that?* Yes, and on `main` it had none
(`grep read_only` over `tables-core/src/controller` found only the sheet-protection checks).
Every content edit of `WorkbookController` (`edit_cell`, `paste_fragment`, `fill`,
`mutate_sheet`, `set_note`, resize, sort) is turned into `Op`s and ends in `apply_ops`, which asks
`authorize_ops` first, and every undo and redo goes through `step`, so one flag covers all of
them. RFC-0005's core pull request adds it: `WorkbookController::set_read_only(true)` refuses
those, counts no protection rejection (the "protected" toast would say the wrong thing), and makes
`can_undo` and `can_redo` false, while trusted replay (`ops::apply`, how a peer's update is applied)
is not a user edit and still works. The shell sets it from the `joined` event's `role`
(`viewer`, and `commenter` until a comment model exists). The sidecar does not rely on it: it
refuses `publish_update` and `publish_version` from a role below `editor` itself, and the binding
refuses them again.

## 11. Conformance

A program claims `loro/1` for a document model of section 8 when it passes (the tests named for
one program are that program's; the rest are the profile's):

| Test | Where |
|---|---|
| The base-version rule: a head that moved does not drop a valid update; an update on another base is skipped and counts toward `seq`; only a covering version restarts a session | `collab/dcp-core` `updates::tests` |
| Convergence and replay equality; deltas equal a replay; a joiner from a snapshot equals a fork | `tables-core` `collab_tests` |
| Determinism of the computed values | `tables-core/tests/determinism.rs` |
| Two processes and two replicas converge through a real Durable Streams server: concurrent edits to different cells and to one cell, a one-cell delta is small, an offline gap catches up, a tampered record is dropped and reported, a viewer cannot publish | `collab/headless` `two_instances` |
| `covers` (section 8.3): a version names the end of the applied prefix, not an update the shell has not applied; a rejected or dropped update does not stall the prefix; two live editors that save after exchanging updates are not in conflict, a writer with no session next to a covered head is; an ordinary save is a child of the version it started from and of every covered head | `collab/collabd` `tests/daemon.rs`; `collab/dcp-core` `graph::tests` and `publish::tests`; `collab/dcp-streams` `a_version_through_a_prefix_claims_the_prefix_and_writes_no_resume_hint` |

## Changelog

* Draft 1: the profile, with the ruling of RFC-0005 open question 1 (section 3).
* Draft 1, `letters/1` and `covers`: the document model `letters/1` (section 8.2) and what `covers` means for
  `loro/1` (section 8.3): a claim about the prefix of the update log the writer's replica has
  applied, written by every live save, which the readers use to tell a second head they already
  hold from a conflict. Section 3 rule 5 and section 6 say that such a version is not a restart point.
