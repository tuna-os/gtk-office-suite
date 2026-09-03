# Document Crash Recovery and Transactional Autosave Strategy

**Date**: 2026-09-03  
**Status**: Proposal / Hold-Gated Review  
**Target Applications**: Letters, Tables, Decks  
**Shared Crate**: `suite-common-core::autosave`  

---

## Executive Summary

Users rely on an office suite to safeguard critical documents against unexpected system events, such as OOM kills, kernel panics, Wayland compositor disconnects, or Flatpak sandbox terminations. A single instance of unrecoverable data loss destroys desktop user trust.

This document establishes the **Document Crash Recovery and Transactional Autosave Strategy** for `gtk-office-suite`. Building on the low-level `suite-common-core::autosave` primitives (`AutosaveSlot`, `find_orphaned_snapshots`), this strategy standardizes session lifecycle integration, user recovery experience (`AdwBanner`), periodic background write-ahead timers, crash-loop quarantine protection, and automated verification suites across **Letters**, **Tables**, and **Decks**.

---

## Current State vs Target Architecture

| Capability | Current State | Target Architecture |
|------------|---------------|---------------------|
| **Core Primitives** | `AutosaveSlot` & `find_orphaned_snapshots` implemented in `suite-common-core` | Retained as atomic, double-buffer I/O substrate |
| **App Integration** | Partial autosave in `Letters` session loop; missing in `Tables` and `Decks` | Standardized `SessionRecoveryManager` lifecycle across all three apps |
| **User Experience** | Silent or un-orchestrated recovery on launch | Non-modal `AdwBanner` prompt at startup offering "Restore Session" or "Discard" |
| **Write Interval** | Hardcoded or irregular periodic ticks | Standardized 30-second background write-ahead timer via `glib::timeout_add_local` |
| **Crash Protection** | Risk of crash-loops if snapshot reading panics | Quarantine protocol: snapshot moved to `quarantine/` if deserialization fails |
| **Verification** | Single-unit tests for `AutosaveSlot` | Full end-to-end integration tests with simulated SIGKILL / process termination |

---

## Architecture and Recovery Lifecycle

```
[ Application Startup ]
          │
          ▼
[ SessionRecoveryManager::scan_orphaned_snapshots() ]
          │
    ┌─────┴────────────────────────┐
    │ Orphaned Snapshots Found?     │
    └─────┬────────────────────────┘
          │
   ┌──────┴──────┐
   │             │
  Yes            No
   │             │
   ▼             ▼
[ Display AdwBanner ]      [ Normal Document Session ]
("Unsaved work recovered")        │
   │                              ▼
   ├─► User clicks "Restore" ──► [ Deserialize & Load into Controller ] ──► [ Clear Slot ]
   │                                                                         │
   └─► User clicks "Discard" ──► [ Purge Snapshot Files ] ─────────────────┘
```

### 1. Atomic Double-Buffered Write-Ahead
Every document session manages an `AutosaveSlot` tied to a stable `doc_id`.
- Autosave trigger: Every 30 seconds when the document is dirty, or 3 seconds after the last keystroke/cell edit.
- Atomic guarantee: Data is written to `doc_id.snapshot.tmp` and renamed atomically to `doc_id.snapshot`, followed by `doc_id.snapshot.meta`.
- Clean Save / Discard: On explicit file save (Ctrl+S) or tab close without saving, `AutosaveSlot::clear()` purges the snapshot files.

### 2. Startup Recovery UX (`AdwBanner`)
When an application launches:
1. `SessionRecoveryManager` scans the app state directory (`XDG_STATE_HOME/gtk-office-suite/<app>/autosave/`).
2. If complete orphaned snapshots exist, an `AdwBanner` is attached to the top of the main window shell (`AdwToolbarView`).
3. Banner action button: **"Review & Restore"**.
   - User can inspect recovered tabs, save them under new/original paths, or discard.
4. Banner dismiss button: **"Discard All"**.

### 3. Crash-Loop Quarantine Protocol
To prevent a corrupted snapshot file from crashing the application repeatedly during startup:
- Snapshot deserialization is wrapped in a catch/result boundary.
- If parsing fails or panics, the snapshot is moved to a `quarantine/` subdirectory with a `.corrupt` extension.
- A notification dialog informs the user that a snapshot was corrupted and preserved for inspection without blocking application startup.

---

## Implementation Roadmap

### Phase 1: Shared Session Recovery Manager (`suite-common-core`)
- Implement `SessionRecoveryManager` struct wrapping `find_orphaned_snapshots`.
- Add recovery state tracking (`Pending`, `Restoring`, `Discarded`, `Quarantined`).
- Add unit tests for multi-document recovery queues.

### Phase 2: App Controller Wiring (`Letters`, `Tables`, `Decks`)
- Wire 30s background timer into `DocumentSession` (Letters), `WorkbookController` (Tables), and `DecksController` (Decks).
- Trigger `AutosaveSlot::write()` on dirty controller state.
- Ensure `AutosaveSlot::clear()` executes upon explicit save or clean shutdown.

### Phase 3: GTK4 / Libadwaita Banner UI Integration
- Add `AdwBanner` widget to `window.rs` layout in all three apps.
- Implement session restoration action handlers.

### Phase 4: Integration & Failure Injection Testing
- Add integration test suite under `tests/gui/` using process kill signals (`SIGKILL`).
- Assert zero data loss when process is terminated mid-session.

---

## Success Criteria

1. **Zero Data Loss**: 100% of unsaved keystrokes/cell changes older than 30s survive process `SIGKILL`.
2. **Crash-Loop Safety**: Zero launch crashes caused by invalid or corrupted snapshot files.
3. **Consistency**: Unified UI recovery banner and behavior across Letters, Tables, and Decks.
