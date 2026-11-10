// SPDX-License-Identifier: GPL-3.0-or-later

//! Management of the media encryption keys of a call.
//!
//! This is a port of `matrix-js-sdk`'s `RTCEncryptionManager`. It is
//! responsible for distributing our key to the other participants and
//! rotating the keys if needed. Used with the to-device transport it shares
//! the existing key only with new joiners, and rotates when someone leaves.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::{
    Engine as _,
    engine::{
        DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig, general_purpose::STANDARD_NO_PAD,
    },
};
use tokio::time::Instant;
use tracing::{debug, error, info, warn};

use crate::{
    call_membership::CallMembership,
    key_transport::{
        CallMembershipIdentity, KeyTransport, ParticipantDeviceInfo, encryption_key_map_key,
    },
    outdated_key_filter::{InboundEncryptionSession, OutdatedKeyFilter},
};

/// A lenient base64 engine matching the reference implementation's decoder:
/// padding optional, non-canonical trailing bits accepted.
const LENIENT_BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// Encode bytes as unpadded base64.
pub fn encode_unpadded_base64(bytes: &[u8]) -> String {
    STANDARD_NO_PAD.encode(bytes)
}

/// Decode (padded or unpadded) base64.
pub fn decode_base64(value: &str) -> Result<Vec<u8>, base64::DecodeError> {
    LENIENT_BASE64.decode(value)
}

/// Configuration for the [`RtcEncryptionManager`].
#[derive(Clone, Debug)]
pub struct EncryptionConfig {
    /// If `true`, generate and share a media key for this participant, and
    /// surface media keys of other participants.
    pub manage_media_keys: bool,

    /// The delay, in milliseconds, between sending a new key and starting to
    /// encrypt with it. This gives others a chance to receive the new key
    /// before media is encrypted with it.
    pub use_key_delay_ms: u64,

    /// Don't rotate the outbound key if the previous one was created less
    /// than this many milliseconds ago, to avoid expensive rotations when
    /// users join in quick succession. Must be higher than
    /// `use_key_delay_ms` to have an effect.
    pub key_rotation_grace_period_ms: u64,

    /// Whether the session uses the MSC4354 sticky event format, which
    /// implies a pseudonymous (hashed) RTC backend identity.
    pub unstable_send_sticky_events: bool,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            manage_media_keys: true,
            use_key_delay_ms: 1000,
            key_rotation_grace_period_ms: 10_000,
            unstable_send_sticky_events: false,
        }
    }
}

/// One entry of a participant's key ring.
#[derive(Clone, Debug)]
pub struct KeyRingEntry {
    /// The key material.
    pub key: Vec<u8>,
    /// The index (id) of the key.
    pub key_index: u32,
    /// The identity of the participant.
    pub membership: CallMembershipIdentity,
    /// The identity of the participant on the RTC backend.
    pub rtc_backend_identity: String,
}

/// The current per-sender outbound media key of this device.
#[derive(Clone, Debug)]
struct OutboundEncryptionSession {
    key: Vec<u8>,
    creation: Instant,
    shared_with: Vec<ParticipantDeviceInfo>,
    key_id: u32,
}

type GetMemberships = dyn Fn() -> Vec<CallMembership> + Send + Sync;
type OnEncryptionKeysChanged = dyn Fn(&[u8], u32, &CallMembershipIdentity, &str) + Send + Sync;
type RtcIdentityProvider = dyn Fn(&str, &str, &str) -> String + Send + Sync;

#[allow(clippy::struct_excessive_bools)]
struct State {
    manage_media_keys: bool,
    use_hashed_rtc_backend_identity: bool,
    own_rtc_backend_identity_cache: Option<String>,
    use_key_delay: Duration,
    key_rotation_grace_period: Duration,
    outbound_session: Option<OutboundEncryptionSession>,
    /// Ensures that there is only one distribute operation at a time.
    rollout_in_progress: bool,
    /// If a new key distribution is requested while one is going on, this is
    /// set so that a new round is started after the current one.
    need_to_ensure_key_again: bool,
    participant_key_rings: HashMap<String, Vec<KeyRingEntry>>,
    keys_without_matching_rtc_membership: Vec<(Vec<u8>, u32, CallMembershipIdentity)>,
    key_buffer: OutdatedKeyFilter,
}

struct Inner {
    own_membership: CallMembershipIdentity,
    get_memberships: Box<GetMemberships>,
    transport: Arc<dyn KeyTransport>,
    /// Callback to notify the media layer of new keys.
    on_encryption_keys_changed: Box<OnEncryptionKeysChanged>,
    rtc_identity_provider: Box<RtcIdentityProvider>,
    state: Mutex<State>,
}

/// `RtcEncryptionManager` is used to manage the encryption keys for a call.
///
/// The encryption manager stores received keys because the application layer
/// might not be ready yet to handle them; they can be retrieved later with
/// [`Self::get_encryption_keys`].
pub struct RtcEncryptionManager {
    inner: Arc<Inner>,
}

impl RtcEncryptionManager {
    /// Construct a manager for our own membership identity.
    ///
    /// * `get_memberships` - Returns the current call memberships.
    /// * `transport` - The key transport used to distribute keys.
    /// * `on_encryption_keys_changed` - Callback notifying the media layer of
    ///   new keys `(key, key_index, membership, rtc_backend_identity)`.
    /// * `rtc_identity_provider` - Optional provider computing the pseudonymous
    ///   RTC backend identity from `(user_id, device_id, member_id)`. Only used
    ///   with the sticky event format; this is the seam for MSC4354, which is
    ///   otherwise out of scope for now.
    pub fn new(
        own_membership: CallMembershipIdentity,
        get_memberships: impl Fn() -> Vec<CallMembership> + Send + Sync + 'static,
        transport: Arc<dyn KeyTransport>,
        on_encryption_keys_changed: impl Fn(&[u8], u32, &CallMembershipIdentity, &str)
        + Send
        + Sync
        + 'static,
        rtc_identity_provider: Option<Box<RtcIdentityProvider>>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                own_membership,
                get_memberships: Box::new(get_memberships),
                transport,
                on_encryption_keys_changed: Box::new(on_encryption_keys_changed),
                rtc_identity_provider: rtc_identity_provider.unwrap_or_else(|| {
                    Box::new(|user_id, device_id, _member_id| {
                        // Computing the real pseudonymous (hashed) identity
                        // is out of scope for now; fall back to the legacy
                        // form.
                        warn!(
                            "No RTC identity provider configured, falling back to the legacy identity"
                        );
                        format!("{user_id}:{device_id}")
                    })
                }),
                state: Mutex::new(State {
                    manage_media_keys: false,
                    use_hashed_rtc_backend_identity: false,
                    own_rtc_backend_identity_cache: None,
                    use_key_delay: Duration::from_secs(5),
                    key_rotation_grace_period: Duration::from_secs(10),
                    outbound_session: None,
                    rollout_in_progress: false,
                    need_to_ensure_key_again: false,
                    participant_key_rings: HashMap::new(),
                    keys_without_matching_rtc_membership: Vec::new(),
                    key_buffer: OutdatedKeyFilter::new(),
                }),
            }),
        }
    }

    /// Join: configure the manager and start the transport.
    #[allow(clippy::needless_pass_by_value)]
    pub fn join(&self, config: EncryptionConfig) {
        {
            let mut state = self.inner.state.lock().unwrap();
            state.manage_media_keys = config.manage_media_keys;
            state.use_hashed_rtc_backend_identity = config.unstable_send_sticky_events;
            state.use_key_delay = Duration::from_millis(config.use_key_delay_ms);
            state.key_rotation_grace_period =
                Duration::from_millis(config.key_rotation_grace_period_ms);
        }
        // Precompute our own identity.
        self.inner.own_rtc_backend_identity();

        info!("Joining room");
        self.inner.transport.start();
    }

    /// Leave: stop the transport and clean up the stored keys.
    pub fn leave(&self) {
        self.inner.transport.stop();
        self.inner
            .state
            .lock()
            .unwrap()
            .participant_key_rings
            .clear();
    }

    /// The encryption keys currently known to the manager, by participant
    /// map key (see
    /// [`encryption_key_map_key`](crate::key_transport::encryption_key_map_key)).
    pub fn get_encryption_keys(&self) -> HashMap<String, Vec<KeyRingEntry>> {
        self.inner
            .state
            .lock()
            .unwrap()
            .participant_key_rings
            .clone()
    }

    /// Call this when the memberships of the session have been updated.
    pub fn on_memberships_update(&self) {
        // Ensure the key is distributed. This is a no-op if the key is
        // already distributed to everyone. If there is an ongoing
        // distribution, it will be completed before a new one is started.
        self.inner.ensure_key_distribution();
        // Ensure key emission to the RTC backend for early-received keys.
        self.inner.check_keys_without_matching_rtc_membership();
    }

    /// Call this when a key was received over the transport.
    pub fn on_new_key_received(
        &self,
        membership: CallMembershipIdentity,
        key_base64: &str,
        index: u32,
        timestamp: u64,
    ) {
        self.inner
            .on_new_key_received(membership, key_base64, index, timestamp);
    }
}

impl Inner {
    fn own_rtc_backend_identity(&self) -> String {
        let mut state = self.state.lock().unwrap();
        if let Some(cached) = &state.own_rtc_backend_identity_cache {
            return cached.clone();
        }

        let identity = if state.use_hashed_rtc_backend_identity {
            let CallMembershipIdentity {
                user_id,
                device_id,
                member_id,
            } = &self.own_membership;
            info!("Computing RTC backend identity for {user_id}:{device_id}:{member_id}");
            (self.rtc_identity_provider)(user_id, device_id, member_id)
        } else {
            format!(
                "{}:{}",
                self.own_membership.user_id, self.own_membership.device_id
            )
        };
        state.own_rtc_backend_identity_cache = Some(identity.clone());
        identity
    }

    #[allow(clippy::needless_pass_by_value)]
    fn on_new_key_received(
        self: &Arc<Self>,
        membership: CallMembershipIdentity,
        key_base64: &str,
        index: u32,
        timestamp: u64,
    ) {
        if !self.state.lock().unwrap().manage_media_keys {
            warn!(
                "Received key over transport {}:{} at index {index} but media keys are disabled",
                membership.user_id, membership.device_id
            );
            return;
        }
        debug!(
            "Received key over transport {}:{} at index {index}",
            membership.user_id, membership.device_id
        );

        let Ok(key) = decode_base64(key_base64) else {
            warn!("Received a key that is not valid base64, dropping it");
            return;
        };
        let candidate = InboundEncryptionSession {
            key,
            membership: membership.clone(),
            key_index: index,
            creation_ts: timestamp,
        };

        let outdated = self
            .state
            .lock()
            .unwrap()
            .key_buffer
            .is_outdated(&membership, &candidate);
        if outdated {
            info!(
                "Received an out of order key for {}:{}, dropping it",
                membership.user_id, membership.device_id
            );
        } else {
            self.add_key_to_participant(candidate.key, candidate.key_index, candidate.membership);
        }
    }

    fn check_keys_without_matching_rtc_membership(self: &Arc<Self>) {
        let pending = std::mem::take(
            &mut self
                .state
                .lock()
                .unwrap()
                .keys_without_matching_rtc_membership,
        );
        for (key, key_index, membership) in pending {
            self.add_key_to_participant(key, key_index, membership);
        }
    }

    fn add_key_to_participant(
        self: &Arc<Self>,
        key: Vec<u8>,
        key_index: u32,
        membership: CallMembershipIdentity,
    ) {
        let known_rtc_memberships = (self.get_memberships)();
        let full_membership = known_rtc_memberships.iter().find(|member| {
            member.user_id() == membership.user_id && member.device_id() == membership.device_id
        });
        let Some(full_membership) = full_membership else {
            info!(
                "No matching RTC membership for key from {}:{}, delaying key addition",
                membership.user_id, membership.device_id
            );
            self.state
                .lock()
                .unwrap()
                .keys_without_matching_rtc_membership
                .push((key, key_index, membership));
            return;
        };
        let rtc_backend_identity = full_membership.rtc_backend_identity();
        self.add_key_to_participant_with_backend_identity(
            &key,
            key_index,
            &membership,
            &rtc_backend_identity,
        );
    }

    fn add_key_to_participant_with_backend_identity(
        &self,
        key: &[u8],
        key_index: u32,
        membership: &CallMembershipIdentity,
        rtc_backend_identity: &str,
    ) {
        {
            let mut state = self.state.lock().unwrap();
            state
                .participant_key_rings
                .entry(encryption_key_map_key(membership))
                .or_default()
                .push(KeyRingEntry {
                    key: key.to_vec(),
                    key_index,
                    membership: membership.clone(),
                    rtc_backend_identity: rtc_backend_identity.to_owned(),
                });
        }
        (self.on_encryption_keys_changed)(key, key_index, membership, rtc_backend_identity);
    }

    /// Ensure that a key is distributed and used to encrypt our media.
    ///
    /// If there is already a key distribution in progress a new distribution
    /// round is scheduled just after the current one; repeated calls while a
    /// distribution is in progress are coalesced into that single new round.
    fn ensure_key_distribution(self: &Arc<Self>) {
        {
            let mut state = self.state.lock().unwrap();
            if !state.manage_media_keys {
                return;
            }
            if state.rollout_in_progress {
                // There is a rollout in progress; remember that a new one is
                // needed after the current one.
                debug!("Rollout in progress, a new rollout will be started after the current one");
                state.need_to_ensure_key_again = true;
                return;
            }
            debug!("No active rollout, start a new one");
            state.rollout_in_progress = true;
        }

        let inner = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                inner.rollout_outbound_key().await;
                debug!("Rollout completed");
                let mut state = inner.state.lock().unwrap();
                if state.need_to_ensure_key_again {
                    debug!("New rollout needed");
                    state.need_to_ensure_key_again = false;
                    continue;
                }
                state.rollout_in_progress = false;
                break;
            }
        });
    }

    #[allow(clippy::too_many_lines)]
    async fn rollout_outbound_key(self: &Arc<Self>) {
        // Create the first key if there is none yet, and roll it out
        // immediately.
        let first_key = {
            let mut state = self.state.lock().unwrap();
            if state.outbound_session.is_none() {
                let key = generate_random_key();
                state.outbound_session = Some(OutboundEncryptionSession {
                    key: key.clone(),
                    creation: Instant::now(),
                    shared_with: Vec::new(),
                    key_id: 0,
                });
                Some(key)
            } else {
                None
            }
        };
        if let Some(key) = first_key {
            let identity = self.own_rtc_backend_identity();
            self.add_key_to_participant_with_backend_identity(
                &key,
                0,
                &self.own_membership,
                &identity,
            );
        }

        // Get the current memberships.
        let to_share_with: Vec<ParticipantDeviceInfo> = (self.get_memberships)()
            .iter()
            .map(|membership| ParticipantDeviceInfo {
                user_id: membership.user_id().to_owned(),
                device_id: membership.device_id().to_owned(),
                membership_ts: membership.created_ts(),
            })
            .collect();

        let (to_distribute_to, key, key_id, has_key_changed) = {
            let mut state = self.state.lock().unwrap();
            let grace_period = state.key_rotation_grace_period;
            let session = state.outbound_session.as_ref().expect("created above");
            let (session_key, session_key_id, session_creation) =
                (session.key.clone(), session.key_id, session.creation);

            // Some users might have re-created their membership event,
            // meaning they might have cleared their key. Treat them as not
            // shared with.
            let already_shared_with: Vec<ParticipantDeviceInfo> = session
                .shared_with
                .iter()
                .filter(|x| {
                    !to_share_with.iter().any(|o| {
                        x.user_id == o.user_id
                            && x.device_id == o.device_id
                            && x.membership_ts != o.membership_ts
                    })
                })
                .cloned()
                .collect();

            let any_left = already_shared_with
                .iter()
                .any(|x| !to_share_with.contains(x));
            let any_joined: Vec<ParticipantDeviceInfo> = to_share_with
                .iter()
                .filter(|x| !already_shared_with.contains(x))
                .cloned()
                .collect();

            if any_left {
                // We need to rotate the key.
                let session = create_new_outbound_session(&mut state);
                (to_share_with, session.key, session.key_id, true)
            } else if !any_joined.is_empty() {
                let key_age = session_creation.elapsed();
                // If the current key was created recently we can keep it and
                // just distribute it to the new joiners.
                if key_age < grace_period {
                    debug!("New joiners detected, but the key is recent enough, keeping it");
                    (any_joined, session_key, session_key_id, false)
                } else {
                    debug!("New joiners detected, rotating the key");
                    let session = create_new_outbound_session(&mut state);
                    (to_share_with, session.key, session.key_id, true)
                }
            } else {
                // No changes.
                return;
            }
        };

        let result = self
            .transport
            .send_key(&encode_unpadded_base64(&key), key_id, &to_distribute_to)
            .await;
        if let Err(err) = result {
            error!("Failed to rollout key: {err}");
            return;
        }

        let use_key_delay = {
            let mut state = self.state.lock().unwrap();
            if let Some(session) = &mut state.outbound_session
                && session.key_id == key_id
            {
                session.shared_with.extend(to_distribute_to);
            }
            state.use_key_delay
        };

        if has_key_changed {
            // It is recommended not to start using a key immediately but to
            // wait for a short time to make sure it is delivered first.
            debug!("Delaying rollout for key {key_id}...");
            tokio::time::sleep(use_key_delay).await;
            debug!("...delayed rollout of index {key_id}");
            let identity = self.own_rtc_backend_identity();
            self.add_key_to_participant_with_backend_identity(
                &key,
                key_id,
                &self.own_membership,
                &identity,
            );
        }
    }
}

/// Create a new outbound session with the next key index and set it as the
/// current one.
fn create_new_outbound_session(state: &mut State) -> OutboundEncryptionSession {
    let next_key_id = state
        .outbound_session
        .as_ref()
        .map_or(0, |session| (session.key_id + 1) % 256);
    let session = OutboundEncryptionSession {
        key: generate_random_key(),
        creation: Instant::now(),
        shared_with: Vec::new(),
        key_id: next_key_id,
    };
    info!("Creating new outbound key index {next_key_id}");
    state.outbound_session = Some(session.clone());
    session
}

fn generate_random_key() -> Vec<u8> {
    let mut key = vec![0u8; 16];
    getrandom::fill(&mut key).expect("system randomness is available");
    key
}
