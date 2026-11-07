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
        developer: between("<developer_name>", "</developer_name>").or_else(|| {
            let dev = between("<developer", "</developer>")?;
            let start = dev.find("<name>")? + "<name>".len();
            let end = start + dev[start..].find("</name>")?;
            Some(dev[start..end].trim().to_string())
        })?,
        version: between("<release version=\"", "\"")?,
    })
}

/// Context for reporting an issue against `app_id`.
pub(crate) fn report_context(id: &str) -> suite_common_core::report::ReportContext {
    let (app_name, version) = about_info(id)
        .map(|i| (i.name, i.version))
        .unwrap_or_else(|| {
            let fallback = id.split('.').next_back().unwrap_or("App").to_string();
            (fallback, env!("CARGO_PKG_VERSION").to_string())
        });
    let os = suite_common_core::report::os_pretty_name();
    let gtk_v = format!("GTK {}.{}.{}", gtk::major_version(), gtk::minor_version(), gtk::micro_version());
    let adw_v = unsafe {
        format!(
            "libadwaita {}.{}.{}",
            adw::ffi::adw_get_major_version(),
            adw::ffi::adw_get_minor_version(),
            adw::ffi::adw_get_micro_version(),
        )
    };
    let toolkit = format!("{gtk_v}, {adw_v}");
    suite_common_core::report::ReportContext {
        app_name,
        version,
        os,
        toolkit,
    }
}

/// Open the tracker's new-issue page in the user's browser, with the app,
/// version and environment filled in.
pub fn report_problem(app: &adw::Application) {
    let id = app.application_id().map(|s| s.to_string()).unwrap_or_default();
    let ctx = report_context(&id);
    let url = suite_common_core::report::report_problem_url(&ctx);
    let active = app.active_window();
    gtk::UriLauncher::new(&url).launch(active.as_ref(), None::<&gtk::gio::Cancellable>, |_| {});
}

/// Register `app.report-problem`, which opens [`report_problem`].
pub(crate) fn add_report_problem_action(app: &adw::Application) {
    let action = gtk::gio::SimpleAction::new("report-problem", None);
    let app_weak = app.downgrade();
    action.connect_activate(move |_, _| {
        if let Some(app) = app_weak.upgrade() {
            report_problem(&app);
        }
    });
    app.add_action(&action);
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
    let ctx = report_context(&id);
    about.set_issue_url(&suite_common_core::report::report_problem_url(&ctx));
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

    #[test]
    fn report_context_builds_for_each_app() {
        for (id, name) in [("org.tunaos.letters", "Letters"), ("org.tunaos.tables", "Tables"), ("org.tunaos.decks", "Decks")] {
            let ctx = super::report_context(id);
            assert_eq!(ctx.app_name, name);
            assert_eq!(ctx.version, "2.1.0");
            assert!(ctx.toolkit.contains("GTK 4"));
        }
    }
}
