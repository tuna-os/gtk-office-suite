//! Native call UI.
//!
//! These widgets are driven entirely by a [`CallState`] model, which the
//! session's [`CallManager`](crate::session::CallManager) binds to the
//! `mandelbrot-matrixrtc` engine and to the media connection.

use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

mod call_bar;
mod dialog;
mod participant_tile;
mod prescreen;
mod state;

#[allow(unused_imports)]
pub(crate) use self::{
    call_bar::CallBar,
    dialog::CallDialog,
    participant_tile::CallParticipantTile,
    prescreen::CallPrescreen,
    state::{CallConnectionState, CallParticipant, CallState},
};
use crate::session::{Room, Session};

/// Present the call dialog for the given room.
///
/// Joining from the prescreen starts the call via the session's
/// [`CallManager`](crate::session::CallManager).
pub(crate) fn present_call_dialog(parent: &impl IsA<gtk::Widget>, session: &Session, room: &Room) {
    let call_manager = session.call_manager();
    let Some(state) = call_manager.get_or_create_call_state(room.room_id()) else {
        return;
    };

    let dialog = CallDialog::new(&state);
    let room_id = room.room_id().to_owned();
    dialog.connect_join(clone!(
        #[weak]
        call_manager,
        move |_| {
            call_manager.join_call(&room_id);
        }
    ));
    dialog.present(Some(parent));
}

/// The delay after which the bars are hidden when there is no motion, in
/// seconds.
const BARS_HIDE_TIMEOUT_SECS: u32 = 3;

mod imp {
    use std::{cell::RefCell, sync::LazyLock};

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/call/mod.ui")]
    #[properties(wrapper_type = super::CallView)]
    pub struct CallView {
        #[template_child]
        participants_grid: TemplateChild<gtk::FlowBox>,
        #[template_child]
        self_view: TemplateChild<adw::Bin>,
        #[template_child]
        self_view_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        self_picture: TemplateChild<gtk::Picture>,
        #[template_child]
        top_bar_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        bottom_bar_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        microphone_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        camera_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        room_name_label: TemplateChild<gtk::Label>,
        #[template_child]
        status_label: TemplateChild<gtk::Label>,
        #[template_child]
        encryption_icon: TemplateChild<gtk::Image>,
        /// The call state driving this view.
        #[property(get, set = Self::set_state, explicit_notify, nullable)]
        state: RefCell<Option<CallState>>,
        state_handlers: RefCell<Vec<glib::SignalHandlerId>>,
        hide_source: RefCell<Option<glib::SourceId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CallView {
        const NAME: &'static str = "CallView";
        type Type = super::CallView;
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
    impl ObjectImpl for CallView {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("pip-requested").build()]);
            &SIGNALS
        }

        fn dispose(&self) {
            self.disconnect_signals();

            if let Some(source) = self.hide_source.take() {
                source.remove();
            }
        }
    }

    impl WidgetImpl for CallView {}
    impl BinImpl for CallView {}

    #[gtk::template_callbacks]
    impl CallView {
        /// Set the call state driving this view.
        fn set_state(&self, state: Option<CallState>) {
            if *self.state.borrow() == state {
                return;
            }

            self.disconnect_signals();

            if let Some(state) = &state {
                self.participants_grid
                    .bind_model(Some(&state.participants()), |item| {
                        item.downcast_ref::<CallParticipant>()
                            .map(CallParticipantTile::new)
                            .unwrap_or_default()
                            .upcast()
                    });

                let mut handlers = Vec::new();
                handlers.push(state.connect_muted_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_controls();
                    }
                )));
                handlers.push(state.connect_camera_on_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_controls();
                        imp.schedule_bars_hide();
                    }
                )));
                handlers.push(state.connect_connection_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_labels();
                    }
                )));
                handlers.push(state.connect_duration_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_labels();
                    }
                )));
                handlers.push(state.connect_room_name_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_labels();
                    }
                )));
                handlers.push(state.connect_encrypted_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_labels();
                    }
                )));
                handlers.push(state.connect_self_paintable_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_self_view();
                    }
                )));
                self.state_handlers.replace(handlers);
            } else {
                self.participants_grid
                    .bind_model(gtk::gio::ListModel::NONE, |_| adw::Bin::new().upcast());
            }

            self.state.replace(state);
            self.update_controls();
            self.update_labels();
            self.update_self_view();
            self.obj().notify_state();
        }

        /// Update the self-view from the state.
        fn update_self_view(&self) {
            let paintable = self
                .state
                .borrow()
                .as_ref()
                .and_then(CallState::self_paintable);
            self.self_picture.set_paintable(paintable.as_ref());
        }

        /// Update the control buttons and the self-view from the state.
        fn update_controls(&self) {
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

            let page = if camera_on { "video" } else { "off" };
            self.self_view_stack.set_visible_child_name(page);
            self.self_view.set_visible(state.is_some());
        }

        /// Update the labels of the top bar from the state.
        fn update_labels(&self) {
            let state = self.state.borrow();
            let Some(state) = state.as_ref() else {
                self.room_name_label.set_label("");
                self.status_label.set_label("");
                self.encryption_icon.set_visible(false);
                return;
            };

            self.room_name_label.set_label(&state.room_name());
            self.encryption_icon.set_visible(state.encrypted());

            let status = match state.connection_state() {
                CallConnectionState::Disconnected => gettext("Disconnected"),
                CallConnectionState::Connecting => gettext("Connecting…"),
                CallConnectionState::Connected => CallState::format_duration(state.duration()),
                CallConnectionState::Reconnecting => gettext("Reconnecting…"),
                CallConnectionState::Failed => gettext("Connection failed"),
            };
            self.status_label.set_label(&status);
        }

        /// Whether the bars should hide automatically.
        ///
        /// The bars only hide when a camera is enabled, so that they never
        /// disappear during a voice-only call.
        fn should_auto_hide(&self) -> bool {
            self.state
                .borrow()
                .as_ref()
                .is_some_and(CallState::camera_on)
        }

        /// Reveal the bars and schedule hiding them again.
        fn reveal_bars(&self) {
            self.top_bar_revealer.set_reveal_child(true);
            self.bottom_bar_revealer.set_reveal_child(true);
            self.schedule_bars_hide();
        }

        /// Schedule hiding the bars after a delay, if they should hide.
        fn schedule_bars_hide(&self) {
            if let Some(source) = self.hide_source.take() {
                source.remove();
            }

            if !self.should_auto_hide() {
                self.top_bar_revealer.set_reveal_child(true);
                self.bottom_bar_revealer.set_reveal_child(true);
                return;
            }

            let source = glib::timeout_add_seconds_local_once(
                BARS_HIDE_TIMEOUT_SECS,
                clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move || {
                        imp.hide_source.take();

                        if imp.should_auto_hide() {
                            imp.top_bar_revealer.set_reveal_child(false);
                            imp.bottom_bar_revealer.set_reveal_child(false);
                        }
                    }
                ),
            );
            self.hide_source.replace(Some(source));
        }

        /// Handle motion over the view.
        #[template_callback]
        fn on_motion(&self, _x: f64, _y: f64) {
            self.reveal_bars();
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

        /// Handle when the back-to-chat button was activated.
        #[template_callback]
        fn pip_clicked(&self) {
            self.obj().emit_by_name::<()>("pip-requested", &[]);
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
            }
        }
    }
}

glib::wrapper! {
    /// The main view of an ongoing call.
    pub struct CallView(ObjectSubclass<imp::CallView>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CallView {
    /// Create a new call view for the given call state.
    pub fn new(state: &CallState) -> Self {
        glib::Object::builder().property("state", state).build()
    }

    /// Connect to the signal emitted when the user wants to collapse the call
    /// view and return to the chat.
    pub fn connect_pip_requested<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "pip-requested",
            true,
            glib::closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}

impl Default for CallView {
    fn default() -> Self {
        glib::Object::new()
    }
}
