//! Native `MatrixRTC` calls for a session.
//!
//! The [`CallManager`] owns one [`RtcCallSession`] engine per room with an
//! ongoing call, watches the `m.call.member` room state from sync, feeds
//! decrypted `io.element.call.encryption_keys` to-device events into the
//! engines, and exposes per-room call activity to the UI.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, LazyLock},
};

use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};
use mandelbrot_matrixrtc::{
    CallMembership, CallMembershipIdentity, MatrixRtcSession, MemberStateEvent, RtcCallSession,
    RtcCallSessionConfig, RtcCallSessionEvent, RtcRoom, Status, ToDeviceEvent, Transport,
};
use matrix_sdk::Client;
use ruma::{
    OwnedRoomId, RoomId, UserId,
    events::{
        StateEventType, ToDeviceEvent as RumaToDeviceEvent, call::member::SyncCallMemberEvent,
        macros::EventContent, room::member::MembershipState,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

#[cfg(feature = "calls-media")]
mod camera;
mod client_api;
#[cfg(feature = "calls-media")]
mod media;

pub(crate) use self::client_api::SdkRtcClientApi;
use super::Session;
use crate::{
    prelude::*,
    session_view::{CallConnectionState, CallParticipant, CallState},
    spawn, spawn_tokio,
};

/// The content of an `io.element.call.encryption_keys` to-device event.
///
/// The exact content is validated by the `MatrixRTC` engine; this type only
/// exists so that the SDK dispatches the (Olm-decrypted) events to us.
#[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
#[ruma_event(type = "io.element.call.encryption_keys", kind = ToDevice)]
pub(crate) struct CallEncryptionKeysEventContent {
    /// The raw fields of the event.
    #[serde(flatten)]
    pub(crate) data: BTreeMap<String, JsonValue>,
}

/// A message from the SDK event handlers to the manager.
enum CallManagerMessage {
    /// The `m.call.member` state of a room changed.
    RoomStateChanged(OwnedRoomId),
    /// A call encryption keys to-device event arrived.
    KeysReceived(ToDeviceEvent),
}

/// An ongoing call with a running engine.
struct ActiveCall {
    engine: Arc<RtcCallSession>,
    state: CallState,
    pump: glib::JoinHandle<()>,
    handlers: Vec<glib::SignalHandlerId>,
    /// The media connection of this call.
    #[cfg(feature = "calls-media")]
    media: Option<media::MediaHandle>,
    /// The task applying media events to the call state.
    #[cfg(feature = "calls-media")]
    media_pump: Option<glib::JoinHandle<()>>,
}

impl Drop for ActiveCall {
    fn drop(&mut self) {
        self.pump.abort();
        for handler in self.handlers.drain(..) {
            self.state.disconnect(handler);
        }
        #[cfg(feature = "calls-media")]
        if let Some(media_pump) = self.media_pump.take() {
            media_pump.abort();
        }
    }
}

impl std::fmt::Debug for ActiveCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveCall").finish_non_exhaustive()
    }
}

/// The local camera of a room.
///
/// The camera can be turned on before the call is joined, in the prescreen,
/// so this is kept per room, independently of the [`ActiveCall`].
#[cfg(feature = "calls-media")]
#[derive(Debug, Default)]
struct CameraSlot {
    /// Where the captured frames are sent, shared with the media task.
    sink: Arc<camera::CameraSink>,
    /// The ongoing capture, if the camera is on.
    capture: Option<camera::CameraCapture>,
    /// Whether the camera is being started.
    starting: bool,
}

mod imp {
    use std::cell::RefCell;

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::CallManager)]
    pub struct CallManager {
        /// The current session.
        #[property(get, set = Self::set_session, explicit_notify, nullable)]
        session: glib::WeakRef<Session>,
        /// The number of call members per room, for rooms with an active
        /// call.
        pub(super) active_calls: RefCell<HashMap<OwnedRoomId, usize>>,
        /// The ongoing calls we joined or are joining.
        pub(super) calls: RefCell<HashMap<OwnedRoomId, super::ActiveCall>>,
        /// The call states driving the call UI, per room.
        pub(super) states: RefCell<HashMap<OwnedRoomId, CallState>>,
        /// The local camera, per room.
        #[cfg(feature = "calls-media")]
        pub(super) cameras: RefCell<HashMap<OwnedRoomId, super::CameraSlot>>,
        /// The preferred foci from the `.well-known` of our homeserver.
        pub(super) preferred_foci: RefCell<Vec<Transport>>,
        /// Guards keeping the SDK event handlers alive.
        pub(super) handler_guards: RefCell<Vec<matrix_sdk::event_handler::EventHandlerDropGuard>>,
        /// The task listening for messages from the SDK event handlers.
        pub(super) listen_task: RefCell<Option<glib::JoinHandle<()>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CallManager {
        const NAME: &'static str = "CallManager";
        type Type = super::CallManager;
    }

    #[glib::derived_properties]
    impl ObjectImpl for CallManager {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    Signal::builder("room-call-active-changed")
                        .param_types([String::static_type(), bool::static_type()])
                        .build(),
                ]
            });
            &SIGNALS
        }

        fn dispose(&self) {
            if let Some(task) = self.listen_task.take() {
                task.abort();
            }
            self.handler_guards.take();
            self.calls.take();
        }
    }

    impl CallManager {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            if self.session.upgrade().as_ref() == session {
                return;
            }

            self.session.set(session);
            self.obj().notify_session();

            if let Some(session) = session {
                self.obj().init(session);
            }
        }
    }
}

glib::wrapper! {
    /// The native `MatrixRTC` calls API for a session.
    pub struct CallManager(ObjectSubclass<imp::CallManager>);
}

impl CallManager {
    pub(crate) fn new() -> Self {
        glib::Object::new()
    }

    /// Initialize the manager for the given session: install the SDK event
    /// handlers and fetch the preferred foci.
    fn init(&self, session: &Session) {
        let client = session.client();
        let (tx, rx) = mpsc::unbounded_channel();

        // Watch the call member state of all rooms.
        let state_tx = tx.clone();
        let state_handle =
            client.add_event_handler(move |_event: SyncCallMemberEvent, room: matrix_sdk::Room| {
                let _ = state_tx.send(CallManagerMessage::RoomStateChanged(
                    room.room_id().to_owned(),
                ));
                async {}
            });

        // Watch for (decrypted) call encryption keys to-device events.
        let keys_tx = tx;
        let keys_handle = client.add_event_handler(
            move |event: RumaToDeviceEvent<CallEncryptionKeysEventContent>| {
                let content = serde_json::to_value(&event.content).unwrap_or_default();
                let _ = keys_tx.send(CallManagerMessage::KeysReceived(ToDeviceEvent {
                    sender: event.sender.to_string(),
                    event_type: mandelbrot_matrixrtc::CALL_ENCRYPTION_KEYS_EVENT_TYPE.to_owned(),
                    content,
                }));
                async {}
            },
        );

        let mut guards = self.imp().handler_guards.borrow_mut();
        guards.push(client.event_handler_drop_guard(state_handle));
        guards.push(client.event_handler_drop_guard(keys_handle));
        drop(guards);

        // Process the messages on the main context.
        let listen_task = spawn!(clone!(
            #[weak(rename_to = obj)]
            self,
            async move {
                let mut rx = rx;
                while let Some(message) = rx.recv().await {
                    match message {
                        CallManagerMessage::RoomStateChanged(room_id) => {
                            obj.handle_room_state_changed(room_id).await;
                        }
                        CallManagerMessage::KeysReceived(event) => {
                            obj.handle_keys_received(&event);
                        }
                    }
                }
            }
        ));
        self.imp().listen_task.replace(Some(listen_task));

        self.fetch_preferred_foci(session);
    }

    /// The Matrix client of our session.
    fn client(&self) -> Option<Client> {
        Some(self.session()?.client())
    }

    /// Whether the given room has an active call.
    pub(crate) fn is_call_active(&self, room_id: &RoomId) -> bool {
        self.imp().active_calls.borrow().contains_key(room_id)
    }

    /// The number of members in the call of the given room.
    pub(crate) fn call_member_count(&self, room_id: &RoomId) -> usize {
        self.imp()
            .active_calls
            .borrow()
            .get(room_id)
            .copied()
            .unwrap_or_default()
    }

    /// The call state of the ongoing call in the given room, if we joined
    /// it.
    pub(crate) fn call_state(&self, room_id: &RoomId) -> Option<CallState> {
        self.imp()
            .calls
            .borrow()
            .get(room_id)
            .map(|call| call.state.clone())
    }

    /// Connect to the signal emitted when the call activity of a room
    /// changed.
    pub(crate) fn connect_room_call_active_changed<F: Fn(&Self, &RoomId, bool) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "room-call-active-changed",
            true,
            glib::closure_local!(move |obj: Self, room_id: String, active: bool| {
                if let Ok(room_id) = RoomId::parse(room_id) {
                    f(&obj, &room_id, active);
                }
            }),
        )
    }

    /// Handle a change of the call member state of the given room.
    async fn handle_room_state_changed(&self, room_id: OwnedRoomId) {
        let Some(client) = self.client() else {
            return;
        };
        let engine = self
            .imp()
            .calls
            .borrow()
            .get(&room_id)
            .map(|call| Arc::clone(&call.engine));

        let update_room_id = room_id.clone();
        let handle = spawn_tokio!(async move {
            let (events, joined) = room_call_member_snapshot(&client, &update_room_id).await?;
            let now = now_ms();

            let member_count = MatrixRtcSession::room_call_memberships(
                &events,
                |user_id| joined.contains(user_id),
                now,
            )
            .len();

            if let Some(engine) = engine {
                engine.on_room_state_update(&events, |user_id| joined.contains(user_id), now);
            }

            Some(member_count)
        });

        let member_count = match handle.await {
            Ok(Some(member_count)) => member_count,
            Ok(None) => return,
            Err(error) => {
                error!("Failed to update call member state: {error}");
                return;
            }
        };

        let was_active = {
            let mut active_calls = self.imp().active_calls.borrow_mut();
            if member_count > 0 {
                active_calls.insert(room_id.clone(), member_count).is_some()
            } else {
                active_calls.remove(&room_id).is_some()
            }
        };

        let is_active = member_count > 0;
        if was_active != is_active {
            debug!(
                "Call in {room_id} is now {}",
                if is_active { "active" } else { "inactive" }
            );
            self.emit_by_name::<()>(
                "room-call-active-changed",
                &[&room_id.to_string(), &is_active],
            );
        }
    }

    /// Feed a call keys to-device event into the ongoing calls.
    fn handle_keys_received(&self, event: &ToDeviceEvent) {
        for call in self.imp().calls.borrow().values() {
            if let Err(error) = call.engine.on_to_device_event(event) {
                warn!("Received invalid call encryption keys event: {error}");
            }
        }
    }

    /// Fetch the preferred RTC foci from the `.well-known` of our
    /// homeserver (MSC4143 `org.matrix.msc4143.rtc_foci`).
    fn fetch_preferred_foci(&self, session: &Session) {
        let server_name = session.user_id().server_name().to_owned();

        let handle = spawn_tokio!(async move {
            let url = format!("https://{server_name}/.well-known/matrix/client");
            let response = matrix_sdk::reqwest::get(&url).await.ok()?;
            let body = response.bytes().await.ok()?;
            let well_known = serde_json::from_slice::<JsonValue>(&body).ok()?;

            let foci = well_known
                .get("org.matrix.msc4143.rtc_foci")?
                .as_array()?
                .iter()
                .filter_map(|focus| serde_json::from_value::<Transport>(focus.clone()).ok())
                .collect::<Vec<_>>();
            Some(foci)
        });

        spawn!(clone!(
            #[weak(rename_to = obj)]
            self,
            async move {
                if let Ok(Some(foci)) = handle.await {
                    debug!("Found {} preferred RTC foci in .well-known", foci.len());
                    obj.imp().preferred_foci.replace(foci);
                }
            }
        ));
    }

    /// The call state driving the call UI for the given room, creating it if
    /// needed.
    ///
    /// Returns `None` if the session is gone.
    pub(crate) fn get_or_create_call_state(&self, room_id: &RoomId) -> Option<CallState> {
        if let Some(state) = self.imp().states.borrow().get(room_id) {
            return Some(state.clone());
        }

        let session = self.session()?;
        let state = CallState::new();
        let room_name = session
            .room_list()
            .get(room_id)
            .map_or_else(|| room_id.to_string(), |room| room.display_name());
        state.set_room_name(room_name);
        state.set_encrypted(true);

        // Turn the local camera on and off with the state, whether the call
        // was joined already or not: the prescreen shows a preview too.
        #[cfg(feature = "calls-media")]
        {
            let camera_room_id = room_id.to_owned();
            state.connect_camera_on_notify(clone!(
                #[weak(rename_to = obj)]
                self,
                move |state| {
                    obj.set_camera_enabled(&camera_room_id, state.camera_on());
                }
            ));
        }

        self.imp()
            .states
            .borrow_mut()
            .insert(room_id.to_owned(), state.clone());
        Some(state)
    }

    /// Turn the local camera of the given room on or off.
    ///
    /// Starting the camera is asynchronous: it might ask the user for access
    /// via the portal. If it fails, the camera is turned back off in the call
    /// state.
    #[cfg(feature = "calls-media")]
    fn set_camera_enabled(&self, room_id: &RoomId, enabled: bool) {
        if !enabled {
            let capture = self
                .imp()
                .cameras
                .borrow_mut()
                .get_mut(room_id)
                .and_then(|slot| slot.capture.take());

            if capture.is_some() {
                // Dropping the capture closes the camera device.
                drop(capture);
                self.camera_sink(room_id).notify_stopped();
                debug!("Stopped the camera in {room_id}");
            }

            if let Some(state) = self.imp().states.borrow().get(room_id) {
                state.set_self_paintable(None::<gtk::gdk::Paintable>);
            }
            return;
        }

        {
            let mut cameras = self.imp().cameras.borrow_mut();
            let slot = cameras.entry(room_id.to_owned()).or_default();
            if slot.capture.is_some() || slot.starting {
                return;
            }
            slot.starting = true;
        }

        let sink = self.camera_sink(room_id);
        let room_id = room_id.to_owned();
        spawn!(clone!(
            #[weak(rename_to = obj)]
            self,
            async move {
                let result = camera::CameraCapture::new(sink).await;

                let paintable = {
                    let mut cameras = obj.imp().cameras.borrow_mut();
                    let Some(slot) = cameras.get_mut(&room_id) else {
                        return;
                    };
                    slot.starting = false;

                    match result {
                        Ok(capture) => {
                            let paintable = capture.paintable().clone();
                            slot.capture = Some(capture);
                            Some(paintable)
                        }
                        Err(error) => {
                            error!("Could not start the camera: {error}");
                            None
                        }
                    }
                };

                let states = obj.imp().states.borrow();
                let Some(state) = states.get(&room_id).cloned() else {
                    return;
                };
                drop(states);

                if let Some(paintable) = paintable {
                    state.set_self_paintable(Some(paintable));
                } else {
                    // This turns the camera back off, which is a no-op for
                    // the capture that never started.
                    state.set_camera_on(false);
                }
            }
        ));
    }

    /// The sink receiving the frames of the local camera of the given room.
    #[cfg(feature = "calls-media")]
    fn camera_sink(&self, room_id: &RoomId) -> Arc<camera::CameraSink> {
        Arc::clone(
            &self
                .imp()
                .cameras
                .borrow_mut()
                .entry(room_id.to_owned())
                .or_default()
                .sink,
        )
    }

    /// Join the call in the given room.
    ///
    /// Returns the state driving the call UI, or `None` if the session is
    /// gone or the room is unknown.
    pub(crate) fn join_call(&self, room_id: &RoomId) -> Option<CallState> {
        if let Some(state) = self.call_state(room_id) {
            return Some(state);
        }

        let session = self.session()?;
        let client = session.client();
        let room = client.get_room(room_id)?;

        let user_id = session.user_id().clone();
        let device_id = client.device_id()?.to_owned();
        let own_identity = CallMembershipIdentity {
            user_id: user_id.to_string(),
            device_id: device_id.to_string(),
            member_id: format!("{user_id}:{device_id}"),
        };

        let engine = RtcCallSession::new(
            Arc::new(SdkRtcClientApi::new(client.clone())),
            RtcRoom {
                room_id: room_id.to_owned(),
                version: room
                    .version()
                    .map_or_else(|| "default".to_owned(), |version| version.to_string()),
            },
            own_identity,
            RtcCallSessionConfig::default(),
        );
        let engine = match engine {
            Ok(engine) => Arc::new(engine),
            Err(error) => {
                error!("Failed to create call session: {error}");
                return None;
            }
        };

        let state = self.get_or_create_call_state(room_id)?;
        state.set_connection_state(CallConnectionState::Connecting);

        // Leave the call when the user hangs up.
        let ended_room_id = room_id.to_owned();
        let ended_handler = state.connect_ended(clone!(
            #[weak(rename_to = obj)]
            self,
            move |_| {
                obj.leave_call(&ended_room_id);
            }
        ));

        // Forward the engine events into the call state.
        let mut events = engine.subscribe();
        let pump = spawn!(clone!(
            #[weak(rename_to = state)]
            state,
            #[strong(rename_to = own_user_id)]
            user_id,
            async move {
                while let Some(event) = events.recv().await {
                    apply_engine_event(&state, own_user_id.as_str(), &event);
                }
            }
        ));

        #[cfg(feature = "calls-media")]
        let (media, media_pump, media_ended_handler) =
            self.start_media(&client, room_id, device_id.to_string(), &engine, &state);

        #[cfg(feature = "calls-media")]
        let handlers = vec![ended_handler, media_ended_handler];
        #[cfg(not(feature = "calls-media"))]
        let handlers = vec![ended_handler];

        self.imp().calls.borrow_mut().insert(
            room_id.to_owned(),
            ActiveCall {
                engine: Arc::clone(&engine),
                state: state.clone(),
                pump,
                handlers,
                #[cfg(feature = "calls-media")]
                media: Some(media),
                #[cfg(feature = "calls-media")]
                media_pump: Some(media_pump),
            },
        );

        // Feed the initial member snapshot into the engine.  When the
        // `calls-media` feature is enabled the membership join is deferred
        // until the SFU connection is confirmed (#9); otherwise we join
        // immediately.
        let join_room_id = room_id.to_owned();
        #[cfg(not(feature = "calls-media"))]
        let foci = self.imp().preferred_foci.borrow().clone();
        let _join_handle = spawn_tokio!(async move {
            if let Some((events, joined)) = room_call_member_snapshot(&client, &join_room_id).await
            {
                engine.on_room_state_update(&events, |user_id| joined.contains(user_id), now_ms());
            }
            #[cfg(not(feature = "calls-media"))]
            engine.join_rtc_session(foci);
        });

        Some(state)
    }

    /// Start the media connection for the given call.
    #[cfg(feature = "calls-media")]
    fn start_media(
        &self,
        client: &Client,
        room_id: &RoomId,
        device_id: String,
        engine: &Arc<RtcCallSession>,
        state: &CallState,
    ) -> (
        media::MediaHandle,
        glib::JoinHandle<()>,
        glib::SignalHandlerId,
    ) {
        let foci = self.imp().preferred_foci.borrow().clone();
        let camera_sink = self.camera_sink(room_id);
        let (handle, mut media_events) = media::start(
            client.clone(),
            room_id.to_owned(),
            device_id,
            Arc::clone(engine),
            foci,
            &camera_sink,
        );
        handle.set_muted(state.muted());

        // Apply the media events to the call state.
        let media_pump = spawn!(clone!(
            #[weak(rename_to = state)]
            state,
            async move {
                while let Some(event) = media_events.recv().await {
                    apply_media_event(&state, event);
                }
            }
        ));

        // Mute and unmute the microphone with the state.
        let muted = handle.muted_flag();
        let muted_handler = state.connect_muted_notify(move |state| {
            muted.store(state.muted(), std::sync::atomic::Ordering::SeqCst);
        });

        (handle, media_pump, muted_handler)
    }

    /// Leave the call in the given room.
    pub(crate) fn leave_call(&self, room_id: &RoomId) {
        let Some(call) = self.imp().calls.borrow_mut().remove(room_id) else {
            return;
        };

        // Close the camera device: the call it was published to is over.
        #[cfg(feature = "calls-media")]
        {
            call.state.set_camera_on(false);
            self.imp().cameras.borrow_mut().remove(room_id);
        }

        let engine = Arc::clone(&call.engine);
        let state = call.state.clone();
        let handle = spawn_tokio!(async move {
            engine
                .leave_rtc_session(Some(std::time::Duration::from_secs(10)))
                .await
        });

        spawn!(async move {
            match handle.await {
                Ok(true) => debug!("Left call"),
                Ok(false) => warn!("Leaving the call timed out"),
                Err(error) => error!("Failed to leave call: {error}"),
            }
            if state.connection_state() != CallConnectionState::Disconnected {
                state.hang_up();
            }
        });
    }
}

impl Default for CallManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Apply an engine event to the call state.
fn apply_engine_event(state: &CallState, own_user_id: &str, event: &RtcCallSessionEvent) {
    match event {
        RtcCallSessionEvent::StatusChanged { current, .. } => {
            let connection_state = match current {
                Status::Disconnected => CallConnectionState::Disconnected,
                Status::Connecting | Status::Disconnecting => CallConnectionState::Connecting,
                Status::Connected => CallConnectionState::Connected,
                Status::Unknown => CallConnectionState::Reconnecting,
            };
            state.set_connection_state(connection_state);
        }
        RtcCallSessionEvent::MembershipsChanged { new, .. } => {
            update_participants(state, own_user_id, new);
        }
        RtcCallSessionEvent::ProbablyLeft(true) => {
            state.set_connection_state(CallConnectionState::Reconnecting);
        }
        RtcCallSessionEvent::MembershipManagerError(error) => {
            error!("Call membership manager failed: {error}");
            state.set_connection_state(CallConnectionState::Failed);
        }
        _ => {}
    }
}

/// Apply a media event to the call state.
#[cfg(feature = "calls-media")]
fn apply_media_event(state: &CallState, event: media::MediaEvent) {
    match event {
        media::MediaEvent::Connected => {}
        media::MediaEvent::VideoFrame {
            identity,
            rgba,
            width,
            height,
        } => {
            let Some(participant) = find_participant(state, &identity) else {
                return;
            };
            let bytes = glib::Bytes::from_owned(rgba);
            let texture = gtk::gdk::MemoryTexture::new(
                i32::try_from(width).unwrap_or(i32::MAX),
                i32::try_from(height).unwrap_or(i32::MAX),
                gtk::gdk::MemoryFormat::R8g8b8a8,
                &bytes,
                (width * 4) as usize,
            );
            participant.set_video_paintable(Some(texture.upcast::<gtk::gdk::Paintable>()));
        }
        media::MediaEvent::VideoEnded { identity } => {
            if let Some(participant) = find_participant(state, &identity) {
                participant.set_video_paintable(None::<gtk::gdk::Paintable>);
            }
        }
        media::MediaEvent::Ended { error } => {
            if error.is_some() && state.connection_state() != CallConnectionState::Disconnected {
                state.set_connection_state(CallConnectionState::Failed);
            }
        }
    }
}

/// Find the participant with the given RTC backend identity in the call
/// state.
#[cfg(feature = "calls-media")]
fn find_participant(state: &CallState, identity: &str) -> Option<CallParticipant> {
    let participants = state.participants();
    (0..participants.n_items()).find_map(|i| {
        participants
            .item(i)
            .and_downcast::<CallParticipant>()
            .filter(|participant| participant.identity() == identity)
    })
}

/// Update the participants list of the call state from the given
/// memberships.
fn update_participants(state: &CallState, own_user_id: &str, memberships: &[CallMembership]) {
    let participants = state.participants();

    let identities = memberships
        .iter()
        .filter(|membership| membership.user_id() != own_user_id)
        .map(CallMembership::rtc_backend_identity)
        .collect::<Vec<_>>();

    // Remove the leavers, keeping the tiles of everyone else.
    let mut i = 0;
    while i < participants.n_items() {
        let participant = participants.item(i).and_downcast::<CallParticipant>();
        if participant.is_some_and(|participant| identities.contains(&participant.identity())) {
            i += 1;
        } else {
            participants.remove(i);
        }
    }

    // Add the joiners.
    for membership in memberships {
        if membership.user_id() == own_user_id {
            continue;
        }
        let identity = membership.rtc_backend_identity();
        let exists = (0..participants.n_items()).any(|i| {
            participants
                .item(i)
                .and_downcast::<CallParticipant>()
                .is_some_and(|participant| participant.identity() == identity)
        });
        if exists {
            continue;
        }

        let user_id = membership.user_id();
        let display_name = UserId::parse(user_id).map_or_else(
            |_| user_id.to_owned(),
            |user_id| user_id.localpart().to_owned(),
        );
        participants.append(&CallParticipant::with_identity(
            &identity,
            &display_name,
            false,
        ));
    }
}

/// The current wall clock time, in milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

/// Read the current `m.call.member` state events of the given room, plus
/// the set of their senders that are joined members of the room.
async fn room_call_member_snapshot(
    client: &Client,
    room_id: &RoomId,
) -> Option<(Vec<MemberStateEvent>, HashSet<String>)> {
    let room = client.get_room(room_id)?;

    let raw_events = room
        .get_state_events(StateEventType::CallMember)
        .await
        .inspect_err(|error| error!("Failed to read call member state of {room_id}: {error}"))
        .ok()?;

    let mut events = Vec::new();
    for raw in raw_events {
        let matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState::Sync(raw) = raw else {
            continue;
        };
        let Ok(value) = raw.deserialize_as::<JsonValue>() else {
            continue;
        };

        let field = |name: &str| {
            value
                .get(name)
                .and_then(JsonValue::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        events.push(MemberStateEvent {
            event_id: field("event_id"),
            sender: field("sender"),
            origin_server_ts: value
                .get("origin_server_ts")
                .and_then(JsonValue::as_u64)
                .unwrap_or_default(),
            state_key: field("state_key"),
            content: value.get("content").cloned().unwrap_or_default(),
        });
    }

    let mut joined = HashSet::new();
    let senders = events
        .iter()
        .map(|event| event.sender.clone())
        .collect::<HashSet<_>>();
    for sender in senders {
        let Ok(user_id) = UserId::parse(&sender) else {
            continue;
        };
        if let Ok(Some(member)) = room.get_member_no_sync(&user_id).await
            && *member.membership() == MembershipState::Join
        {
            joined.insert(sender);
        }
    }

    Some((events, joined))
}
