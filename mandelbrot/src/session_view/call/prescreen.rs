use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};

use super::state::CallState;

mod imp {
    use std::{cell::RefCell, sync::LazyLock};

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/call/prescreen.ui")]
    #[properties(wrapper_type = super::CallPrescreen)]
    pub struct CallPrescreen {
        #[template_child]
        preview_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        preview_picture: TemplateChild<gtk::Picture>,
        #[template_child]
        microphone_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        camera_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        join_button: TemplateChild<gtk::Button>,
        /// The call state driven by this prescreen.
        #[property(get, set = Self::set_state, explicit_notify, nullable)]
        state: RefCell<Option<CallState>>,
        state_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CallPrescreen {
        const NAME: &'static str = "CallPrescreen";
        type Type = super::CallPrescreen;
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
    impl ObjectImpl for CallPrescreen {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("join").build()]);
            &SIGNALS
        }

        fn dispose(&self) {
            self.disconnect_signals();
        }
    }

    impl WidgetImpl for CallPrescreen {
        fn grab_focus(&self) -> bool {
            self.join_button.grab_focus()
        }
    }

    impl BinImpl for CallPrescreen {}

    #[gtk::template_callbacks]
    impl CallPrescreen {
        /// Set the call state driven by this prescreen.
        fn set_state(&self, state: Option<CallState>) {
            if *self.state.borrow() == state {
                return;
            }

            self.disconnect_signals();

            if let Some(state) = &state {
                let muted_handler = state.connect_muted_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_toggles();
                    }
                ));
                let camera_handler = state.connect_camera_on_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_toggles();
                    }
                ));
                let paintable_handler = state.connect_self_paintable_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_preview();
                    }
                ));
                self.state_handlers
                    .replace(vec![muted_handler, camera_handler, paintable_handler]);
            }

            self.state.replace(state);
            self.update_toggles();
            self.update_preview();
            self.obj().notify_state();
        }

        /// Update the camera preview from the state.
        fn update_preview(&self) {
            let paintable = self
                .state
                .borrow()
                .as_ref()
                .and_then(CallState::self_paintable);
            self.preview_picture.set_paintable(paintable.as_ref());
        }

        /// Update the toggle buttons and preview from the state.
        fn update_toggles(&self) {
            let state = self.state.borrow();
            let (muted, camera_on) = state
                .as_ref()
                .map_or((false, false), |state| (state.muted(), state.camera_on()));

            self.microphone_button.set_active(!muted);
            self.microphone_button.set_icon_name(if muted {
                "microphone-disabled-symbolic"
            } else {
                "microphone-symbolic"
            });

            self.camera_button.set_active(camera_on);
            self.camera_button.set_icon_name(if camera_on {
                "camera-web-symbolic"
            } else {
                "camera-disabled-symbolic"
            });

            let page = if camera_on { "preview" } else { "off" };
            self.preview_stack.set_visible_child_name(page);
        }

        /// Handle when the microphone toggle was activated.
        #[template_callback]
        fn microphone_toggled(&self) {
            if let Some(state) = self.state.borrow().as_ref() {
                state.set_muted(!self.microphone_button.is_active());
            }
        }

        /// Handle when the camera toggle was activated.
        #[template_callback]
        fn camera_toggled(&self) {
            if let Some(state) = self.state.borrow().as_ref() {
                state.set_camera_on(self.camera_button.is_active());
            }
        }

        /// Handle when the join button was activated.
        #[template_callback]
        fn join_clicked(&self) {
            self.obj().emit_by_name::<()>("join", &[]);
        }

        /// Disconnect the signal handlers of the current state.
        fn disconnect_signals(&self) {
            if let Some(state) = self.state.borrow().as_ref() {
                for handler in self.state_handlers.take() {
                    state.disconnect(handler);
                }
            }
        }
    }
}

glib::wrapper! {
    /// A view to configure the microphone and camera before joining a call.
    pub struct CallPrescreen(ObjectSubclass<imp::CallPrescreen>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CallPrescreen {
    /// Create a new prescreen for the given call state.
    pub fn new(state: &CallState) -> Self {
        glib::Object::builder().property("state", state).build()
    }

    /// Connect to the signal emitted when the user wants to join the call.
    pub fn connect_join<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "join",
            true,
            glib::closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}

impl Default for CallPrescreen {
    fn default() -> Self {
        glib::Object::new()
    }
}
