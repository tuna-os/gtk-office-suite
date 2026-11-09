//! Contact photos by address, for the avatars: an index of where each
//! photo is, kept fresh by a watcher thread.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use super::read::{open_book_db, CACHED_BOOKS, EDS_LAYOUTS, LOCAL_BOOKS};
use super::registry::registry_books;
use super::vcard::{vcard_photo, vcard_photo_with};

/// Photo bytes and the EDS database state they came from. The inexpensive
/// fingerprint check lets a long-running Hylki notice CardDAV synchronizations
/// without subscribing to backend-specific EDS signals.
#[derive(Clone)]
struct PhotoLocation {
    db: PathBuf,
    uid: String,
    cached_book: bool,
    /// The photo lives in the local Hylki book (`db` is unused).
    local: bool,
}

/// (path, size, mtime-nanos) per EDS database file — the cheap change signal.
type DbFingerprint = Vec<(PathBuf, u64, u128)>;

struct ContactPhotoIndex {
    photos: HashMap<String, Vec<PhotoLocation>>,
    fingerprint: DbFingerprint,
    generation: u64,
    ready: bool,
}

static CONTACT_PHOTOS: OnceLock<Arc<Mutex<ContactPhotoIndex>>> = OnceLock::new();
static PHOTO_LOAD_STARTED: OnceLock<()> = OnceLock::new();
static PHOTO_WATCH_STARTED: OnceLock<()> = OnceLock::new();
type PhotoChangeCallback = Box<dyn Fn() + Send + Sync + 'static>;
static PHOTO_CHANGE_CALLBACKS: OnceLock<Mutex<Vec<PhotoChangeCallback>>> = OnceLock::new();
const PHOTO_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

fn new_photo_index() -> Arc<Mutex<ContactPhotoIndex>> {
    Arc::new(Mutex::new(ContactPhotoIndex {
        photos: HashMap::new(),
        fingerprint: Vec::new(),
        generation: 1,
        ready: false,
    }))
}

fn load_stable_photo_index() -> (HashMap<String, Vec<PhotoLocation>>, DbFingerprint) {
    let mut last = (HashMap::new(), Vec::new());
    for _ in 0..3 {
        let before = photo_db_fingerprint();
        let photos = load_contact_photo_index();
        let after = photo_db_fingerprint();
        last = (photos, after.clone());
        if before == after {
            break;
        }
    }
    last
}

fn notify_photo_change() {
    if let Some(callbacks) = PHOTO_CHANGE_CALLBACKS.get() {
        for callback in callbacks.lock().unwrap_or_else(|p| p.into_inner()).iter() {
            callback();
        }
    }
}

fn start_photo_load() {
    PHOTO_LOAD_STARTED.get_or_init(|| {
        let index = CONTACT_PHOTOS.get_or_init(new_photo_index).clone();
        let _ = std::thread::Builder::new()
            .name("eds-photo-load".into())
            .spawn(move || {
                let (photos, fingerprint) = load_stable_photo_index();
                let mut current = index.lock().unwrap_or_else(|p| p.into_inner());
                current.photos = photos;
                current.fingerprint = fingerprint;
                current.generation = current.generation.wrapping_add(1).max(1);
                current.ready = true;
                drop(current);
                notify_photo_change();
            });
    });
}

fn start_photo_watcher() {
    PHOTO_WATCH_STARTED.get_or_init(|| {
        let index = CONTACT_PHOTOS.get_or_init(new_photo_index).clone();
        let _ = std::thread::Builder::new()
            .name("eds-photo-watch".into())
            .spawn(move || loop {
                std::thread::sleep(PHOTO_REFRESH_INTERVAL);
                let fingerprint = photo_db_fingerprint();
                let changed = {
                    let current = index.lock().unwrap_or_else(|p| p.into_inner());
                    fingerprint != current.fingerprint
                };
                if !changed {
                    continue;
                }
                // Scan outside the lock. UI lookups continue using the previous
                // immutable snapshot until the replacement is ready.
                let (photos, stable_fingerprint) = load_stable_photo_index();
                let mut current = index.lock().unwrap_or_else(|p| p.into_inner());
                current.photos = photos;
                current.fingerprint = stable_fingerprint;
                current.generation = current.generation.wrapping_add(1).max(1);
                drop(current);
                notify_photo_change();
            });
    });
}

pub(crate) fn watch_photo_changes<F: Fn() + Send + Sync + 'static>(callback: F) {
    PHOTO_CHANGE_CALLBACKS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(Box::new(callback));
    start_photo_load();
    start_photo_watcher();
}

/// Current index generation. Avatar textures and negative results use this to
/// invalidate themselves after an EDS/CardDAV database change.
pub(crate) fn photo_generation() -> u64 {
    start_photo_load();
    start_photo_watcher();
    CONTACT_PHOTOS
        .get_or_init(new_photo_index)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .generation
}

pub(crate) fn photos_ready() -> bool {
    start_photo_load();
    CONTACT_PHOTOS
        .get_or_init(new_photo_index)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .ready
}

/// Find contact-photo candidates by email in the local EDS caches. Multiple
/// books can contain the same address; the avatar loader tries each candidate.
/// CardDAV backends such as Nextcloud keep PHOTO in the cached vCard, so this
/// does not contact the address-book server or disclose the sender address to a third
/// party.
pub(crate) fn contact_photos(email: &str) -> Vec<Arc<[u8]>> {
    let key = email.trim().to_lowercase();
    if key.is_empty() {
        return Vec::new();
    }
    start_photo_load();
    start_photo_watcher();
    let locations = CONTACT_PHOTOS
        .get_or_init(new_photo_index)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .photos
        .get(&key)
        .cloned()
        .unwrap_or_default();
    locations
        .iter()
        .filter_map(read_photo_at)
        .map(Arc::from)
        .collect()
}

fn photo_db_fingerprint() -> DbFingerprint {
    let mut files = Vec::new();
    for db in photo_db_paths() {
        files.push(db.clone());
        for suffix in ["-journal", "-wal"] {
            files.push(PathBuf::from(format!("{}{suffix}", db.to_string_lossy())));
        }
    }
    files.extend(crate::contacts::local::db_paths());
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let metadata = std::fs::metadata(&path).ok()?;
            let modified = metadata
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH)
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            Some((path, metadata.len(), modified))
        })
        .collect()
}

fn photo_db_paths() -> Vec<PathBuf> {
    let mut dbs: Vec<PathBuf> = EDS_LAYOUTS.iter().flat_map(|l| l.dbs()).collect();
    dbs.sort();
    dbs
}

fn load_contact_photo_index() -> HashMap<String, Vec<PhotoLocation>> {
    let mut photos = HashMap::new();
    // A book switched off, in EDS or for Hylki, lends no faces.
    let active = registry_books();
    for layout in EDS_LAYOUTS {
        let sql = format!(
            "SELECT e.value, c.{uid} FROM {cards} c JOIN {emails} e ON c.{uid} = e.uid \
             WHERE c.{vcard} LIKE '%PHOTO%'",
            uid = layout.uid,
            cards = layout.cards,
            emails = layout.emails,
            vcard = layout.vcard,
        );
        for db in layout.wanted_dbs(&active, false) {
            read_photo_locations(&db, &sql, layout.cached, &mut photos);
        }
    }
    add_local_photo_locations(&mut photos);
    tracing::debug!(count = photos.len(), "indexed EDS contact photos");
    photos
}

/// Index the local Hylki book's contacts that carry a photo.
fn add_local_photo_locations(photos: &mut HashMap<String, Vec<PhotoLocation>>) {
    if !crate::contacts::local::exists() || !crate::contacts::contact_sources::hylki_used() {
        return;
    }
    let entries = match crate::contacts::local::photo_entries() {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("contacts: cannot index local photos: {e}");
            return;
        }
    };
    for (email, uid) in entries {
        let location = PhotoLocation {
            db: PathBuf::new(),
            uid,
            cached_book: false,
            local: true,
        };
        photos.entry(email).or_default().push(location);
    }
}

/// A local write happened: re-index the photos now (rather than at the next
/// 30 second check) so avatars and the Contacts list follow at once.
pub(crate) fn local_changed() {
    start_photo_load();
    start_photo_watcher();
    let index = CONTACT_PHOTOS.get_or_init(new_photo_index).clone();
    let _ = std::thread::Builder::new()
        .name("local-photo-reload".into())
        .spawn(move || {
            let (photos, fingerprint) = load_stable_photo_index();
            let mut current = index.lock().unwrap_or_else(|p| p.into_inner());
            current.photos = photos;
            current.fingerprint = fingerprint;
            current.generation = current.generation.wrapping_add(1).max(1);
            drop(current);
            notify_photo_change();
        });
}

fn read_photo_locations(
    path: &std::path::Path,
    query: &str,
    cached_book: bool,
    photos: &mut HashMap<String, Vec<PhotoLocation>>,
) {
    let conn = match open_book_db(path) {
        Ok(conn) => conn,
        Err(e) => {
            tracing::warn!("contacts: cannot open {} for photos: {e}", path.display());
            return;
        }
    };
    let mut stmt = match conn.prepare(query) {
        Ok(stmt) => stmt,
        Err(e) => {
            tracing::warn!("contacts: photo query failed on {}: {e}", path.display());
            return;
        }
    };
    let rows = match stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }) {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("contacts: photo rows failed on {}: {e}", path.display());
            return;
        }
    };
    for (email, uid) in rows.flatten() {
        let key = email.trim().to_lowercase();
        if !key.is_empty() {
            let location = PhotoLocation {
                db: path.to_path_buf(),
                uid,
                cached_book,
                local: false,
            };
            photos.entry(key).or_default().push(location);
        }
    }
}

fn read_photo_at(location: &PhotoLocation) -> Option<Vec<u8>> {
    if location.local {
        let vcard = crate::contacts::local::vcard_by_uid(&location.uid).ok()??;
        return vcard_photo_with(&vcard, false);
    }
    let conn = open_book_db(&location.db).ok()?;
    let layout = if location.cached_book {
        &CACHED_BOOKS
    } else {
        &LOCAL_BOOKS
    };
    let query = format!(
        "SELECT {} FROM {} WHERE {} = ?1",
        layout.vcard, layout.cards, layout.uid
    );
    let vcard: Option<String> = conn
        .query_row(&query, [&location.uid], |row| row.get(0))
        .ok()?;
    vcard.as_deref().and_then(vcard_photo)
}

pub(super) fn find_dbs(root: &std::path::Path, file: &str) -> Vec<PathBuf> {
    let mut dbs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let db = entry.path().join(file);
            if db.is_file() {
                dbs.push(db);
            }
        }
    }
    dbs.sort();
    dbs
}
