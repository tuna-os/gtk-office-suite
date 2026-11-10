use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::glib;
use matrix_sdk::ruma::events::room::create::RoomCreateEventContent;
use ruma::events::StateEventContentChange;

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/state/creation.ui")]
    pub struct StateCreation {
        #[template_child]
        previous_room_btn: TemplateChild<gtk::Button>,
        #[template_child]
        description: TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StateCreation {
        const NAME: &'static str = "ContentStateCreation";
        type Type = super::StateCreation;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for StateCreation {}
    impl WidgetImpl for StateCreation {}
    impl BinImpl for StateCreation {}

    impl StateCreation {
        /// Set the room create state event to display.
        pub(super) fn set_event(&self, event: &StateEventContentChange<RoomCreateEventContent>) {
            let predecessor = match event {
                StateEventContentChange::Original { content, .. } => content.predecessor.as_ref(),
                StateEventContentChange::Redacted(_) => None,
            };

            if let Some(predecessor) = &predecessor {
                self.previous_room_btn.set_detailed_action_name(&format!(
                    "session.show-room::{}",
                    predecessor.room_id
                ));
                self.previous_room_btn.set_visible(true);
                self.description
                    .set_label(&gettext("This conversation started in another room."));
            } else {
                self.previous_room_btn.set_visible(false);
                self.previous_room_btn.set_action_name(None);
                self.description
                    .set_label(&gettext("The conversation starts here."));
            }
        }
    }
}

glib::wrapper! {
    /// A widget presenting a room create state event.
    pub struct StateCreation(ObjectSubclass<imp::StateCreation>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl StateCreation {
    pub fn new(event: &StateEventContentChange<RoomCreateEventContent>) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().set_event(event);
        obj
    }
}
