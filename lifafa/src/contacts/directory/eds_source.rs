//! Editing Evolution Data Server sources over D-Bus, shared by the LDAP
//! directories and the CardDAV accounts: the one bus connection, the
//! source registry, and making, rewriting, removing and signing in a
//! source.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::i18n::i18n;

pub(crate) const SOURCE_IFACE: &str = "org.gnome.evolution.dataserver.Source";
const WRITABLE_IFACE: &str = "org.gnome.evolution.dataserver.Source.Writable";
const REMOVABLE_IFACE: &str = "org.gnome.evolution.dataserver.Source.Removable";
pub(super) const MANAGER_PATH: &str = "/org/gnome/evolution/dataserver/SourceManager";
pub(super) const MANAGER_IFACE: &str = "org.gnome.evolution.dataserver.SourceManager";

/// One session-bus connection for the process: EDS closes a book when the
/// connection that opened it goes, so a connection per search would open
/// and close the directory every time.
pub(crate) fn bus() -> Result<zbus::blocking::Connection, String> {
    static BUS: Mutex<Option<zbus::blocking::Connection>> = Mutex::new(None);
    let mut guard = BUS.lock().map_err(|e| e.to_string())?;
    if let Some(c) = guard.as_ref() {
        return Ok(c.clone());
    }
    let c = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    *guard = Some(c.clone());
    Ok(c)
}

/// What is said when EDS cannot be reached.
pub(crate) fn not_available() -> String {
    i18n("Evolution Data Server is not available")
}

/// The source registry's bus name.
pub(crate) fn registry() -> Result<String, String> {
    crate::contacts::sources_dest().ok_or_else(not_available)
}

/// A UID for a new source, in the form EDS gives its own.
pub(crate) fn new_source_uid() -> String {
    gtk::glib::uuid_string_random().replace('-', "")
}

/// Make the source `uid` from the key file `data`.
pub(crate) fn create_source(
    conn: &zbus::blocking::Connection,
    dest: &str,
    uid: &str,
    data: String,
) -> Result<(), String> {
    let sources: HashMap<&str, String> = HashMap::from([(uid, data)]);
    conn.call_method(
        Some(dest),
        MANAGER_PATH,
        Some(MANAGER_IFACE),
        "CreateSources",
        &(sources,),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Replace the key file of the source at `path`.
pub(crate) fn write_source(
    conn: &zbus::blocking::Connection,
    dest: &str,
    path: &str,
    data: String,
) -> Result<(), String> {
    conn.call_method(Some(dest), path, Some(WRITABLE_IFACE), "Write", &(data,))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Remove the source at `path`, and what EDS keeps for it.
pub(crate) fn remove_source(
    conn: &zbus::blocking::Connection,
    dest: &str,
    path: &str,
) -> Result<(), String> {
    conn.call_method(Some(dest), path, Some(REMOVABLE_IFACE), "Remove", &())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Hand the source at `path` a user name and password to sign in with now.
pub(crate) fn invoke_authenticate(
    conn: &zbus::blocking::Connection,
    dest: &str,
    path: &str,
    user: &str,
    password: &str,
) -> Result<(), String> {
    let credentials = vec![
        format!("username:{}", user.trim()),
        format!("password:{password}"),
    ];
    conn.call_method(
        Some(dest),
        path,
        Some(SOURCE_IFACE),
        "InvokeAuthenticate",
        &(credentials,),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}
