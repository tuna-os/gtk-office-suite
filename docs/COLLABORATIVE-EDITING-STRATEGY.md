# Collaborative Editing & Real-Time Synchronization Strategy

**Author**: Strategist Agent  
**Horizon**: Mid-term / Long-term  
**Status**: Proposal  

---

## Executive Summary

As `gtk-office-suite` matures post-v1.0 with robust single-user document processing and format-parity gates, multi-user collaboration and network synchronization represent the next essential frontier for user adoption and enterprise viability. This document outlines the architecture, synchronization primitives, and integration strategy for bringing real-time collaborative editing to Letters, Tables, and Decks without violating core architecture constraints.

---

## Strategic Rationale

1. **User Adoption**: Modern document editing increasingly occurs in team contexts. Supporting real-time co-authoring bridges the parity gap between native desktop Linux applications and web-based suites (Google Docs, Microsoft 365, Nextcloud Office).
2. **Architectural Purity**: Adhering strictly to `AGENTS.md` ("No business logic in widget code"), synchronization state machines and Conflict-Free Replicated Data Types (CRDTs) must reside entirely in GTK-free core crates (`suite-common-core` or dedicated core crates).
3. **Local-First & Offline Resilience**: Local offline editing must remain non-blocking. Synchronization engines must seamlessly merge offline edits upon reconnection.

---

## Technical Architecture

```
+-------------------------------------------------------------+
|               GTK4 / Libadwaita Application Layer          |
|          (Letters / Tables / Decks - UI Widgets)             |
+-------------------------------------------------------------+
                              |
                     Signals & Actions
                              v
+-------------------------------------------------------------+
|                   `suite-common-core`                       |
|  +-------------------------------------------------------+  |
|  |             CRDT / Sync Engine Coordinator            |  |
|  | - Operational Transformation / State-Vector Merging   |  |
|  | - Conflict Resolution & Peer State Management         |  |
|  +-------------------------------------------------------+  |
|  |           Document State Model (GTK-free)             |  |
|  +-------------------------------------------------------+  |
+-------------------------------------------------------------+
                              |
                   Network Transport Abstraction
                              v
+-------------------------------------------------------------+
|    Sync Adapters: WebSockets, WebRTC, Nextcloud/Matrix    |
+-------------------------------------------------------------+
```

### 1. Core Synchronization Primitives (`suite-common-core`)

- **State-Vector Delta Engine**: Implement sequence-based state vectors tracking document mutations as atomic operations.
- **CRDT Interoperability**: Support lightweight document-tree delta encoding (compatible with Yjs/Automerge binary protocols) to facilitate interop with web endpoints and self-hosted storage.
- **Presence & Remote Cursors**: Abstract remote cursor tracking, selection ranges, and collaborator metadata in GTK-free data structures.

### 2. Application-Specific Models

- **Letters (Text Processing)**: Sequence CRDT for rich-text delta formatting, paragraph attribute merging, and inline element locks.
- **Tables (Spreadsheets)**: Cell-based sparse matrix synchronization with formula cell dependency recalculation and cell-range locking.
- **Decks (Presentations)**: Object-graph scene tree delta sync with spatial z-order conflict resolution.

---

## Implementation Roadmap

### Phase 1: Core Sync State Machine (Q4 2026)
- Implement `crdt` module in `suite-common-core`.
- Add deterministic state-vector serialization and unit test suite.
- Establish remote presence and cursor models.

### Phase 2: Transport & Network Adapters (Q1 2027)
- Define `SyncProvider` trait in `suite-common-core`.
- Implement local peer-to-peer (mDNS/WebRTC) and WebSocket sync relays.
- Add mock network simulation tests in non-GUI integration suites.

### Phase 3: GUI Integration & Presence Rendering (Q1/Q2 2027)
- Wire remote selection and collaborator cursor rendering in `window.rs` shells via action signals.
- Add UI indicators for network sync status and active co-authors in AdwHeaderBar.

---

## Success Criteria

1. **Deterministic Conformance**: Zero data loss across concurrent multi-user editing simulations in headless core unit tests.
2. **Performance Budget**: Sync delta processing under 5ms for standard document edits.
3. **Zero UI Blocking**: Core sync state updates run asynchronously without locking the GTK main looper thread.
