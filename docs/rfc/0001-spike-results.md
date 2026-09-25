# RFC-0001 Phase 1: CRDT spike results

Date: 2026-09-24 · Tracks: [RFC-0001](0001-crdt-collaboration.md), [#544](https://github.com/tuna-os/gtk-office-suite/issues/544) · Code: [`tools/crdt-spike/`](../../tools/crdt-spike/)

**These numbers come from one run on one machine, described below, against
our own generated workloads. They are not a published benchmark.** They say
how the three libraries behaved on our data shapes, with the simplest
reasonable encoding for each, and no tuning. They should not be quoted as the
libraries' general performance.

## Recommendation

**Loro**, for Tables and Decks, with Automerge as the fallback if open
question 3 (a web or mobile client) is ever answered "yes".

The deciding result is Decks. Loro was the only candidate whose merged deck
was well-formed: two peers moved objects between slides, regrouped them and
changed z-order concurrently, and Loro's `MovableTree` came back with no
duplicated objects, no lost objects and no cycles. Automerge and yrs have no
tree type. With a parent-pointer encoding they never duplicate, but concurrent
regrouping formed cycles, and 13 (Automerge) or 14 (yrs) of 1527 nodes dropped
out of the deck. The obvious nested-list encoding, where a move is delete plus
re-insert, duplicated 499 nodes. Both workarounds would need tree-repair code
of our own, and that code is where the correctness risk would sit.

On Tables all three were correct. Each one's final state equalled a plain
replay into `SheetModel`, and each passed the merge oracle with zero
violations. Loro and yrs applied the 20,000-op session in about 60 ms, against
about 1,100 ms for Automerge. For Letters, Loro and Automerge can both express
`RunStyle`'s per-mark expand rules natively. yrs cannot; the application would
have to emulate them.

Choosing Loro costs three things, and none of them is a blocker:

1. **Size.** Loro's snapshot of the Tables session is 440 KiB, against
   156 KiB for Automerge's full history. Even Loro's state-only export
   (223 KiB) is larger than Automerge's full history. This matters for the
   sidecar (option C), and not for session-scoped B.
2. **Delete against move.** When one peer deletes an object and another moves
   it at the same time, Loro keeps the move: the deleted object comes back.
   In the concurrent deck run, 32 of 183 deletes were undone this way, and all
   5 of the deliberate cases. That is defensible, but it is a product decision
   and has to be made on purpose, with a journey that shows it.
3. **Build surface.** Loro has the largest dependency tree (115 unique crates)
   and the slowest clean build (198 s) of the three. See [Build cost](#build-cost).

## Method

`tools/crdt-spike/` is a standalone Cargo project. It is not a workspace
member: it is `exclude`d in the root `Cargo.toml` and has its own
`[workspace]` and `Cargo.lock`, seeded from the root lockfile. None of the
candidates reaches a product crate. Each candidate sits behind its own feature
(`automerge`, `loro`, `yrs`). The spike reads the model through a path
dependency on `tables-core`.

- **Versions:** automerge 0.12.0, loro 1.16.2 and yrs **0.26** (not 0.28;
  see [Build cost](#build-cost)). Rust 1.94.1, release profile.
- **Workloads** come from a seeded SplitMix64, so every run is identical.
  They are generated in memory rather than recorded from the app; the stress
  harness the RFC mentions does not emit a replayable op log. Each run is a
  separate process, so peak RSS (`VmHWM`) is per run. Timings are the median
  of three runs.
- **One user action is one transaction/commit** in every library. A styled
  range is one action; each keystroke-sized edit is its own action.

### Tables encoding

The RFC's "map keyed by `(sheet, row, col)`" needs one change to survive row
inserts (see [What contradicts the RFC](#what-contradicts-the-rfc)):

- `rows`: a sequence CRDT of **stable row ids** (`List`/`Array`);
- `cells`: one flat map with a last-writer-wins register per **field**, keyed
  `"<row id>.<col>.<field>"`. The fields are `v` (value; a formula is text
  starting with `=`), `nf` (number format) and the style map `b`, `i`, `u`,
  `fill`, `color` and `ha`. `CellStyle` (`tables-core/src/style.rs`) is on the
  `tables-cell-style` branch, not on main, so style is modelled as that small
  per-cell map, mirroring `CellStyle`'s fields.
- Deleting a row removes its id and the cell keys this peer knows about.

The session starts from 1000 empty rows × 26 columns. It then runs 3,427
actions (20,002 field-level ops): runs of typed values, a fifth of them
formulas; range styling; number formats down a column; overwrites and clears;
and inserts and deletes of 1–3 rows. The session ends with 1098 rows and
13,432 cell fields. In the concurrent variant, two peers each start from the
same base and run about 10,000 ops; then each pulls the other's delta.

**Correctness checks.** The sequential result must equal a replay of the same
ops into tables-core's real `SheetModel`: `insert_rows`/`delete_rows`, `data`,
`formulas` and `formats`, with the style map held in a parallel grid. The
concurrent result must pass a library-neutral merge oracle:

- the row set is base ∪ inserts − deletes, with no duplicates;
- each peer's own row order is preserved;
- every one-sided write survives;
- every conflicting write resolves to one of the two sides.

Each check is also run on the document re-loaded from its encoding.

### Decks encoding

The base deck has 50 slides with 30 nodes each: 12 top-level objects, plus
two groups that each hold 3 objects and a nested group of 4. That makes 1550
nodes. `decks-core`'s `Slide::objects` is a flat `Vec` today; the groups are
there because grouping is where a tree CRDT earns its keep. Each object has
`kind`, `x`, `y`, `w`, `h` and `rot`, and text objects also have `text`.

- **Loro:** a `LoroTree` with fractional-index sibling order (`mov_to`), and
  fields in each node's metadata map.
- **Automerge and yrs, parent pointers:** one flat map of registers per node:
  `<id>.p` (parent), `<id>.z` (fractional z position), `<id>.d` (tombstone)
  and `<id>.<field>`.
- **Automerge, naive:** nested lists of child maps, where a move is delete
  plus an inserted copy. This is the encoding a JSON-shaped document invites.

In the sequential run, one peer performs 3,000 actions: move, resize, rotate,
edit text, z-order, move to another slide, regroup, add, delete and reorder
slides. The result must equal a reference tree.

In the concurrent run, each peer first performs deliberately conflicting
actions from the same base, then 1,500 random actions. The conflicting
actions are:

- 20 × the same object moved to two different slides;
- 5 × group A into B while group B goes into A, the cycle case;
- 5 × delete on one peer, move on the other;
- 10 × bring-to-front against send-to-back.

A merged deck is **well-formed** when every node that should exist (created,
and not explicitly deleted by either peer) appears exactly once, reachable
from the root, with no cycles and no dangling parent.

### Letters

These are qualitative probes (`crdt-spike probes`), with the results listed
under [Letters marks](#letters-marks).

## Machine

AWS VM, Intel Xeon Platinum 8259CL @ 2.50 GHz, **2 vCPUs**, 7.6 GiB RAM,
Linux 7.0.0-1013-aws. Everything ran inside the project's `render-lab`
container (rustc 1.94.1). The host was shared with other workloads. The load
average was 3.4 at the start of the measurement run and 1.6 at the end. It
reached 15–25 during the earlier builds, which is why the build times below
are indicative only.

## Results

"—" means that library has no such encoding or the measure does not apply.
Sizes are in KiB and times in milliseconds. "Load" is decode only; "load +
read" adds materialising the whole state, because Loro loads lazily.

### Tables, one peer, 20,002 ops

| | Automerge | Loro | yrs | reference |
|---|---|---|---|---|
| apply whole session (ms) | 1081 | 63 | 65 | 75 (`SheetModel` replay) |
| state equals `SheetModel` replay | yes | yes | yes | — |
| full history (KiB) | 156 (`save`); 372 uncompressed | 440 (snapshot); 385 (updates only) | — (GC'd; no history) | — |
| state only (KiB) | — (no such export) | 223 (shallow) | 446 (update v1); 393 (v2) | 206 raw keys+values; 68 xlsx without styles |
| load (ms) | 54 | 2 | 46 | — |
| load + read (ms) | 63 | 28 | 63 | — |
| peak RSS (MiB) | 26 | 37 | 31 | 21 |

### Tables, two peers × about 10,000 ops, then merge

| | Automerge | Loro | yrs |
|---|---|---|---|
| apply, both peers (ms) | 1017 | 47 | 30 |
| merge A←B / B←A (ms) | 167 / 168 | 67 / 64 | 21 / 20 |
| delta exchanged each way (KiB) | 346 | 183 | 226 |
| peers converge | yes | yes | yes |
| merge oracle violations | 0 | 0 | 0 |
| conflicting cell fields: A won / B won | 250 / 392 | 250 / 392 | 0 / 642 |
| merged document (KiB) | 156 (`save`) | 433 (snapshot); 433 (state only) | 456 (v1); 406 (v2) |
| load merged (ms) | 55 | 3 | 33 |
| peak RSS (MiB) | 30 | 47 | 38 |

All three agree on the merged row set (1141 rows), and 2,472 edits landed on
rows the other peer had deleted. Loro's state-only export did not shrink
after the concurrent merge (433 against 433 KiB). We did not investigate why.

### Decks, one peer, 3,000 actions on 1550 nodes

| | Automerge (parent ptr) | Automerge (naive lists) | Loro (tree) | yrs (parent ptr) |
|---|---|---|---|---|
| apply, incl. building the base deck (ms) | 650 | 1137 | 40 | 29 |
| equals reference tree | yes | yes | yes | yes |
| encoded (KiB) | 105 (`save`) | 145 (`save`) | 243 snapshot; 126 state only | 317 (v1); 262 (v2) |
| load (ms) | 47 | 111 | 17 | 30 |
| peak RSS (MiB) | 22 | 25 | 24 | 24 |

### Decks, two peers × 1,540 actions, then merge

| | Automerge (parent ptr) | Automerge (naive lists) | Loro (tree) | yrs (parent ptr) |
|---|---|---|---|---|
| apply, both peers (ms) | 290 | 731 | 27 | 14 |
| merge A←B / B←A (ms) | 97 / 109 | 192 / 180 | 28 / 25 | 8 / 7 |
| delta each way (KiB) | 200 | 382 | 37 | 44 |
| peers converge | yes | yes | yes | yes |
| **well-formed** | **no** | **no** | **yes** | **no** |
| duplicated nodes | 0 | 499 | 0 | 0 |
| lost nodes (expected 1527 alive) | 13, all stuck in cycles | 108 | 0 | 14, all stuck in cycles |
| cycle pairs that swallowed a group (of 5) | 2 | 1 | 0 | 2 |
| deleted nodes revived by a concurrent move | 0 of 183 | 108 of 183 | 32 of 183 | 0 of 183 |
| delete against concurrent move: move wins (of 5) | 0 | 5 | 5 | 0 |
| same object moved to two slides: A's / B's / both / elsewhere | 0 / 13 / 1 / 6 | 5 / 4 / 8 / 3 | 3 / 10 / 1 / 6 | 0 / 15 / 1 / 4 |
| encoded (KiB) | 109 | 158 | 249 snapshot; 240 state only | 316 (v1); 263 (v2) |
| load (ms) | 43 | 128 | 18 | 27 |
| peak RSS (MiB) | 28 | 40 | 39 | 35 |

In the "same object moved to two slides" row, one of the 20 pairs picked the
same target slide on both peers. That pair shows as "both" in every column; it
is not a duplicate. "Elsewhere" means a later random move of the same object.
Only the naive encoding's other "both" cases are real duplicates.

## Correctness findings

- **Tables: all three are correct on our data.** Each library's final state
  was byte-identical to the `SheetModel` replay (digest `6cae77c56a662e55` for
  all four), before and after re-loading. Concurrently, every library passed
  the merge oracle with zero violations: no row lost or duplicated, each
  peer's row order kept, no one-sided write lost, and each conflict resolved
  to one side.
- **The libraries do not agree with each other on concurrent results.** Each
  converged with its own peer, but the three merged states differ (three
  digests). That is expected: conflict winners and the order of concurrently
  inserted rows are library-defined. Documents cannot be moved between
  libraries mid-session.
- **yrs resolves conflicting map writes by client id, not by time.** Peer B
  won all 642 conflicts. Automerge and Loro, which both order by Lamport
  timestamp and then by actor, picked the same winner in all 642. For a
  spreadsheet, "last writer wins" in yrs actually means "higher client id
  wins".
- **Concurrently created nested containers lose writes in all three.** Two
  peers each create the map for cell `r1.0` and write different fields into
  it. After the merge, one peer's field is gone in Automerge and yrs, and in
  Loro with `insert_container`. Loro's `ensure_mergeable_map` keeps both. The
  flat per-field keys used here avoid the problem in every library, and any
  per-cell or per-object nested map must avoid it too.
- **Deleting a row discards concurrent edits to it.** 2,472 cell writes landed
  on rows the other peer deleted. They are dropped silently. A journey should
  decide whether that needs to be surfaced.

## Decks tree findings

- **Loro:** a well-formed tree every time. Cycles are prevented; when two
  moves would form one, one move loses. Nothing was duplicated or lost, and
  z-order converged. The semantic to decide on is that **a concurrent move
  beats a delete**: 32 of 183 deleted objects came back, including all 5 of
  the deliberate cases.
- **Automerge and yrs, parent pointers:** no duplicates, because a move is one
  register write, and deletes win, because the tombstone is a separate
  register. But **concurrent regrouping forms cycles**: 2 of the 5 deliberate
  cycle pairs still formed a loop after the other random edits, and 13 or 14
  nodes became unreachable. The application would need deterministic cycle
  repair on every read, plus fractional-index maintenance (we used `f64`
  midpoints, which run out of precision after about 50 inserts at the same
  place).
- **Automerge, naive nested lists:** unusable. It produced 499 duplicated
  nodes, and 108 deleted objects came back as copies. Every move is a copy of
  a subtree, so history and delta size also doubled.
- yrs `Array` can move items only within one array, so it does not help with
  moving an object to another slide.

## Letters marks

`letters-core`'s `RunStyle` has `bold`, `italic` and similar marks that should
extend when you type at their end, and `link` (and arguably `code`), which
should not. Each probe marks "bold link plain", types at
the end of each mark, and then tests one concurrent case.

| | Automerge | Loro | yrs |
|---|---|---|---|
| per-mark expand configuration | yes, per call (`ExpandMark::After/None/Both/Before`) | yes, per key for the document (`config_text_style`, `ExpandType`) | **no**; inserted text inherits the formatting on its left |
| typed at end of bold → bold | yes | yes | yes |
| typed at end of link → not linked | yes | yes | **no** (linked) |
| typed before bold → not bold | yes | yes | yes |
| emulate with explicit attributes | — | — | yes: `insert_with_attributes(link: null)` |
| mark values (link URL, font size) | yes (scalar) | yes (any value) | yes (any value) |
| concurrent: A bolds a word while B types at its end | Z not bold | Z not bold | Z not bold |

Automerge and Loro can express `RunStyle` directly. yrs can too, but only if
every insert in the app computes attributes explicitly, which is the kind of
rule that eventually gets missed. From the API surface only, not probed:
inline images and footnote markers need an embed. yrs has
`Text::insert_embed`. Automerge and Loro would need a placeholder character
carrying a mark. None of this can be exercised until Phase 0 gives Letters a
live model.

## Build cost

Each figure is a clean `cargo build --release -p <candidate>` in an empty
target directory, from the spike's lockfile, on the contended 2-core host.
Treat the times as ±30 %.

| | Automerge 0.12.0 | Loro 1.16.2 | yrs 0.26 |
|---|---|---|---|
| clean release build of the crate and its deps (s) | 103 | 198 | 45 |
| `cargo tree -p <c> -e normal \| wc -l` | 74 | 238 | 55 |
| unique crates, normal + build | 49 | 115 | 35 |
| added to the stripped spike binary (MiB), over 4.9 MiB without a candidate | +2.5 | +4.0 | +0.9 |
| lines added to the spike's `cargo tree -e normal` (1058 without a candidate) | +35 | +192 | +29 |

Every candidate builds on its own behind its feature
(`--no-default-features --features <c>`).

**yrs 0.28.0 (latest) and 0.27.x do not compile on our toolchain.** They use
`if let` guards, which are stable only from Rust 1.95, and they declare no
`rust-version`, so Cargo resolves them anyway and the build fails. The spike
pins yrs 0.26. For constraint 7 (`--locked`, reproducible Flatpak builds),
this is the kind of silent MSRV break the release gate exists to catch.

## What contradicts the RFC

1. **"Map keyed by `(sheet, row, col)`" does not survive row insert or
   delete.** A positional key re-addresses every cell below a concurrent
   insert. Rows need stable ids in a sequence CRDT, with cells keyed by row
   id. Columns will need the same treatment once column insert and delete are
   collaborative.
2. **"Last-writer-wins per cell" should be per field.** Otherwise a
   concurrent "bold this" and "type a value" in the same cell lose one of the
   two. With per-field registers, only edits to the same field of the same
   cell conflict (642 in our run); per-cell registers would also have
   discarded every concurrent edit to different fields of one cell.
3. **"The published encoding claims [for Loro] are favourable."** On our
   Tables data, Loro's snapshot is 2.8× the size of Automerge's full history,
   and its state-only export is still larger than Automerge's full history.
   Automerge had the smallest encoding in every scenario. Loro wins on speed
   and on the tree, not on size.
4. **"Last writer" is not what yrs implements** for concurrent map writes;
   the client id decides.
5. **The RFC credits only Automerge with per-mark expand semantics.** Loro
   has them too, configured per key (`config_text_style`).
6. The RFC is right that the movable tree is the differentiator. It is the
   only measurement here that separates the candidates on correctness rather
   than speed.

Also relevant to constraint 4 (per-user undo), though not measured: Loro
(`UndoManager`) and yrs (`UndoManager` with tracked origins) ship local,
per-peer undo. We found none in Automerge 0.12's Rust API.

## Reproducing

```bash
podman run --rm -v $PWD:/workspace:Z -v $HOME/.cache/render-lab-cargo:/cargo-home:Z \
  -e CARGO_HOME=/cargo-home -e PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin \
  -w /workspace/tools/crdt-spike render-lab bash -c '
    export CARGO_TARGET_DIR=/workspace/tools/crdt-spike/target
    cargo build --release --locked
    target/release/crdt-spike probes   # Letters marks + nested-map probes
    target/release/crdt-spike all      # every backend x scenario, median of 3
    ./build-cost.sh'
```

`crdt-spike run <backend> <scenario>` runs one cell. The backends are
`reference`, `automerge`, `automerge-naive`, `loro` and `yrs`. The scenarios
are `tables-seq`, `tables-conc`, `decks-seq` and `decks-conc`.
