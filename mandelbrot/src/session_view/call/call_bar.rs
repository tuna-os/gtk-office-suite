use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use super::state::{CallConnectionState, CallState};
use crate::i18n::ngettext_f;

mod imp {
    use std::{cell::RefCell, sync::LazyLock};

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/call/call_bar.ui")]
    #[properties(wrapper_type = super::CallBar)]
    pub struct CallBar {
        #[template_child]
        title_label: TemplateChild<gtk::Label>,
        #[template_child]
        subtitle_label: TemplateChild<gtk::Label>,
        /// The call state displayed by this bar.
        #[property(get, set = Self::set_state, explicit_notify, nullable)]
        state: RefCell<Option<CallState>>,
        state_handlers: RefCell<Vec<glib::SignalHandlerId>>,
        participants_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CallBar {
        const NAME: &'static str = "CallBar";
        type Type = super::CallBar;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for CallBar {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("return-requested").build()]);
            &SIGNALS
        }

        fn dispose(&self) {
            self.disconnect_signals();
        }
    }

    impl WidgetImpl for CallBar {}
    impl BinImpl for CallBar {}

    #[gtk::template_callbacks]
    impl CallBar {
        /// Set the call state displayed by this bar.
        fn set_state(&self, state: Option<CallState>) {
            if *self.state.borrow() == state {
                return;
            }

            self.disconnect_signals();

            if let Some(state) = &state {
                let duration_handler = state.connect_duration_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_labels();
                    }
                ));
                let connection_handler = state.connect_connection_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_labels();
                    }
                ));
                self.state_handlers
                    .replace(vec![duration_handler, connection_handler]);

                let participants_handler = state.participants().connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, _, _, _| {
                        imp.update_labels();
                    }
                ));
                self.participants_handler
                    .replace(Some(participants_handler));
            }

            self.state.replace(state);
            self.update_labels();
            self.obj().notify_state();
        }

        /// Update the labels and visibility of this bar from the state.
        fn update_labels(&self) {
            let state = self.state.borrow();
            let Some(state) = state.as_ref() else {
                self.obj().set_visible(false);
                return;
            };

            let connected = matches!(
                state.connection_state(),
                CallConnectionState::Connected | CallConnectionState::Reconnecting
            );
            self.obj().set_visible(connected);

            if !connected {
                return;
            }

            let count = state.participant_count();
            let participants = ngettext_f(
                "{n} participant",
                "{n} participants",
                count,
                &[("n", &count.to_string())],
            );
            let duration = CallState::format_duration(state.duration());
            self.subtitle_label
                .set_label(&format!("{participants} · {duration}"));

            let room_name = state.room_name();
            if room_name.is_empty() {
                self.title_label.set_label(&gettext("Ongoing call"));
            } else {
                self.title_label.set_label(&room_name);
            }
        }

        /// Handle when the return button was activated.
        #[template_callback]
        fn return_clicked(&self) {
            self.obj().emit_by_name::<()>("return-requested", &[]);
        }

        /// Handle when the hang-up button was activated.
        #[template_callback]
        fn hang_up_clicked(&self) {
            if let Some(state) = self.state.borrow().as_ref() {
                state.hang_up();
            }
        }

        /// Disconnect the signal handlers of the current state.
        fn disconnect_signals(&self) {
            if let Some(state) = self.state.borrow().as_ref() {
                for handler in self.state_handlers.take() {
                    state.disconnect(handler);
                }

                if let Some(handler) = self.participants_handler.take() {
                    state.participants().disconnect(handler);
                }
            }
        }
    }
}

glib::wrapper! {
    /// A compact bar presenting an ongoing call.
    ///
    /// Meant to be embedded above the room history when the user navigates
    /// away from the call view.
    pub struct CallBar(ObjectSubclass<imp::CallBar>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CallBar {
    /// Create a new call bar for the given call state.
    pub fn new(state: &CallState) -> Self {
        glib::Object::builder().property("state", state).build()
    }

    /// Connect to the signal emitted when the user wants to return to the
    /// call view.
    pub fn connect_return_requested<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "return-requested",
            true,
            glib::closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}
