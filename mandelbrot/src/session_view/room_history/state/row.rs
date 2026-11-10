use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use super::StateContent;
use crate::{session::Event, session_view::room_history::ReadReceiptsList};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/state/row.ui")]
    #[properties(wrapper_type = super::StateRow)]
    pub struct StateRow {
        /// The state event displayed by this widget.
        #[property(get, set)]
        event: RefCell<Option<Event>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StateRow {
        const NAME: &'static str = "ContentStateRow";
        type Type = super::StateRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            ReadReceiptsList::ensure_type();
            StateContent::ensure_type();

            Self::bind_template(klass);
            klass.set_css_name("state-row");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for StateRow {}

    impl WidgetImpl for StateRow {}
    impl BinImpl for StateRow {}
}

glib::wrapper! {
    /// A row presenting a state event.
    pub struct StateRow(ObjectSubclass<imp::StateRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl StateRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for StateRow {
    fn default() -> Self {
        Self::new()
    }
}
