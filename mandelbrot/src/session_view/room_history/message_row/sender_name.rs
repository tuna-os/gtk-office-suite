use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use crate::{
    prelude::*,
    session::{Member, Membership},
    session_view::RoomHistory,
    utils::{BoundObject, key_bindings},
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_row/sender_name.ui"
    )]
    #[properties(wrapper_type = super::MessageSenderName)]
    pub struct MessageSenderName {
        #[template_child]
        label: TemplateChild<gtk::Label>,
        /// The displayed member.
        #[property(get, set = Self::set_sender, explicit_notify, nullable)]
        sender: BoundObject<Member>,
        permissions_handler: RefCell<Option<glib::SignalHandlerId>>,
        gesture_click: RefCell<Option<gtk::GestureClick>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageSenderName {
        const NAME: &'static str = "ContentMessageSenderName";
        type Type = super::MessageSenderName;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("message-sender");

            klass.install_action("message-sender.activate", None, |obj, _, _| {
                obj.imp().mention_sender();
            });

            key_bindings::add_activate_bindings(klass, "message-sender.activate");

            klass.set_accessible_role(gtk::AccessibleRole::Button);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MessageSenderName {}

    impl WidgetImpl for MessageSenderName {
        fn focus(&self, _direction_type: gtk::DirectionType) -> bool {
            // Regardless of the direction, we can only focus this widget and no children.
            let obj = self.obj();
            if obj.is_focus() {
                false
            } else {
                obj.grab_focus()
            }
        }
    }

    impl BinImpl for MessageSenderName {}

    #[gtk::template_callbacks]
    impl MessageSenderName {
        /// Set the displayed member.
        fn set_sender(&self, sender: Option<Member>) {
            let prev_sender = self.sender.obj();

            if prev_sender == sender {
                return;
            }

            self.disconnect_signals();

            if let Some(sender) = sender {
                let room = sender.room();

                let permissions_handler = room.permissions().connect_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_activatable();
                    }
                ));
                self.permissions_handler.replace(Some(permissions_handler));

                let membership_handler = sender.connect_membership_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_activatable();
                    }
                ));

                let is_ignored_handler = sender.connect_is_ignored_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_activatable();
                    }
                ));

                self.sender
                    .set(sender, vec![membership_handler, is_ignored_handler]);
            }

            self.update_activatable();
            self.obj().notify_sender();
        }

        /// Disconnect all the signals.
        fn disconnect_signals(&self) {
            if let Some(sender) = self.sender.obj() {
                let room = sender.room();

                if let Some(handler) = self.permissions_handler.take() {
                    room.permissions().disconnect(handler);
                }
            }

            self.sender.disconnect_signals();
        }

        /// Update whether this widget is activatable.
        fn update_activatable(&self) {
            let activatable = self.sender.obj().is_some_and(|sender| {
                !sender.is_own_user()
                    && sender.membership() == Membership::Join
                    && sender.room().permissions().can_send_message()
            });
            let prev_activatable = self.gesture_click.borrow().is_some();

            let obj = self.obj();
            obj.action_set_enabled("message-sender.activate", activatable);

            if activatable == prev_activatable {
                // Nothing to update.
                return;
            }

            if activatable {
                let gesture_click = gtk::GestureClick::new();

                gesture_click.connect_released(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, _, _, _| {
                        imp.mention_sender();
                    }
                ));

                obj.add_controller(gesture_click.clone());
                self.gesture_click.replace(Some(gesture_click));
            } else if let Some(gesture_click) = self.gesture_click.take() {
                obj.remove_controller(&gesture_click);
            }

            let role = if activatable {
                gtk::AccessibleRole::Button
            } else {
                gtk::AccessibleRole::Label
            };
            obj.set_accessible_role(role);

            if activatable {
                obj.add_css_class("activatable");
            } else {
                obj.remove_css_class("activatable");
            }

            // Translators: This is a verb, as in 'Mention user'.
            let tooltip_text = activatable.then(|| gettext("Mention"));
            obj.set_tooltip_text(tooltip_text.as_deref());
        }

        /// Mention the sender.
        #[template_callback]
        fn mention_sender(&self) {
            let Some(sender) = self.sender.obj() else {
                return;
            };
            let Some(room_history) = self
                .obj()
                .ancestor(RoomHistory::static_type())
                .and_downcast::<RoomHistory>()
            else {
                return;
            };

            room_history.message_toolbar().mention_member(&sender);
        }
    }
}

glib::wrapper! {
    /// A widget displaying the name of a sender in the timeline.
    pub struct MessageSenderName(ObjectSubclass<imp::MessageSenderName>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MessageSenderName {
    /// Create a new `MessageSenderName`.
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for MessageSenderName {
    fn default() -> Self {
        Self::new()
    }
}
