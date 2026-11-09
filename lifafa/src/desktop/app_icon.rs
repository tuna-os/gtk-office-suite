//! The app icon the user chose, and how it reaches the desktop.
//!
//! The app ships one icon (the opened envelope, `data/icons/src/default.svg`)
//! and carries a gallery of alternative envelopes inside the binary. The
//! beta ships the default's `.Devel` twin, GNOME's development-build
//! styling (the hazard stripe). A choice is applied by writing that artwork
//! over the app's icon name in the user's own icon directory
//! (`~/.local/share/icons/hicolor`), which every desktop searches before
//! the install's — the Flatpak export included — so the dock, app grid and
//! switcher pick it up under the same name. "Default" removes the override
//! and whatever the build installed shows again. Fresh installs pick in the
//! welcome wizard.
//!
//! The tray icon and the in-app uses draw the same choice, so it needs no
//! restart; the window's own icon (X11 fallback, some panels) does, and a
//! restart is offered — see [`launch_restart_helper`].

use std::path::PathBuf;

use crate::i18n::i18n_noop;

mod launcher;
mod restart;

use launcher::Launcher;
pub(crate) use launcher::{launcher_settle_remaining, sync_mime_cache};
pub(crate) use restart::{launch_restart_helper, run_restart_helper, RESTART_FLAG};

/// One gallery entry: the id stored in settings, its label, and the art.
pub(crate) struct IconChoice {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    png: &'static [u8],
}

/// The id meaning "whatever this build ships".
pub(crate) const DEFAULT_ID: &str = "default";
/// Bumped when a release's new default icon is to replace every existing
/// choice once (1.23's blue bird envelope was generation 1; the Hylki
/// envelope of 1.35, the first release under the new name, was 2; the
/// opened envelope of 1.40 is 3): the first start on such a release resets
/// the stored choice to the default, and records the generation so a
/// choice made afterwards stands.
const ICON_GENERATION: u32 = 3;

/// The icon this build installs under its app ID.
#[cfg(not(feature = "beta"))]
const DEFAULT_PNG: &[u8] =
    include_bytes!("../../data/icons/hicolor/512x512/apps/co.hyprlab.Hylki.png");
#[cfg(feature = "beta")]
const DEFAULT_PNG: &[u8] =
    include_bytes!("../../data/icons/hicolor/512x512/apps/co.hyprlab.Hylki.Beta.png");

/// The gallery, in display order: the build's own icon (the opened
/// envelope, so it has no entry of its own), the same envelope on a blue,
/// a navy and a yellow square, and the classic icon last. It was eighteen
/// entries until #277 asked for one look rather than every one the app had
/// worn. The labels are written out rather than passed through a macro,
/// where the translation template's extractor never saw them.
const CATALOG: &[IconChoice] = &[
    IconChoice {
        id: DEFAULT_ID,
        label: i18n_noop("Default"),
        png: DEFAULT_PNG,
    },
    IconChoice {
        id: "square",
        label: i18n_noop("Blue square"),
        png: include_bytes!("../../data/icons/alt/square.png"),
    },
    IconChoice {
        id: "square-dark",
        label: i18n_noop("Navy square"),
        png: include_bytes!("../../data/icons/alt/square-dark.png"),
    },
    IconChoice {
        id: "square-yellow",
        label: i18n_noop("Yellow square"),
        png: include_bytes!("../../data/icons/alt/square-yellow.png"),
    },
    IconChoice {
        id: "classic",
        label: i18n_noop("Classic"),
        png: include_bytes!("../../data/icons/alt/classic.png"),
    },
];

/// Every choice the gallery offers.
pub(crate) fn catalog() -> impl Iterator<Item = &'static IconChoice> {
    CATALOG.iter()
}

/// Normalise a stored id to one this build offers. A squircle becomes the
/// square of its color, the nearest thing left (#277); every other retired
/// id (the envelopes, the birds, the patterns) falls back to the default.
fn effective(id: &str) -> &'static str {
    let id = match id {
        "squircle" => "square",
        "squircle-dark" => "square-dark",
        "squircle-yellow" => "square-yellow",
        other => other,
    };
    catalog()
        .find(|c| c.id == id)
        .map(|c| c.id)
        .unwrap_or(DEFAULT_ID)
}

/// The artwork for an id (the default's for an unknown one).
pub(crate) fn png_for(id: &str) -> &'static [u8] {
    let id = effective(id);
    CATALOG
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.png)
        .unwrap_or(DEFAULT_PNG)
}

/// The image the tray's App icon option draws: the icon the dock shows.
/// That is the launcher's own icon when somebody set one outside Hylki
/// (#252), a file or a name in the icon theme, and otherwise the gallery
/// choice. An icon that cannot be read falls back to the choice.
pub(crate) fn tray_image(id: &str) -> std::borrow::Cow<'static, [u8]> {
    use std::borrow::Cow;
    let Some(icon) = custom_icon() else {
        return Cow::Borrowed(png_for(id));
    };
    let path = if std::path::Path::new(&icon).is_absolute() {
        Some(std::path::PathBuf::from(&icon))
    } else {
        // In the desktop's icon folders: Hylki's own search leaves them out.
        crate::desktop::icon_theme::desktop_icon_theme()
            .lookup_icon(
                &icon,
                &[],
                256,
                1,
                gtk::TextDirection::None,
                gtk::IconLookupFlags::empty(),
            )
            .file()
            .and_then(|f| gtk::gio::prelude::FileExt::path(&f))
    };
    match path.and_then(|p| std::fs::read(p).ok()) {
        Some(bytes) => Cow::Owned(bytes),
        None => Cow::Borrowed(png_for(id)),
    }
}

/// The choice in force, settling it on the first start that finds none
/// (the default; the wizard lets a fresh install pick). The override on
/// disk is brought in line either way, so a reinstall (or a changed
/// default) never silently swaps the icon someone chose, unless the
/// launcher carries an icon set outside Hylki: that one is left alone
/// until a pick in the app replaces it (#252).
pub(crate) fn init_on_startup() -> String {
    if std::env::var("HYLKI_DEMO").is_ok() {
        return DEFAULT_ID.to_string();
    }
    let id = if crate::config::load_app_icon_generation() < ICON_GENERATION {
        // A release with a new authoritative icon: everyone gets it once,
        // over whatever the previous install had chosen or kept.
        crate::config::save_app_icon(DEFAULT_ID);
        crate::config::save_app_icon_generation(ICON_GENERATION);
        DEFAULT_ID.to_string()
    } else {
        match crate::config::load_app_icon() {
            Some(id) => id,
            None => {
                crate::config::save_app_icon(DEFAULT_ID);
                DEFAULT_ID.to_string()
            }
        }
    };
    let id = effective(&id).to_string();
    apply(&id, false);
    id
}

/// Persist a choice and put it on the desktop, over an icon set outside
/// Hylki too: this is the user picking one in the app.
pub(crate) fn set(id: &str) -> String {
    let id = effective(id).to_string();
    crate::config::save_app_icon(&id);
    if std::env::var("HYLKI_DEMO").is_err() {
        apply(&id, true);
    }
    id
}

/// The icon the app's launcher names when somebody other than Hylki put it
/// there: a menu editor, a hand edit, an icon theme's name (#252). Hylki
/// only ever writes the app's own icon name or the file of a gallery
/// choice, so anything else was the user's doing.
pub(crate) fn custom_icon() -> Option<String> {
    if std::env::var("HYLKI_DEMO").is_ok() {
        return None;
    }
    Launcher::find().custom_icon()
}

/// Whether a launcher's `Icon=` value is one Hylki puts there: the name the
/// build installs its icon under, or the file a choice is written to.
fn icon_is_ours(value: &str) -> bool {
    if value == crate::APP_ID {
        return true;
    }
    let path = std::path::Path::new(value);
    let prefix = format!("{}-", crate::APP_ID);
    path.is_absolute()
        && path
            .file_name()
            .map(|n| n.to_string_lossy())
            .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".png"))
}

/// The icon name a choice is written under: `<APP_ID>-<choice>`, the
/// default included. A fresh name per choice is deliberate — GNOME Shell
/// caches app icons by name for the life of the session and never re-reads
/// a file it has already shown, so swapping the art under one name shows
/// nothing until the next login.
fn icon_name(id: &str) -> String {
    format!("{}-{id}", crate::APP_ID)
}

/// The user's share directory as the *desktop* sees it. Inside Flatpak
/// XDG_DATA_HOME is the sandbox's private dir, so the host's default is
/// used (that is where the manifest mounts `xdg-data/icons/hicolor` and
/// `xdg-data/applications`).
fn host_data_home() -> Option<PathBuf> {
    if !crate::desktop::platform::is_flatpak() {
        if let Some(d) = dirs::data_dir() {
            return Some(d);
        }
    }
    Some(crate::desktop::platform::home()?.join(".local/share"))
}

fn hicolor_dir() -> Option<PathBuf> {
    Some(host_data_home()?.join("icons/hicolor"))
}

/// An icon folder only Hylki writes to, which its own icon lookups search in
/// place of the shared ones (`icon_theme::apply_icon_theme`): the chosen app
/// icon is copied here so the window finds it by name.
pub(crate) fn private_icons_root() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("hylki/icons"))
}

/// Keep the private folder's copy of the chosen icon, and nothing else of
/// ours there.
fn sync_private_icon(id: &str, name: &str, write: bool) {
    let Some(dir) = private_icons_root().map(|r| r.join("hicolor/512x512/apps")) else {
        return;
    };
    let prefix = format!("{}-", crate::APP_ID);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with(&prefix) && fname != format!("{name}.png") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    if !write {
        return;
    }
    let path = dir.join(format!("{name}.png"));
    if std::fs::read(&path).is_ok_and(|cur| cur == png_for(id)) {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, png_for(id)))
    {
        tracing::warn!("could not keep the app icon in {}: {e}", dir.display());
    }
}

/// The icon sizes the install ships — every one is written under the
/// chosen name, so whichever a desktop asks for resolves to the choice.
const SIZES: [i32; 2] = [512, 256];

/// Put a choice on the desktop: the art under its own name in the user's
/// icon directory, and the launcher pointed at that file.
///
/// The launcher names the file by absolute path, never by icon name. The
/// desktop then loads it directly, outside its icon-theme cache — which
/// GNOME Shell only re-scans on its own schedule: a freshly written name
/// stays unknown for seconds, and a removed file keeps resolving to its
/// old path (drawn as a blank) until the next re-scan. A changed path also
/// counts as a changed launcher, so the dock rebuilds the icon at once.
///
/// At startup (`replace_custom` false) a launcher whose icon the user set
/// themselves is left as it is, file and all.
fn apply(id: &str, replace_custom: bool) {
    let name = icon_name(id);
    let Some(hicolor) = hicolor_dir() else { return };
    let launcher = Launcher::find();
    if !replace_custom {
        if let Some(icon) = launcher.custom_icon() {
            tracing::info!("the launcher's icon ({icon}) was set outside Hylki, leaving it");
            // A theme name serves the windows too, where Hylki's icon search
            // finds it; a file path cannot.
            let found = !icon.contains('/')
                && gtk::gdk::Display::default()
                    .is_some_and(|d| gtk::IconTheme::for_display(&d).has_icon(&icon));
            gtk::Window::set_default_icon_name(if found { icon.as_str() } else { crate::APP_ID });
            return;
        }
    }
    let is_default = id == DEFAULT_ID;
    // The default needs no file of its own where the install's launcher
    // can simply be restored (the copy is removed); a launcher the user's
    // own install owns keeps pointing at a file, so its default is written
    // like any other choice.
    let own_file = !is_default || matches!(launcher, Launcher::Owned(_));
    sync_icon_files(&hicolor, id, &name, own_file);
    sync_private_icon(id, &name, own_file);
    let path = hicolor.join("512x512/apps").join(format!("{name}.png"));
    launcher.point_at(&path.to_string_lossy(), is_default && !own_file);
    // Hylki's own windows (X11, panels that draw window icons).
    if own_file {
        gtk::Window::set_default_icon_name(&name);
    } else {
        gtk::Window::set_default_icon_name(crate::APP_ID);
    }
}

/// Write the current choice's art (if it is not already there) and clear
/// the art of earlier choices, then nudge the directories' timestamps so
/// icon caches re-scan them.
fn sync_icon_files(hicolor: &std::path::Path, id: &str, name: &str, write: bool) {
    let prefix = format!("{}-", crate::APP_ID);
    let bare = format!("{}.png", crate::APP_ID);
    // The bare-name override an earlier build wrote over the shipped icon
    // is cleared too — but only when it is one of ours (judged by the 512,
    // the unscaled art): a native install keeps its own copy of the
    // shipped icon there.
    let bare_is_ours = is_alt_art(&hicolor.join("512x512/apps").join(&bare));
    let mut changed = false;
    for size in SIZES {
        let dir = hicolor.join(format!("{size}x{size}/apps"));
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                let stale = (fname.starts_with(&prefix)
                    && fname.ends_with(".png")
                    && fname != format!("{name}.png"))
                    || (fname == bare && bare_is_ours);
                if stale && std::fs::remove_file(entry.path()).is_ok() {
                    changed = true;
                }
            }
        }
        if !write {
            continue;
        }
        let path = dir.join(format!("{name}.png"));
        let png = png_for(id);
        let bytes = if size == 512 {
            std::borrow::Cow::Borrowed(png)
        } else {
            match scaled_png(png, size) {
                Some(b) => std::borrow::Cow::Owned(b),
                None => continue,
            }
        };
        if std::fs::read(&path)
            .map(|cur| cur == *bytes)
            .unwrap_or(false)
        {
            continue;
        }
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!("could not create {}: {e}", dir.display());
            continue;
        }
        match std::fs::write(&path, &*bytes) {
            Ok(()) => changed = true,
            Err(e) => tracing::warn!("could not write {}: {e}", path.display()),
        }
    }
    if changed {
        touch(hicolor);
        if let Some(parent) = hicolor.parent() {
            touch(parent);
        }
    }
}

/// Whether a file is one of the gallery's non-default icons, byte for byte.
fn is_alt_art(path: &std::path::Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    CATALOG
        .iter()
        .any(|c| c.id != DEFAULT_ID && c.png == bytes.as_slice())
}

/// The artwork re-encoded at `size`.
fn scaled_png(png: &[u8], size: i32) -> Option<Vec<u8>> {
    use gtk::gdk_pixbuf::{InterpType, PixbufLoader};
    use gtk::prelude::PixbufLoaderExt;
    let loader = PixbufLoader::with_type("png").ok()?;
    loader.write(png).ok()?;
    loader.close().ok()?;
    let pb = loader
        .pixbuf()?
        .scale_simple(size, size, InterpType::Hyper)?;
    pb.save_to_bufferv("png", &[]).ok()
}

fn touch(dir: &std::path::Path) {
    if let Ok(f) = std::fs::File::open(dir) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
}

/// A texture of an icon, decoded at `px` pixels (call with 2x the display
/// size for HiDPI) rather than the full 512, to keep a gallery cheap.
pub(crate) fn texture(id: &str, px: i32) -> Option<gtk::gdk::Texture> {
    use gtk::gdk_pixbuf::PixbufLoader;
    use gtk::prelude::PixbufLoaderExt;
    thread_local! {
        // Decoded once per (icon, size): the gallery is rebuilt with every
        // Settings window, and decoding the catalogue is the slow part.
        static DECODED: std::cell::RefCell<std::collections::HashMap<(String, i32), gtk::gdk::Texture>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let key = (id.to_string(), px);
    if let Some(t) = DECODED.with(|c| c.borrow().get(&key).cloned()) {
        return Some(t);
    }
    let loader = PixbufLoader::with_type("png").ok()?;
    loader.set_size(px, px);
    loader.write(png_for(id)).ok()?;
    loader.close().ok()?;
    let texture = gtk::gdk::Texture::for_pixbuf(&loader.pixbuf()?);
    DECODED.with(|c| c.borrow_mut().insert(key, texture.clone()));
    Some(texture)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retired_icon_choice_lands_on_one_still_offered() {
        assert_eq!(effective("squircle-dark"), "square-dark");
        assert_eq!(effective("square-yellow"), "square-yellow");
        assert_eq!(effective("envelope-wave"), DEFAULT_ID);
        assert_eq!(effective("classic"), "classic");
    }

    #[test]
    fn only_icons_hylki_writes_count_as_its_own() {
        let id = crate::APP_ID;
        assert!(icon_is_ours(id));
        assert!(icon_is_ours(&format!(
            "/home/u/.local/share/icons/hicolor/512x512/apps/{id}-envelope-wave.png"
        )));
        assert!(icon_is_ours(&format!(
            "/home/u/.local/share/icons/hicolor/512x512/apps/{id}-default.png"
        )));
        // A theme's name, the symbolic, another app's file, a relative path.
        assert!(!icon_is_ours("mail-client"));
        assert!(!icon_is_ours(&format!("{id}-symbolic")));
        assert!(!icon_is_ours("/home/u/icons/hylki-square.svg"));
        assert!(!icon_is_ours(&format!("{id}-default.png")));
    }
}
