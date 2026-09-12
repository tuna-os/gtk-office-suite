# RFC-0002: Offline-First Cloud Storage Sync & GVFS Provider Adapter Architecture

Date: 2026-09-12 · Status: **draft, not accepted** · Tracks: [#677](https://github.com/tuna-os/gtk-office-suite/issues/677)

## Summary

This RFC proposes an offline-first cloud storage synchronization and remote file adapter architecture for GTK Office Suite (Letters, Tables, Decks).

As the suite matures into a production daily driver on the Linux desktop, user documents are increasingly saved across local disk, network mounts (`smb://`, `sftp://`), and cloud storage services (Nextcloud, WebDAV, Google Drive via GNOME Online Accounts and GVFS).

While basic atomic file save operations (`suite-common-core::atomic_save`) safely replace local files whole-buffer, remote mounts and active sync agents (e.g. Syncthing, Nextcloud Desktop client) introduce unique risks:
- High latency during remote sync write flushes.
- File-locking conflicts when background sync daemons index open `.odt`/`.ods`/`.odp` archives.
- Mid-edit remote modification collisions when documents are edited across multiple devices.
- Network drops during save transactions over remote GVFS mounts.

This proposal establishes a GTK-free storage adapter protocol (`suite-common-core::sync`), GTK shell non-blocking sync status indicators, and pre-save snapshot protection to guarantee zero data loss during cloud and network file transactions.

---

## Constraints and Architectural Rules

1. **Core crates remain GTK-free**: Storage adapter traits (`StorageProvider`, `SyncState`, `ConflictStrategy`) and local sidecar recovery logic belong in `suite-common-core`. Shell-specific GLib/Gio `GFileMonitor` integration and GTK headerbar widgets reside in `suite-common` / app binaries.
2. **File remains the canonical source of truth**: Parity with OpenDocument standards (ODT, ODS, ODP) and LibreOffice interop (`docs/PARITY.md`) requires the saved file on disk to be the authoritative document.
3. **Atomic save integrity maintained**: Atomic save semantics must be preserved. Remote saves write first to local temporary sidecars before atomic rename/sync flushes.
4. **Non-blocking UI main loop**: Remote network I/O, sync status checks, and conflict detection must never execute on the GTK main thread.

---

## Architecture & Component Design

### 1. Core Adapter Trait (`suite-common-core::sync`)

```rust
pub enum SyncStatus {
    LocalOnly,
    Synced,
    Syncing { progress: f32 },
    Conflict { remote_mtime: u64 },
    Offline,
    Error(String),
}

pub trait StorageProvider: Send + Sync {
    fn provider_name(&self) -> &str;
    fn check_status(&self, path: &std::path::Path) -> Result<SyncStatus, String>;
    fn prepare_pre_save_snapshot(&self, path: &std::path::Path) -> Result<std::path::PathBuf, String>;
    fn resolve_conflict(&self, local_path: &std::path::Path, strategy: ConflictStrategy) -> Result<(), String>;
}

pub enum ConflictStrategy {
    KeepLocal,
    OverwriteWithRemote,
    SaveCopyAs(std::path::PathBuf),
}
```

### 2. Pre-Save Snapshot Sidecar Pattern

Before attempting write operations to a GVFS-mounted remote directory or cloud-synced folder:
1. `suite-common-core` creates a local snapshot sidecar in the staging location (`.odt.~sync-backup`).
2. Atomic save executes whole-buffer write to temporary target.
3. Upon verified disk sync, the temporary snapshot is rotated or retained in emergency recovery.
4. If a remote write fails or disconnects, the user's unsaved edits are preserved in local staging and flagged in UI status.

### 3. Desktop Shell & GVFS Integration

- **GLib/Gio `GFileMonitor`**: Watches document file descriptors for external modification events.
- **Headerbar Sync Badge**: Displays clean visual indicators (`adw::StatusPage` / `GtkSpinner` / status icon) without blocking user input during background network transfers.
- **Conflict Resolution Dialog**: Standardized libadwaita modal dialog offering side-by-side comparison ("Keep My Changes", "Load Cloud Version", "Save Local Copy").

---

## Roadmap Alignment & Phasing

- **Phase 1 (Q4 2026)**: Implement `suite-common-core::sync` traits and local sidecar pre-save snapshot guard.
- **Phase 2 (Q4 2026)**: Integrates Gio file monitoring in `suite-common` with headerbar status indicators in Letters, Tables, Decks.
- **Phase 3 (Q1 2027)**: Full GNOME Online Accounts / Nextcloud conflict resolution UI and automated retry queues.

---

## Open Questions

1. Should high-latency GVFS operations fall back automatically to async background thread pools?
2. How should file lock contention with third-party sync engines (e.g. Syncthing locking open files) be reported to the desktop user?
