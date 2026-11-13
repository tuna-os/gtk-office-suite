// shell_search.rs — export an app's recent documents to GNOME Shell search
// (INTEGRATION-PLAN B1), the D-Bus half of `suite_common_core::search_provider`.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The Shell calls `org.gnome.Shell.SearchProvider2` on the app's bus name at an
// object path the app's `*.search-provider.ini` names. This module registers
// that object on the app's own session-bus connection and answers from the
// matching and ranking code, which is tested without a bus. Activating a result
// opens the document through the app's normal `open` handler.
//
// What it searches is the app's recent-files list (the `recent-files` GSettings
// key), the same set a person types a name of; nothing else is read.

use std::collections::HashMap;

use gtk4::gio::{self, prelude::*};
use gtk4::glib;

use suite_common_core::search_provider::{
    initial_result_set, items_from_paths, result_metas, subsearch_result_set,
};

/// The interface the Shell speaks to a search provider.
pub const INTERFACE: &str = "org.gnome.Shell.SearchProvider2";

/// How many results one provider offers; the Shell shows a handful per app.
const LIMIT: usize = 6;

/// How long the app stays alive with no window after a Shell call, so a
/// bus-activated provider answers the next keystroke without restarting.
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

/// The interface description, for tests and for `export`.
pub fn interface_xml() -> &'static str {
    XML
}

fn recent(settings: &gio::Settings) -> Vec<suite_common_core::recent::RecentItem> {
    items_from_paths(&crate::get_recent_files(settings))
}

fn meta_variant(meta: &suite_common_core::search_provider::ResultMeta) -> HashMap<String, glib::Variant> {
    let mut map = HashMap::new();
    map.insert("id".to_string(), meta.id.to_variant());
    map.insert("name".to_string(), meta.name.to_variant());
    map.insert("description".to_string(), meta.description.to_variant());
    // A themed icon name is a valid serialised GIcon.
    map.insert("gicon".to_string(), meta.icon.to_variant());
    map
}

/// Register the provider when `app` starts. `object_path` must match the
/// `ObjectPath=` in the app's `*.search-provider.ini`; `icon` is its themed
/// icon name. Failure to register is logged and the app carries on: search is
/// an extra, never a reason not to start.
pub fn export<A: IsA<gio::Application>>(app: &A, settings: gio::Settings, object_path: &'static str, icon: &'static str) {
    let app_for_open = app.as_ref().clone();
    app.connect_startup(move |app| {
        // Stay up briefly after a Shell call when there is no window. Only in
        // service mode (`--gapplication-service`): on a normal launch an
        // inactivity timeout would keep the process alive for IDLE_MS after
        // the last window closes, which a user and the GUI journeys that wait
        // for the process to exit both notice.
        if app.flags().contains(gio::ApplicationFlags::IS_SERVICE) {
            app.set_inactivity_timeout(IDLE_MS);
        }
        let Some(connection) = app.dbus_connection() else {
            eprintln!("shell search: the application has no D-Bus connection");
            return;
        };
        let node = match gio::DBusNodeInfo::for_xml(XML) {
            Ok(node) => node,
            Err(e) => {
                eprintln!("shell search: bad interface description: {e}");
                return;
            }
        };
        let Some(interface) = node.lookup_interface(INTERFACE) else {
            eprintln!("shell search: interface {INTERFACE} missing from its description");
            return;
        };
        let settings = settings.clone();
        let app = app_for_open.clone();
        let registered = connection
            .register_object(object_path, &interface)
            .method_call(move |_connection, _sender, _path, _interface, method, parameters, invocation| {
                // Keep the application alive until this call is answered.
                let _hold = app.hold();
                let reply = match method {
                    "GetInitialResultSet" => parameters.get::<(Vec<String>,)>().map(|(terms,)| {
                        (initial_result_set(&recent(&settings), &terms, LIMIT),).to_variant()
                    }),
                    "GetSubsearchResultSet" => parameters.get::<(Vec<String>, Vec<String>)>().map(|(previous, terms)| {
                        (subsearch_result_set(&recent(&settings), &previous, &terms, LIMIT),).to_variant()
                    }),
                    "GetResultMetas" => parameters.get::<(Vec<String>,)>().map(|(ids,)| {
                        let metas: Vec<HashMap<String, glib::Variant>> =
                            result_metas(&recent(&settings), &ids, icon).iter().map(meta_variant).collect();
                        (metas,).to_variant()
                    }),
                    "ActivateResult" => parameters.get::<(String, Vec<String>, u32)>().map(|(id, _terms, _time)| {
                        // The id is the document's path (search_provider::result_id).
                        app.open(&[gio::File::for_path(&id)], "");
                        ().to_variant()
                    }),
                    // The Shell's "show all results" asks the app to search itself;
                    // Letters has no document search window, so just present it.
                    "LaunchSearch" => parameters.get::<(Vec<String>, u32)>().map(|_| {
                        app.activate();
                        ().to_variant()
                    }),
                    other => {
                        invocation.return_gerror(glib::Error::new(gio::DBusError::UnknownMethod, &format!("no method {other}")));
                        return;
                    }
                };
                match reply {
                    Some(value) => invocation.return_value(Some(&value)),
                    None => invocation.return_gerror(glib::Error::new(gio::DBusError::InvalidArgs, "unexpected arguments")),
                }
            })
            .build();
        if let Err(e) = registered {
            eprintln!("shell search: could not register {object_path}: {e}");
        }
    });
}
