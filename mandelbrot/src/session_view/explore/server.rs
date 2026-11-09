use gtk::{glib, prelude::*, subclass::prelude::*};
use ruma::OwnedServerName;

mod imp {
    use std::{cell::OnceCell, marker::PhantomData};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::ExploreServer)]
    pub struct ExploreServer {
        /// The server to query.
        ///
        /// If this is `None`, our own homeserver will be queried.
        server: OnceCell<OwnedServerName>,
        /// The server to query, as a string.
        #[property(get = Self::server_string)]
        server_string: PhantomData<Option<String>>,
        /// The third-party network to query, if any.
        ///
        /// If this is `None`, the Matrix network will be queried.
        #[property(get, construct_only)]
        third_party_network: OnceCell<Option<String>>,
        /// The name of the server to display in the list.
        #[property(get, construct_only)]
        name: OnceCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ExploreServer {
        const NAME: &'static str = "ExploreServer";
        type Type = super::ExploreServer;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ExploreServer {}

    impl ExploreServer {
        /// Initialize the server and network.
        pub(super) fn init_server(&self, server: OwnedServerName) {
            self.server
                .set(server)
                .expect("server should not be initialized");
        }

        /// The server to query.
        ///
        /// If this is `None`, our own homeserver will be queried.
        pub(super) fn server(&self) -> Option<&OwnedServerName> {
            self.server.get()
        }

        /// The server to query, as a string.
        fn server_string(&self) -> Option<String> {
            self.server().map(ToString::to_string)
        }
    }
}

glib::wrapper! {
    /// A server with an optional third-party network that can be queried to search for public rooms.
    pub struct ExploreServer(ObjectSubclass<imp::ExploreServer>);
}

impl ExploreServer {
    /// Construct an `ExploreServer` for the Matrix network on the default
    /// server.
    pub(crate) fn with_default_server(name: &str) -> Self {
        glib::Object::builder().property("name", name).build()
    }

    /// Construct an `ExploreServer` for the given third-party protocol on the
    /// default server.
    pub(crate) fn with_third_party_protocol(
        desc: &str,
        protocol_id: &str,
        instance_id: &str,
    ) -> Self {
        glib::Object::builder()
            .property("name", format!("{desc} ({protocol_id})"))
            .property("third-party-network", instance_id)
            .build()
    }

    /// Construct an `ExploreServer` for the Matrix network on the given server.
    pub(crate) fn with_server(server: OwnedServerName) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("name", server.as_str())
            .build();
        obj.imp().init_server(server);
        obj
    }

    /// The server to query.
    ///
    /// If this is `None`, our own homeserver will be queried.
    pub(crate) fn server(&self) -> Option<&OwnedServerName> {
        self.imp().server()
    }
}
