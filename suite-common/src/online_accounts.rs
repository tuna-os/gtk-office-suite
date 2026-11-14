// SPDX-License-Identifier: GPL-3.0-or-later
//
// The desktop's GNOME Online Accounts, read into the suite's `AccountBook`
// (INTEGRATION-PLAN B7): the D-Bus half of `suite_common_core::accounts`,
// which decides what each account can do.
//
// Only the account list is read, never a token. Inside Flatpak the office
// apps hold no `--talk-name=org.gnome.OnlineAccounts` (that name also hands
// out the accounts' access tokens), so there the read fails and `read` says
// so with `None`: "could not look" must never be taken for "no accounts",
// or the welcome would ask a person who has a mail account to add one.
//
// The Matrix account is not GOA's: Mandelbrot owns it (ADR 0013) and says
// which it is through the state of its `matrix-account` action, read with
// `org.gtk.Actions.Describe` (`matrix`). That needs
// `--talk-name=org.tunaos.mandelbrot`, which only Decks holds today.

use std::collections::HashMap;

use gtk4::{gio, glib, glib::prelude::*};
use suite_common_core::accounts::{self, from_goa_props, Account, AccountBook, GoaAccountProps};

const NAME: &str = "org.gnome.OnlineAccounts";
const PATH: &str = "/org/gnome/OnlineAccounts";
const ACCOUNT_IFACE: &str = "org.gnome.OnlineAccounts.Account";
const MANDELBROT: &str = "org.tunaos.mandelbrot";
const MANDELBROT_PATH: &str = "/org/tunaos/mandelbrot";
/// A stuck GOA must not hold a thread for long.
const TIMEOUT_MS: i32 = 3000;

/// Every account GOA lists, or `None` when GOA could not be asked (no
/// session bus, no GOA, or a sandbox without the grant). Blocking: run it off
/// the main thread.
pub fn read() -> Option<AccountBook> {
    let conn = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
    let reply = conn
        .call_sync(
            Some(NAME),
            PATH,
            "org.freedesktop.DBus.ObjectManager",
            "GetManagedObjects",
            None,
            None,
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
        .ok()?;
    type Props = HashMap<String, glib::Variant>;
    type Objects = HashMap<glib::variant::ObjectPath, HashMap<String, Props>>;
    let (objects,) = reply.get::<(Objects,)>()?;
    let mut book = AccountBook::default();
    for props in objects.values().filter_map(|ifaces| ifaces.get(ACCOUNT_IFACE)) {
        if let Some(account) = account_props(props) {
            book.add(from_goa_props(&account));
        }
    }
    Some(book)
}

/// The Matrix account Mandelbrot is signed in to, or `None` when it is not
/// running, has no account, or cannot be asked. Never starts Mandelbrot: a
/// look at the accounts must not open a chat app. Blocking: run it off the
/// main thread.
pub fn matrix() -> Option<Account> {
    let conn = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
    let reply = conn
        .call_sync(
            Some(MANDELBROT),
            MANDELBROT_PATH,
            "org.gtk.Actions",
            "Describe",
            Some(&("matrix-account",).to_variant()),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
        .ok()?;
    matrix_from_describe(&reply)
}

/// `Describe`'s reply, `((bgav))`: the action's state is its first `av`
/// entry, the user ID, empty when no one is signed in.
fn matrix_from_describe(reply: &glib::Variant) -> Option<Account> {
    let description = reply.try_child_value(0)?;
    let state = description.try_child_value(2)?;
    let user_id = state.try_child_value(0)?.as_variant()?.str()?.to_owned();
    (user_id.starts_with('@') && user_id.contains(':')).then(|| accounts::matrix(&user_id))
}

fn account_props(props: &HashMap<String, glib::Variant>) -> Option<GoaAccountProps> {
    let text = |key: &str| props.get(key).and_then(|v| v.str()).map(str::to_owned);
    let flag = |key: &str| props.get(key).and_then(|v| v.get::<bool>()).unwrap_or(false);
    Some(GoaAccountProps {
        id: text("Id")?,
        provider_type: text("ProviderType")?,
        presentation_identity: text("PresentationIdentity").unwrap_or_default(),
        mail_disabled: flag("MailDisabled"),
        calendar_disabled: flag("CalendarDisabled"),
        contacts_disabled: flag("ContactsDisabled"),
        attention_needed: flag("AttentionNeeded"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_common_core::accounts::Capability;

    fn props(pairs: &[(&str, glib::Variant)]) -> HashMap<String, glib::Variant> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn an_account_object_reads_into_the_model() {
        let p = props(&[
            ("Id", "account_1".to_variant()),
            ("ProviderType", "google".to_variant()),
            ("PresentationIdentity", "ann@example.org".to_variant()),
            ("MailDisabled", false.to_variant()),
            ("CalendarDisabled", true.to_variant()),
        ]);
        let a = from_goa_props(&account_props(&p).unwrap());
        assert_eq!(a.identity, "ann@example.org");
        assert!(a.can(Capability::Mail));
        assert!(!a.can(Capability::Calendar));
    }

    fn describe(state: Option<&str>) -> glib::Variant {
        let state = state.map(|s| format!("<{}>", s.to_variant().print(true))).unwrap_or_default();
        let text = format!("((true, signature '', [{state}]),)");
        glib::Variant::parse(Some(glib::VariantTy::new("((bgav))").unwrap()), &text).unwrap()
    }

    #[test]
    fn mandelbrot_s_action_state_names_the_matrix_account() {
        let a = matrix_from_describe(&describe(Some("@ann:example.org"))).unwrap();
        assert_eq!(a.id, "matrix:@ann:example.org");
        assert!(a.can(Capability::Chat));
    }

    #[test]
    fn no_one_signed_in_or_no_state_is_no_matrix_account() {
        assert!(matrix_from_describe(&describe(Some(""))).is_none());
        assert!(matrix_from_describe(&describe(None)).is_none());
        assert!(matrix_from_describe(&describe(Some("not an id"))).is_none());
    }

    #[test]
    fn an_object_without_an_id_or_provider_is_skipped() {
        assert!(account_props(&props(&[("ProviderType", "google".to_variant())])).is_none());
        assert!(account_props(&props(&[("Id", "a".to_variant())])).is_none());
    }
}
