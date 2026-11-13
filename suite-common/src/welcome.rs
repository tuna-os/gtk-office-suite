// SPDX-License-Identifier: GPL-3.0-or-later
//! The first-run welcome (INTEGRATION-PLAN C5): the first suite app a person
//! opens says, once for the whole suite, which of the other apps are not
//! installed. What it says and whether to say it is
//! `suite_common_core::onboarding::welcome`; this is the dialog, the
//! installed-apps lookup and the "shown" flag (`welcome-shown` in the shared
//! `org.tunaos.suite` schema).
//!
//! It also says where to add a mail account when the desktop's online
//! accounts (`crate::online_accounts`, INTEGRATION-PLAN B7) have none that can
//! send mail. Where those cannot be read (inside Flatpak, which has no grant
//! for them) it says nothing about accounts rather than guess.
//!
//! `SUITE_NO_WELCOME=1` turns it off, for kiosks, packaging checks and the
//! GUI journeys, which would otherwise meet a dialog on every first launch.

use std::{cell::Cell, rc::Rc};

use adw::prelude::*;
use gtk4::{gio, glib};
use libadwaita as adw;
use suite_common_core::onboarding::{welcome, Welcome};
pub use suite_common_core::onboarding::SuiteApp;

use crate::i18n;

const SCHEMA: &str = "org.tunaos.suite";
const KEY: &str = "welcome-shown";

/// The environment variable that turns the welcome off.
pub const OPT_OUT_ENV: &str = "SUITE_NO_WELCOME";

/// The shared settings, or `None` where the schema or the key is not installed
/// (an older install, or a test environment): the welcome is then skipped
/// rather than aborting the app.
fn shared_settings() -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup(SCHEMA, true)?;
    schema.has_key(KEY).then(|| gio::Settings::new(SCHEMA))
}

/// The suite apps the system has a desktop file for: the ids of every
/// application the desktop knows, matched against `<app id>.desktop`.
fn installed() -> Vec<&'static str> {
    let known: Vec<String> = gio::AppInfo::all().iter().filter_map(|a| a.id()).map(|id| id.to_string()).collect();
    SuiteApp::ALL
        .iter()
        .map(|a| a.app_id())
        .filter(|id| known.iter().any(|k| *k == format!("{id}.desktop")))
        .collect()
}

/// The dialog's text for what `w` offers.
fn body(this: SuiteApp, w: &Welcome) -> String {
    let mut parts = Vec::new();
    if !w.missing_apps.is_empty() {
        let names: Vec<String> = w.missing_apps.iter().map(|a| format!("\u{2022} {}", i18n(a.name()))).collect();
        parts.push(i18n("%s works with the other apps in the suite.").replace("%s", &i18n(this.name())));
        parts.push(names.join("\n"));
        parts.push(i18n("Install them from your software centre and they will work together."));
    }
    if w.offer_accounts {
        parts.push(i18n(
            "To send documents by email, add a mail account in Settings, under Online Accounts.",
        ));
    }
    parts.join("\n\n")
}

/// Show the welcome over the application's first window, if it is due.
/// Call once, before the application runs.
pub fn install(app: &adw::Application, this: SuiteApp) {
    if std::env::var_os(OPT_OUT_ENV).is_some_and(|v| !v.is_empty() && v != "0") {
        return;
    }
    let done = Rc::new(Cell::new(false));
    app.connect_active_window_notify(move |app| {
        if done.get() {
            return;
        }
        let Some(window) = app.active_window() else { return };
        done.set(true);
        let Some(settings) = shared_settings() else { return };
        if settings.boolean(KEY) {
            return;
        }
        let window = window.downgrade();
        glib::spawn_future_local(async move {
            // Off the main thread: GOA may have to be started first. `None`
            // (could not look) offers no account, never a wrong one.
            let book = gio::spawn_blocking(crate::online_accounts::read).await.ok().flatten();
            let needs_mail_account = book.is_some_and(|b| b.needs_first_run());
            let ids = installed();
            let Some(w) = welcome(this, &ids, needs_mail_account, settings.boolean(KEY)) else { return };
            let Some(window) = window.upgrade() else { return };
            let dialog = adw::AlertDialog::new(Some(&i18n("Welcome to the Suite")), Some(&body(this, &w)));
            dialog.add_response("ok", &i18n("_OK"));
            dialog.set_default_response(Some("ok"));
            dialog.set_close_response("ok");
            dialog.present(Some(&window));
            // Said once for the whole suite, even if this app is closed at once.
            let _ = settings.set_boolean(KEY, true);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_names_each_missing_app_and_how_to_get_it() {
        let w = Welcome { missing_apps: vec![SuiteApp::Tables, SuiteApp::Decks], offer_accounts: false };
        let text = body(SuiteApp::Letters, &w);
        assert!(text.contains("\u{2022} Tables") && text.contains("\u{2022} Decks"), "{text}");
        assert!(!text.contains("\u{2022} Letters"), "{text}");
        assert!(text.starts_with("Letters works with the other apps in the suite."), "{text}");
        assert!(text.contains("software centre"), "{text}");
        assert!(!text.contains("Online Accounts"), "{text}");
    }

    #[test]
    fn with_every_app_installed_it_only_says_where_to_add_mail() {
        let w = Welcome { missing_apps: vec![], offer_accounts: true };
        let text = body(SuiteApp::Letters, &w);
        assert!(text.starts_with("To send documents by email"), "{text}");
        assert!(text.contains("Online Accounts") && !text.contains("software centre"), "{text}");
    }
}
