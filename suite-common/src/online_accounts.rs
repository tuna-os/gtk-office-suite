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

use std::collections::HashMap;

use gtk4::{gio, glib};
use suite_common_core::accounts::{from_goa_props, AccountBook, GoaAccountProps};

const NAME: &str = "org.gnome.OnlineAccounts";
const PATH: &str = "/org/gnome/OnlineAccounts";
const ACCOUNT_IFACE: &str = "org.gnome.OnlineAccounts.Account";
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
    use glib::prelude::*;
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

    #[test]
    fn an_object_without_an_id_or_provider_is_skipped() {
        assert!(account_props(&props(&[("ProviderType", "google".to_variant())])).is_none());
        assert!(account_props(&props(&[("Id", "a".to_variant())])).is_none());
    }
}
