//! The suite's own symbolic icons, for commands the Adwaita theme has no
//! icon for (a highlighter, line spacing, inserting a row, a chart, ...).
//! They were drawn with the nearest theme icon instead: a lightbulb for
//! Shape, Chart and Merge Cells, strikethrough for cell borders, the same
//! "+" for inserting rows and columns, and an icon name the theme lacks
//! for the comments sidebar, which GTK drew as a missing image.
//!
//! Each is a 16px symbolic SVG named `office-*-symbolic`, so GTK recolours
//! it like a theme icon and no theme icon can shadow it.

use gtk4 as gtk;

/// (icon name, SVG) for every icon in suite-common/icons/.
pub const ICONS: &[(&str, &[u8])] = &[
    ("office-border-symbolic", include_bytes!("../icons/office-border-symbolic.svg")),
    ("office-chart-symbolic", include_bytes!("../icons/office-chart-symbolic.svg")),
    ("office-column-delete-symbolic", include_bytes!("../icons/office-column-delete-symbolic.svg")),
    ("office-column-hide-symbolic", include_bytes!("../icons/office-column-hide-symbolic.svg")),
    ("office-column-insert-symbolic", include_bytes!("../icons/office-column-insert-symbolic.svg")),
    ("office-columns-symbolic", include_bytes!("../icons/office-columns-symbolic.svg")),
    ("office-comment-symbolic", include_bytes!("../icons/office-comment-symbolic.svg")),
    ("office-filter-symbolic", include_bytes!("../icons/office-filter-symbolic.svg")),
    ("office-highlight-symbolic", include_bytes!("../icons/office-highlight-symbolic.svg")),
    ("office-line-spacing-symbolic", include_bytes!("../icons/office-line-spacing-symbolic.svg")),
    ("office-merge-cells-symbolic", include_bytes!("../icons/office-merge-cells-symbolic.svg")),
    ("office-name-symbolic", include_bytes!("../icons/office-name-symbolic.svg")),
    ("office-number-format-symbolic", include_bytes!("../icons/office-number-format-symbolic.svg")),
    ("office-row-delete-symbolic", include_bytes!("../icons/office-row-delete-symbolic.svg")),
    ("office-row-hide-symbolic", include_bytes!("../icons/office-row-hide-symbolic.svg")),
    ("office-row-insert-symbolic", include_bytes!("../icons/office-row-insert-symbolic.svg")),
    ("office-shapes-symbolic", include_bytes!("../icons/office-shapes-symbolic.svg")),
    ("office-table-symbolic", include_bytes!("../icons/office-table-symbolic.svg")),
];

/// Make the suite's icons available by name to every widget on `display`.
///
/// GTK finds unthemed icons in a search path directory, so they are written
/// once to the user cache and the directory added to the display's theme.
pub fn install(display: &gtk::gdk::Display) {
    let dir = gtk::glib::user_cache_dir().join("gtk-office-suite").join("icons");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    for (name, svg) in ICONS {
        let path = dir.join(format!("{name}.svg"));
        if std::fs::read(&path).ok().as_deref() != Some(*svg) {
            let _ = std::fs::write(&path, svg);
        }
    }
    gtk::IconTheme::for_display(display).add_search_path(&dir);
}

#[cfg(test)]
mod tests {
    /// Every `*-symbolic` icon name the three apps use resolves to an
    /// icon: the theme's or the suite's own. A name the theme lacks is drawn
    /// as a broken image, as the comments sidebar's "chat-bubble-text" was,
    /// and nothing else catches it.
    #[test]
    fn every_icon_the_apps_name_exists() {
        crate::gtk_test::run(|| {
            let display = gtk4::gdk::Display::default().expect("a display");
            super::install(&display);
            let theme = gtk4::IconTheme::for_display(&display);
            assert!(theme.has_icon("document-new-symbolic"), "the Adwaita icon theme is not installed");
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
            let mut missing = Vec::new();
            for dir in ["letters/src", "tables/src", "decks/src", "suite-common/src"] {
                for name in symbolic_names(&root.join(dir)) {
                    if !theme.has_icon(&name) {
                        missing.push(name);
                    }
                }
            }
            missing.sort();
            missing.dedup();
            assert!(missing.is_empty(), "icons that exist nowhere: {missing:?}");
        });
    }

    /// The `"…-symbolic"` string literals in the .rs files under `dir`.
    fn symbolic_names(dir: &std::path::Path) -> Vec<String> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(symbolic_names(&path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for piece in text.split('"').skip(1).step_by(2) {
                    let named = piece.starts_with(|c: char| c.is_ascii_lowercase()) && piece.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
                    if named && piece.ends_with("-symbolic") {
                        out.push(piece.to_string());
                    }
                }
            }
        }
        out
    }

    /// Every file in icons/ is installed, and every installed icon is a
    /// symbolic SVG named for the suite.
    #[test]
    fn every_icon_file_is_installed_under_a_suite_name() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("icons");
        let mut files: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().trim_end_matches(".svg").to_string())
            .collect();
        files.sort();
        let names: Vec<String> = super::ICONS.iter().map(|(n, _)| n.to_string()).collect();
        assert_eq!(files, names);
        for (name, svg) in super::ICONS {
            assert!(name.starts_with("office-") && name.ends_with("-symbolic"), "{name}");
            assert!(std::str::from_utf8(svg).unwrap().starts_with("<svg"), "{name}");
        }
    }
}
