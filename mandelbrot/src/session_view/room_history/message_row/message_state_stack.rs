use adw::subclass::prelude::*;
use gtk::{glib, glib::clone, prelude::*};

use crate::session::MessageState;

/// The number of seconds for which we show the icon acknowledging that the
/// message was sent.
const SENT_VISIBLE_SECONDS: u32 = 3;

mod imp {
    use std::cell::Cell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_row/message_state_stack.ui"
    )]
    #[properties(wrapper_type = super::MessageStateStack)]
    pub struct MessageStateStack {
        /// The state that is currently displayed.
        #[property(get, set = Self::set_state, explicit_notify, builder(MessageState::default()))]
        state: Cell<MessageState>,
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageStateStack {
        const NAME: &'static str = "MessageStateStack";
        type Type = super::MessageStateStack;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MessageStateStack {}

    impl WidgetImpl for MessageStateStack {}
    impl BinImpl for MessageStateStack {}

    impl MessageStateStack {
        /// Set the state to display.
        fn set_state(&self, state: MessageState) {
            let prev_state = self.state.get();

            if prev_state == state {
                return;
            }

            let stack = &self.stack;

            let name = match state {
                MessageState::None => {
                    if matches!(
                        prev_state,
                        MessageState::Sending
                            | MessageState::RecoverableError
                            | MessageState::PermanentError
                    ) {
                        // Show the sent icon for a few seconds.
                        glib::timeout_add_seconds_local_once(
                            SENT_VISIBLE_SECONDS,
                            clone!(
                                #[weak]
                                stack,
                                move || {
                                    stack.set_visible_child_name("none");
                                }
                            ),
                        );

                        "sent"
                    } else {
                        "none"
                    }
                }
                MessageState::Sending => "sending",
                MessageState::RecoverableError => "warning",
                MessageState::PermanentError => "error",
                MessageState::Edited => {
                    if matches!(
                        prev_state,
                        MessageState::Sending
                            | MessageState::RecoverableError
                            | MessageState::PermanentError
                    ) {
                        // Show the sent icon for a few seconds.
                        glib::timeout_add_seconds_local_once(
                            SENT_VISIBLE_SECONDS,
                            clone!(
                                #[weak]
                                stack,
                                move || {
                                    stack.set_visible_child_name("edited");
                                }
                            ),
                        );

                        "sent"
                    } else {
                        "edited"
                    }
                }
            };
            stack.set_visible_child_name(name);

            self.state.set(state);
            self.obj().notify_state();
        }
    }
}

glib::wrapper! {
    /// A stack to display the different message states.
    pub struct MessageStateStack(ObjectSubclass<imp::MessageStateStack>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MessageStateStack {
    /// Create a new `MessageStateStack`.
    pub fn new() -> Self {
        glib::Object::new()
    }
}
