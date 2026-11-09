//! Turning fetched bytes into a texture, and remembering the answer (or
//! the miss) for the domain.

use super::bundled::{bundled_entry, looks_like_svg, square_svg};
use super::{bimi_path, domain_of, img_path, miss_path, CACHE, LOGO_PX, MISSES};

/// Decode icon bytes into a texture and cache them under the sender's domain.
///
/// `GdkTexture` reads PNG and JPEG; an `.ico` needs GdkPixbuf, which the platform
/// supplies loaders for. Failing to decode is remembered as a miss, so a domain
/// serving something unreadable is not asked on every row.
pub(crate) fn decode_and_cache(email: &str, bytes: &[u8]) -> Option<gtk::gdk::Texture> {
    let domain = domain_of(email)?;
    let tex = decode(bytes);
    match tex {
        Some(tex) => {
            CACHE.with(|c| {
                c.borrow_mut().insert(domain, tex.clone());
            });
            Some(tex)
        }
        None => {
            // Undecodable bytes: drop the stored copies, and — unless a
            // bundled mark stands in — persist the miss so the next
            // session doesn't fetch and fail to decode them again.
            if let Some(p) = img_path(&domain) {
                let _ = std::fs::remove_file(p);
            }
            if let Some(p) = bimi_path(&domain) {
                let _ = std::fs::write(p, b"");
            }
            if bundled_entry(email).is_none() {
                if let Some(p) = miss_path(&domain) {
                    let _ = std::fs::write(p, b"");
                }
                remember_miss(&domain);
            }
            None
        }
    }
}

/// Remember that a sender's domain has no usable icon.
pub(crate) fn remember_missing(email: &str) {
    // Not looked up at all, offline or metered (#442).
    if !crate::desktop::power::fetches_ahead() {
        return;
    }
    if let Some(domain) = domain_of(email) {
        remember_miss(&domain);
    }
}

fn remember_miss(domain: &str) {
    MISSES.with(|m| {
        m.borrow_mut().insert(domain.to_string());
    });
}

pub(super) fn decode(bytes: &[u8]) -> Option<gtk::gdk::Texture> {
    use gtk::gdk_pixbuf::prelude::*;
    use std::cell::Cell;
    use std::rc::Rc;

    if looks_like_svg(bytes) {
        return decode_svg(bytes);
    }

    // These textures live in the session-long cache above and are drawn at
    // avatar size, but sites publish `apple-touch-icon`s at up to 1024² — a
    // few MB of decoded pixels each, held forever per domain (issue #106).
    // Downscale during decode, exactly as avatars do; the pixel limit also
    // rejects decompression bombs. Going through a size-prepared PixbufLoader
    // covers PNG, JPEG and ICO alike.
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
    loader.close().ok()?;
    if !valid.get() {
        return None;
    }
    let pixbuf = loader.pixbuf()?;
    Some(gtk::gdk::Texture::for_pixbuf(&pixbuf))
}

/// An SVG rasterised at logo size: framed to a `LOGO_PX` square first, so
/// the renderer has a definite size whatever the document declares (a
/// bare `viewBox`, `100%`, or a nominal 16px), then loaded as a texture.
fn decode_svg(bytes: &[u8]) -> Option<gtk::gdk::Texture> {
    if bytes.len() > 512 * 1024 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let framed = square_svg(text, LOGO_PX, None, 0.0, None)?;
    let data = gtk::glib::Bytes::from_owned(framed.into_bytes());
    match gtk::gdk::Texture::from_bytes(&data) {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::debug!("logo svg not rendered: {e}");
            None
        }
    }
}
