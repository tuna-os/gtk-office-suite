use gtk::{glib, prelude::*, subclass::prelude::*};

use super::ExploreServer;
use crate::utils::TemplateCallbacks;

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/explore/server_row.ui")]
    #[properties(wrapper_type = super::ExploreServerRow)]
    pub struct ExploreServerRow {
        #[template_child]
        remove_button: TemplateChild<gtk::Button>,
        /// The server displayed by this row.
        #[property(get, set = Self::set_server, construct_only)]
        server: RefCell<Option<ExploreServer>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ExploreServerRow {
        const NAME: &'static str = "ExploreServerRow";
        type Type = super::ExploreServerRow;
        type ParentType = gtk::ListBoxRow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            TemplateCallbacks::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for ExploreServerRow {}

    impl WidgetImpl for ExploreServerRow {}
    impl ListBoxRowImpl for ExploreServerRow {}

    impl ExploreServerRow {
        /// Set the server displayed by this row.
        fn set_server(&self, server: ExploreServer) {
            if let Some(server_string) = server.server_string() {
                self.remove_button.set_action_target(Some(server_string));
                self.remove_button
                    .set_action_name(Some("explore-servers-popover.remove-server"));
            }

            self.server.replace(Some(server));
        }
    }
}

glib::wrapper! {
    /// A row representing a server to explore.
    pub struct ExploreServerRow(ObjectSubclass<imp::ExploreServerRow>)
        @extends gtk::Widget, gtk::ListBoxRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl ExploreServerRow {
    pub fn new(server: &ExploreServer) -> Self {
        glib::Object::builder().property("server", server).build()
    }
}
