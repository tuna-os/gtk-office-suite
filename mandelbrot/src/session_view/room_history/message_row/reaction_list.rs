use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};

use super::reaction::MessageReaction;
use crate::session::{MemberList, ReactionList};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_row/reaction_list.ui"
    )]
    pub struct MessageReactionList {
        #[template_child]
        flow_box: TemplateChild<gtk::FlowBox>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageReactionList {
        const NAME: &'static str = "ContentMessageReactionList";
        type Type = super::MessageReactionList;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_css_name("message-reactions");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for MessageReactionList {}
    impl WidgetImpl for MessageReactionList {}
    impl BinImpl for MessageReactionList {}

    impl MessageReactionList {
        /// Set the list of reactions.
        pub(super) fn set_reaction_list(&self, members: &MemberList, reaction_list: &ReactionList) {
            self.flow_box.bind_model(
                Some(reaction_list),
                clone!(
                    #[weak]
                    members,
                    #[upgrade_or_else]
                    || { gtk::FlowBoxChild::new().upcast() },
                    move |obj| {
                        MessageReaction::new(
                            members,
                            obj.clone()
                                .downcast()
                                .expect("reaction list item is a reaction group"),
                        )
                        .upcast()
                    }
                ),
            );
        }
    }
}

glib::wrapper! {
    /// A widget displaying the reactions of a message.
    pub struct MessageReactionList(ObjectSubclass<imp::MessageReactionList>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MessageReactionList {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the list of reactions.
    pub(crate) fn set_reaction_list(&self, members: &MemberList, reaction_list: &ReactionList) {
        self.imp().set_reaction_list(members, reaction_list);
    }
}
