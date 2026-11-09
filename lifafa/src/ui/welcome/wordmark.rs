//! The wordmark art, shared with the About window and the carry-over
//! window, and its size on the wizard's carousel.

use super::*;

/// Wordmark art, embedded so the wizard needs nothing on disk. A 1024px
/// PNG master rather than the SVG the website uses: neither librsvg nor
/// gdk-pixbuf draws the mark's soft shadow the way a browser does, and a
/// raster export of the same artwork is the one thing both agree on.
const WORDMARK_PNG: &[u8] = include_bytes!("../../../data/welcome/wordmark-black.png");

/// The same wordmark with white lettering, for a dark ground.
const WORDMARK_DARK_PNG: &[u8] = include_bytes!("../../../data/welcome/wordmark-white.png");

/// The wordmark with the app icon beside it, for the About window and the
/// wizard: "Hylki" set in Cantarell, dark or white, as an SVG on a 128x42
/// box (the pixbuf loader tells the formats apart by their bytes). The
/// wizard always takes the white one, on its blue ground.
const ABOUT_WORDMARK_SVG: &[u8] = include_bytes!("../../../data/about/wordmark-black.svg");

const ABOUT_WORDMARK_DARK_SVG: &[u8] = include_bytes!("../../../data/about/wordmark-white.svg");

/// Which wordmark art a picture shows.
#[derive(Clone, Copy)]
pub(crate) enum Wordmark {
    /// The script lettering alone (the carry-over notice).
    Plain,
    /// The Cantarell lettering with the app icon beside it, dark or white
    /// by scheme (the About window).
    WithIcon,
    /// The same art, always with white lettering (the wizard's floating
    /// wordmark, on the blue ground).
    WithIconWhite,
}

impl Wordmark {
    fn art(self, dark: bool) -> &'static [u8] {
        match (self, dark) {
            (Wordmark::Plain, false) => WORDMARK_PNG,
            (Wordmark::Plain, true) => WORDMARK_DARK_PNG,
            (Wordmark::WithIcon, false) => ABOUT_WORDMARK_SVG,
            (Wordmark::WithIcon, true) | (Wordmark::WithIconWhite, _) => ABOUT_WORDMARK_DARK_SVG,
        }
    }

    /// The art's aspect (height over width).
    fn aspect(self) -> f64 {
        match self {
            Wordmark::Plain => 293.0 / 1024.0,
            Wordmark::WithIcon | Wordmark::WithIconWhite => 42.0 / 128.0,
        }
    }
}

/// Render the script lettering at 2x for crisp HiDPI, displayed at
/// `width` px.
pub(crate) fn wordmark_picture(width: i32) -> gtk::Picture {
    wordmark_picture_of(Wordmark::Plain, width)
}

/// Render the given wordmark art at 2x for crisp HiDPI, displayed at
/// `width` px.
pub(crate) fn wordmark_picture_of(art: Wordmark, width: i32) -> gtk::Picture {
    let pic = gtk::Picture::new();
    pic.set_can_shrink(true);
    pic.set_content_fit(gtk::ContentFit::Contain);
    // Black lettering on a light ground, white on a dark one; follows the
    // style manager while the window is open.
    let style = adw::StyleManager::default();
    let apply = move |pic: &gtk::Picture, dark: bool| {
        if let Some(tex) = wordmark_texture(art, width, dark) {
            pic.set_paintable(Some(&tex));
        }
    };
    apply(&pic, style.is_dark());
    let weak = pic.downgrade();
    style.connect_dark_notify(move |s| {
        if let Some(pic) = weak.upgrade() {
            apply(&pic, s.is_dark());
        }
    });
    pic
}

/// The wordmark rendered at 2x `width` for crisp HiDPI.
fn wordmark_texture(art: Wordmark, width: i32, dark: bool) -> Option<gtk::gdk::Texture> {
    let loader = gtk::gdk_pixbuf::PixbufLoader::new();
    loader.connect_size_prepared(move |l, w, h| {
        let scale = (width * 2) as f64 / w.max(1) as f64;
        l.set_size(width * 2, (h as f64 * scale) as i32);
    });
    loader.write(art.art(dark)).ok()?;
    loader.close().ok()?;
    loader.pixbuf().map(|pb| gtk::gdk::Texture::for_pixbuf(&pb))
}

/// Hero and shrunk geometry for the floating wordmark.
pub(super) const HERO_TOP: i32 = 150;

pub(super) const HERO_SIZE: f64 = 240.0;

const SMALL_TOP: f64 = 6.0;

const SMALL_BOTTOM: f64 = 16.0;

const SMALL_SIZE: f64 = 128.0;

/// The script lettering's height for a given width.
pub(crate) fn wordmark_height(width: f64) -> i32 {
    wordmark_height_of(Wordmark::Plain, width)
}

/// A wordmark's height for a given width.
pub(crate) fn wordmark_height_of(art: Wordmark, width: f64) -> i32 {
    (width * art.aspect()) as i32
}

/// Tie the wordmark's size and lift to the carousel's live position: while
/// the spring runs from the hero to page 2, the wordmark shrinks and rises in
/// lockstep (a shared-element move — no second timeline to fall out of sync),
/// and it parks small for every later page.
pub(super) fn bind_wordmark_to_position(
    carousel: &adw::Carousel,
    frame: &gtk::Box,
    holder: &gtk::Box,
) {
    let frame = frame.clone();
    let holder = holder.clone();
    carousel.connect_position_notify(move |car| {
        let p = car.position().clamp(0.0, 1.0);
        let s = HERO_SIZE + (SMALL_SIZE - HERO_SIZE) * p;
        frame.set_size_request(s as i32, wordmark_height_of(Wordmark::WithIconWhite, s));
        holder.set_margin_top((HERO_TOP as f64 + (SMALL_TOP - HERO_TOP as f64) * p) as i32);
        // The small form also keeps 16px of air below itself; the hero's
        // spacing is the tagline's own margin, so this fades in with p.
        holder.set_margin_bottom((SMALL_BOTTOM * p) as i32);
    });
}
