// SPDX-License-Identifier: GPL-3.0-or-later

//! The join/leave state machine for the own MatrixRTC membership.
//!
//! This is a port of `matrix-js-sdk`'s `MembershipManager` and its
//! `ActionScheduler`. It is responsible for:
//!
//! - Sending the user's delayed leave event before sending the membership.
//! - Sending the user's membership state event when joining.
//! - Checking if the delayed event was cancelled due to sending the membership,
//!   and rescheduling it if so.
//! - Restarting ("keep-alive") the delayed leave event.
//! - Updating the state event before its `expires` timeout passes.
//! - Rejoining when the membership got removed externally.
//! - Falling back to plain state events when the server does not support
//!   MSC4140 delayed events.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use ruma::{OwnedDeviceId, OwnedRoomId, OwnedUserId, events::StateEventType};
use serde_json::json;
use tokio::{
    sync::{mpsc, watch},
    time::Instant,
};
use tracing::{debug, error, info, warn};

use crate::{
    call_membership::CallMembership,
    client::{ClientError, RtcClientApi, UpdateDelayedEventAction},
    membership_data::{DEFAULT_EXPIRE_DURATION_MS, FocusActive, SessionMembershipData, Transport},
    session::SlotDescription,
};

/// Call membership should always remain sticky for this amount of time.
const MEMBERSHIP_STICKY_DURATION_MS: u64 = 60 * 60 * 1000;

/// The connection status of the [`MembershipManager`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Not connected to the session.
    Disconnected,
    /// In the process of sending the membership.
    Connecting,
    /// The membership is set up.
    Connected,
    /// In the process of removing the membership.
    Disconnecting,
    /// The action queue is in an unexpected state.
    Unknown,
}

/// The different types of actions the manager can take.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum MembershipActionType {
    SendDelayedEvent,
    SendJoinEvent,
    RestartDelayedEvent,
    UpdateExpiry,
    SendScheduledDelayedLeaveEvent,
    SendLeaveEvent,
}

#[derive(Clone, Debug)]
struct Action {
    at: Instant,
    kind: MembershipActionType,
}

#[derive(Clone, Debug)]
enum ActionUpdate {
    /// Replace all existing scheduled actions with this new list.
    Replace(Vec<Action>),
    /// Add these actions to the existing scheduled actions.
    Insert(Vec<Action>),
    /// Leave the scheduled actions unchanged.
    None,
}

fn insert_now(kind: MembershipActionType) -> ActionUpdate {
    ActionUpdate::Insert(vec![Action {
        at: Instant::now(),
        kind,
    }])
}

fn insert_in(kind: MembershipActionType, offset: Duration) -> ActionUpdate {
    ActionUpdate::Insert(vec![Action {
        at: Instant::now() + offset,
        kind,
    }])
}

fn replace_now(kind: MembershipActionType) -> ActionUpdate {
    ActionUpdate::Replace(vec![Action {
        at: Instant::now(),
        kind,
    }])
}

/// Events emitted by the [`MembershipManager`].
#[derive(Clone, Debug)]
pub enum MembershipManagerEvent {
    /// The connection status changed.
    StatusChanged {
        /// The previous status.
        previous: Status,
        /// The new status.
        current: Status,
    },
    /// Whether the server probably already sent our delayed leave event
    /// because we could not restart it in time.
    ProbablyLeft(bool),
    /// The ID of our scheduled delayed leave event changed.
    DelayIdChanged(Option<String>),
    /// The manager shut down because of an unrecoverable error.
    Error(String),
}

/// Configuration for the [`MembershipManager`].
#[derive(Clone, Debug)]
pub struct MembershipConfig {
    /// The timeout, in milliseconds, after we joined the call, that our
    /// membership should expire unless we have explicitly updated it.
    pub membership_event_expiry_ms: u64,
    /// The time, in milliseconds, by which the manager sends the updated
    /// state event before the `expires` time is reached.
    pub membership_event_expiry_headroom_ms: u64,
    /// The delay, in milliseconds, with which the delayed leave event on the
    /// server is configured.
    pub delayed_leave_event_delay_ms: u64,
    /// The interval, in milliseconds, at which the client sends keep-alive
    /// restarts for the delayed leave event.
    pub delayed_leave_event_restart_ms: u64,
    /// The time, in milliseconds, after which a delayed event restart
    /// request is considered to have failed locally.
    pub delayed_leave_event_restart_local_timeout_ms: u64,
    /// The time, in milliseconds, after which a request is retried on a
    /// network error.
    pub network_error_retry_ms: u64,
    /// The maximum number of retries on rate limit errors.
    pub maximum_rate_limit_retry_count: u32,
    /// The maximum number of retries on network errors.
    pub maximum_network_error_retry_count: u32,
    /// The call intent to advertise in the membership, e.g. `audio` or
    /// `video`.
    pub call_intent: Option<String>,
}

impl Default for MembershipConfig {
    fn default() -> Self {
        Self {
            membership_event_expiry_ms: DEFAULT_EXPIRE_DURATION_MS,
            membership_event_expiry_headroom_ms: 5_000,
            delayed_leave_event_delay_ms: 8_000,
            delayed_leave_event_restart_ms: 5_000,
            delayed_leave_event_restart_local_timeout_ms: 2_000,
            network_error_retry_ms: 3_000,
            maximum_rate_limit_retry_count: 10,
            maximum_network_error_retry_count: 10,
            call_intent: None,
        }
    }
}

/// The room properties the manager needs.
#[derive(Clone, Debug)]
pub struct RtcRoom {
    /// The ID of the room.
    pub room_id: OwnedRoomId,
    /// The version of the room, used to decide whether the room supports
    /// user-owned state events (MSC3757/MSC3779).
    pub version: String,
}

impl RtcRoom {
    fn supports_user_owned_state_events(&self) -> bool {
        ["org.matrix.msc3757", "org.matrix.msc3779"]
            .iter()
            .any(|prefix| {
                self.version.strip_prefix(prefix).is_some_and(|rest| {
                    rest.is_empty() || !rest.starts_with(|c: char| c.is_ascii_alphanumeric())
                })
            })
    }
}

#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
struct State {
    running: bool,
    activated: bool,
    leave_initiated: bool,
    actions: Vec<Action>,
    foci_preferred: Vec<Transport>,
    delay_id: Option<String>,
    probably_left: bool,
    has_member_state_event: bool,
    start_time: Option<Instant>,
    expire_update_iterations: u64,
    rate_limit_retries: HashMap<MembershipActionType, u32>,
    network_error_retries: HashMap<MembershipActionType, u32>,
    expected_server_delay_leave_at: Option<Instant>,
    own_membership: Option<CallMembership>,
    delayed_leave_delay_override_ms: Option<u64>,
    emitted_status: Status,
    wakeup_tx: Option<mpsc::UnboundedSender<ActionUpdate>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            running: false,
            activated: false,
            leave_initiated: false,
            actions: Vec::new(),
            foci_preferred: Vec::new(),
            delay_id: None,
            probably_left: false,
            has_member_state_event: false,
            start_time: None,
            expire_update_iterations: 1,
            rate_limit_retries: HashMap::new(),
            network_error_retries: HashMap::new(),
            expected_server_delay_leave_at: None,
            own_membership: None,
            delayed_leave_delay_override_ms: None,
            emitted_status: Status::Disconnected,
            wakeup_tx: None,
        }
    }
}

struct Ctx {
    config: MembershipConfig,
    room: RtcRoom,
    user_id: OwnedUserId,
    device_id: OwnedDeviceId,
    slot: SlotDescription,
    state_key: String,
    client: Arc<dyn RtcClientApi>,
    state: Mutex<State>,
    subscribers: Mutex<Vec<mpsc::UnboundedSender<MembershipManagerEvent>>>,
    running_tx: watch::Sender<bool>,
}

/// The state machine responsible for sending all events relating to the own
/// membership of a MatrixRTC call.
pub struct MembershipManager {
    ctx: Arc<Ctx>,
}

impl MembershipManager {
    /// Construct a manager for the given room, user and device.
    pub fn new(
        config: MembershipConfig,
        room: RtcRoom,
        user_id: OwnedUserId,
        device_id: OwnedDeviceId,
        slot: SlotDescription,
        client: Arc<dyn RtcClientApi>,
    ) -> Self {
        let state_key = Self::make_membership_state_key(&room, &user_id, &device_id, &slot);
        let (running_tx, _) = watch::channel(false);

        Self {
            ctx: Arc::new(Ctx {
                config,
                room,
                user_id,
                device_id,
                slot,
                state_key,
                client,
                state: Mutex::new(State::default()),
                subscribers: Mutex::new(Vec::new()),
                running_tx,
            }),
        }
    }

    /// This creates `{user_id}_{device_id}_{application}{slot_id}`, with a
    /// leading `_` unless the room supports user-owned state events.
    fn make_membership_state_key(
        room: &RtcRoom,
        user_id: &OwnedUserId,
        device_id: &OwnedDeviceId,
        slot: &SlotDescription,
    ) -> String {
        // Revert back to "" just for the state key.
        let needs_empty_string_room_fix = slot.application == "m.call" && slot.id == "ROOM";
        let slot_id = if needs_empty_string_room_fix {
            ""
        } else {
            &slot.id
        };
        let state_key = format!("{user_id}_{device_id}_{}{slot_id}", slot.application);

        if room.supports_user_owned_state_events() {
            state_key
        } else {
            format!("_{state_key}")
        }
    }

    /// Subscribe to the events emitted by this manager.
    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<MembershipManagerEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.ctx.subscribers.lock().unwrap().push(tx);
        rx
    }

    /// Whether the manager tries to be joined.
    pub fn is_activated(&self) -> bool {
        self.ctx.state.lock().unwrap().activated
    }

    /// The current connection status.
    pub fn status(&self) -> Status {
        let state = self.ctx.state.lock().unwrap();
        Ctx::compute_status(&state)
    }

    /// Whether the server probably already sent our delayed leave event.
    pub fn probably_left(&self) -> bool {
        self.ctx.state.lock().unwrap().probably_left
    }

    /// The ID of our scheduled delayed leave event, if any.
    pub fn delay_id(&self) -> Option<String> {
        self.ctx.state.lock().unwrap().delay_id.clone()
    }

    /// Our own membership as last seen in the session member list.
    pub fn own_membership(&self) -> Option<CallMembership> {
        self.ctx.state.lock().unwrap().own_membership.clone()
    }

    /// Put the manager in a state where it tries to be joined.
    ///
    /// It will send delayed leave events and membership events. Errors that
    /// cannot be handled internally are reported with
    /// [`MembershipManagerEvent::Error`].
    pub fn join(&self, foci_preferred: Vec<Transport>) {
        {
            let mut state = self.ctx.state.lock().unwrap();
            if state.running {
                error!("[MembershipManager] is already running. Ignoring join request.");
                return;
            }

            let delayed_leave_delay_override_ms = state.delayed_leave_delay_override_ms;
            *state = State {
                activated: true,
                running: true,
                foci_preferred,
                delayed_leave_delay_override_ms,
                emitted_status: Status::Disconnected,
                actions: vec![Action {
                    at: Instant::now(),
                    kind: MembershipActionType::SendDelayedEvent,
                }],
                ..State::default()
            };

            let (tx, rx) = mpsc::unbounded_channel();
            state.wakeup_tx = Some(tx);
            drop(state);

            self.ctx.running_tx.send_replace(true);

            let ctx = Arc::clone(&self.ctx);
            tokio::spawn(async move {
                ctx.run_loop(rx).await;
            });
        }
    }

    /// Leave the call.
    ///
    /// Returns `true` if the manager managed to leave, and `false` if the
    /// timeout was reached first.
    pub async fn leave(&self, timeout: Option<Duration>) -> bool {
        let mut running_rx = self.ctx.running_tx.subscribe();

        {
            let mut state = self.ctx.state.lock().unwrap();
            if !state.running {
                warn!(
                    "Called MembershipManager.leave() even though the MembershipManager is not running"
                );
                return true;
            }

            if !state.leave_initiated {
                state.leave_initiated = true;
                state.activated = false;
                if let Some(tx) = &state.wakeup_tx {
                    let _ = tx.send(replace_now(
                        MembershipActionType::SendScheduledDelayedLeaveEvent,
                    ));
                }
            }
        }

        let wait = running_rx.wait_for(|running| !running);
        if let Some(timeout) = timeout {
            tokio::time::timeout(timeout, wait).await.is_ok()
        } else {
            let _ = wait.await;
            true
        }
    }

    /// Call this whenever the memberships of the session changed.
    ///
    /// If our own membership went missing while we are activated, a rejoin is
    /// initiated.
    pub fn on_rtc_session_member_update(&self, memberships: &[CallMembership]) {
        let mut state = self.ctx.state.lock().unwrap();
        if !state.activated {
            return;
        }

        state.own_membership = memberships
            .iter()
            .find(|m| {
                m.user_id() == self.ctx.user_id.as_str()
                    && m.device_id() == self.ctx.device_id.as_str()
            })
            .cloned();

        if state.own_membership.is_none() {
            warn!("Missing own membership: force re-join");
            state.has_member_state_event = false;

            // If one of these actions is scheduled, we already take care of
            // our missing membership.
            let sending_membership_scheduled = state.actions.iter().any(|a| {
                matches!(
                    a.kind,
                    MembershipActionType::SendDelayedEvent | MembershipActionType::SendJoinEvent
                )
            });

            if sending_membership_scheduled {
                error!(
                    "Tried adding another `SendDelayedEvent` action even though we already have one in the queue"
                );
            } else if let Some(tx) = &state.wakeup_tx {
                let _ = tx.send(replace_now(MembershipActionType::SendDelayedEvent));
            }
        }
    }
}

impl Ctx {
    fn emit(&self, event: &MembershipManagerEvent) {
        let mut subscribers = self.subscribers.lock().unwrap();
        subscribers.retain(|tx| tx.send(event.clone()).is_ok());
    }

    fn membership_event_expiry_ms(&self) -> u64 {
        self.config.membership_event_expiry_ms
    }

    fn delayed_leave_event_delay_ms(&self) -> u64 {
        self.state
            .lock()
            .unwrap()
            .delayed_leave_delay_override_ms
            .unwrap_or(self.config.delayed_leave_event_delay_ms)
    }

    fn compute_next_expiry_action_at(&self, state: &State, iteration: u64) -> Instant {
        let start_time = state.start_time.unwrap_or_else(Instant::now);
        let interval = self
            .membership_event_expiry_ms()
            .min(MEMBERSHIP_STICKY_DURATION_MS);
        let offset =
            (interval * iteration).saturating_sub(self.config.membership_event_expiry_headroom_ms);
        start_time + Duration::from_millis(offset)
    }

    fn compute_status(state: &State) -> Status {
        use MembershipActionType as A;

        let kinds: Vec<_> = state.actions.iter().map(|a| a.kind).collect();
        match kinds.as_slice() {
            [A::SendDelayedEvent | A::SendJoinEvent] => return Status::Connecting,
            // when no delayed events are in use
            [A::UpdateExpiry] => return Status::Connected,
            [A::SendScheduledDelayedLeaveEvent | A::SendLeaveEvent] => {
                return Status::Disconnecting;
            }
            kinds if kinds.len() == 2 => {
                // Normal state for connected with delayed events.
                if (kinds.contains(&A::RestartDelayedEvent)
                    || (kinds.contains(&A::SendDelayedEvent) && state.has_member_state_event))
                    && kinds.contains(&A::UpdateExpiry)
                {
                    return Status::Connected;
                }
            }
            // It is a correct connected state if we already scheduled the
            // next restart but have not yet cleaned up the current one.
            kinds
                if kinds.len() == 3
                    && kinds
                        .iter()
                        .filter(|k| **k == A::RestartDelayedEvent)
                        .count()
                        == 2
                    && kinds.contains(&A::UpdateExpiry) =>
            {
                return Status::Connected;
            }
            _ => {}
        }

        if !state.running {
            return Status::Disconnected;
        }

        error!("[MembershipManager] has an unknown state. Actions: {kinds:?}");
        Status::Unknown
    }

    fn emit_status_transition(&self) {
        let (previous, current) = {
            let mut state = self.state.lock().unwrap();
            let previous = state.emitted_status;
            let current = Self::compute_status(&state);
            state.emitted_status = current;
            (previous, current)
        };
        if previous != current {
            debug!("[MembershipManager] status changed: {previous:?} -> {current:?}");
            self.emit(&MembershipManagerEvent::StatusChanged { previous, current });
        }
    }

    fn set_and_emit_delay_id(&self, delay_id: Option<String>) {
        {
            let mut state = self.state.lock().unwrap();
            if state.delay_id == delay_id {
                return;
            }
            state.delay_id.clone_from(&delay_id);
        }
        self.emit(&MembershipManagerEvent::DelayIdChanged(delay_id));
    }

    fn set_and_emit_probably_left(&self, probably_left: bool) {
        {
            let mut state = self.state.lock().unwrap();
            if state.probably_left == probably_left {
                return;
            }
            state.probably_left = probably_left;
        }
        self.emit(&MembershipManagerEvent::ProbablyLeft(probably_left));
    }

    fn reset_rate_limit_counter(&self, kind: MembershipActionType) {
        let mut state = self.state.lock().unwrap();
        state.rate_limit_retries.insert(kind, 0);
        state.network_error_retries.insert(kind, 0);
    }

    async fn run_loop(self: Arc<Self>, mut rx: mpsc::UnboundedReceiver<ActionUpdate>) {
        let result = self.run_loop_inner(&mut rx).await;

        let (previous, current) = {
            let mut state = self.state.lock().unwrap();
            state.running = false;
            // Should already be `false` when `leave()` initiated the
            // shutdown, in non-error cases.
            state.activated = false;
            state.wakeup_tx = None;
            let previous = state.emitted_status;
            let current = Self::compute_status(&state);
            state.emitted_status = current;
            (previous, current)
        };

        if let Err(message) = result {
            let message =
                format!("The MembershipManager shut down because of the end condition: {message}");
            error!("[MembershipManager] {message}");
            self.emit(&MembershipManagerEvent::Error(message));
        }
        if previous != current {
            self.emit(&MembershipManagerEvent::StatusChanged { previous, current });
        }

        self.running_tx.send_replace(false);
    }

    async fn run_loop_inner(
        &self,
        rx: &mut mpsc::UnboundedReceiver<ActionUpdate>,
    ) -> Result<(), String> {
        loop {
            let next = {
                let mut state = self.state.lock().unwrap();
                if state.actions.is_empty() {
                    return Ok(());
                }
                // Sort so the next (smallest timestamp) action is at the
                // beginning.
                state.actions.sort_by_key(|a| a.at);
                state.actions[0].clone()
            };

            let mut wakeup_update = None;
            if next.at > Instant::now() {
                tokio::select! {
                    update = rx.recv() => {
                        if let Some(update) = update {
                            wakeup_update = Some(update);
                        }
                    }
                    () = tokio::time::sleep_until(next.at) => {}
                }
            }

            let mut handler_result = ActionUpdate::None;
            if wakeup_update.is_none() {
                self.emit_status_transition();
                debug!("[MembershipManager] processing action {:?}", next.kind);
                handler_result = self.handle_action(next.kind).await?;
                // A wakeup that happened while we were in the handler wins
                // over the handler result, since it is a direct external
                // update.
                while let Ok(update) = rx.try_recv() {
                    wakeup_update = Some(update);
                }
            }

            let mut state = self.state.lock().unwrap();
            // Remove the processed action only after we are done processing.
            if !state.actions.is_empty() {
                state.actions.remove(0);
            }
            match wakeup_update.unwrap_or(handler_result) {
                ActionUpdate::Replace(actions) => state.actions = actions,
                ActionUpdate::Insert(actions) => state.actions.extend(actions),
                ActionUpdate::None => {}
            }
        }
    }

    async fn handle_action(&self, kind: MembershipActionType) -> Result<ActionUpdate, String> {
        match kind {
            MembershipActionType::SendDelayedEvent => {
                let delay_id = self.state.lock().unwrap().delay_id.clone();
                if let Some(delay_id) = delay_id {
                    // This can happen if someone else (or another client)
                    // removed our own membership event. We might still have
                    // our delayed event from the previous participation, so
                    // try to cancel it before setting up a new one.
                    self.cancel_known_delay_id_before_send_delayed_event(&delay_id)
                        .await
                } else {
                    self.send_or_resend_delayed_leave_event().await
                }
            }
            MembershipActionType::RestartDelayedEvent => {
                let delay_id = self.state.lock().unwrap().delay_id.clone();
                if let Some(delay_id) = delay_id {
                    self.restart_delayed_event(&delay_id).await
                } else {
                    // The delay ID got reset. This action was used to check
                    // if the homeserver cancelled the delayed event when the
                    // join state got sent.
                    Ok(insert_now(MembershipActionType::SendDelayedEvent))
                }
            }
            MembershipActionType::SendScheduledDelayedLeaveEvent => {
                let (has_member_state_event, delay_id) = {
                    let state = self.state.lock().unwrap();
                    (state.has_member_state_event, state.delay_id.clone())
                };
                if !has_member_state_event {
                    // We are already good.
                    return Ok(ActionUpdate::Replace(Vec::new()));
                }
                if let Some(delay_id) = delay_id {
                    self.send_scheduled_delayed_leave_event_or_fallback(&delay_id)
                        .await
                } else {
                    Ok(insert_now(MembershipActionType::SendLeaveEvent))
                }
            }
            MembershipActionType::SendJoinEvent => self.send_join_event().await,
            MembershipActionType::UpdateExpiry => self.update_expiry_on_joined_event().await,
            MembershipActionType::SendLeaveEvent => {
                if !self.state.lock().unwrap().has_member_state_event {
                    // We are good already.
                    return Ok(ActionUpdate::Replace(Vec::new()));
                }
                // This is only a fallback in case we do not have working
                // delayed events support.
                self.send_fallback_leave_event().await
            }
        }
    }

    async fn send_or_resend_delayed_leave_event(&self) -> Result<ActionUpdate, String> {
        let delay_ms = self.delayed_leave_event_delay_ms();
        let result = self
            .client
            .send_delayed_state_event(
                &self.room.room_id,
                Duration::from_millis(delay_ms),
                StateEventType::CallMember,
                &self.state_key,
                json!({}),
            )
            .await;

        let has_member_state_event = self.state.lock().unwrap().has_member_state_event;

        match result {
            Ok(response) => {
                self.state.lock().unwrap().expected_server_delay_leave_at =
                    Some(Instant::now() + Duration::from_millis(delay_ms));
                self.set_and_emit_probably_left(false);
                self.reset_rate_limit_counter(MembershipActionType::SendDelayedEvent);
                self.set_and_emit_delay_id(Some(response.delay_id));

                if has_member_state_event {
                    // This action was scheduled because the previous delayed
                    // event was cancelled by sending the state event.
                    Ok(insert_in(
                        MembershipActionType::RestartDelayedEvent,
                        Duration::from_millis(self.config.delayed_leave_event_restart_ms),
                    ))
                } else {
                    // This action was scheduled because we are in the
                    // process of joining.
                    Ok(insert_now(MembershipActionType::SendJoinEvent))
                }
            }
            Err(error) => {
                let repeat = MembershipActionType::SendDelayedEvent;
                if self.manage_max_delay_exceeded_situation(&error) {
                    return Ok(insert_now(repeat));
                }
                if let Some(update) =
                    self.action_update_from_errors(&error, repeat, "send_delayed_state_event")?
                {
                    return Ok(update);
                }

                if has_member_state_event {
                    // Don't do any other delayed event work if it is not
                    // supported.
                    if matches!(error, ClientError::UnsupportedDelayedEventsEndpoint) {
                        return Ok(ActionUpdate::None);
                    }
                    Err(format!(
                        "Could not send delayed event, even though delayed events are supported. {error}"
                    ))
                } else {
                    // On any other error we fall back to not using delayed
                    // events and send the join state event immediately.
                    if matches!(error, ClientError::UnsupportedDelayedEventsEndpoint) {
                        info!("Not using delayed event because the endpoint is not supported");
                    } else {
                        info!("Not using delayed event because: {error}");
                    }
                    Ok(insert_now(MembershipActionType::SendJoinEvent))
                }
            }
        }
    }

    async fn cancel_known_delay_id_before_send_delayed_event(
        &self,
        delay_id: &str,
    ) -> Result<ActionUpdate, String> {
        let result = self
            .client
            .update_delayed_event(delay_id, UpdateDelayedEventAction::Cancel)
            .await;

        match result {
            Ok(()) => {
                self.set_and_emit_delay_id(None);
                self.reset_rate_limit_counter(MembershipActionType::SendDelayedEvent);
                Ok(replace_now(MembershipActionType::SendDelayedEvent))
            }
            Err(error) => {
                let repeat = MembershipActionType::SendDelayedEvent;
                if let Some(update) =
                    self.action_update_from_errors(&error, repeat, "update_delayed_event")?
                {
                    return Ok(update);
                }

                if error.is_not_found() {
                    // The delayed event got already removed, we are good.
                    self.set_and_emit_delay_id(None);
                    return Ok(replace_now(repeat));
                }
                if matches!(error, ClientError::UnsupportedDelayedEventsEndpoint) {
                    return Ok(replace_now(MembershipActionType::SendJoinEvent));
                }

                Err(
                    "We failed to cancel a delayed event where we already had a delay id \
                     with an error we cannot automatically handle"
                        .to_owned(),
                )
            }
        }
    }

    async fn restart_delayed_event(&self, delay_id: &str) -> Result<ActionUpdate, String> {
        let local_timeout =
            Duration::from_millis(self.config.delayed_leave_event_restart_local_timeout_ms);
        let (expected_at, probably_left) = {
            let state = self.state.lock().unwrap();
            (state.expected_server_delay_leave_at, state.probably_left)
        };

        // We abort at the time we expect the server to send the delayed
        // leave event, at the latest. Once we are already in the
        // probably-left state, we use the unaltered local timeout.
        let timeout = match expected_at {
            Some(expected_at) if !probably_left => {
                local_timeout.min(expected_at.saturating_duration_since(Instant::now()))
            }
            _ => local_timeout,
        };

        let result = match tokio::time::timeout(
            timeout,
            self.client
                .update_delayed_event(delay_id, UpdateDelayedEventAction::Restart),
        )
        .await
        {
            Ok(result) => result,
            Err(_elapsed) => Err(ClientError::LocalTimeout),
        };

        match result {
            Ok(()) => {
                self.state.lock().unwrap().expected_server_delay_leave_at = Some(
                    Instant::now() + Duration::from_millis(self.delayed_leave_event_delay_ms()),
                );
                self.reset_rate_limit_counter(MembershipActionType::RestartDelayedEvent);
                self.set_and_emit_probably_left(false);
                Ok(insert_in(
                    MembershipActionType::RestartDelayedEvent,
                    Duration::from_millis(self.config.delayed_leave_event_restart_ms),
                ))
            }
            Err(error) => {
                let expected_at = self.state.lock().unwrap().expected_server_delay_leave_at;
                if expected_at.is_some_and(|at| at <= Instant::now()) {
                    // It is likely that the server sent the delayed leave
                    // event by now. We emit `probably_left = false` again
                    // once we notice our leave through sync and successfully
                    // set up a new state event.
                    self.set_and_emit_probably_left(true);
                }

                if error.is_not_found() {
                    self.set_and_emit_delay_id(None);
                    return Ok(insert_now(MembershipActionType::SendDelayedEvent));
                }
                // If the homeserver does not support delayed events we won't
                // reschedule.
                if matches!(error, ClientError::UnsupportedDelayedEventsEndpoint) {
                    return Ok(ActionUpdate::None);
                }
                if let Some(update) = self.action_update_from_errors(
                    &error,
                    MembershipActionType::RestartDelayedEvent,
                    "update_delayed_event",
                )? {
                    return Ok(update);
                }

                Err(format!(
                    "Could not restart delayed event, even though delayed events are supported. {error}"
                ))
            }
        }
    }

    async fn send_scheduled_delayed_leave_event_or_fallback(
        &self,
        delay_id: &str,
    ) -> Result<ActionUpdate, String> {
        let result = self
            .client
            .update_delayed_event(delay_id, UpdateDelayedEventAction::Send)
            .await;

        match result {
            Ok(()) => {
                self.state.lock().unwrap().has_member_state_event = false;
                self.set_and_emit_delay_id(None);
                self.reset_rate_limit_counter(MembershipActionType::SendScheduledDelayedLeaveEvent);
                Ok(ActionUpdate::Replace(Vec::new()))
            }
            Err(error) => {
                let repeat = MembershipActionType::SendLeaveEvent;
                if matches!(error, ClientError::UnsupportedDelayedEventsEndpoint) {
                    return Ok(ActionUpdate::None);
                }
                if error.is_not_found() {
                    self.set_and_emit_delay_id(None);
                    return Ok(insert_now(repeat));
                }
                if let Some(update) =
                    self.action_update_from_errors(&error, repeat, "update_delayed_event")?
                {
                    return Ok(update);
                }

                // On any other error we fall back to sending the leave state
                // event directly.
                warn!(
                    "Encountered unexpected error during SendScheduledDelayedLeaveEvent. \
                     Falling back to SendLeaveEvent: {error}"
                );
                Ok(insert_now(repeat))
            }
        }
    }

    async fn send_join_event(&self) -> Result<ActionUpdate, String> {
        let content = self.make_my_membership(self.membership_event_expiry_ms());
        let result = self
            .client
            .send_state_event(
                &self.room.room_id,
                StateEventType::CallMember,
                &self.state_key,
                serde_json::to_value(content).expect("membership data always serializes"),
            )
            .await;

        match result {
            Ok(_) => {
                self.set_and_emit_probably_left(false);
                self.reset_rate_limit_counter(MembershipActionType::SendJoinEvent);
                let mut state = self.state.lock().unwrap();
                state.start_time = Some(Instant::now());
                // The next update should already use twice the expiry
                // timeout.
                state.expire_update_iterations = 1;
                state.has_member_state_event = true;

                // An `UpdateExpiry` action might be left over from a
                // previous join event, remove it together with this
                // `SendJoinEvent` action.
                let mut actions: Vec<Action> = state
                    .actions
                    .iter()
                    .filter(|a| {
                        a.kind != MembershipActionType::UpdateExpiry
                            && a.kind != MembershipActionType::SendJoinEvent
                    })
                    .cloned()
                    .collect();
                // To check if the delayed event is still there or got
                // removed by inserting the state event, we need to restart
                // it.
                actions.push(Action {
                    at: Instant::now(),
                    kind: MembershipActionType::RestartDelayedEvent,
                });
                actions.push(Action {
                    at: self.compute_next_expiry_action_at(&state, state.expire_update_iterations),
                    kind: MembershipActionType::UpdateExpiry,
                });
                Ok(ActionUpdate::Replace(actions))
            }
            Err(error) => {
                if let Some(update) = self.action_update_from_errors(
                    &error,
                    MembershipActionType::SendJoinEvent,
                    "send_state_event",
                )? {
                    return Ok(update);
                }
                Err(error.to_string())
            }
        }
    }

    async fn update_expiry_on_joined_event(&self) -> Result<ActionUpdate, String> {
        let next_iteration = self.state.lock().unwrap().expire_update_iterations + 1;
        let content = self.make_my_membership(self.membership_event_expiry_ms() * next_iteration);
        let result = self
            .client
            .send_state_event(
                &self.room.room_id,
                StateEventType::CallMember,
                &self.state_key,
                serde_json::to_value(content).expect("membership data always serializes"),
            )
            .await;

        match result {
            Ok(_) => {
                self.reset_rate_limit_counter(MembershipActionType::UpdateExpiry);
                let mut state = self.state.lock().unwrap();
                state.expire_update_iterations = next_iteration;
                let at = self.compute_next_expiry_action_at(&state, next_iteration);
                Ok(ActionUpdate::Insert(vec![Action {
                    at,
                    kind: MembershipActionType::UpdateExpiry,
                }]))
            }
            Err(error) => {
                if let Some(update) = self.action_update_from_errors(
                    &error,
                    MembershipActionType::UpdateExpiry,
                    "send_state_event",
                )? {
                    return Ok(update);
                }
                Err(error.to_string())
            }
        }
    }

    async fn send_fallback_leave_event(&self) -> Result<ActionUpdate, String> {
        let result = self
            .client
            .send_state_event(
                &self.room.room_id,
                StateEventType::CallMember,
                &self.state_key,
                json!({}),
            )
            .await;

        match result {
            Ok(_) => {
                self.reset_rate_limit_counter(MembershipActionType::SendLeaveEvent);
                self.state.lock().unwrap().has_member_state_event = false;
                Ok(ActionUpdate::Replace(Vec::new()))
            }
            Err(error) => {
                if let Some(update) = self.action_update_from_errors(
                    &error,
                    MembershipActionType::SendLeaveEvent,
                    "send_state_event",
                )? {
                    return Ok(update);
                }
                Err(error.to_string())
            }
        }
    }

    /// Constructs our own membership data.
    fn make_my_membership(&self, expires: u64) -> SessionMembershipData {
        let state = self.state.lock().unwrap();
        let needs_empty_string_room_fix =
            self.slot.application == "m.call" && self.slot.id == "ROOM";

        SessionMembershipData {
            application: self.slot.application.clone(),
            // Revert back to "" just for sending the event.
            call_id: if needs_empty_string_room_fix {
                String::new()
            } else {
                self.slot.id.clone()
            },
            scope: Some(ruma::events::call::member::CallScope::Room),
            device_id: self.device_id.to_string(),
            // For session events we use the colon separated user ID and
            // device ID. The SFU will automatically assign those values to
            // the media participant.
            membership_id: Some(format!("{}:{}", self.user_id, self.device_id)),
            expires: Some(expires),
            call_intent: self.config.call_intent.clone(),
            focus_active: FocusActive::livekit_oldest_membership(),
            foci_preferred: state.foci_preferred.clone(),
            created_ts: state
                .own_membership
                .as_ref()
                .map(CallMembership::created_ts),
        }
    }

    /// Check if this is an MSC4140 `M_MAX_DELAY_EXCEEDED` error and update
    /// the delay override for the next try.
    fn manage_max_delay_exceeded_situation(&self, error: &ClientError) -> bool {
        if let Some(max_delay) = error.max_delay() {
            let max_delay_ms = u64::try_from(max_delay.as_millis()).unwrap_or(u64::MAX);
            if self.delayed_leave_event_delay_ms() > max_delay_ms {
                self.state.lock().unwrap().delayed_leave_delay_override_ms = Some(max_delay_ms);
            }
            warn!(
                "Retry sending delayed disconnection event due to server timeout limitations: {error}"
            );
            return true;
        }
        false
    }

    fn action_update_from_errors(
        &self,
        error: &ClientError,
        kind: MembershipActionType,
        method: &str,
    ) -> Result<Option<ActionUpdate>, String> {
        if let Some(update) = self.action_update_from_rate_limit_error(error, kind, method)? {
            return Ok(Some(update));
        }
        self.action_update_from_network_error_retry(error, kind)
    }

    /// Check if we have a rate limit error and schedule the same action
    /// again if we don't exceed the rate limit retry count yet.
    fn action_update_from_rate_limit_error(
        &self,
        error: &ClientError,
        kind: MembershipActionType,
        method: &str,
    ) -> Result<Option<ActionUpdate>, String> {
        if !error.is_rate_limit() {
            return Ok(None);
        }

        let mut state = self.state.lock().unwrap();
        let retries = *state.rate_limit_retries.get(&kind).unwrap_or(&0);
        if retries < self.config.maximum_rate_limit_retry_count {
            let resend_delay = error.retry_after().unwrap_or(Duration::from_secs(5));
            info!("Rate limited by server, retrying in {resend_delay:?}");
            state.rate_limit_retries.insert(kind, retries + 1);
            return Ok(Some(insert_in(kind, resend_delay)));
        }

        Err(format!(
            "Exceeded maximum retries for {kind:?} attempts (client.{method})"
        ))
    }

    /// Check if we have a transient network error and schedule the same
    /// action again if we don't exceed the network error retry count yet.
    fn action_update_from_network_error_retry(
        &self,
        error: &ClientError,
        kind: MembershipActionType,
    ) -> Result<Option<ActionUpdate>, String> {
        if !error.is_network_error() {
            return Ok(None);
        }

        // We do not wait for the retry duration on local timeouts.
        let retry_duration = if matches!(error, ClientError::LocalTimeout) {
            Duration::ZERO
        } else {
            Duration::from_millis(self.config.network_error_retry_ms)
        };

        let mut state = self.state.lock().unwrap();
        let retries = *state.network_error_retries.get(&kind).unwrap_or(&0);
        if retries < self.config.maximum_network_error_retry_count {
            warn!(
                "Network error while sending event, retrying in {retry_duration:?} \
                 ({retries}/{}): {error}",
                self.config.maximum_network_error_retry_count
            );
            state.network_error_retries.insert(kind, retries + 1);
            return Ok(Some(insert_in(kind, retry_duration)));
        }

        Err(format!(
            "Reached maximum ({}) retries cause by: {error}",
            self.config.maximum_network_error_retry_count
        ))
    }
}
