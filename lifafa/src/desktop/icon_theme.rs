//! Where the app's icons come from: the bundle built into the binary
//! (Hylki's own icon theme) or, when Settings asks, the desktop's theme.
//! The app sets it up at start and on that switch; app_icon looks up a
//! launcher icon in the desktop's folders.

use gtk::prelude::*;

/// Draw every icon from the copies built into the binary, and register the
/// app icon so windows and dialogs can find it by name.
///
/// The bundle (tools/gen-icon-gresource.sh) holds every icon Hylki names,
/// every one GTK and libadwaita draw for their own widgets, as found on the
/// build machine, and the app's own icon. Icons once followed the user's
/// theme (#260); a theme that restyled some of them made the app look
/// different from one distribution to the next, so by default none of them
/// do. `desktop` (Settings → Appearance) gives the desktop's theme back, the
/// bundle filling in what it lacks.
pub(crate) fn register_icons(desktop: bool) {
    if let Some(display) = gtk::gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        DESKTOP_ICON_PATH.get_or_init(|| theme.search_path());
        // Ahead of GTK's and libadwaita's own copies, so their widgets take
        // the bundle's: those copies come with whatever GTK the system has.
        let mut paths = vec![ICON_RESOURCE.to_string()];
        paths.extend(
            theme
                .resource_path()
                .into_iter()
                .map(|p| p.to_string())
                .filter(|p| p.trim_end_matches('/') != ICON_RESOURCE),
        );
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        theme.set_resource_path(&paths);
    }
    apply_icon_theme(desktop);
    gtk::Window::set_default_icon_name(crate::APP_ID);
}

/// Draw icons from Hylki's own theme, or from the desktop's. Applies at once:
/// GTK redraws every icon when the name changes.
///
/// Hylki's own way, the folders on disk leave the search altogether but for
/// the ones only Hylki writes to. Naming a theme that does not exist is not
/// enough by itself: GTK looks through hicolor's folders before the bundle,
/// and an icon of the same name there, from an icon theme installed in the
/// home folder or another program, drew a gear for the main menu on one
/// system. With nothing foreign left to search, the bundle answers for every
/// name.
pub(crate) fn apply_icon_theme(desktop: bool) {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    if desktop {
        // Back to whatever the desktop says, now and when it changes.
        settings.reset_property("gtk-icon-theme-name");
    } else {
        // Set by the app, the name outranks the desktop's setting, now and
        // whenever it changes: GTK would otherwise switch the theme back to it.
        settings.set_gtk_icon_theme_name(Some(ICON_THEME));
    }
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let theme = gtk::IconTheme::for_display(&display);
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    if desktop {
        paths.extend(DESKTOP_ICON_PATH.get().cloned().unwrap_or_default());
        // The user's icon directory: inside Flatpak XDG_DATA_HOME is the
        // sandbox's, so GTK wouldn't look there on its own.
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(std::path::Path::new(&home).join(".local/share/icons"));
        }
    }
    // The app icon chosen in Settings, for the window's own icon.
    if let Some(own) = crate::desktop::app_icon::private_icons_root() {
        paths.push(own);
    }
    // Running from the source tree: the app's icons as the install has them.
    let dev = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/data/icons"));
    if dev.is_dir() {
        paths.push(dev.to_path_buf());
    }
    let paths: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
    theme.set_search_path(&paths);
    if !desktop {
        let theme = theme.clone();
        gtk::glib::idle_add_local_once(move || check_icon_sources(&theme));
    }
}

/// An icon theme over the desktop's own icon folders, for the icons that are
/// the desktop's business rather than Hylki's: a launcher icon set outside
/// the app (#252).
pub(crate) fn desktop_icon_theme() -> gtk::IconTheme {
    let theme = gtk::IconTheme::new();
    let mut paths = DESKTOP_ICON_PATH.get().cloned().unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(std::path::Path::new(&home).join(".local/share/icons"));
    }
    let paths: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
    theme.set_search_path(&paths);
    theme
}

/// Whether every bundled icon is drawn from the bundle: any that resolves
/// elsewhere is logged with the file that won, so the exported log names
/// the culprit when an icon looks wrong on someone's system.
fn check_icon_sources(theme: &gtk::IconTheme) {
    let dir = format!("{ICON_RESOURCE}/scalable/actions");
    let names = gtk::gio::resources_enumerate_children(&dir, gtk::gio::ResourceLookupFlags::NONE)
        .unwrap_or_default();
    let mut foreign = 0;
    for file in &names {
        let Some(name) = file.strip_suffix(".svg") else {
            continue;
        };
        let icon = theme.lookup_icon(
            name,
            &[],
            16,
            1,
            gtk::TextDirection::Ltr,
            gtk::IconLookupFlags::empty(),
        );
        let uri = icon.file().map(|f| f.uri().to_string()).unwrap_or_default();
        if !uri.starts_with(&format!("resource://{ICON_RESOURCE}/")) {
            foreign += 1;
            tracing::warn!("icon {name} is drawn from {uri}, not from Hylki's own icons");
        }
    }
    tracing::debug!(
        "icons: {} bundled, {foreign} drawn from elsewhere",
        names.len()
    );
    // The app icon may come from a source checkout's data/icons, which is
    // the same art, so it is only reported.
    let app = theme.lookup_icon(
        crate::APP_ID,
        &[],
        128,
        1,
        gtk::TextDirection::Ltr,
        gtk::IconLookupFlags::empty(),
    );
    tracing::debug!(
        "app icon: {}",
        app.file().map(|f| f.uri().to_string()).unwrap_or_default()
    );
}

/// Hylki's icon theme: a name no theme on disk carries, so the user's icon
/// theme is never consulted and the app looks the same on every desktop.
/// Lookups fall through to hicolor, which holds the bundle below and GTK's
/// and libadwaita's built-in icons.
const ICON_THEME: &str = "Hylki";

/// Where the bundled icons sit in the app's GResource (registered in `main`).
const ICON_RESOURCE: &str = "/co/hyprlab/Hylki/icons";

/// The icon folders GTK searched before Hylki narrowed them: what "Use the
/// desktop's icons" gives back.
static DESKTOP_ICON_PATH: std::sync::OnceLock<Vec<std::path::PathBuf>> = std::sync::OnceLock::new();

#[cfg(test)]
mod tests {
    /// Every icon the code names is in the bundle Hylki draws from: with
    /// the icon folders on disk out of the search, one missing from it shows
    /// as a broken image on every system. tools/gen-icon-gresource.sh adds
    /// any the code has come to name.
    #[test]
    fn every_icon_named_in_the_code_is_bundled() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest = std::fs::read_to_string(root.join("resources/hylki.gresource.xml")).unwrap();
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        walk(&root.join("src"), &mut files);
        let mut missing = std::collections::BTreeSet::new();
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            for (at, _) in text.match_indices("-symbolic\"") {
                let start = text[..at].rfind('"').map_or(at, |q| q + 1);
                let name = &text[start..at + "-symbolic".len()];
                if !name.starts_with('-')
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                {
                    let entry = format!("icons/scalable/actions/{name}.svg");
                    if !manifest.contains(&entry) {
                        missing.insert(name.to_string());
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "not in the icon bundle, run tools/gen-icon-gresource.sh: {missing:?}"
        );
    }
}
