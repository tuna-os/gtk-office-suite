//! Sender-avatar loading and caching. Local GNOME Contacts / CardDAV photos are
//! preferred. Gravatar is an optional fallback and is only queried when enabled.
//! Below both sit the sender's domain icon (`crate::services::logo`, also opt-in) and the
//! UI's colored-initials fallback; `message_row::find_face` walks the chain.
//!
//! Privacy note: local contact photos are read from Evolution Data Server's
//! on-disk cache and make no network request. Gravatar sends a hash of the
//! sender's email to Automattic, so it remains off by default.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};

use gtk::prelude::{Cast, TextureExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AvatarSource {
    Contact,
    Gravatar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FetchMode {
    ContactOnly,
    ContactThenGravatar,
    GravatarOnly,
}

#[derive(Debug, Clone)]
struct DecodedImage {
    pixels: Arc<[u8]>,
    width: i32,
    height: i32,
}

#[derive(Debug)]
pub(crate) struct FetchedAvatar {
    image: DecodedImage,
    source: AvatarSource,
}

#[derive(Debug)]
pub(crate) enum FetchOutcome {
    Found(FetchedAvatar),
    Missing,
    /// A transient Gravatar failure; retry on a later render.
    Retry,
}

pub(crate) enum CacheLookup {
    Texture(gtk::gdk::Texture),
    Missing,
    Fetch { generation: u64, mode: FetchMode },
}

struct ContactCache {
    generation: u64,
    texture: Option<gtk::gdk::Texture>,
}

thread_local! {
    // Contact state is generation-bound; Gravatar state survives EDS changes so
    // a CardDAV sync never causes unnecessary third-party requests.
    static CONTACT_CACHE: RefCell<HashMap<String, ContactCache>> = RefCell::new(HashMap::new());
    static GRAVATAR_CACHE: RefCell<HashMap<String, Option<gtk::gdk::Texture>>> =
        RefCell::new(HashMap::new());
}

fn key(email: &str) -> String {
    email.trim().to_lowercase()
}

/// The session's sender-face caches, for the memory section of an export:
/// contact entries and the pixel bytes of their photos, then Gravatar
/// entries (hits and misses alike) and the pixel bytes of the hits.
pub(crate) fn cache_stats() -> (usize, u64, usize, u64) {
    let (contacts, contact_bytes) = CONTACT_CACHE.with(|c| {
        let c = c.borrow();
        let bytes = c
            .values()
            .filter_map(|e| e.texture.as_ref())
            .map(crate::desktop::memory_report::texture_bytes)
            .sum();
        (c.len(), bytes)
    });
    let (gravatars, gravatar_bytes) = GRAVATAR_CACHE.with(|c| {
        let c = c.borrow();
        let bytes = c
            .values()
            .flatten()
            .map(crate::desktop::memory_report::texture_bytes)
            .sum();
        (c.len(), bytes)
    });
    (contacts, contact_bytes, gravatars, gravatar_bytes)
}

/// One of the user's own mailboxes, as its account was set up in Accounts
/// (#162): what its circle draws, in the order it is tried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnFace {
    /// The account asked for its own Gravatar (#189). It leads when the
    /// address has one; when it has none the rest of this still applies.
    pub(crate) gravatar: bool,
    /// The account's avatar picture, when it has one and the file is there.
    pub(crate) picture: Option<std::path::PathBuf>,
    /// Its avatar emoji, for an account that chose one instead.
    pub(crate) emoji: Option<String>,
    /// The account color ("#rrggbb"), the ground behind the emoji.
    pub(crate) color: String,
}

impl OwnFace {
    /// Whether there is anything here to draw. An account that asked for
    /// nothing shows the initials circle any sender would get.
    pub(crate) fn is_set(&self) -> bool {
        self.gravatar || self.picture.is_some() || self.emoji.is_some()
    }
}

thread_local! {
    /// Every address the user sends from — an account's own and its send-as
    /// aliases — for the accounts that asked for a face of their own. Kept
    /// here rather than handed down so the sidebar, the reader's cards and
    /// the list's rows can all ask; the app refreshes it whenever the
    /// accounts change.
    static OWN_FACES: RefCell<HashMap<String, OwnFace>> = RefCell::new(HashMap::new());
    /// Whether that face is also worn by the mail the user sent, or only by
    /// the account itself (Settings; #189). Either way the sidebar circle and
    /// the account editor show it — this is about messages.
    static FACES_ON_OWN_MAIL: Cell<bool> = const { Cell::new(true) };
    /// Bumped whenever any of the above actually changes, so a view holding
    /// on to what it last drew (the reader's document) knows to draw again.
    static OWN_FACES_GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// Hand the face chain the mailboxes the user owns, keyed by address. Only
/// accounts that asked for something belong here: one showing its initials
/// has nothing to add to the circle a sender already gets.
pub(crate) fn set_own_faces(faces: impl IntoIterator<Item = (String, OwnFace)>) {
    let faces: HashMap<String, OwnFace> = faces
        .into_iter()
        .map(|(address, face)| (key(&address), face))
        .collect();
    OWN_FACES.with(|current| {
        if *current.borrow() == faces {
            return;
        }
        *current.borrow_mut() = faces;
        note_faces_changed();
    });
}

/// Whether the mail the user sent wears its mailbox's face, or the circle any
/// other sender would get (the Settings choice; #189).
pub(crate) fn set_faces_on_own_mail(on: bool) {
    FACES_ON_OWN_MAIL.with(|flag| {
        if flag.get() == on {
            return;
        }
        flag.set(on);
        note_faces_changed();
    });
}

/// The face `email`'s own account asked for, whatever the mail setting says:
/// the sidebar's circle and the account editor's preview draw this.
pub(crate) fn account_face(email: &str) -> Option<OwnFace> {
    let key = key(email);
    OWN_FACES.with(|faces| faces.borrow().get(&key).cloned())
}

/// What to draw for `email` when it is one of the user's own addresses and
/// its face is worn by its mail (#189): the Gravatar, picture or emoji chosen
/// for that mailbox is the face on the messages it sent, in a conversation's
/// cards and in the list. `None` for everyone else, for own accounts showing
/// initials, and for everyone once the Settings switch is off.
pub(crate) fn own_face(email: &str) -> Option<OwnFace> {
    if !FACES_ON_OWN_MAIL.with(Cell::get) {
        return None;
    }
    account_face(email)
}

/// Note that what the user's mailboxes show has changed (a Gravatar arrived,
/// say), so cached renders are rebuilt.
fn note_faces_changed() {
    OWN_FACES_GENERATION.with(|g| g.set(g.get().wrapping_add(1)));
}

/// How many times what the user's mailboxes show has changed — a render token
/// for views that cache their output.
pub(crate) fn own_faces_generation() -> u64 {
    OWN_FACES_GENERATION.with(Cell::get)
}

thread_local! {
    /// Own addresses whose Gravatar has been asked about this session. A miss
    /// or a network failure must not become a request on every render.
    static OWN_GRAVATAR_ASKED: RefCell<std::collections::HashSet<String>> =
        RefCell::new(std::collections::HashSet::new());
    /// Own Gravatars already scaled for a reader card, by address and pixels.
    static OWN_GRAVATAR_URIS: RefCell<HashMap<(String, i32), String>> = RefCell::new(HashMap::new());
}

/// The Gravatar `email` asked for, once it has been fetched and found (#189).
/// `None` covers "not looked up yet" and "there isn't one" alike: both mean
/// the account's picture, emoji or initials are what to draw.
pub(crate) fn own_gravatar(email: &str) -> Option<gtk::gdk::Texture> {
    let key = key(email);
    GRAVATAR_CACHE.with(|cache| cache.borrow().get(&key).cloned().flatten())
}

/// Whether `email`'s Gravatar still wants looking up: true at most once a
/// session per address, so a miss or a failure costs one request rather than
/// one per redraw.
pub(crate) fn wants_own_gravatar(email: &str) -> bool {
    let key = key(email);
    if GRAVATAR_CACHE.with(|cache| cache.borrow().contains_key(&key)) {
        return false;
    }
    OWN_GRAVATAR_ASKED.with(|asked| asked.borrow_mut().insert(key))
}

/// Look up one of the user's own Gravatars. Blocking — for a worker thread;
/// hand what comes back to [`cache_own_gravatar`] on the main thread.
pub(crate) fn fetch_own_gravatar(email: &str) -> FetchOutcome {
    fetch(email, FetchMode::GravatarOnly)
}

/// Store what [`fetch_own_gravatar`] found, and say whether it gave the
/// mailbox a face it did not have — which is when the views must redraw.
pub(crate) fn cache_own_gravatar(email: &str, outcome: FetchOutcome) -> bool {
    let found = matches!(outcome, FetchOutcome::Found(_));
    // A request that failed (no network yet at startup, say) settles nothing:
    // forget that it was asked, so the next thing that would ask — an account
    // saved, the switch moved, waking from sleep — asks again.
    if matches!(outcome, FetchOutcome::Retry) {
        OWN_GRAVATAR_ASKED.with(|asked| asked.borrow_mut().remove(&key(email)));
    }
    cache_result(
        email,
        crate::contacts::photo_generation(),
        FetchMode::GravatarOnly,
        outcome,
    );
    let key = key(email);
    OWN_GRAVATAR_URIS.with(|cache| cache.borrow_mut().retain(|(address, _), _| address != &key));
    if found {
        note_faces_changed();
    }
    found
}

/// That Gravatar as a `data:` PNG of `size` CSS pixels, for a reader card —
/// a document cannot be handed a texture. Scaled on the way in and kept, the
/// same way the account pictures beside it are.
pub(crate) fn own_gravatar_data_uri(email: &str, size: i32) -> Option<String> {
    // Twice the CSS size, as the account pictures are drawn: enough for a
    // HiDPI screen without carrying a full-size portrait per card.
    let px = (size * 2).max(1);
    let cache_key = (key(email), px);
    if let Some(hit) = OWN_GRAVATAR_URIS.with(|cache| cache.borrow().get(&cache_key).cloned()) {
        return Some(hit);
    }
    let texture = own_gravatar(email)?;
    let png = texture.save_to_png_bytes();
    let stream = gtk::gio::MemoryInputStream::from_bytes(&png);
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &stream,
        px,
        px,
        false,
        None::<&gtk::gio::Cancellable>,
    )
    .ok()?;
    let scaled = pixbuf.save_to_bufferv("png", &[]).ok()?;
    let uri = format!(
        "data:image/png;base64,{}",
        gtk::glib::base64_encode(&scaled)
    );
    OWN_GRAVATAR_URIS.with(|cache| {
        cache.borrow_mut().insert(cache_key, uri.clone());
    });
    Some(uri)
}

/// Consult the main-thread texture/miss caches without doing I/O.
pub(crate) fn lookup(email: &str, allow_gravatar: bool) -> CacheLookup {
    let key = key(email);
    let generation = crate::contacts::photo_generation();
    if !crate::contacts::photos_ready() {
        // Never disclose a sender hash to Gravatar before the local EDS index
        // has had a chance to answer.
        return CacheLookup::Fetch {
            generation,
            mode: FetchMode::ContactOnly,
        };
    }
    let contact = CONTACT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache
            .get(&key)
            .is_some_and(|cached| cached.generation != generation)
        {
            cache.remove(&key);
        }
        cache.get(&key).map(|cached| cached.texture.clone())
    });

    if let Some(Some(texture)) = contact {
        return CacheLookup::Texture(texture);
    }
    let gravatar = GRAVATAR_CACHE.with(|cache| cache.borrow().get(&key).cloned());
    if contact.is_some() {
        return match (allow_gravatar, gravatar) {
            (false, _) | (true, Some(None)) => CacheLookup::Missing,
            (true, Some(Some(texture))) => CacheLookup::Texture(texture),
            (true, None) => CacheLookup::Fetch {
                generation,
                mode: FetchMode::GravatarOnly,
            },
        };
    }

    // Contact status is unknown for this EDS generation. Check it first, but do
    // not refetch a Gravatar whose result is already cached.
    let mode = if allow_gravatar && gravatar.is_none() {
        FetchMode::ContactThenGravatar
    } else {
        FetchMode::ContactOnly
    };
    CacheLookup::Fetch { generation, mode }
}

fn gravatar_url(email: &str) -> String {
    use sha2::Digest as _;
    // SHA-256, not MD5: Gravatar accepts either, and an MD5 of an address is
    // trivially reversed by dictionary. d=404 → no image when the sender has no
    // Gravatar, so the caller can fall through to the next tier.
    let hash = sha2::Sha256::digest(key(email).as_bytes());
    format!("https://www.gravatar.com/avatar/{hash:x}?s=160&d=404")
}

/// Blocking sender-avatar lookup. The chain is local vCard PHOTO, optional
/// Gravatar, then the UI's initials fallback. Call off the main thread.
pub(crate) fn fetch(email: &str, mode: FetchMode) -> FetchOutcome {
    if mode != FetchMode::GravatarOnly {
        for bytes in crate::contacts::contact_photos(email) {
            // Decode/downscale on this background thread. A corrupt candidate
            // must not hide a valid duplicate or Gravatar fallback.
            if supported_raster(&bytes) {
                if let Some(image) = decode_image(&bytes) {
                    return FetchOutcome::Found(FetchedAvatar {
                        image,
                        source: AvatarSource::Contact,
                    });
                }
            }
        }
        if mode == FetchMode::ContactOnly {
            return FetchOutcome::Missing;
        }
    }

    match gravatar_image(email) {
        Ok(Some(image)) => FetchOutcome::Found(FetchedAvatar {
            image,
            source: AvatarSource::Gravatar,
        }),
        Ok(None) => FetchOutcome::Missing,
        Err(()) => FetchOutcome::Retry,
    }
}

/// Record a completed request. Contact results are stamped with the generation
/// that was actually queried; stale results are discarded rather than masking a
/// newly synchronized vCard photo.
pub(crate) fn cache_result(
    email: &str,
    generation: u64,
    mode: FetchMode,
    outcome: FetchOutcome,
) -> bool {
    let key = key(email);
    let generation_current = crate::contacts::photo_generation() == generation;
    match outcome {
        FetchOutcome::Found(fetched) => {
            let bytes = gtk::glib::Bytes::from(fetched.image.pixels.as_ref());
            let texture = Some(
                gtk::gdk::MemoryTexture::new(
                    fetched.image.width,
                    fetched.image.height,
                    gtk::gdk::MemoryFormat::R8g8b8a8,
                    &bytes,
                    fetched.image.width as usize * 4,
                )
                .upcast::<gtk::gdk::Texture>(),
            );
            match fetched.source {
                AvatarSource::Contact if generation_current => {
                    CONTACT_CACHE.with(|cache| {
                        cache.borrow_mut().insert(
                            key,
                            ContactCache {
                                generation,
                                texture,
                            },
                        );
                    });
                }
                AvatarSource::Gravatar => {
                    GRAVATAR_CACHE.with(|cache| {
                        cache.borrow_mut().insert(key.clone(), texture);
                    });
                    if mode == FetchMode::ContactThenGravatar && generation_current {
                        cache_missing_contact(&key, generation);
                    }
                }
                AvatarSource::Contact => {}
            }
        }
        FetchOutcome::Missing => {
            if mode != FetchMode::GravatarOnly && generation_current {
                cache_missing_contact(&key, generation);
            }
            if mode != FetchMode::ContactOnly {
                GRAVATAR_CACHE.with(|cache| {
                    cache.borrow_mut().insert(key, None);
                });
            }
        }
        FetchOutcome::Retry => {
            if mode == FetchMode::ContactThenGravatar && generation_current {
                cache_missing_contact(&key, generation);
            }
        }
    }
    !generation_current && mode != FetchMode::GravatarOnly
}

fn cache_missing_contact(email: &str, generation: u64) {
    CONTACT_CACHE.with(|cache| {
        cache.borrow_mut().insert(
            email.to_string(),
            ContactCache {
                generation,
                texture: None,
            },
        );
    });
}

// Raw Gravatar results coalesce duplicate rows for one sender and preserve a
// definitive 404 across list rebuilds. A transient failure is kept only for
// a short while (`RetryAfter`), so the rows asking at once share one try.
enum RawGravatar {
    Found(DecodedImage),
    Missing,
    RetryAfter(std::time::Instant),
}

type RawGravatarCache = HashMap<String, RawGravatar>;
static GRAVATAR_BYTES: OnceLock<Mutex<RawGravatarCache>> = OnceLock::new();
static GRAVATAR_EMAIL_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
static GRAVATAR_GATE: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
const MAX_GRAVATAR_REQUESTS: usize = 4;

/// The Gravatar results so far, locked; a poisoned lock is taken over, as
/// every lock here is.
fn gravatar_bytes() -> MutexGuard<'static, RawGravatarCache> {
    GRAVATAR_BYTES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

/// The count of Gravatar requests in flight, and the wake-up for a request
/// waiting for a slot.
fn gravatar_gate() -> &'static (Mutex<usize>, Condvar) {
    GRAVATAR_GATE.get_or_init(|| (Mutex::new(0), Condvar::new()))
}

struct GravatarSlot;

impl GravatarSlot {
    fn acquire() -> Self {
        let (active, wake) = gravatar_gate();
        let mut active = active.lock().unwrap_or_else(|p| p.into_inner());
        while *active >= MAX_GRAVATAR_REQUESTS {
            active = wake.wait(active).unwrap_or_else(|p| p.into_inner());
        }
        *active += 1;
        Self
    }
}

impl Drop for GravatarSlot {
    fn drop(&mut self) {
        let (active, wake) = gravatar_gate();
        let mut active = active.lock().unwrap_or_else(|p| p.into_inner());
        *active = active.saturating_sub(1);
        wake.notify_one();
    }
}

fn gravatar_image(email: &str) -> Result<Option<DecodedImage>, ()> {
    use std::io::Read;

    const MAX_BYTES: usize = 2_000_000;
    let key = key(email);
    let email_lock = {
        let mut locks = GRAVATAR_EMAIL_LOCKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _email_guard = email_lock.lock().unwrap_or_else(|p| p.into_inner());
    {
        let mut cache = gravatar_bytes();
        match cache.get(&key) {
            Some(RawGravatar::Found(bytes)) => return Ok(Some(bytes.clone())),
            Some(RawGravatar::Missing) => return Ok(None),
            Some(RawGravatar::RetryAfter(when)) if *when > std::time::Instant::now() => {
                return Err(());
            }
            Some(RawGravatar::RetryAfter(_)) => {
                cache.remove(&key);
            }
            None => {}
        }
    }

    // Offline or metered, a later bind asks again (#442).
    if !crate::desktop::power::fetches_ahead() {
        return Err(());
    }
    let _slot = GravatarSlot::acquire();
    let response = match ureq::get(&gravatar_url(email))
        .timeout(std::time::Duration::from_secs(10))
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(404, _)) => {
            gravatar_bytes().insert(key, RawGravatar::Missing);
            return Ok(None);
        }
        Err(_) => {
            cache_gravatar_retry(key);
            return Err(());
        }
    };
    let mut bytes = Vec::new();
    if response
        .into_reader()
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        cache_gravatar_retry(key);
        return Err(());
    }
    let image = if bytes.len() <= MAX_BYTES && supported_raster(&bytes) {
        decode_image(&bytes)
    } else {
        None
    };
    let Some(image) = image else {
        // Only a 404 is a definitive absence. A malformed 200 response may be a
        // proxy/captive-portal/CDN failure, so coalesce it behind a short retry.
        cache_gravatar_retry(key);
        return Err(());
    };
    gravatar_bytes().insert(key, RawGravatar::Found(image.clone()));
    Ok(Some(image))
}

fn cache_gravatar_retry(key: String) {
    gravatar_bytes().insert(
        key,
        RawGravatar::RetryAfter(std::time::Instant::now() + std::time::Duration::from_secs(30)),
    );
}

/// Only hand known raster formats to GdkPixbuf. In particular, an address-book
/// entry cannot smuggle an SVG with external references into the decoder.
fn supported_raster(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || bytes.starts_with(b"BM")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
}

/// The Contacts list's photo avatars, decoded once each for the life of the
/// app and keyed by [`crate::contacts::photo_key`]: iCloud keeps photos of a
/// megapixel or more, and decoding a few hundred of them on the main thread
/// held the page for seconds at every visit (#477). `None` is a photo that
/// would not decode.
static PHOTO_THUMBS: OnceLock<Mutex<HashMap<u64, Option<gtk::gdk::Texture>>>> = OnceLock::new();

/// Longest side of a list photo: a 34 px avatar at twice the scale, with room.
const PHOTO_THUMB_EDGE: i32 = 96;

/// The list photos decoded, and their pixel bytes, for the memory report.
pub(crate) fn photo_thumb_stats() -> (usize, u64) {
    let Some(Ok(map)) = PHOTO_THUMBS.get().map(Mutex::lock) else {
        return (0, 0);
    };
    (
        map.len(),
        map.values()
            .flatten()
            .map(crate::desktop::memory_report::texture_bytes)
            .sum(),
    )
}

/// A photo's list avatar, if [`decode_photo_thumbnails`] has been at it.
pub(crate) fn photo_thumbnail(key: u64) -> Option<Option<gtk::gdk::Texture>> {
    PHOTO_THUMBS.get()?.lock().ok()?.get(&key).cloned()
}

/// Decode the photos not cached yet, on every core, calling `progress` after
/// each few. Blocking: for a worker thread.
pub(crate) fn decode_photo_thumbnails(photos: Vec<(u64, Vec<u8>)>, progress: impl Fn() + Sync) {
    let cache = PHOTO_THUMBS.get_or_init(Default::default);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                let mut done = 0;
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((key, bytes)) = photos.get(i) else {
                        break;
                    };
                    if photo_thumbnail(*key).is_none() {
                        let texture = shrink_photo(bytes);
                        if let Ok(mut map) = cache.lock() {
                            map.insert(*key, texture);
                        }
                    }
                    done += 1;
                    if done % 16 == 0 {
                        progress();
                    }
                }
            });
        }
    });
    progress();
}

/// Forget the photos no longer in any address book.
pub(crate) fn retain_photo_thumbnails(keep: &std::collections::HashSet<u64>) {
    if let Some(Ok(mut map)) = PHOTO_THUMBS.get().map(Mutex::lock) {
        map.retain(|key, _| keep.contains(key));
    }
}

/// A photo at list-avatar size, cut square. GTK's own decoder takes a third
/// of the time GdkPixbuf's does on these photos; the nearest-pixel pass
/// first keeps the smoothing pass small.
fn shrink_photo(bytes: &[u8]) -> Option<gtk::gdk::Texture> {
    use gtk::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};

    if !supported_raster(bytes) {
        return None;
    }
    let full = gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from(bytes)).ok()?;
    let (width, height) = (full.width(), full.height());
    if width == height && width <= PHOTO_THUMB_EDGE {
        return Some(full);
    }
    let mut downloader = gtk::gdk::TextureDownloader::new(&full);
    downloader.set_format(gtk::gdk::MemoryFormat::R8g8b8a8);
    let (pixels, stride) = downloader.download_bytes();
    // The middle square: the list shows faces in a circle.
    let side = width.min(height);
    let mut pixbuf = Pixbuf::from_bytes(
        &pixels,
        Colorspace::Rgb,
        true,
        8,
        width,
        height,
        stride as i32,
    )
    .new_subpixbuf((width - side) / 2, (height - side) / 2, side, side);
    if side > PHOTO_THUMB_EDGE * 4 {
        pixbuf = pixbuf.scale_simple(
            PHOTO_THUMB_EDGE * 4,
            PHOTO_THUMB_EDGE * 4,
            InterpType::Nearest,
        )?;
    }
    let edge = side.min(PHOTO_THUMB_EDGE);
    let small = pixbuf.scale_simple(edge, edge, InterpType::Tiles)?;
    Some(
        gtk::gdk::MemoryTexture::new(
            edge,
            edge,
            gtk::gdk::MemoryFormat::R8g8b8a8,
            &small.read_pixel_bytes(),
            small.rowstride() as usize,
        )
        .upcast(),
    )
}

/// Decode through a size-prepared PixbufLoader so large source images are
/// downscaled during decoding. A pixel limit also rejects decompression bombs.
fn decode_image(bytes: &[u8]) -> Option<DecodedImage> {
    use gtk::gdk_pixbuf::prelude::*;

    const MAX_PIXELS: i64 = 4_194_304;
    const THUMBNAIL_EDGE: i32 = 160;
    let loader = gtk::gdk_pixbuf::PixbufLoader::new();
    let valid = Rc::new(Cell::new(false));
    loader.connect_size_prepared({
        let valid = valid.clone();
        move |loader, width, height| {
            if width <= 0 || height <= 0 || i64::from(width) * i64::from(height) > MAX_PIXELS {
                loader.set_size(1, 1);
                return;
            }
            valid.set(true);
            let longest = width.max(height);
            if longest > THUMBNAIL_EDGE {
                loader.set_size(
                    (width * THUMBNAIL_EDGE / longest).max(1),
                    (height * THUMBNAIL_EDGE / longest).max(1),
                );
            }
        }
    });
    if loader.write(bytes).is_err() {
        let _ = loader.close();
        return None;
    }
    if loader.close().is_err() || !valid.get() {
        return None;
    }
    let pixbuf = loader.pixbuf()?;
    if pixbuf.bits_per_sample() != 8 || pixbuf.n_channels() < 3 {
        return None;
    }
    let width = pixbuf.width();
    let height = pixbuf.height();
    let channels = pixbuf.n_channels() as usize;
    let rowstride = pixbuf.rowstride() as usize;
    let source = pixbuf.read_pixel_bytes();
    let source = source.as_ref();
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let offset = y * rowstride + x * channels;
            pixels.extend_from_slice(&source[offset..offset + 3]);
            pixels.push(if channels >= 4 {
                source[offset + 3]
            } else {
                255
            });
        }
    }
    Some(DecodedImage {
        pixels: pixels.into(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        account_face, decode_photo_thumbnails, own_face, own_faces_generation, photo_thumbnail,
        set_faces_on_own_mail, set_own_faces, supported_raster, OwnFace, PHOTO_THUMB_EDGE,
    };
    use gtk::prelude::TextureExt;

    /// A big photo comes back list-sized and square, for the list's round
    /// faces; one that will not decode is remembered as such, so it is not
    /// tried again (#477).
    #[test]
    fn contact_photos_are_shrunk_once_and_kept() {
        let (width, height) = (500, 300);
        let pixels = gtk::glib::Bytes::from_owned(vec![200u8; (width * height * 4) as usize]);
        let png = gtk::gdk::MemoryTexture::new(
            width,
            height,
            gtk::gdk::MemoryFormat::R8g8b8a8,
            &pixels,
            width as usize * 4,
        )
        .save_to_png_bytes()
        .to_vec();
        let svg = b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec();
        let (photo, broken) = (
            crate::contacts::photo_key(&png),
            crate::contacts::photo_key(&svg),
        );
        assert!(
            photo_thumbnail(photo).is_none(),
            "nothing is cached before it is decoded"
        );

        let reports = std::sync::atomic::AtomicUsize::new(0);
        decode_photo_thumbnails(vec![(photo, png), (broken, svg)], || {
            reports.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        assert!(
            reports.into_inner() >= 1,
            "the last photo is always reported"
        );
        let small = photo_thumbnail(photo).flatten().expect("decoded");
        assert_eq!(
            (small.width(), small.height()),
            (PHOTO_THUMB_EDGE, PHOTO_THUMB_EDGE),
            "cut square"
        );
        assert!(matches!(photo_thumbnail(broken), Some(None)));
    }

    #[test]
    fn accepts_common_rasters_but_not_svg() {
        assert!(supported_raster(b"\x89PNG\r\n\x1a\nrest"));
        assert!(supported_raster(b"\xff\xd8\xffrest"));
        assert!(!supported_raster(
            b"<svg xmlns='http://www.w3.org/2000/svg'/>"
        ));
    }

    /// The user's own addresses are matched however they were typed, and a
    /// map that did not change must not make every open document re-render.
    #[test]
    fn own_mailbox_faces_are_found_by_address() {
        let face = OwnFace {
            gravatar: false,
            picture: None,
            emoji: Some("\u{1F98A}".to_string()),
            color: "#e66100".to_string(),
        };
        set_own_faces([(" Ada@Example.COM ".to_string(), face.clone())]);
        assert_eq!(own_face("ada@example.com").as_ref(), Some(&face));
        assert_eq!(own_face("ADA@example.com").as_ref(), Some(&face));
        assert_eq!(own_face("grace@example.com"), None);

        let generation = own_faces_generation();
        set_own_faces([("ada@example.com".to_string(), face.clone())]);
        assert_eq!(
            own_faces_generation(),
            generation,
            "the same faces are not a change"
        );

        set_own_faces(std::iter::empty());
        assert_eq!(own_face("ada@example.com"), None);
        assert!(own_faces_generation() > generation, "clearing them is");
    }

    /// Turning the Settings switch off (#189) takes the mailbox's face off
    /// its mail — and leaves the account's own circle alone, which is what
    /// the sidebar and the account editor draw.
    #[test]
    fn faces_can_be_kept_off_your_own_mail() {
        let face = OwnFace {
            gravatar: true,
            picture: None,
            emoji: None,
            color: "#3584e4".to_string(),
        };
        assert!(
            face.is_set(),
            "asking for a Gravatar is asking for something"
        );
        set_own_faces([("ada@example.com".to_string(), face.clone())]);

        set_faces_on_own_mail(false);
        let generation = own_faces_generation();
        assert_eq!(
            own_face("ada@example.com"),
            None,
            "her mail is like anyone's"
        );
        assert_eq!(
            account_face("ada@example.com").as_ref(),
            Some(&face),
            "her account still shows what it was given"
        );

        set_faces_on_own_mail(false);
        assert_eq!(own_faces_generation(), generation, "no change, no redraw");
        set_faces_on_own_mail(true);
        assert_eq!(own_face("ada@example.com").as_ref(), Some(&face));
        assert!(
            own_faces_generation() > generation,
            "back on, draw it again"
        );

        set_own_faces(std::iter::empty());
    }
}
