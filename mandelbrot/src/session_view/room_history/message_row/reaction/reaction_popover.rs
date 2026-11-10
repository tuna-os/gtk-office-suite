use adw::{prelude::*, subclass::prelude::*};
use gtk::{gio, glib, glib::clone};

use crate::{
    components::UserProfileDialog,
    session_view::room_history::member_timestamp::{MemberTimestamp, row::MemberTimestampRow},
};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_row/reaction/reaction_popover.ui"
    )]
    #[properties(wrapper_type = super::ReactionPopover)]
    pub struct ReactionPopover {
        #[template_child]
        list: TemplateChild<gtk::ListView>,
        /// The reaction senders to display.
        #[property(get, set = Self::set_senders, construct_only)]
        senders: glib::WeakRef<gio::ListStore>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ReactionPopover {
        const NAME: &'static str = "ContentMessageReactionPopover";
        type Type = super::ReactionPopover;
        type ParentType = gtk::Popover;

        fn class_init(klass: &mut Self::Class) {
            MemberTimestampRow::ensure_type();

            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for ReactionPopover {}

    impl WidgetImpl for ReactionPopover {}
    impl PopoverImpl for ReactionPopover {}

    impl ReactionPopover {
        /// Set the reaction senders to display.
        fn set_senders(&self, senders: gio::ListStore) {
            self.senders.set(Some(&senders));
            self.list
                .set_model(Some(&gtk::NoSelection::new(Some(senders))));
            self.list.connect_activate(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, pos| {
                    let Some(member) = imp
                        .senders
                        .upgrade()
                        .and_then(|list| list.item(pos))
                        .and_downcast::<MemberTimestamp>()
                        .and_then(|ts| ts.member())
                    else {
                        return;
                    };

                    let obj = imp.obj();

                    let dialog = UserProfileDialog::new();
                    dialog.set_room_member(member);
                    dialog.present(Some(&*obj));

                    obj.popdown();
                }
            ));
        }
    }
}

glib::wrapper! {
    /// A popover to display the senders of a reaction.
    pub struct ReactionPopover(ObjectSubclass<imp::ReactionPopover>)
        @extends gtk::Widget, gtk::Popover,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native, gtk::ShortcutManager;
}

impl ReactionPopover {
    /// Constructs a new `ReactionPopover` with the given reaction senders.
    pub fn new(senders: &gio::ListStore) -> Self {
        glib::Object::builder().property("senders", senders).build()
    }
}
