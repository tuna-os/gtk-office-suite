// SPDX-License-Identifier: GPL-3.0-or-later
//
// The desktop's address book, for the suite's person pickers (INTEGRATION-PLAN
// A7): every enabled Evolution Data Server address book, read over D-Bus once
// when the app's first window appears, kept for the rest of the run.
//
// What a card means, which books count and which bus name to use are the
// GTK-free rules in `suite_common_core::people` (`from_vcard`,
// `eds_address_books`, `eds_bus_name`); this is only the D-Bus half. Inside
// Flatpak it needs `--talk-name=org.gnome.evolution.dataserver.*`, the grant
// Lifafa has. Without EDS, or without the grant, there are simply no
// contacts: every call is bounded and a failure is an empty book, never an
// error shown to anyone.

use std::cell::RefCell;
use std::collections::HashMap;

use gtk4::{gio, glib, prelude::*};
use suite_common_core::people::{self, Person};

const SOURCES: &str = "org.gnome.evolution.dataserver.Sources";
const FACTORY: &str = "org.gnome.evolution.dataserver.AddressBook";
const FACTORY_PATH: &str = "/org/gnome/evolution/dataserver/AddressBookFactory";
const FACTORY_IFACE: &str = "org.gnome.evolution.dataserver.AddressBookFactory";
const BOOK_IFACE: &str = "org.gnome.evolution.dataserver.AddressBook";
/// Each call's limit: a stuck EDS must not hold a thread for long.
const TIMEOUT_MS: i32 = 3000;
/// Set to turn the reader off, as the GUI harness does for its journeys.
pub const OPT_OUT_ENV: &str = "SUITE_NO_CONTACTS";

thread_local! {
    static PEOPLE: RefCell<Vec<Person>> = const { RefCell::new(Vec::new()) };
}

/// The people read so far: empty until the read started by `install` ends,
/// and empty for good without EDS.
pub fn people() -> Vec<Person> {
    PEOPLE.with(|p| p.borrow().clone())
}

/// Read the address books in the background once `app` shows its first
/// window, off the main thread, and keep what was read for `people`.
pub fn install(app: &impl IsA<gtk4::Application>) {
    if std::env::var_os(OPT_OUT_ENV).is_some_and(|v| !v.is_empty() && v != "0") {
        return;
    }
    let started = std::rc::Rc::new(std::cell::Cell::new(false));
    app.as_ref().connect_active_window_notify(move |app| {
        if started.get() || app.active_window().is_none() {
            return;
        }
        started.set(true);
        let _ = glib::spawn_future_local(async {
            let read = gio::spawn_blocking(read_all).await.unwrap_or_default();
            PEOPLE.with(|p| *p.borrow_mut() = read);
        });
    });
}

/// Every person with an address in every enabled address book. Blocking:
/// run it off the main thread.
fn read_all() -> Vec<Person> {
    let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return Vec::new();
    };
    let names = bus_names(&conn);
    let (Some(sources), Some(factory)) = (people::eds_bus_name(&names, SOURCES), people::eds_bus_name(&names, FACTORY)) else {
        return Vec::new();
    };
    let books = people::eds_address_books(&registry(&conn, &sources));
    books.iter().flat_map(|uid| read_book(&conn, &factory, uid)).collect()
}

fn call(conn: &gio::DBusConnection, dest: &str, path: &str, iface: &str, method: &str, args: Option<&glib::Variant>) -> Option<glib::Variant> {
    conn.call_sync(Some(dest), path, iface, method, args, None, gio::DBusCallFlags::NONE, TIMEOUT_MS, gio::Cancellable::NONE)
        .ok()
}

/// The names on the session bus and those it can start: inside Flatpak the
/// host's EDS service files are not visible, but the bus lists the names.
fn bus_names(conn: &gio::DBusConnection) -> Vec<String> {
    ["ListNames", "ListActivatableNames"]
        .iter()
        .filter_map(|method| call(conn, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", method, None))
        .filter_map(|reply| reply.get::<(Vec<String>,)>())
        .flat_map(|(names,)| names)
        .collect()
}

/// Every source the registry holds, as (UID, key file).
fn registry(conn: &gio::DBusConnection, dest: &str) -> Vec<(String, String)> {
    type Props = HashMap<String, glib::Variant>;
    type Objects = HashMap<glib::variant::ObjectPath, HashMap<String, Props>>;
    let Some(reply) = call(conn, dest, "/org/gnome/evolution/dataserver/SourceManager", "org.freedesktop.DBus.ObjectManager", "GetManagedObjects", None) else {
        return Vec::new();
    };
    let Some((objects,)) = reply.get::<(Objects,)>() else {
        return Vec::new();
    };
    objects
        .values()
        .filter_map(|ifaces| ifaces.get("org.gnome.evolution.dataserver.Source"))
        .filter_map(|props| {
            let uid = props.get("UID")?.str()?.to_string();
            let data = props.get("Data")?.str()?.to_string();
            Some((uid, data))
        })
        .collect()
}

/// The people in one book: open it through the factory, ask for the cards
/// with an address, and close it again.
fn read_book(conn: &gio::DBusConnection, factory: &str, uid: &str) -> Vec<Person> {
    let Some(reply) = call(conn, factory, FACTORY_PATH, FACTORY_IFACE, "OpenAddressBook", Some(&(uid.to_string(),).to_variant())) else {
        return Vec::new();
    };
    // Returns (object path, bus name). Read with `str`, which takes a string
    // or an object path, so either typing of the first works.
    let field = |i: usize| (i < reply.n_children()).then(|| reply.child_value(i)).and_then(|v| v.str().map(str::to_string));
    let (Some(path), Some(bus)) = (field(0), field(1)) else {
        return Vec::new();
    };
    call(conn, &bus, &path, BOOK_IFACE, "Open", None);
    let cards = call(conn, &bus, &path, BOOK_IFACE, "GetContactList", Some(&(people::EDS_QUERY_WITH_EMAIL.to_string(),).to_variant()))
        .and_then(|reply| reply.get::<(Vec<String>,)>())
        .map(|(cards,)| cards)
        .unwrap_or_default();
    call(conn, &bus, &path, BOOK_IFACE, "Close", None);
    cards.iter().filter_map(|card| people::from_vcard(card)).collect()
}
