// accounts.rs — the GTK-free model of "sign in once" (INTEGRATION-PLAN B7).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Mail and calendar accounts come from GNOME Online Accounts (Google,
// Microsoft, generic IMAP/SMTP); Matrix is not a GOA provider, so its one
// account (ADR 0013) is kept by the app that owns it and its secret goes to the
// Secret portal. Every app reads the same `AccountBook` so signing in once
// shows up everywhere. Talking to GOA or the portal is the shell half; this is
// what it fills in and what the apps ask.

/// What an account is used for.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Capability {
    Mail,
    Calendar,
    Contacts,
    Chat,
}

/// Where the credentials live.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    /// GNOME Online Accounts: the OAuth tokens never reach the app.
    OnlineAccounts,
    /// The Secret portal: the app stored a secret there.
    SecretPortal,
}

/// One signed-in identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Account {
    /// A stable id: GOA's account id, or `matrix:@user:server`.
    pub id: String,
    /// What the person sees: an address or a Matrix id.
    pub identity: String,
    pub provider: String,
    pub source: Source,
    pub capabilities: Vec<Capability>,
}

impl Account {
    pub fn can(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }
}

/// What a GOA provider type gives, from the provider names GOA reports
/// (`google`, `ms_graph`, `exchange`, `imap_smtp`, `owncloud`, `fedora` ...).
/// Unknown providers give nothing rather than a guess.
pub fn goa_capabilities(provider: &str) -> Vec<Capability> {
    use Capability::*;
    match provider {
        "google" | "ms_graph" | "exchange" => vec![Mail, Calendar, Contacts],
        "imap_smtp" => vec![Mail],
        "owncloud" | "webdav" => vec![Calendar, Contacts],
        _ => vec![],
    }
}

/// An account GOA listed.
pub fn from_goa(id: &str, provider: &str, identity: &str) -> Account {
    Account {
        id: id.to_owned(),
        identity: identity.to_owned(),
        provider: provider.to_owned(),
        source: Source::OnlineAccounts,
        capabilities: goa_capabilities(provider),
    }
}

/// One account as GOA's D-Bus object reports it (the
/// `org.gnome.OnlineAccounts.Account` interface's properties), for the shell
/// half that reads them.
#[derive(Clone, Debug, Default)]
pub struct GoaAccountProps {
    pub id: String,
    pub provider_type: String,
    pub presentation_identity: String,
    pub mail_disabled: bool,
    pub calendar_disabled: bool,
    pub contacts_disabled: bool,
    /// GOA wants the person to sign in again: its tokens no longer work, so
    /// nothing can use the account until they do.
    pub attention_needed: bool,
}

/// An account GOA listed, with what the person switched off in Settings
/// removed: a Google account with mail turned off cannot send mail.
pub fn from_goa_props(props: &GoaAccountProps) -> Account {
    let mut account = from_goa(&props.id, &props.provider_type, &props.presentation_identity);
    account.capabilities.retain(|c| match c {
        Capability::Mail => !props.mail_disabled && !props.attention_needed,
        Capability::Calendar => !props.calendar_disabled && !props.attention_needed,
        Capability::Contacts => !props.contacts_disabled && !props.attention_needed,
        Capability::Chat => true,
    });
    account
}

/// The Matrix account (ADR 0013: there is exactly one).
pub fn matrix(user_id: &str) -> Account {
    Account {
        id: format!("matrix:{user_id}"),
        identity: user_id.to_owned(),
        provider: "matrix".to_owned(),
        source: Source::SecretPortal,
        capabilities: vec![Capability::Chat],
    }
}

/// The accounts every app sees.
#[derive(Clone, Debug, Default)]
pub struct AccountBook {
    accounts: Vec<Account>,
    defaults: Vec<(Capability, String)>,
}

impl AccountBook {
    /// Add an account, or replace the one with the same id.
    pub fn add(&mut self, account: Account) {
        match self.accounts.iter_mut().find(|a| a.id == account.id) {
            Some(existing) => *existing = account,
            None => self.accounts.push(account),
        }
    }

    /// Remove an account and any default that named it.
    pub fn remove(&mut self, id: &str) {
        self.accounts.retain(|a| a.id != id);
        self.defaults.retain(|(_, d)| d != id);
    }

    /// The account for sending/reading `capability` by default: the one the
    /// person chose if it still exists, else the first that can.
    pub fn default_for(&self, capability: Capability) -> Option<&Account> {
        self.defaults
            .iter()
            .find(|(c, _)| *c == capability)
            .and_then(|(_, id)| self.accounts.iter().find(|a| a.id == *id && a.can(capability)))
            .or_else(|| self.for_capability(capability).next())
    }

    /// Choose the default for `capability`. Refused if the account is unknown
    /// or cannot do it.
    pub fn set_default(&mut self, capability: Capability, id: &str) -> bool {
        if !self.accounts.iter().any(|a| a.id == id && a.can(capability)) {
            return false;
        }
        self.defaults.retain(|(c, _)| *c != capability);
        self.defaults.push((capability, id.to_owned()));
        true
    }

    pub fn for_capability(&self, capability: Capability) -> impl Iterator<Item = &Account> {
        self.accounts.iter().filter(move |a| a.can(capability))
    }

    pub fn all(&self) -> &[Account] {
        &self.accounts
    }

    /// Whether first-run should ask the person to sign in: nothing can send
    /// mail yet.
    pub fn needs_first_run(&self) -> bool {
        self.for_capability(Capability::Mail).next().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Capability::*;

    #[test]
    fn goa_providers_map_to_what_they_give() {
        assert_eq!(goa_capabilities("google"), vec![Mail, Calendar, Contacts]);
        assert_eq!(goa_capabilities("imap_smtp"), vec![Mail]);
        assert!(goa_capabilities("something_new").is_empty());
    }

    #[test]
    fn matrix_is_one_chat_account_kept_in_the_secret_portal() {
        let m = matrix("@a:example.org");
        assert_eq!(m.id, "matrix:@a:example.org");
        assert_eq!(m.source, Source::SecretPortal);
        assert!(m.can(Chat) && !m.can(Mail));
    }

    #[test]
    fn adding_the_same_id_replaces_it() {
        let mut book = AccountBook::default();
        book.add(from_goa("g1", "imap_smtp", "a@x.org"));
        book.add(from_goa("g1", "google", "a@x.org"));
        assert_eq!(book.all().len(), 1);
        assert!(book.all()[0].can(Calendar));
    }

    #[test]
    fn the_default_is_the_chosen_one_else_the_first_that_can() {
        let mut book = AccountBook::default();
        book.add(from_goa("g1", "google", "a@x.org"));
        book.add(from_goa("g2", "ms_graph", "b@y.org"));
        assert_eq!(book.default_for(Mail).unwrap().id, "g1");
        assert!(book.set_default(Mail, "g2"));
        assert_eq!(book.default_for(Mail).unwrap().id, "g2");
        book.remove("g2");
        assert_eq!(book.default_for(Mail).unwrap().id, "g1");
    }

    #[test]
    fn a_default_must_be_able_to_do_the_thing() {
        let mut book = AccountBook::default();
        book.add(from_goa("g1", "imap_smtp", "a@x.org"));
        book.add(matrix("@a:x.org"));
        assert!(!book.set_default(Calendar, "g1"));
        assert!(!book.set_default(Mail, "matrix:@a:x.org"));
        assert!(!book.set_default(Mail, "nope"));
        assert_eq!(book.default_for(Chat).unwrap().identity, "@a:x.org");
    }

    #[test]
    fn first_run_is_wanted_until_something_can_send_mail() {
        let mut book = AccountBook::default();
        assert!(book.needs_first_run());
        book.add(matrix("@a:x.org"));
        assert!(book.needs_first_run());
        book.add(from_goa("g1", "owncloud", "a@cloud"));
        assert!(book.needs_first_run());
        book.add(from_goa("g2", "google", "a@x.org"));
        assert!(!book.needs_first_run());
    }

    fn google() -> GoaAccountProps {
        GoaAccountProps {
            id: "account_1".into(),
            provider_type: "google".into(),
            presentation_identity: "ann@example.org".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_goa_account_keeps_what_its_provider_gives() {
        let a = from_goa_props(&google());
        assert_eq!(a.capabilities, vec![Mail, Calendar, Contacts]);
        assert_eq!(a.identity, "ann@example.org");
        assert_eq!(a.source, Source::OnlineAccounts);
    }

    #[test]
    fn what_the_person_switched_off_in_settings_is_left_out() {
        let a = from_goa_props(&GoaAccountProps { mail_disabled: true, contacts_disabled: true, ..google() });
        assert_eq!(a.capabilities, vec![Calendar]);
        let mut book = AccountBook::default();
        book.add(a);
        assert!(book.needs_first_run(), "an account with mail off cannot send mail");
    }

    #[test]
    fn an_account_that_needs_signing_in_again_can_do_nothing_yet() {
        let a = from_goa_props(&GoaAccountProps { attention_needed: true, ..google() });
        assert!(a.capabilities.is_empty());
    }

}
