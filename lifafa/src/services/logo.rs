//! Sender logos (opt-in): the brand's logo in place of colored initials, so
//! mail from Apple, Amazon or PayPal is recognisable at a glance (#30).
//!
//! Three sources, best first:
//!
//! 1. **BIMI** — the logo the sender publishes for mail clients: a DNS TXT
//!    record at `default._bimi.<domain>` naming an SVG on the sender's own
//!    site (RFC draft "Brand Indicators for Message Identification"). Vector,
//!    authoritative, and what Apple Mail and Gmail show. The sending host is
//!    asked first, then the registrable domain.
//! 2. **Bundled** — `data/logos/`: a curated map of sender domains to marks
//!    from gilbarbara/logos (full color) and Simple Icons (a glyph on the
//!    brand color), plus the app's own service marks (`data/brands/`).
//!    Shipped in the binary, so these show with no request at all.
//! 3. **The site's own icon**, the largest it declares first: the icons its
//!    home page links (`<link rel="icon" sizes="192x192">`,
//!    `apple-touch-icon`, an SVG icon) and the ones in its web manifest —
//!    where the 512px icons usually live — then the well-known paths,
//!    `apple-touch-icon.png` (180px) and `favicon.ico` (16–48px, the last
//!    resort).
//!
//! Addresses at a **mailbox host** (Gmail, Outlook, iCloud, Yahoo, Proton and
//! the rest of `MAILBOX_HOSTS`) are skipped entirely, whichever source might
//! have answered: such an address belongs to a person, not to the provider, so
//! it keeps its colored initials rather than wearing the provider's mark.
//!
//! No third-party service is involved and no per-user identifier is sent,
//! but the BIMI and site requests do tell that domain your IP address —
//! which is exactly what blocking remote content avoids. So this is off by
//! default and gated behind a Preferences switch, as Gravatar is.
//!
//! One fetch per domain per session, remembered either way: a miss is cached too,
//! or every row from the same sender would ask again.
//!
//! Icons persist on disk (`~/.local/share/hylki/logos/<domain>.img`) so a
//! restart shows them without touching the network; a `.miss` marker remembers
//! a domain with nothing to give. Both go stale after a week: the next message
//! from that sender then re-asks the domain — a changed brand icon appears, a
//! domain that gained one is picked up — while the stale icon keeps showing in
//! the meantime.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

mod bimi;
mod bundled;
mod decode;
mod mailbox;
mod site;

use bimi::{bimi_logo, Bimi};
use bundled::{bundled_bytes, bundled_entry};
use decode::decode;
pub(crate) use decode::{decode_and_cache, remember_missing};
pub(crate) use mailbox::is_mailbox_domain;
use mailbox::is_mailbox_host;
use site::candidate_urls;
use site::get;
pub(crate) use site::USER_AGENT;

/// The edge every logo is decoded to: drawn at avatar size, kept per
/// domain for the session (issue #106), and what vector sources are
/// rasterised at.
const LOGO_PX: u32 = 160;

/// A BIMI logo is a small SVG (the spec asks for 32KB); anything bigger
/// is not one.
const BIMI_MAX_BYTES: u64 = 64 * 1024;

thread_local! {
    static CACHE: RefCell<HashMap<String, gtk::gdk::Texture>> = RefCell::new(HashMap::new());
    /// Domains with no usable icon, so they are asked once and not again.
    static MISSES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// Domains whose weekly refresh has already been kicked off this session.
    static REFRESHED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// Domains whose stored files were looked at for that this session: a
    /// list row asks each time it is filled, and the answer stands.
    static CHECKED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// The session's logo cache: domains with a texture, their pixel bytes, and
/// domains remembered as having none. For the memory section of an export.
pub(crate) fn cache_stats() -> (usize, u64, usize) {
    let (n, bytes) = CACHE.with(|c| {
        let c = c.borrow();
        (
            c.len(),
            c.values()
                .map(crate::desktop::memory_report::texture_bytes)
                .sum(),
        )
    });
    (n, bytes, MISSES.with(|m| m.borrow().len()))
}

/// How long a stored icon (or miss) is trusted before the domain is re-asked.
const REFRESH_AFTER: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

fn store_dir() -> Option<PathBuf> {
    // Made once: this is asked for every list row filled.
    static DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = crate::config::data_base()?.join("hylki").join("logos");
        let _ = std::fs::create_dir_all(&dir);
        Some(dir)
    })
    .clone()
}

fn img_path(domain: &str) -> Option<PathBuf> {
    Some(store_dir()?.join(format!("{domain}.img")))
}

fn miss_path(domain: &str) -> Option<PathBuf> {
    Some(store_dir()?.join(format!("{domain}.miss")))
}

/// The sender's BIMI logo when it has one (the SVG bytes), or an empty
/// marker saying it was asked within the week and has none. Kept apart
/// from the site icon (`.img`) so a bundled mark can outrank a stored
/// favicon without outranking a stored BIMI logo.
fn bimi_path(domain: &str) -> Option<PathBuf> {
    Some(store_dir()?.join(format!("{domain}.bimi")))
}

fn stored_bimi(domain: &str) -> Option<Vec<u8>> {
    let p = bimi_path(domain)?;
    let bytes = std::fs::read(p).ok()?;
    (!bytes.is_empty()).then_some(bytes)
}

/// The host part of an address, lowercased — the sending host itself
/// (`notifications.usbank.com`), as BIMI is looked up on it first.
fn host_of(email: &str) -> Option<String> {
    let host = email
        .rsplit('@')
        .next()?
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() || !host.contains('.') || host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    Some(host)
}

/// Whether the file at `path` exists and was written within [`REFRESH_AFTER`].
fn fresh(path: &PathBuf) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < REFRESH_AFTER)
}

/// The registrable domain of an email address: the part that owns the brand.
///
/// Mail comes from `em6917.cloudways.com` or `mail.notifications.apple.com`, and
/// the icon lives at the domain those hang off. Two labels, or three when the
/// last two are a country-code pair like `co.uk`, which is a heuristic rather
/// than the public suffix list — close enough to point a favicon request at, and
/// wrong only for a handful of unusual suffixes.
pub(crate) fn domain_of(email: &str) -> Option<String> {
    let host = email.rsplit('@').next()?.trim().trim_end_matches('.');
    let host = host.to_ascii_lowercase();
    if host.is_empty() || !host.contains('.') || host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    if labels.len() < 2 {
        return None;
    }
    let tail = &labels[labels.len().saturating_sub(2)..];
    let take = if labels.len() > 2 && tail[0].len() <= 3 && tail[1].len() == 2 {
        3
    } else {
        2
    };
    Some(labels[labels.len() - take.min(labels.len())..].join("."))
}

/// A previously decoded logo for this sender's domain (main thread only).
/// Falls back to the on-disk copy — however old — so a restart shows icons
/// without a single network request; [`wants_refresh`] handles staleness.
pub(crate) fn cached(email: &str) -> Option<gtk::gdk::Texture> {
    if is_mailbox_host(email) {
        return None;
    }
    let domain = domain_of(email)?;
    if let Some(tex) = CACHE.with(|c| c.borrow().get(&domain).cloned()) {
        return Some(tex);
    }
    if MISSES.with(|m| m.borrow().contains(&domain)) {
        return None;
    }
    // The stored BIMI logo, else a bundled mark, else the stored site icon.
    let bytes = stored_bimi(&domain)
        .or_else(|| bundled_entry(email).and_then(bundled_bytes))
        .or_else(|| img_path(&domain).and_then(|p| std::fs::read(p).ok()))?;
    let tex = decode(&bytes)?;
    CACHE.with(|c| {
        c.borrow_mut().insert(domain, tex.clone());
    });
    Some(tex)
}

/// Whether the stored icon for this sender's domain is a week old — time to
/// look again in the background while the old one keeps showing. Says yes at
/// most once a session per domain, so a screenful of rows from one sender
/// doesn't fan out into a fetch per row.
pub(crate) fn wants_refresh(email: &str) -> bool {
    // Spent offline, the session's one look would find nothing (#442).
    if is_mailbox_host(email) || !crate::desktop::power::fetches_ahead() {
        return false;
    }
    let Some(domain) = domain_of(email) else {
        return false;
    };
    if !CHECKED.with(|c| c.borrow_mut().insert(domain.clone())) {
        return false;
    }
    // Due when nothing on disk is within the week: a stale site icon, a
    // stale BIMI answer, or a bundled mark whose sender has never been
    // asked for a BIMI logo at all.
    let bimi_fresh = bimi_path(&domain).is_some_and(|p| fresh(&p));
    let img_fresh = img_path(&domain).is_some_and(|p| fresh(&p));
    let due = !bimi_fresh && !img_fresh;
    due && REFRESHED.with(|r| r.borrow_mut().insert(domain))
}

/// Whether this domain has already been asked about and had nothing to give.
/// A miss remembered on disk expires after a week, so a domain that gains an
/// icon is eventually found.
pub(crate) fn known_missing(email: &str) -> bool {
    // Nothing will ever be looked up for a person's own mailbox.
    if is_mailbox_host(email) {
        return true;
    }
    if bundled_entry(email).is_some() {
        return false;
    }
    match domain_of(email) {
        Some(domain) => {
            if MISSES.with(|m| m.borrow().contains(&domain)) {
                return true;
            }
            // Remembered for the session once read, as a miss found now is.
            let missing = miss_path(&domain).is_some_and(|p| fresh(&p));
            if missing {
                MISSES.with(|m| m.borrow_mut().insert(domain));
            }
            missing
        }
        // Nothing to look up counts as answered.
        None => true,
    }
}

/// Blocking fetch of a domain's icon. Call off the main thread.
///
/// A fresh on-disk copy answers without touching the network; otherwise the
/// domain is asked and the answer stored — icon or miss. When the network
/// fails with a stale copy in hand, the stale copy stands (and stays due for
/// refresh, so it is retried later).
pub(crate) fn fetch(email: &str) -> Option<Vec<u8>> {
    fetch_from(email).map(|(bytes, _)| bytes)
}

/// What [`fetch`] would answer for an address and where from, in words —
/// for the `HYLKI_LOGO_PROBE` hook.
pub(crate) fn probe(email: &str) -> String {
    match fetch_from(email) {
        Some((bytes, source)) => format!("{source} ({} bytes)", bytes.len()),
        None => "nothing".to_string(),
    }
}

/// The fetch, with the source it answered from: the stored BIMI logo, a
/// fresh BIMI lookup, a bundled mark, a stored or fetched site icon.
fn fetch_from(email: &str) -> Option<(Vec<u8>, String)> {
    if is_mailbox_host(email) {
        return None;
    }
    let domain = domain_of(email)?;
    let img = img_path(&domain);
    let bimi = bimi_path(&domain);
    let bimi_fresh = bimi.as_ref().is_some_and(fresh);
    // Offline or metered, only what is on disk answers, and no answer is
    // stored: a miss now says nothing about the domain (#442).
    let online = crate::desktop::power::fetches_ahead();
    if bimi_fresh || !online {
        if let Some(bytes) = stored_bimi(&domain) {
            return Some((bytes, "stored BIMI logo".into()));
        }
    } else {
        // Asked at most once a week: a hit is kept as the logo, a
        // confirmed miss as an empty marker. A lookup that could not be
        // made (no network) leaves no marker, so it is tried again.
        match bimi_logo(email) {
            Bimi::Logo(bytes) => {
                if let Some(p) = bimi.as_ref() {
                    let _ = std::fs::write(p, &bytes);
                }
                if let Some(p) = miss_path(&domain) {
                    let _ = std::fs::remove_file(p);
                }
                return Some((bytes, "BIMI logo".into()));
            }
            Bimi::None => {
                if let Some(p) = bimi.as_ref() {
                    let _ = std::fs::write(p, b"");
                }
            }
            Bimi::Unreachable => {}
        }
    }
    if let Some(entry) = bundled_entry(email) {
        if let Some(bytes) = bundled_bytes(entry) {
            if let Some(p) = miss_path(&domain) {
                let _ = std::fs::remove_file(p);
            }
            return Some((bytes, format!("bundled {}:{}", entry.source, entry.file)));
        }
    }
    if let Some(p) = img.as_ref().filter(|p| fresh(p)) {
        if let Ok(bytes) = std::fs::read(p) {
            return Some((bytes, "stored site icon".into()));
        }
    }
    if miss_path(&domain).is_some_and(|p| fresh(&p)) {
        return None;
    }
    if !online {
        let stale = img.as_ref().and_then(|p| std::fs::read(p).ok());
        return stale.map(|b| (b, "stale site icon (no network)".into()));
    }
    for url in candidate_urls(&domain) {
        if let Some(bytes) = get(&url) {
            if let Some(p) = img.as_ref() {
                let _ = std::fs::write(p, &bytes);
            }
            if let Some(p) = miss_path(&domain) {
                let _ = std::fs::remove_file(p);
            }
            return Some((bytes, format!("site icon {url}")));
        }
    }
    if let Some(p) = img.as_ref().filter(|p| p.exists()) {
        // Nothing new, but yesterday's icon beats initials.
        return std::fs::read(p).ok().map(|b| (b, "stale site icon".into()));
    }
    if let Some(p) = miss_path(&domain) {
        let _ = std::fs::write(p, b"");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brand_domain_is_found_under_its_sending_subdomains() {
        assert_eq!(
            domain_of("no-reply@apple.com").as_deref(),
            Some("apple.com")
        );
        assert_eq!(
            domain_of("news@mail.notifications.apple.com").as_deref(),
            Some("apple.com")
        );
        assert_eq!(
            domain_of("bounces+42933322-387e@em6917.cloudways.com").as_deref(),
            Some("cloudways.com")
        );
        // Country-code pairs keep three labels.
        assert_eq!(domain_of("a@mail.bbc.co.uk").as_deref(), Some("bbc.co.uk"));
        assert_eq!(
            domain_of("a@shop.example.com.au").as_deref(),
            Some("example.com.au")
        );
    }

    #[test]
    fn addresses_with_no_domain_to_ask_are_skipped() {
        assert_eq!(domain_of(""), None);
        assert_eq!(domain_of("someone"), None);
        assert_eq!(domain_of("someone@localhost"), None);
        // An IP literal is nobody's brand.
        assert_eq!(domain_of("a@192.168.1.1"), None);
        // Nothing to look up is treated as already answered, so no fetch is made.
        assert!(known_missing("someone"));
    }
}
