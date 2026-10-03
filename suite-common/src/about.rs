//! The About dialog: each app's own name, summary, developer, icon and
//! latest release, from its AppStream metainfo.

use gtk4 as gtk;
use libadwaita as adw;
use adw::prelude::*;

/// Each app's AppStream metainfo, the one source of its name, summary,
/// developer and release history: the About dialog shows what the software
/// centre shows.
const METAINFO: [(&str, &str); 3] = [
    ("org.tunaos.letters", include_str!("../../flatpak/org.tunaos.letters.metainfo.xml")),
    ("org.tunaos.tables", include_str!("../../flatpak/org.tunaos.tables.metainfo.xml")),
    ("org.tunaos.decks", include_str!("../../flatpak/org.tunaos.decks.metainfo.xml")),
];

/// What the About dialog of `app_id` shows.
#[derive(Debug, PartialEq)]
pub(crate) struct AboutInfo {
    pub name: String,
    pub summary: String,
    pub developer: String,
    pub version: String,
}

/// The About details of `app_id`, from its metainfo: its own name and its
/// latest release. It used to show the suite's name and a hard-coded
/// "0.1.0" in every app, with no app icon.
pub(crate) fn about_info(app_id: &str) -> Option<AboutInfo> {
    let xml = METAINFO.iter().find(|(id, _)| *id == app_id)?.1;
    let between = |open: &str, close: &str| -> Option<String> {
        let start = xml.find(open)? + open.len();
        let end = start + xml[start..].find(close)?;
        Some(xml[start..end].trim().to_string())
    };
    Some(AboutInfo {
        name: between("<name>", "</name>")?,
        summary: between("<summary>", "</summary>")?,
        developer: between("<developer_name>", "</developer_name>")?,
        version: between("<release version=\"", "\"")?,
    })
}

/// Show the About dialog of the app, over its active window.
pub fn show(app: &adw::Application) {
    let id = app.application_id().map(|s| s.to_string()).unwrap_or_default();
    let about = adw::AboutDialog::new();
    if let Some(info) = about_info(&id) {
        about.set_application_name(&info.name);
        about.set_version(&info.version);
        about.set_developer_name(&info.developer);
        about.set_comments(&info.summary);
    }
    about.set_application_icon(&id);
    about.set_license_type(gtk::License::Gpl30);
    about.set_website("https://github.com/tuna-os/gtk-office-suite");
    about.set_issue_url("https://github.com/tuna-os/gtk-office-suite/issues");
    about.present(app.active_window().as_ref());
}

#[cfg(test)]
mod tests {
    /// Each app's About names the app and its latest release, not the
    /// suite and a hard-coded version.
    #[test]
    fn each_app_is_about_itself_at_its_latest_release() {
        for (id, name) in [("org.tunaos.letters", "Letters"), ("org.tunaos.tables", "Tables"), ("org.tunaos.decks", "Decks")] {
            let info = super::about_info(id).expect(id);
            assert_eq!(info.name, name);
            assert_eq!(info.version, "2.1.0", "{id}: the first <release> is the latest");
            assert!(!info.summary.is_empty() && !info.developer.is_empty());
        }
        assert_eq!(super::about_info("org.example.other"), None);
    }
}
