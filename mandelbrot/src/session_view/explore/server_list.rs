use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};
use ruma::{OwnedServerName, ServerName, api::client::thirdparty::get_protocols};
use tracing::error;

use super::ExploreServer;
use crate::{prelude::*, session::Session, spawn, spawn_tokio};

mod imp {
    use std::cell::RefCell;

    use indexmap::IndexMap;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::ExploreServerList)]
    pub struct ExploreServerList {
        /// The current session.
        #[property(get, set = Self::set_session)]
        session: glib::WeakRef<Session>,
        /// The item for our own server.
        own_server: RefCell<Option<ExploreServer>>,
        /// The list of third-party networks on our own server.
        third_party_networks: RefCell<Vec<ExploreServer>>,
        /// The list of custom homeservers.
        custom_servers: RefCell<IndexMap<OwnedServerName, ExploreServer>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ExploreServerList {
        const NAME: &'static str = "ExploreServerList";
        type Type = super::ExploreServerList;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for ExploreServerList {}

    impl ListModelImpl for ExploreServerList {
        fn item_type(&self) -> glib::Type {
            ExploreServer::static_type()
        }

        fn n_items(&self) -> u32 {
            (usize::from(self.own_server.borrow().is_some())
                + self.third_party_networks.borrow().len()
                + self.custom_servers.borrow().len()) as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            let mut position = position as usize;

            // We always have our own server if we have a session.
            if position == 0 {
                return self.own_server.borrow().clone().and_upcast();
            }

            position -= 1;

            let third_party_len = self.third_party_networks.borrow().len();
            if position < third_party_len {
                return self
                    .third_party_networks
                    .borrow()
                    .get(position)
                    .cloned()
                    .and_upcast();
            }

            position -= third_party_len;

            self.custom_servers
                .borrow()
                .get_index(position)
                .map(|(_, server)| server.clone().upcast())
        }
    }

    impl ExploreServerList {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            if self.session.upgrade().as_ref() == session {
                return;
            }

            self.session.set(session);

            self.load_servers();
            self.obj().notify_session();
        }

        /// Load the servers.
        fn load_servers(&self) {
            let removed = self.n_items();

            self.own_server.take();
            self.third_party_networks.borrow_mut().clear();
            self.custom_servers.borrow_mut().clear();

            let Some(session) = self.session.upgrade() else {
                self.obj().items_changed(0, removed, 0);
                return;
            };

            // Add our own server.
            let own_server =
                ExploreServer::with_default_server(session.user_id().server_name().as_str());
            self.own_server.replace(Some(own_server));

            // Load the custom servers.
            let custom_servers = session.settings().explore_custom_servers();
            self.custom_servers.borrow_mut().extend(
                custom_servers
                    .into_iter()
                    .map(|server| (server.clone(), ExploreServer::with_server(server))),
            );

            let added = self.n_items();
            self.obj().items_changed(0, removed, added);

            // Make a request to get the third-party networks.
            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.load_third_party_networks().await;
                }
            ));
        }

        /// Load the list of third-party networks.
        async fn load_third_party_networks(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let client = session.client();
            let handle =
                spawn_tokio!(async move { client.send(get_protocols::v3::Request::new()).await });

            let protocols = match handle.await.expect("task was not aborted") {
                Ok(response) => response.protocols,
                Err(error) => {
                    error!("Could not get third-party networks: {error}");
                    Default::default()
                }
            };

            let added = if protocols.is_empty() {
                0
            } else {
                let mut third_party_networks = self.third_party_networks.borrow_mut();
                third_party_networks.extend(protocols.iter().flat_map(
                    |(protocol_id, protocol)| {
                        protocol.instances.iter().filter_map(|instance| {
                            instance.instance_id.as_deref().map(|instance_id| {
                                ExploreServer::with_third_party_protocol(
                                    &instance.desc,
                                    protocol_id,
                                    instance_id,
                                )
                            })
                        })
                    },
                ));

                third_party_networks.len()
            };

            self.obj().items_changed(1, 0, added as u32);
        }

        /// Whether this list contains the given Matrix server.
        pub(super) fn contains_matrix_server(&self, server_name: &ServerName) -> bool {
            self.own_server
                .borrow()
                .as_ref()
                // The user's matrix server is a special case that doesn't have a "server", so
                // compare to its name, which should be a server name.
                .is_some_and(|server| server.name().as_str() == server_name)
                || self.custom_servers.borrow().contains_key(server_name)
        }

        /// Add a custom Matrix server.
        pub(super) fn add_custom_server(&self, server_name: OwnedServerName) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let server = ExploreServer::with_server(server_name.clone());
            if self
                .custom_servers
                .borrow_mut()
                .insert(server_name.clone(), server)
                .is_some()
            {
                // The server already existed, the list did not change.
                return;
            }

            // Update the list in the settings.
            let settings = session.settings();
            let mut servers = settings.explore_custom_servers();
            servers.insert(server_name);
            settings.set_explore_custom_servers(servers);

            self.obj().items_changed(self.n_items() - 1, 0, 1);
        }

        /// Remove a custom Matrix server.
        pub(super) fn remove_custom_server(&self, server_name: &ServerName) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let Some((pos, ..)) = self
                .custom_servers
                .borrow_mut()
                .shift_remove_full(server_name)
            else {
                // The list did not change.
                return;
            };

            // Update the list in the settings.
            let settings = session.settings();
            let mut servers = settings.explore_custom_servers();
            servers.shift_remove(server_name);
            settings.set_explore_custom_servers(servers);

            let pos = self.third_party_networks.borrow().len() + pos + 1;
            self.obj().items_changed(pos as u32, 1, 0);
        }
    }
}

glib::wrapper! {
    /// The list of servers to explore.
    pub struct ExploreServerList(ObjectSubclass<imp::ExploreServerList>)
        @implements gio::ListModel;
}

impl ExploreServerList {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Whether this list contains the given Matrix server.
    pub(crate) fn contains_matrix_server(&self, server_name: &ServerName) -> bool {
        self.imp().contains_matrix_server(server_name)
    }

    /// Add a custom Matrix server.
    pub(crate) fn add_custom_server(&self, server_name: OwnedServerName) {
        self.imp().add_custom_server(server_name);
    }

    /// Remove a custom Matrix server.
    pub(crate) fn remove_custom_server(&self, server_name: &ServerName) {
        self.imp().remove_custom_server(server_name);
    }
}

impl Default for ExploreServerList {
    fn default() -> Self {
        Self::new()
    }
}
