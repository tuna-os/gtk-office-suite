//! Mail in GNOME Shell's overview search (INTEGRATION-PLAN B1).
//!
//! The Shell calls `org.gnome.Shell.SearchProvider2` on the app's bus name at
//! the object path `data/co.hyprlab.Hylki.search-provider.ini` names. This
//! registers that object on the app's own session-bus connection when it
//! starts. A result is a message (its Message-ID); activating one opens a
//! `mid:` link, the way the app already opens one handed to it. What is
//! searched is the local mail cache (`Cache::shell_search`: subject, sender
//! and preview, never bodies), read through the read-only connection the
//! assistant uses, so a provider call never writes.
//!
//! The suite's office apps share `suite_common::shell_search`, which searches
//! a recent-files list; mail has no such list and the app does not link the
//! suite's crates, so this is the same few lines of D-Bus glue over a
//! different source.

use std::collections::HashMap;

use gtk::gio::{self, prelude::*};
use gtk::glib;

use crate::cache::{Cache, ShellHit};

/// The interface the Shell speaks to a search provider.
pub(crate) const INTERFACE: &str = "org.gnome.Shell.SearchProvider2";

/// The object path the `.search-provider.ini` names.
pub(crate) const OBJECT_PATH: &str = "/co/hyprlab/Hylki/SearchProvider";

/// How many results the provider offers; the Shell shows a handful per app.
const LIMIT: usize = 6;

/// How long a bus-activated provider stays up with no window after a call, so
/// the next keystroke is answered without a restart. Service mode only: on a
/// normal launch the timeout would keep the process alive after the last
/// window closes.
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

/// What the Shell shows for one result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Meta {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// A hit as a result row: the subject on top, the sender beneath.
pub(crate) fn meta(hit: &ShellHit) -> Meta {
    let name = if hit.subject.trim().is_empty() {
        "(no subject)".to_string()
    } else {
        hit.subject.trim().to_string()
    };
    Meta {
        id: hit.message_id.clone(),
        name,
        description: hit.from.trim().to_string(),
    }
}

/// The `mid:` link (RFC 2392) that opens the message `message_id`.
pub(crate) fn mid_uri(message_id: &str) -> String {
    format!("mid:{}", glib::Uri::escape_string(message_id, Some("@"), true))
}

fn hits(terms: &[String]) -> Vec<ShellHit> {
    match Cache::open_reader() {
        Ok(cache) => cache.shell_search(terms, LIMIT),
        Err(e) => {
            tracing::warn!("shell search: the mail cache is not readable: {e}");
            Vec::new()
        }
    }
}

fn ids(terms: &[String]) -> Vec<String> {
    hits(terms).into_iter().map(|h| h.message_id).collect()
}

fn meta_variant(m: &Meta, icon: &str) -> HashMap<String, glib::Variant> {
    let mut map = HashMap::new();
    map.insert("id".to_string(), m.id.to_variant());
    map.insert("name".to_string(), m.name.to_variant());
    map.insert("description".to_string(), m.description.to_variant());
    // A themed icon name is a valid serialised GIcon.
    map.insert("gicon".to_string(), icon.to_variant());
    map
}

/// Register the provider when `app` starts. A failure is logged and the app
/// carries on: search is an extra, never a reason not to start.
pub(crate) fn export<A: IsA<gio::Application>>(app: &A, icon: &'static str) {
    let app_for_calls = app.as_ref().clone();
    app.connect_startup(move |app| {
        if app.flags().contains(gio::ApplicationFlags::IS_SERVICE) {
            app.set_inactivity_timeout(IDLE_MS);
        }
        let Some(connection) = app.dbus_connection() else {
            tracing::warn!("shell search: the application has no D-Bus connection");
            return;
        };
        let node = match gio::DBusNodeInfo::for_xml(XML) {
            Ok(node) => node,
            Err(e) => {
                tracing::warn!("shell search: bad interface description: {e}");
                return;
            }
        };
        let Some(interface) = node.lookup_interface(INTERFACE) else {
            tracing::warn!("shell search: interface {INTERFACE} missing from its description");
            return;
        };
        let app = app_for_calls.clone();
        let registered = connection
            .register_object(OBJECT_PATH, &interface)
            .method_call(move |_connection, _sender, _path, _interface, method, parameters, invocation| {
                // Keep the application alive until this call is answered.
                let _hold = app.hold();
                let reply = match method {
                    "GetInitialResultSet" => parameters
                        .get::<(Vec<String>,)>()
                        .map(|(terms,)| (ids(&terms),).to_variant()),
                    // The next keystroke narrows the last; asking the cache
                    // again with all the terms is the same answer.
                    "GetSubsearchResultSet" => parameters
                        .get::<(Vec<String>, Vec<String>)>()
                        .map(|(_previous, terms)| (ids(&terms),).to_variant()),
                    "GetResultMetas" => parameters.get::<(Vec<String>,)>().map(|(wanted,)| {
                        // The identifiers are Message-IDs; look each up by
                        // searching for it, so a result that has since gone is
                        // simply not described.
                        let metas: Vec<HashMap<String, glib::Variant>> = wanted
                            .iter()
                            .filter_map(|id| {
                                Cache::open_reader().ok().and_then(|c| c.shell_hit(id))
                            })
                            .map(|h| meta_variant(&meta(&h), icon))
                            .collect();
                        (metas,).to_variant()
                    }),
                    "ActivateResult" => parameters.get::<(String, Vec<String>, u32)>().map(|(id, _terms, _time)| {
                        app.open(&[gio::File::for_uri(&mid_uri(&id))], "");
                        ().to_variant()
                    }),
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
            })
            .build();
        if let Err(e) = registered {
            tracing::warn!("shell search: could not register {OBJECT_PATH}: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(subject: &str, from: &str) -> ShellHit {
        ShellHit {
            message_id: "a@example.org".into(),
            subject: subject.into(),
            from: from.into(),
            preview: String::new(),
        }
    }

    #[test]
    fn a_result_row_is_the_subject_over_the_sender() {
        let m = meta(&hit("  Quarterly report ", " Ada Lovelace "));
        assert_eq!(m.name, "Quarterly report");
        assert_eq!(m.description, "Ada Lovelace");
        assert_eq!(m.id, "a@example.org");
    }

    #[test]
    fn a_message_with_no_subject_still_has_a_name() {
        assert_eq!(meta(&hit("", "Ada")).name, "(no subject)");
    }

    #[test]
    fn the_link_that_opens_a_message_escapes_what_a_uri_cannot_carry() {
        assert_eq!(mid_uri("a@example.org"), "mid:a@example.org");
        assert_eq!(mid_uri("a b/c"), "mid:a%20b%2Fc");
    }

    #[test]
    fn the_interface_description_parses_and_names_the_five_methods() {
        let node = gio::DBusNodeInfo::for_xml(XML).expect("valid XML");
        let interface = node.lookup_interface(INTERFACE).expect("interface");
        let mut methods: Vec<_> = interface.methods().iter().map(|m| m.name().to_string()).collect();
        methods.sort();
        assert_eq!(
            methods,
            ["ActivateResult", "GetInitialResultSet", "GetResultMetas", "GetSubsearchResultSet", "LaunchSearch"]
        );
    }
}
