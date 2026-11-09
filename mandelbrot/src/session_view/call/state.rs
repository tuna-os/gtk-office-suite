//! State model for a native Matrix call.
//!
//! This is a narrow UI-facing model, driven by the
//! [`CallManager`](crate::session::CallManager): the `MembershipsChanged` and
//! `StatusChanged` events of the `mandelbrot_matrixrtc::RtcCallSession`
//! engine map onto the `participants` list model and the `connection-state`
//! property, while the `muted` and `camera-on` properties control the
//! microphone and camera published to the SFU.

use gtk::{gio, glib, prelude::*, subclass::prelude::*};

use crate::components::AvatarData;

/// The connection state of a call.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, glib::Enum)]
#[enum_type(name = "CallConnectionState")]
pub enum CallConnectionState {
    /// No connection to the call.
    #[default]
    Disconnected,
    /// The connection to the SFU is being established.
    Connecting,
    /// The call is connected.
    Connected,
    /// The connection was lost and is being re-established.
    Reconnecting,
    /// The connection failed.
    Failed,
}

mod imp_participant {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::CallParticipant)]
    pub struct CallParticipant {
        /// The identity of this participant on the RTC backend.
        #[property(get, set, construct_only)]
        identity: RefCell<String>,
        /// The display name of this participant.
        #[property(get, set = Self::set_display_name, explicit_notify)]
        display_name: RefCell<String>,
        /// The paintable displaying the video stream of this participant.
        #[property(get, set, nullable)]
        video_paintable: RefCell<Option<gtk::gdk::Paintable>>,
        /// The avatar data of this participant.
        #[property(get, set, nullable)]
        avatar_data: RefCell<Option<AvatarData>>,
        /// Whether this participant is currently speaking.
        #[property(get, set)]
        speaking: Cell<bool>,
        /// Whether this participant's microphone is muted.
        #[property(get, set)]
        muted: Cell<bool>,
        /// Whether this participant's camera is enabled.
        #[property(get, set)]
        camera_on: Cell<bool>,
        /// Whether this participant is the local user.
        #[property(get, set, construct_only)]
        is_local: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CallParticipant {
        const NAME: &'static str = "CallParticipant";
        type Type = super::CallParticipant;
    }

    #[glib::derived_properties]
    impl ObjectImpl for CallParticipant {}

    impl CallParticipant {
        /// Set the display name of this participant.
        fn set_display_name(&self, display_name: String) {
            if *self.display_name.borrow() == display_name {
                return;
            }

            // Keep the avatar fallback initials in sync.
            if let Some(avatar_data) = self.avatar_data.borrow().as_ref() {
                avatar_data.set_display_name(display_name.clone());
            }

            self.display_name.replace(display_name);
            self.obj().notify_display_name();
        }
    }
}

glib::wrapper! {
    /// A single participant of a call.
    pub struct CallParticipant(ObjectSubclass<imp_participant::CallParticipant>);
}

impl CallParticipant {
    /// Create a new participant with the given display name.
    pub fn new(display_name: &str, is_local: bool) -> Self {
        Self::with_identity("", display_name, is_local)
    }

    /// Create a new participant with the given RTC backend identity and
    /// display name.
    pub fn with_identity(identity: &str, display_name: &str, is_local: bool) -> Self {
        let avatar_data = AvatarData::new();
        avatar_data.set_display_name(display_name.to_owned());

        glib::Object::builder()
            .property("identity", identity)
            .property("display-name", display_name)
            .property("avatar-data", avatar_data)
            .property("is-local", is_local)
            .build()
    }
}

mod imp_state {
    use std::{
        cell::{Cell, RefCell},
        sync::LazyLock,
    };

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, glib::Properties)]
    #[properties(wrapper_type = super::CallState)]
    pub struct CallState {
        /// The connection state of the call.
        #[property(get, set = Self::set_connection_state, explicit_notify, builder(CallConnectionState::default()))]
        connection_state: Cell<CallConnectionState>,
        /// Whether the local microphone is muted.
        #[property(get, set = Self::set_muted, explicit_notify)]
        muted: Cell<bool>,
        /// Whether the local camera is enabled.
        #[property(get, set = Self::set_camera_on, explicit_notify)]
        camera_on: Cell<bool>,
        /// The duration of the call, in seconds.
        #[property(get, set = Self::set_duration, explicit_notify)]
        duration: Cell<u64>,
        /// The list of remote participants of the call.
        #[property(get)]
        participants: gio::ListStore,
        /// The name of the room this call takes place in.
        #[property(get, set = Self::set_room_name, explicit_notify)]
        room_name: RefCell<String>,
        /// Whether the media of this call is end-to-end encrypted.
        #[property(get, set)]
        encrypted: Cell<bool>,
        /// The paintable displaying the local camera stream.
        ///
        /// This is set by the
        /// [`CallManager`](crate::session::CallManager) while the local
        /// camera is on, and displayed by the self-view of the call view and
        /// the preview of the prescreen.
        #[property(get, set, nullable)]
        self_paintable: RefCell<Option<gtk::gdk::Paintable>>,
        pub(super) duration_source: RefCell<Option<glib::SourceId>>,
    }

    impl Default for CallState {
        fn default() -> Self {
            Self {
                connection_state: Cell::default(),
                muted: Cell::default(),
                camera_on: Cell::default(),
                duration: Cell::default(),
                participants: gio::ListStore::new::<CallParticipant>(),
                room_name: RefCell::default(),
                encrypted: Cell::default(),
                self_paintable: RefCell::default(),
                duration_source: RefCell::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CallState {
        const NAME: &'static str = "CallState";
        type Type = super::CallState;
    }

    #[glib::derived_properties]
    impl ObjectImpl for CallState {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("ended").build()]);
            &SIGNALS
        }

        fn dispose(&self) {
            if let Some(source) = self.duration_source.take() {
                source.remove();
            }
        }
    }

    impl CallState {
        /// Set the connection state of the call.
        fn set_connection_state(&self, connection_state: CallConnectionState) {
            if self.connection_state.get() == connection_state {
                return;
            }

            self.connection_state.set(connection_state);
            self.obj().notify_connection_state();
        }

        /// Set whether the local microphone is muted.
        fn set_muted(&self, muted: bool) {
            if self.muted.get() == muted {
                return;
            }

            self.muted.set(muted);
            // The media task of the `CallManager` stops sending microphone
            // samples while this is set.
            self.obj().notify_muted();
        }

        /// Set whether the local camera is enabled.
        fn set_camera_on(&self, camera_on: bool) {
            if self.camera_on.get() == camera_on {
                return;
            }

            self.camera_on.set(camera_on);
            // The `CallManager` starts or stops the camera capture, which
            // publishes or unpublishes the camera track.
            self.obj().notify_camera_on();
        }

        /// Set the duration of the call, in seconds.
        fn set_duration(&self, duration: u64) {
            if self.duration.get() == duration {
                return;
            }

            self.duration.set(duration);
            self.obj().notify_duration();
        }

        /// Set the name of the room this call takes place in.
        fn set_room_name(&self, room_name: String) {
            if *self.room_name.borrow() == room_name {
                return;
            }

            self.room_name.replace(room_name);
            self.obj().notify_room_name();
        }
    }
}

glib::wrapper! {
    /// The state of a native Matrix call, driving the call UI.
    pub struct CallState(ObjectSubclass<imp_state::CallState>);
}

impl CallState {
    /// Create a new, disconnected call state.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// The number of participants in the call.
    pub fn participant_count(&self) -> u32 {
        self.participants().n_items()
    }

    /// Populate this state with demo data and start a duration timer.
    ///
    /// This is a placeholder for the engine binding: it simulates a connected
    /// call with a few remote participants so the UI can be exercised without
    /// a `LiveKit` connection.
    pub fn start_demo(&self) {
        let participants = self.participants();
        participants.remove_all();

        let alice = CallParticipant::new("Alice", false);
        alice.set_speaking(true);
        let bob = CallParticipant::new("Bob", false);
        bob.set_muted(true);
        let carol = CallParticipant::new("Carol", false);

        participants.append(&alice);
        participants.append(&bob);
        participants.append(&carol);

        self.set_room_name("Demo Room");
        self.set_encrypted(true);
        self.set_connection_state(CallConnectionState::Connected);
        self.set_duration(0);

        let imp = self.imp();
        if let Some(source) = imp.duration_source.take() {
            source.remove();
        }

        let source = glib::timeout_add_seconds_local(
            1,
            glib::clone!(
                #[weak(rename_to = obj)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    if obj.connection_state() == CallConnectionState::Connected {
                        obj.set_duration(obj.duration() + 1);
                    }
                    glib::ControlFlow::Continue
                }
            ),
        );
        imp.duration_source.replace(Some(source));
    }

    /// Leave the call.
    ///
    /// Integration point: this will call `RtcCallSession::leave()` once the
    /// engine is bound.
    pub fn hang_up(&self) {
        let imp = self.imp();
        if let Some(source) = imp.duration_source.take() {
            source.remove();
        }

        self.set_connection_state(CallConnectionState::Disconnected);
        self.participants().remove_all();
        self.set_duration(0);
        self.emit_by_name::<()>("ended", &[]);
    }

    /// Connect to the signal emitted when the call has ended.
    pub fn connect_ended<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "ended",
            true,
            glib::closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }

    /// Format the given duration in seconds as `MM:SS` or `H:MM:SS`.
    pub fn format_duration(duration: u64) -> String {
        let hours = duration / 3600;
        let minutes = (duration % 3600) / 60;
        let seconds = duration % 60;

        if hours > 0 {
            format!("{hours}:{minutes:02}:{seconds:02}")
        } else {
            format!("{minutes:02}:{seconds:02}")
        }
    }
}

impl Default for CallState {
    fn default() -> Self {
        Self::new()
    }
}
