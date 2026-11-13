// onboarding.rs — the GTK-free half of first-run onboarding (INTEGRATION-PLAN C5).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The first time any suite app starts it offers the others and the shared
// accounts, once. What the welcome shows (the other apps that are not
// installed, whether an account is still needed) and whether to show it at all
// is decided here; the dialog and the "installed" lookup (desktop-file ids from
// Gio) are the shell's.

/// An app in the suite.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SuiteApp {
    Letters,
    Tables,
    Decks,
    Lifafa,
    Mandelbrot,
}

impl SuiteApp {
    pub const ALL: [SuiteApp; 5] = [Self::Letters, Self::Tables, Self::Decks, Self::Lifafa, Self::Mandelbrot];

    /// The app id, which is also its desktop-file id without `.desktop`.
    pub fn app_id(self) -> &'static str {
        match self {
            Self::Letters => "org.tunaos.letters",
            Self::Tables => "org.tunaos.tables",
            Self::Decks => "org.tunaos.decks",
            Self::Lifafa => "org.tunaos.lifafa",
            Self::Mandelbrot => "org.tunaos.mandelbrot",
        }
    }

    /// Untranslated name; callers localize.
    pub fn name(self) -> &'static str {
        match self {
            Self::Letters => "Letters",
            Self::Tables => "Tables",
            Self::Decks => "Decks",
            Self::Lifafa => "Lifafa",
            Self::Mandelbrot => "Mandelbrot",
        }
    }
}

/// What the welcome offers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Welcome {
    /// The other suite apps that are not installed, in suite order.
    pub missing_apps: Vec<SuiteApp>,
    /// Whether to ask for a mail account (none can send mail yet).
    pub offer_accounts: bool,
}

/// Decide the welcome for `this` app, or `None` when there is nothing to say or
/// it was already shown. `installed` is the app ids the system reports;
/// `already_shown` is the suite-wide flag, so the second app a person opens
/// does not repeat what the first said.
pub fn welcome(this: SuiteApp, installed: &[&str], needs_mail_account: bool, already_shown: bool) -> Option<Welcome> {
    if already_shown {
        return None;
    }
    let missing_apps: Vec<SuiteApp> = SuiteApp::ALL
        .into_iter()
        .filter(|app| *app != this && !installed.contains(&app.app_id()))
        .collect();
    if missing_apps.is_empty() && !needs_mail_account {
        return None;
    }
    Some(Welcome { missing_apps, offer_accounts: needs_mail_account })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_offers_the_apps_that_are_not_installed_but_never_this_one() {
        let w = welcome(SuiteApp::Letters, &["org.tunaos.letters", "org.tunaos.tables"], false, false).unwrap();
        assert_eq!(w.missing_apps, [SuiteApp::Decks, SuiteApp::Lifafa, SuiteApp::Mandelbrot]);
        assert!(!w.offer_accounts);
    }

    #[test]
    fn it_is_silent_once_shown_anywhere_in_the_suite() {
        assert_eq!(welcome(SuiteApp::Tables, &[], true, true), None);
    }

    #[test]
    fn with_every_app_installed_it_only_speaks_if_an_account_is_needed() {
        let all: Vec<&str> = SuiteApp::ALL.iter().map(|a| a.app_id()).collect();
        assert_eq!(welcome(SuiteApp::Decks, &all, false, false), None);
        let w = welcome(SuiteApp::Decks, &all, true, false).unwrap();
        assert!(w.missing_apps.is_empty() && w.offer_accounts);
    }

    #[test]
    fn ids_are_unique_and_follow_the_org_prefix() {
        let mut ids: Vec<_> = SuiteApp::ALL.iter().map(|a| a.app_id()).collect();
        assert!(ids.iter().all(|i| i.starts_with("org.tunaos.")));
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), SuiteApp::ALL.len());
    }
}
