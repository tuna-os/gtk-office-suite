// SPDX-License-Identifier: GPL-3.0-or-later
//! Documents at any GIO location, not only local paths (RFC-0003).
//!
//! A file chooser, a drag and drop or the command line can hand over a
//! location with no local path: a GVfs mount of Google Drive, Nextcloud
//! (WebDAV) or an SMB share that has no FUSE path. The apps read and write
//! documents through paths, because the zip readers want `Read + Seek` and
//! saving is atomic through a sibling temp file. So a remote document is
//! staged:
//!
//! - [`open_location`] downloads it to a private cache file with the same
//!   name, so extension dispatch and titles work, and remembers which
//!   location that file stands for and the etag it was downloaded at.
//! - The app reads and writes the staged path as it does any other path.
//! - After every save, [`commit_save`] uploads a staged file back with
//!   `replace_contents`. It passes the etag from the download or the last
//!   upload, so a document changed elsewhere in the meantime is refused
//!   with [`CHANGED_ELSEWHERE`] rather than overwritten.
//!
//! A local location keeps its own path, and nothing about it changes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gtk4::gio;
use gtk4::gio::prelude::*;
use gtk4::glib;

/// The start of the error [`commit_save`] returns when the document
/// changed at its location since it was opened or last saved.
pub const CHANGED_ELSEWHERE: &str = "The document was changed elsewhere since it was opened";

struct Remote {
    location: gio::File,
    etag: Option<String>,
}

thread_local! {
    static STAGED: RefCell<HashMap<PathBuf, Remote>> = RefCell::new(HashMap::new());
}

/// The private file that stands for `location`: the same name, in a
/// directory of its own per location.
fn staging_path(location: &gio::File) -> Result<PathBuf, String> {
    let name = location
        .basename()
        .filter(|n| !n.as_os_str().is_empty() && n.as_os_str() != "/")
        .unwrap_or_else(|| PathBuf::from("document"));
    let uri = location.uri();
    let key = glib::compute_checksum_for_string(glib::ChecksumType::Sha256, &uri)
        .map(|s| s[..16].to_string())
        .unwrap_or_else(|| "location".into());
    let dir = glib::user_cache_dir().join("tunaos-office").join("remote").join(key);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Can't stage {uri}: {e}"))?;
    Ok(dir.join(name))
}

/// The path of a local file. A remote location can report a path too,
/// through GVfs's FUSE bridge, but it isn't used: that path goes through
/// no etag check, and inside the Flatpak it points into the host's
/// `/run/user/<uid>/gvfs`, which the sandbox can't see (the spike found
/// both, `docs/rfc/0003-spike-results.md`). A remote location goes
/// through GIO instead.
fn native_path(location: &gio::File) -> Option<PathBuf> {
    location.is_native().then(|| location.path()).flatten()
}

fn remember(path: &Path, location: &gio::File, etag: Option<String>) {
    STAGED.with(|s| s.borrow_mut().insert(path.to_path_buf(), Remote { location: location.clone(), etag }));
}

/// A local path to read the document at `location` from: its own path, or
/// a staged copy of a remote location, downloaded now.
pub fn open_location(location: &gio::File) -> Result<PathBuf, String> {
    if let Some(path) = native_path(location) {
        return Ok(path);
    }
    let (bytes, etag) = location
        .load_contents(None::<&gio::Cancellable>)
        .map_err(|e| format!("Can't read {}: {}", location.uri(), e.message()))?;
    let path = staging_path(location)?;
    std::fs::write(&path, &bytes).map_err(|e| format!("Can't stage {}: {e}", location.uri()))?;
    let etag = etag.map(|t| t.to_string()).filter(|t| !t.is_empty()).or_else(|| current_etag(location));
    remember(&path, location, etag);
    Ok(path)
}

/// A local path to save the document to `location` at: its own path, or a
/// staging path that [`commit_save`] uploads after the app has written it.
pub fn save_location(location: &gio::File) -> Result<PathBuf, String> {
    if let Some(path) = native_path(location) {
        return Ok(path);
    }
    let path = staging_path(location)?;
    // A new destination the user just chose: overwriting what's there is
    // what they confirmed in the dialog, so there is no etag to check.
    remember(&path, location, None);
    Ok(path)
}

/// After the app has written `path`: if it stands for a remote location,
/// upload it there. A local path needs nothing.
pub fn commit_save(path: &Path) -> Result<(), String> {
    let Some((location, etag)) = STAGED.with(|s| s.borrow().get(path).map(|r| (r.location.clone(), r.etag.clone()))) else {
        return Ok(());
    };
    let bytes = std::fs::read(path).map_err(|e| format!("Can't read the saved copy: {e}"))?;
    // Compare the etag here as well as handing it to GIO: the spike found
    // that GVfs's WebDAV backend doesn't make the server check it, so a
    // stale save went through (`docs/rfc/0003-spike-results.md`). A save
    // could still slip in between this check and the upload.
    if let Some(expected) = &etag {
        let now = current_etag(&location);
        if now.as_ref().is_some_and(|now| now != expected) {
            return Err(format!("{CHANGED_ELSEWHERE} ({}).", location.uri()));
        }
    }
    let new_etag = location
        .replace_contents(&bytes, etag.as_deref(), false, gio::FileCreateFlags::NONE, None::<&gio::Cancellable>)
        .map_err(|e| {
            if e.matches(gio::IOErrorEnum::WrongEtag) {
                format!("{CHANGED_ELSEWHERE} ({}).", location.uri())
            } else {
                format!("Can't save to {}: {}", location.uri(), e.message())
            }
        })?;
    // Some backends don't return the new etag from the upload: ask.
    let new_etag = new_etag.map(|t| t.to_string()).filter(|t| !t.is_empty()).or_else(|| current_etag(&location));
    remember(path, &location, new_etag);
    Ok(())
}

/// The location's etag as the backend reports it now, if it has one.
fn current_etag(location: &gio::File) -> Option<String> {
    location
        .query_info(gio::FILE_ATTRIBUTE_ETAG_VALUE, gio::FileQueryInfoFlags::NONE, None::<&gio::Cancellable>)
        .ok()
        .and_then(|info| info.etag())
        .map(|t| t.to_string())
        .filter(|t| !t.is_empty())
}

/// The location a staged path stands for, as a URI; `None` for a local
/// path. For titles, recent files and messages.
pub fn remote_uri(path: &Path) -> Option<String> {
    STAGED.with(|s| s.borrow().get(path).map(|r| r.location.uri().to_string()))
}

/// The remote location a staged path stands for; `None` for a local path.
pub fn remote_location(path: &Path) -> Option<gio::File> {
    STAGED.with(|s| s.borrow().get(path).map(|r| r.location.clone()))
}

// ---------------------------------------------------------------- async

/// Why an upload didn't happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadError {
    /// The document changed at its location since it was opened or last
    /// saved here. The message names the location.
    ChangedElsewhere(String),
    Failed(String),
}

/// The chunk a download reads at a time, and so how often progress moves.
const CHUNK: usize = 256 * 1024;

async fn current_etag_async(location: &gio::File) -> Option<String> {
    location
        .query_info_future(gio::FILE_ATTRIBUTE_ETAG_VALUE, gio::FileQueryInfoFlags::NONE, glib::Priority::DEFAULT)
        .await
        .ok()
        .and_then(|info| info.etag())
        .map(|t| t.to_string())
        .filter(|t| !t.is_empty())
}

/// [`open_location`] without blocking: a remote document is read in
/// chunks, reporting `(bytes so far, total if known)` to `progress`.
/// Dropping the future (see [`gio::CancellableFuture`]) cancels it.
pub async fn download(location: &gio::File, progress: impl Fn(u64, Option<u64>)) -> Result<PathBuf, String> {
    if let Some(path) = native_path(location) {
        return Ok(path);
    }
    let failed = |e: glib::Error| format!("Can't read {}: {}", location.uri(), e.message());
    let info = location
        .query_info_future("standard::size,etag::value", gio::FileQueryInfoFlags::NONE, glib::Priority::DEFAULT)
        .await
        .ok();
    let total = info.as_ref().map(|i| i.size()).filter(|s| *s > 0).map(|s| s as u64);
    let etag = info.and_then(|i| i.etag()).map(|t| t.to_string()).filter(|t| !t.is_empty());
    let stream = location.read_future(glib::Priority::DEFAULT).await.map_err(failed)?;
    let mut bytes = Vec::with_capacity(total.unwrap_or(0) as usize);
    loop {
        let chunk = stream.read_bytes_future(CHUNK, glib::Priority::DEFAULT).await.map_err(failed)?;
        if chunk.is_empty() {
            break;
        }
        bytes.extend_from_slice(&chunk);
        progress(bytes.len() as u64, total);
    }
    let _ = stream.close_future(glib::Priority::DEFAULT).await;
    let path = staging_path(location)?;
    std::fs::write(&path, &bytes).map_err(|e| format!("Can't stage {}: {e}", location.uri()))?;
    let etag = match etag {
        Some(t) => Some(t),
        None => current_etag_async(location).await,
    };
    remember(&path, location, etag);
    Ok(path)
}

/// [`commit_save`] without blocking. With `check`, a document changed at
/// its location since it was opened or last saved is refused with
/// [`UploadError::ChangedElsewhere`]; without it (the user chose to
/// overwrite), it's replaced. Dropping the future cancels the upload, and
/// GIO then leaves the location as it was.
pub async fn upload(path: &Path, check: bool) -> Result<(), UploadError> {
    let Some((location, etag)) = STAGED.with(|s| s.borrow().get(path).map(|r| (r.location.clone(), r.etag.clone()))) else {
        return Ok(());
    };
    let changed = || UploadError::ChangedElsewhere(format!("{CHANGED_ELSEWHERE} ({}).", location.uri()));
    let bytes = std::fs::read(path).map_err(|e| UploadError::Failed(format!("Can't read the saved copy: {e}")))?;
    let etag = if check { etag } else { None };
    // Compared here as well as handed to GIO: GVfs's WebDAV backend
    // doesn't make every server check it (docs/rfc/0003-spike-results.md).
    if let Some(expected) = &etag {
        if current_etag_async(&location).await.is_some_and(|now| now != *expected) {
            return Err(changed());
        }
    }
    let new_etag = location
        .replace_contents_future(bytes, etag.as_deref(), false, gio::FileCreateFlags::NONE)
        .await
        .map(|(_, etag)| etag)
        .map_err(|(_, e)| {
            if e.matches(gio::IOErrorEnum::WrongEtag) {
                changed()
            } else {
                UploadError::Failed(format!("Can't save to {}: {}", location.uri(), e.message()))
            }
        })?;
    let new_etag = match new_etag.map(|t| t.to_string()).filter(|t| !t.is_empty()) {
        Some(t) => Some(t),
        None => current_etag_async(&location).await,
    };
    remember(path, &location, new_etag);
    Ok(())
}

/// Stage a copy of the staged or local file `from` for `to`, the "Save as
/// Copy" destination, so [`upload`] (with no etag to check) puts it there.
pub fn stage_copy(from: &Path, to: &gio::File) -> Result<PathBuf, String> {
    let target = save_location(to)?;
    if target != from {
        std::fs::copy(from, &target).map_err(|e| format!("Can't copy to {}: {e}", to.uri()))?;
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_location_keeps_its_own_path() {
        let dir = std::env::temp_dir().join(format!("locations-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("book.xlsx");
        std::fs::write(&path, b"local").unwrap();
        let file = gio::File::for_path(&path);
        assert_eq!(open_location(&file).unwrap(), path);
        assert_eq!(save_location(&file).unwrap(), path);
        assert!(commit_save(&path).is_ok());
        assert_eq!(remote_uri(&path), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_location_without_a_path_is_read_through_gio() {
        // A GResource has no local path, like a GVfs location without a
        // FUSE mount, and needs no daemon.
        let bytes = glib::Bytes::from_static(b"not a local file");
        let resource_bytes = {
            // A one-file resource bundle, built in memory.
            let dir = std::env::temp_dir().join(format!("locations-res-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("sheet.csv"), bytes.as_ref()).unwrap();
            std::fs::write(
                dir.join("r.gresource.xml"),
                "<gresources><gresource prefix=\"/org/tunaos/test\"><file>sheet.csv</file></gresource></gresources>",
            )
            .unwrap();
            let status = std::process::Command::new("glib-compile-resources")
                .current_dir(&dir)
                .args(["--target=r.gresource", "r.gresource.xml"])
                .status();
            if !status.is_ok_and(|s| s.success()) {
                eprintln!("glib-compile-resources isn't available; skipping");
                return;
            }
            let data = std::fs::read(dir.join("r.gresource")).unwrap();
            std::fs::remove_dir_all(&dir).unwrap();
            data
        };
        let resource = gio::Resource::from_data(&glib::Bytes::from_owned(resource_bytes)).unwrap();
        gio::resources_register(&resource);
        let file = gio::File::for_uri("resource:///org/tunaos/test/sheet.csv");
        assert!(file.path().is_none(), "the test needs a location with no local path");
        let staged = open_location(&file).unwrap();
        assert_eq!(staged.file_name().unwrap(), "sheet.csv", "the name, so the format is still known");
        assert_eq!(std::fs::read(&staged).unwrap(), b"not a local file");
        assert_eq!(remote_uri(&staged).as_deref(), Some("resource:///org/tunaos/test/sheet.csv"));
        // A resource is read-only: saving back says so rather than
        // pretending it saved.
        let err = commit_save(&staged).unwrap_err();
        assert!(err.contains("resource:///org/tunaos/test/sheet.csv"), "{err}");
    }

    #[test]
    fn a_location_that_cannot_be_read_says_so() {
        let file = gio::File::for_uri("dav://nowhere.invalid/book.xlsx");
        assert!(file.path().is_none());
        let err = open_location(&file).unwrap_err();
        assert!(err.contains("dav://nowhere.invalid/book.xlsx"), "{err}");
    }
}
