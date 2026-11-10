use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;
use ruma::ServerName;
use tracing::error;

use super::{ExploreServer, ExploreServerList, ExploreServerRow};
use crate::session::Session;

mod imp {
    use std::marker::PhantomData;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/explore/servers_popover.ui")]
    #[properties(wrapper_type = super::ExploreServersPopover)]
    pub struct ExploreServersPopover {
        #[template_child]
        pub(super) listbox: TemplateChild<gtk::ListBox>,
        #[template_child]
        server_entry: TemplateChild<gtk::Entry>,
        /// The current session.
        #[property(get, set = Self::set_session, explicit_notify)]
        session: glib::WeakRef<Session>,
        /// The server list.
        #[property(get)]
        server_list: ExploreServerList,
        /// The selected server, if any.
        #[property(get = Self::selected_server)]
        selected_server: PhantomData<Option<ExploreServer>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ExploreServersPopover {
        const NAME: &'static str = "ExploreServersPopover";
        type Type = super::ExploreServersPopover;
        type ParentType = gtk::Popover;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.install_action("explore-servers-popover.add-server", None, |obj, _, _| {
                obj.imp().add_server();
            });

            klass.install_action(
                "explore-servers-popover.remove-server",
                Some(&String::static_variant_type()),
                |obj, _, variant| {
                    let Some(value) = variant.and_then(String::from_variant) else {
                        error!("Could not remove server without a server name");
                        return;
                    };
                    let Ok(server_name) = ServerName::parse(&value) else {
                        error!("Could not remove server with an invalid server name");
                        return;
                    };

                    obj.imp().remove_server(&server_name);
                },
            );
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for ExploreServersPopover {
        fn constructed(&self) {
            self.parent_constructed();

            self.listbox.bind_model(Some(&self.server_list), |obj| {
                let Some(server) = obj.downcast_ref::<ExploreServer>() else {
                    error!("explore servers GtkListBox did not receive an ExploreServer");
                    return adw::Bin::new().upcast();
                };

                ExploreServerRow::new(server).upcast()
            });

            self.update_add_server_state();
        }
    }

    impl WidgetImpl for ExploreServersPopover {}
    impl PopoverImpl for ExploreServersPopover {}

    #[gtk::template_callbacks]
    impl ExploreServersPopover {
        /// Set the current session.
        fn set_session(&self, session: &Session) {
            if self.session.upgrade().as_ref() == Some(session) {
                return;
            }

            self.session.set(Some(session));
            self.server_list.set_session(session);

            // Select the first server by default.
            self.listbox
                .select_row(self.listbox.row_at_index(0).as_ref());

            self.obj().notify_session();
        }

        /// Handle when the selected server has changed.
        #[template_callback]
        fn selected_server_changed(&self) {
            self.obj().notify_selected_server();
        }

        /// Handle when the user selected a server.
        #[template_callback]
        fn server_activated(&self) {
            self.obj().popdown();
        }

        /// The server that is currently selected, if any.
        fn selected_server(&self) -> Option<ExploreServer> {
            self.listbox
                .selected_row()
                .and_downcast_ref()
                .and_then(ExploreServerRow::server)
        }

        /// Whether the server currently in the text entry can be added.
        fn can_add_server(&self) -> bool {
            let Ok(server_name) = ServerName::parse(self.server_entry.text()) else {
                return false;
            };

            // Don't allow duplicates
            !self.server_list.contains_matrix_server(&server_name)
        }

        /// Update the state of the action to add a server according to the
        /// current state.
        #[template_callback]
        fn update_add_server_state(&self) {
            self.obj()
                .action_set_enabled("explore-servers-popover.add-server", self.can_add_server());
        }

        /// Add the server currently in the text entry.
        #[template_callback]
        fn add_server(&self) {
            if !self.can_add_server() {
                return;
            }

            let Ok(server_name) = ServerName::parse(self.server_entry.text()) else {
                return;
            };
            self.server_entry.set_text("");

            self.server_list.add_custom_server(server_name);

            // Select the new server, it should be the last row in the list.
            let index = i32::try_from(self.server_list.n_items()).unwrap_or(i32::MAX);
            self.listbox
                .select_row(self.listbox.row_at_index(index - 1).as_ref());
        }

        /// Remove the given server.
        fn remove_server(&self, server_name: &ServerName) {
            // If the selected server is gonna be removed, select the first one.
            if self
                .selected_server()
                .as_ref()
                .and_then(|server| server.server())
                .is_some_and(|s| s == server_name)
            {
                self.listbox
                    .select_row(self.listbox.row_at_index(0).as_ref());
            }

            self.server_list.remove_custom_server(server_name);
        }
    }
}

glib::wrapper! {
    /// A popover that lists the servers that can be explored.
    pub struct ExploreServersPopover(ObjectSubclass<imp::ExploreServersPopover>)
        @extends gtk::Widget, gtk::Popover,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native, gtk::ShortcutManager;
}

impl ExploreServersPopover {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }
}
