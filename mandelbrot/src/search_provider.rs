//! Rooms in GNOME Shell's overview search (INTEGRATION-PLAN B1).
//!
//! The Shell calls `org.gnome.Shell.SearchProvider2` on the app's bus name at
//! the object path `org.tunaos.mandelbrot.search-provider.ini` names. This
//! registers that object on the app's own session-bus connection when it
//! starts. A result is a joined room (favourite, normal or low priority; not
//! invites, left rooms or upgraded ones), matched by its display name; it is
//! offered only once a session has loaded, so a search just after login can be
//! empty. Activating a result opens the room's `matrix:` link, the way a link
//! handed to the app is opened. Nothing is read from a room but its name and
//! whether it is a direct chat.
//!
//! The suite's office apps share `suite_common::shell_search`, which searches
//! a recent-files list. This app does not link the suite's crates and has no
//! such list, so it carries the same few lines of D-Bus glue over its room
//! list.

use std::collections::HashMap;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use tracing::warn;

use crate::{
    Application,
    prelude::*,
    session::{Room, RoomCategory, Session},
};

/// The interface the Shell speaks to a search provider.
pub(crate) const INTERFACE: &str = "org.gnome.Shell.SearchProvider2";

/// The object path the `.search-provider.ini` names.
pub(crate) const OBJECT_PATH: &str = "/org/tunaos/mandelbrot/SearchProvider";

/// How many results the provider offers; the Shell shows a handful per app.
const LIMIT: usize = 6;

/// How long a bus-activated provider stays up with no window after a call.
/// Service mode only: on a normal launch the timeout would keep the process
/// alive after the last window closes.
const IDLE_MS: u32 = 20_000;

const XML: &str = r#"
<node>
  <interface name="org.gnome.Shell.SearchProvider2">
    <method name="GetInitialResultSet">
      <arg type="as" name="terms" direction="in"/>
      <arg type="as" name="results" direction="out"/>
    </method>
    <method name="GetSubsearchResultSet">
      <arg type="as" name="previous_results" direction="in"/>
      <arg type="as" name="terms" direction="in"/>
      <arg type="as" name="results" direction="out"/>
    </method>
    <method name="GetResultMetas">
      <arg type="as" name="identifiers" direction="in"/>
      <arg type="aa{sv}" name="metas" direction="out"/>
    </method>
    <method name="ActivateResult">
      <arg type="s" name="identifier" direction="in"/>
      <arg type="as" name="terms" direction="in"/>
      <arg type="u" name="timestamp" direction="in"/>
    </method>
    <method name="LaunchSearch">
      <arg type="as" name="terms" direction="in"/>
      <arg type="u" name="timestamp" direction="in"/>
    </method>
  </interface>
</node>
"#;

/// Whether `name` contains every one of `terms`, ignoring case. No terms, or
/// only blank ones, match nothing.
pub(crate) fn name_matches(name: &str, terms: &[String]) -> bool {
    let name = name.to_lowercase();
    let terms: Vec<String> = terms
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    !terms.is_empty() && terms.iter().all(|t| name.contains(t))
}

/// The `matrix:` link (the Matrix URI scheme) that opens the room `room_id`.
pub(crate) fn room_uri(room_id: &str) -> String {
    let id = room_id.strip_prefix('!').unwrap_or(room_id);
    format!("matrix:roomid/{id}")
}

/// The joined rooms of every loaded session.
fn joined_rooms(app: &Application) -> Vec<Room> {
    let sessions = app.session_list();
    let mut rooms = Vec::new();
    for i in 0..sessions.n_items() {
        let Some(session) = sessions.item(i).and_downcast::<Session>() else {
            continue;
        };
        let list = session.room_list();
        for j in 0..list.n_items() {
            if let Some(room) = list.item(j).and_downcast::<Room>()
                && matches!(
                    room.category(),
                    RoomCategory::Favorite | RoomCategory::Normal | RoomCategory::LowPriority
                )
            {
                rooms.push(room);
            }
        }
    }
    rooms
}

/// The rooms whose name contains every term, by name.
fn matching(app: &Application, terms: &[String]) -> Vec<Room> {
    let mut rooms: Vec<Room> = joined_rooms(app)
        .into_iter()
        .filter(|r| name_matches(&r.display_name(), terms))
        .collect();
    rooms.sort_by_key(|r| r.display_name().to_lowercase());
    rooms.truncate(LIMIT);
    rooms
}

fn meta_variant(room: &Room, icon: &str) -> HashMap<String, glib::Variant> {
    let description = if room.is_direct() {
        gettext("Direct message")
    } else {
        gettext("Room")
    };
    let mut map = HashMap::new();
    map.insert("id".to_string(), room.room_id_string().to_variant());
    map.insert("name".to_string(), room.display_name().to_variant());
    map.insert("description".to_string(), description.to_variant());
    // A themed icon name is a valid serialised GIcon.
    map.insert("gicon".to_string(), icon.to_variant());
    map
}

/// Register the provider on `app`'s bus connection. Call once the application
/// has started. A failure is logged and the app carries on: search is an
/// extra, never a reason not to start.
pub(crate) fn export(app: &Application, icon: &'static str) {
    if app.flags().contains(gio::ApplicationFlags::IS_SERVICE) {
        app.set_inactivity_timeout(IDLE_MS);
    }
    let Some(connection) = app.dbus_connection() else {
        warn!("shell search: the application has no D-Bus connection");
        return;
    };
    let node = match gio::DBusNodeInfo::for_xml(XML) {
        Ok(node) => node,
        Err(error) => {
            warn!("shell search: bad interface description: {error}");
            return;
        }
    };
    let Some(interface) = node.lookup_interface(INTERFACE) else {
        warn!("shell search: interface {INTERFACE} missing from its description");
        return;
    };
    let app = app.clone();
    let registered = connection
        .register_object(OBJECT_PATH, &interface)
        .method_call(
            move |_connection, _sender, _path, _interface, method, parameters, invocation| {
                // Keep the application alive until this call is answered.
                let _hold = app.hold();
                let ids = |terms: &[String]| -> Vec<String> {
                    matching(&app, terms)
                        .iter()
                        .map(|r| r.room_id_string())
                        .collect()
                };
                let reply = match method {
                    "GetInitialResultSet" => parameters
                        .get::<(Vec<String>,)>()
                        .map(|(terms,)| (ids(&terms),).to_variant()),
                    // The next keystroke narrows the last; matching again with
                    // all the terms gives the same answer.
                    "GetSubsearchResultSet" => parameters
                        .get::<(Vec<String>, Vec<String>)>()
                        .map(|(_previous, terms)| (ids(&terms),).to_variant()),
                    "GetResultMetas" => parameters.get::<(Vec<String>,)>().map(|(wanted,)| {
                        let rooms = joined_rooms(&app);
                        let metas: Vec<HashMap<String, glib::Variant>> = wanted
                            .iter()
                            .filter_map(|id| rooms.iter().find(|r| &r.room_id_string() == id))
                            .map(|r| meta_variant(r, icon))
                            .collect();
                        (metas,).to_variant()
                    }),
                    "ActivateResult" => {
                        parameters
                            .get::<(String, Vec<String>, u32)>()
                            .map(|(id, _terms, _time)| {
                                app.open(&[gio::File::for_uri(&room_uri(&id))], "");
                                ().to_variant()
                            })
                    }
                    // "Show all results": present the window.
                    "LaunchSearch" => parameters.get::<(Vec<String>, u32)>().map(|_| {
                        app.activate();
                        ().to_variant()
                    }),
                    other => {
                        invocation.return_gerror(glib::Error::new(
                            gio::DBusError::UnknownMethod,
                            &format!("no method {other}"),
                        ));
                        return;
                    }
                };
                match reply {
                    Some(value) => invocation.return_value(Some(&value)),
                    None => invocation.return_gerror(glib::Error::new(
                        gio::DBusError::InvalidArgs,
                        "unexpected arguments",
                    )),
                }
            },
        )
        .build();
    if let Err(error) = registered {
        warn!("shell search: could not register {OBJECT_PATH}: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn every_term_must_be_in_the_name_whatever_its_case() {
        assert!(name_matches("Rust Matrix Room", &terms(&["rust", "ROOM"])));
        assert!(!name_matches("Rust Matrix Room", &terms(&["rust", "gnome"])));
    }

    #[test]
    fn no_terms_or_blank_terms_match_nothing() {
        assert!(!name_matches("Anything", &[]));
        assert!(!name_matches("Anything", &terms(&["  "])));
    }

    #[test]
    fn the_link_that_opens_a_room_drops_the_sigil() {
        assert_eq!(room_uri("!abc:example.org"), "matrix:roomid/abc:example.org");
    }

    #[test]
    fn the_interface_description_parses_and_names_the_five_methods() {
        let node = gio::DBusNodeInfo::for_xml(XML).expect("valid XML");
        let interface = node.lookup_interface(INTERFACE).expect("interface");
        for method in [
            "ActivateResult",
            "GetInitialResultSet",
            "GetResultMetas",
            "GetSubsearchResultSet",
            "LaunchSearch",
        ] {
            assert!(interface.lookup_method(method).is_some(), "{method}");
        }
    }
}
