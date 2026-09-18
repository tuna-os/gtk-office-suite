# Architecture Strategy: Offline-First CRDT Document Collaboration

## Vision & Scope

GTK Office Suite (Letters, Tables, Decks) is designed with a strict architectural boundary between pure-Rust domain logic (`suite-common-core`) and GTK4 presentation logic (`window.rs`, UI widgets). To enable modern multi-device sync, local-first document history, and real-time co-authoring without vendor lock-in, document state transitions in `suite-common-core` are planned for incremental adoption of Conflict-free Replicated Data Types (CRDTs).

Related issue: [#823](https://github.com/tuna-os/gtk-office-suite/issues/823)

---

## Architectural Principles

1. **GTK-Free Engine State**:
   All CRDT structures, sequence logs, and operation merging logic reside strictly inside `suite-common-core`. No GTK types or event loops are referenced.

2. **Asynchronous Delta Exchange**:
   Document changes are emitted as deterministic state deltas or sequence operations. The engine can serialize deltas to binary stream representations for peer-to-peer transport (e.g. over local LAN or self-hosted sync daemons) or local disk logging.

3. **Deterministic Conflict Resolution**:
   Document model changes (text edits in Letters, cell value updates in Tables, slide ordering in Decks) use causal ordering or deterministic tie-breaking (e.g. LWW / RGA algorithms) to guarantee state convergence across multiple clients without requiring a central authority.

4. **Zero-Overhead Local Editing**:
   Local single-user editing incurs zero networking overhead. CRDT tracking structures operate in-memory with optional compaction/snapshotting during atomic file saves (`.odt`, `.ods`, `.odp`).

---

## Strategic Implementation Phases

| Phase | Horizon | Focus Areas | Deliverable |
|-------|---------|-------------|-------------|
| **Phase 1** | Q4 2026 | Event Sourcing & Delta Logging | `suite-common-core` change event journal and deterministic undo/redo state serialization |
| **Phase 2** | Q1 2027 | CRDT Model Integration | Integration of text sequence CRDT (RGA/Yrs) into Letters engine and cell grid delta log into Tables |
| **Phase 3** | Q2 2027 | Peer-to-Peer & Daemon Sync | P2P local network sync via libp2p / local socket daemons and multi-device state convergence |

---

## Verification & Test Strategy

- Unit tests in `suite-common-core` testing out-of-order delta application and state convergence across synthetic peer nodes.
- Zero GUI dependencies required for collaboration tests.
