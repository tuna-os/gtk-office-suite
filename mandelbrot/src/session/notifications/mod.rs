use std::{borrow::Cow, time::Duration};

use gettextrs::gettext;
use gtk::{gdk, gio, glib, prelude::*, subclass::prelude::*};
use matrix_sdk::{Room as MatrixRoom, sync::Notification};
use ruma::{
    OwnedRoomId, RoomId, UserId,
    api::client::device::get_device,
    events::{
        AnyMessageLikeEventContent, AnyStrippedStateEvent, AnySyncStateEvent, AnySyncTimelineEvent,
        SyncStateEvent,
        room::{member::MembershipState, message::MessageType},
        rtc::notification::CallIntent,
    },
    html::{HtmlSanitizerMode, RemoveReplyFallback},
};
use tracing::{debug, warn};

mod notifications_settings;

pub(crate) use self::notifications_settings::{
    NotificationsGlobalSetting, NotificationsRoomSetting, NotificationsSettings,
};
use super::{IdentityVerification, Session, VerificationKey};
use crate::{
    Application, Window, gettext_f,
    intent::SessionIntent,
    prelude::*,
    spawn_tokio,
    utils::{
        OneshotNotifier,
        matrix::{AnySyncOrStrippedTimelineEvent, MatrixEventIdUri, MatrixIdUri, MatrixRoomIdUri},
    },
};

/// The maximum number of lines we want to display for the body of a
/// notification.
// This is taken from GNOME Shell's behavior:
// <https://gitlab.gnome.org/GNOME/gnome-shell/-/blob/c7778e536b094fae4d0694af6103cf4ad75050d3/js/ui/messageList.js#L24>
const MAX_BODY_LINES: usize = 6;
/// The maximum number of characters that we want to display for the body of a
/// notification. We assume that the system shows at most 100 characters per
/// line, so this is `MAX_BODY_LINES * 100`.
const MAX_BODY_CHARS: usize = MAX_BODY_LINES * 100;

mod imp {
    use std::{
        cell::RefCell,
        collections::{HashMap, HashSet},
    };

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::Notifications)]
    pub struct Notifications {
        /// The current session.
        #[property(get, set = Self::set_session, explicit_notify, nullable)]
        session: glib::WeakRef<Session>,
        /// The push notifications that were presented.
        ///
        /// A map of room ID to list of notification IDs.
        pub(super) push: RefCell<HashMap<OwnedRoomId, HashSet<String>>>,
        /// The identity verification notifications that were presented.
        ///
        /// A map of verification key to notification ID.
        pub(super) identity_verifications: RefCell<HashMap<VerificationKey, String>>,
        /// The notifications settings for this session.
        #[property(get)]
        settings: NotificationsSettings,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Notifications {
        const NAME: &'static str = "Notifications";
        type Type = super::Notifications;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Notifications {}

    impl Notifications {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            if self.session.upgrade().as_ref() == session {
                return;
            }

            self.session.set(session);
            self.obj().notify_session();

            self.settings.set_session(session);
        }
    }
}

glib::wrapper! {
    /// The notifications of a `Session`.
    pub struct Notifications(ObjectSubclass<imp::Notifications>);
}

impl Notifications {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Whether notifications are enabled for the current session.
    pub(crate) fn enabled(&self) -> bool {
        let settings = self.settings();
        settings.account_enabled() && settings.session_enabled()
    }

    /// Helper method to create notification
    fn send_notification(
        id: &str,
        title: &str,
        body: &str,
        session_id: &str,
        intent: &SessionIntent,
        icon: Option<&gdk::Texture>,
        accept_call_intent: Option<&SessionIntent>,
    ) {
        let notification = gio::Notification::new(title);

        if let Some(accept_call_intent) = accept_call_intent {
            // An incoming call. Not all notification servers know the
            // category or the extra actions, this degrades gracefully to a
            // regular notification.
            notification.set_category(Some("call.incoming"));
            notification.set_priority(gio::NotificationPriority::Urgent);

            let accept_target =
                accept_call_intent.to_variant_with_session_id(session_id.to_owned());
            notification.add_button_with_target_value(
                &gettext("Accept"),
                accept_call_intent.app_action_name(),
                Some(&accept_target),
            );
            notification.add_button_with_target_value(
                &gettext("Decline"),
                "app.withdraw-notification",
                Some(&id.to_variant()),
            );
        } else {
            notification.set_category(Some("im.received"));
            notification.set_priority(gio::NotificationPriority::High);
        }

        // Truncate the body if necessary.
        let body = if let Some((end, _)) = body.char_indices().nth(MAX_BODY_CHARS) {
            let mut body = body[..end].trim_end().to_owned();
            if !body.ends_with('…') {
                body.push('…');
            }
            Cow::Owned(body)
        } else {
            Cow::Borrowed(body)
        };

        notification.set_body(Some(&body));

        let action = intent.app_action_name();
        let target_value = intent.to_variant_with_session_id(session_id.to_owned());
        notification.set_default_action_and_target_value(action, Some(&target_value));

        if let Some(notification_icon) = icon {
            notification.set_icon(notification_icon);
        }

        Application::default().send_notification(Some(id), &notification);
    }

    /// Ask the system to show the given push notification, if applicable.
    ///
    /// The notification will not be shown if the application is active and the
    /// room of the event is displayed.
    #[allow(clippy::too_many_lines)]
    pub(crate) async fn show_push(
        &self,
        matrix_notification: Notification,
        matrix_room: MatrixRoom,
    ) {
        // Do not show notifications if they are disabled.
        if !self.enabled() {
            return;
        }

        let Some(session) = self.session() else {
            return;
        };

        let app = Application::default();
        let window = app.active_window().and_downcast::<Window>();
        let session_id = session.session_id();
        let room_id = matrix_room.room_id();

        // Do not show notifications for the current room in the current session if the
        // window is active.
        if window.is_some_and(|w| {
            w.is_active()
                && w.current_session_id().as_deref() == Some(session_id)
                && w.session_view()
                    .selected_room()
                    .is_some_and(|r| r.room_id() == room_id)
        }) {
            return;
        }

        let Some(room) = session
            .room_list()
            .get_wait(room_id, Some(Duration::from_secs(10)))
            .await
        else {
            warn!("Could not display notification for missing room {room_id}",);
            return;
        };

        if !room.is_room_info_initialized() {
            // Wait for the room to finish initializing, otherwise we will not have the
            // display name or the avatar.
            let notifier = OneshotNotifier::<()>::new("Notifications::show_push");
            let receiver = notifier.listen();

            let handler_id = room.connect_is_room_info_initialized_notify(move |_| {
                notifier.notify();
            });

            receiver.await;
            room.disconnect(handler_id);
        }

        let event = match AnySyncOrStrippedTimelineEvent::from_raw(&matrix_notification.event) {
            Ok(event) => event,
            Err(error) => {
                warn!(
                    "Could not display notification for unrecognized event in room {room_id}: {error}",
                );
                return;
            }
        };

        let is_direct = room.direct_member().is_some();
        let sender_id = event.sender();
        let owned_sender_id = sender_id.to_owned();
        let handle =
            spawn_tokio!(async move { matrix_room.get_member_no_sync(&owned_sender_id).await });

        let sender = match handle.await.expect("task was not aborted") {
            Ok(member) => member,
            Err(error) => {
                warn!("Could not get member for notification: {error}");
                None
            }
        };

        let sender_name = sender.as_ref().map_or_else(
            || sender_id.localpart().to_owned(),
            |member| {
                let name = member.name();

                if member.name_ambiguous() {
                    format!("{name} ({})", member.user_id())
                } else {
                    name.to_owned()
                }
            },
        );

        let (body, is_invite, is_call) =
            // These are ordered by the likelihood of an event being of the type to reduce checking
            // in the common case.
            if let Some(body) =
                message_notification_body(&event, &sender_name, !is_direct)
            {
                (body, false, false)
            } else if let Some(body) =
                incoming_call_notification_body(&event, &sender_name, is_direct)
            {
                (body, false, true)
            } else if let Some(body) =
                own_invite_notification_body(&event, &sender_name, session.user_id())
            {
                (body, true, false)
            } else {
                debug!("Received notification for event of unexpected type {event:?}",);
                return;
            };

        let room_id = room.room_id().to_owned();
        let event_id = event.event_id();

        let room_uri = MatrixRoomIdUri {
            id: room_id.clone().into(),
            via: vec![],
        };
        let matrix_uri = if let Some(event_id) = event_id {
            MatrixIdUri::Event(MatrixEventIdUri {
                event_id: event_id.to_owned(),
                room_uri,
            })
        } else {
            MatrixIdUri::Room(room_uri)
        };

        let id = if event_id.is_some() {
            format!("{session_id}//{matrix_uri}")
        } else {
            let random_id = glib::uuid_string_random();
            format!("{session_id}//{matrix_uri}//{random_id}")
        };

        let inhibit_image = is_invite && !session.global_account_data().invite_avatars_enabled();
        let icon = room.avatar_data().as_notification_icon(inhibit_image).await;

        let accept_call_intent = is_call.then(|| {
            SessionIntent::ShowRoomCall(MatrixIdUri::Room(MatrixRoomIdUri {
                id: room_id.clone().into(),
                via: vec![],
            }))
        });

        Self::send_notification(
            &id,
            &room.display_name(),
            &body,
            session_id,
            &SessionIntent::ShowMatrixId(matrix_uri),
            icon.as_ref(),
            accept_call_intent.as_ref(),
        );

        self.imp()
            .push
            .borrow_mut()
            .entry(room_id)
            .or_default()
            .insert(id);
    }

    /// Show a notification for the given in-room identity verification.
    pub(crate) async fn show_in_room_identity_verification(
        &self,
        verification: &IdentityVerification,
    ) {
        // Do not show notifications if they are disabled.
        if !self.enabled() {
            return;
        }

        let Some(session) = self.session() else {
            return;
        };
        let Some(room) = verification.room() else {
            return;
        };

        let room_id = room.room_id().to_owned();
        let session_id = session.session_id();
        let flow_id = verification.flow_id();

        // In-room verifications should only happen for other users.
        let user = verification.user();
        let user_id = user.user_id();

        let title = gettext("Verification Request");
        let body = gettext_f(
            // Translators: Do NOT translate the content between '{' and '}', this is a
            // variable name.
            "{user} sent a verification request",
            &[("user", &user.display_name())],
        );

        let icon = user.avatar_data().as_notification_icon(false).await;

        let id = format!("{session_id}//{room_id}//{user_id}//{flow_id}");
        Self::send_notification(
            &id,
            &title,
            &body,
            session_id,
            &SessionIntent::ShowIdentityVerification(verification.key()),
            icon.as_ref(),
            None,
        );

        self.imp()
            .identity_verifications
            .borrow_mut()
            .insert(verification.key(), id);
    }

    /// Show a notification for the given to-device identity verification.
    pub(crate) async fn show_to_device_identity_verification(
        &self,
        verification: &IdentityVerification,
    ) {
        // Do not show notifications if they are disabled.
        if !self.enabled() {
            return;
        }

        let Some(session) = self.session() else {
            return;
        };
        // To-device verifications should only happen for other sessions.
        let Some(other_device_id) = verification.other_device_id() else {
            return;
        };

        let session_id = session.session_id();
        let flow_id = verification.flow_id();

        let client = session.client();
        let request = get_device::v3::Request::new(other_device_id.clone());
        let handle = spawn_tokio!(async move { client.send(request).await });

        let display_name = match handle.await.expect("task was not aborted") {
            Ok(res) => res.device.display_name,
            Err(error) => {
                warn!("Could not get device for notification: {error}");
                None
            }
        };
        let display_name = display_name
            .as_deref()
            .unwrap_or_else(|| other_device_id.as_str());

        let title = gettext("Login Request From Another Session");
        let body = gettext_f(
            // Translators: Do NOT translate the content between '{' and '}', this is a
            // variable name.
            "Verify your new session “{name}”",
            &[("name", display_name)],
        );

        let id = format!("{session_id}//{other_device_id}//{flow_id}");

        Self::send_notification(
            &id,
            &title,
            &body,
            session_id,
            &SessionIntent::ShowIdentityVerification(verification.key()),
            None,
            None,
        );

        self.imp()
            .identity_verifications
            .borrow_mut()
            .insert(verification.key(), id);
    }

    /// Ask the system to remove the known notifications for the room with the
    /// given ID.
    ///
    /// Only the notifications that were shown since the application's startup
    /// are known, older ones might still be present.
    pub(crate) fn withdraw_all_for_room(&self, room_id: &RoomId) {
        if let Some(notifications) = self.imp().push.borrow_mut().remove(room_id) {
            let app = Application::default();

            for id in notifications {
                app.withdraw_notification(&id);
            }
        }
    }

    /// Ask the system to remove the known notification for the identity
    /// verification with the given key.
    pub(crate) fn withdraw_identity_verification(&self, key: &VerificationKey) {
        if let Some(id) = self.imp().identity_verifications.borrow_mut().remove(key) {
            let app = Application::default();
            app.withdraw_notification(&id);
        }
    }

    /// Ask the system to remove all the known notifications for this session.
    ///
    /// Only the notifications that were shown since the application's startup
    /// are known, older ones might still be present.
    pub(crate) fn clear(&self) {
        let app = Application::default();

        for id in self.imp().push.take().values().flatten() {
            app.withdraw_notification(id);
        }
        for id in self.imp().identity_verifications.take().values() {
            app.withdraw_notification(id);
        }
    }
}

impl Default for Notifications {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate the notification body for the given event, if it is a message-like
/// event.
///
/// If it's a media message, this will return a localized body.
///
/// Returns `None` if it is not a message-like event or if the message type is
/// not supported.
pub(crate) fn message_notification_body(
    event: &AnySyncOrStrippedTimelineEvent,
    sender_name: &str,
    show_sender: bool,
) -> Option<String> {
    let AnySyncOrStrippedTimelineEvent::Sync(sync_event) = event else {
        return None;
    };
    let AnySyncTimelineEvent::MessageLike(message_event) = &**sync_event else {
        return None;
    };

    match message_event.original_content()? {
        AnyMessageLikeEventContent::RoomMessage(mut message) => {
            message.sanitize(HtmlSanitizerMode::Compat, RemoveReplyFallback::Yes);

            let body = match message.msgtype {
                MessageType::Audio(_) => {
                    gettext_f("{user} sent an audio file.", &[("user", sender_name)])
                }
                MessageType::Emote(content) => format!("{sender_name} {}", content.body),
                MessageType::File(_) => gettext_f("{user} sent a file.", &[("user", sender_name)]),
                MessageType::Image(_) => {
                    gettext_f("{user} sent an image.", &[("user", sender_name)])
                }
                MessageType::Location(_) => {
                    gettext_f("{user} sent their location.", &[("user", sender_name)])
                }
                MessageType::Notice(content) => {
                    text_event_body(content.body, sender_name, show_sender)
                }
                MessageType::ServerNotice(content) => {
                    text_event_body(content.body, sender_name, show_sender)
                }
                MessageType::Text(content) => {
                    text_event_body(content.body, sender_name, show_sender)
                }
                MessageType::Video(_) => {
                    gettext_f("{user} sent a video.", &[("user", sender_name)])
                }
                _ => return None,
            };
            Some(body)
        }
        AnyMessageLikeEventContent::Sticker(_) => Some(gettext_f(
            "{user} sent a sticker.",
            &[("user", sender_name)],
        )),
        _ => None,
    }
}

fn text_event_body(message: String, sender_name: &str, show_sender: bool) -> String {
    if show_sender {
        gettext_f(
            "{user}: {message}",
            &[("user", sender_name), ("message", &message)],
        )
    } else {
        message
    }
}

/// Generate the notification body for the given event, if it is an invite for
/// our own user.
///
/// This will return a localized body.
///
/// Returns `None` if it is not an invite for our own user.
pub(crate) fn own_invite_notification_body(
    event: &AnySyncOrStrippedTimelineEvent,
    sender_name: &str,
    own_user_id: &UserId,
) -> Option<String> {
    let (membership, state_key) = match event {
        AnySyncOrStrippedTimelineEvent::Sync(sync_event) => {
            if let AnySyncTimelineEvent::State(AnySyncStateEvent::RoomMember(member_event)) =
                &**sync_event
            {
                match member_event {
                    SyncStateEvent::Original(original_event) => (
                        &original_event.content.membership,
                        &original_event.state_key,
                    ),
                    SyncStateEvent::Redacted(redacted_event) => (
                        &redacted_event.content.membership,
                        &redacted_event.state_key,
                    ),
                }
            } else {
                return None;
            }
        }
        AnySyncOrStrippedTimelineEvent::Stripped(stripped_event) => {
            if let AnyStrippedStateEvent::RoomMember(member_event) = &**stripped_event {
                (&member_event.content.membership, &member_event.state_key)
            } else {
                return None;
            }
        }
    };

    if *membership == MembershipState::Invite && state_key == own_user_id {
        // Translators: Do NOT translate the content between '{' and '}', this is a
        // variable name.
        Some(gettext_f("{user} invited you", &[("user", sender_name)]))
    } else {
        None
    }
}

/// Generate the notification body for a call, if it is an invite for
/// the current user.
///
/// This will return a localized body.
///
/// Returns `None` if it is not an invite for the current user.
pub(crate) fn incoming_call_notification_body(
    event: &AnySyncOrStrippedTimelineEvent,
    sender_name: &str,
    from_dm_room: bool,
) -> Option<String> {
    let AnySyncOrStrippedTimelineEvent::Sync(sync_event) = event else {
        return None;
    };
    let AnySyncTimelineEvent::MessageLike(message_event) = &**sync_event else {
        return None;
    };

    match message_event.original_content()? {
        AnyMessageLikeEventContent::RtcNotification(content) => {
            let body = match (content.call_intent, from_dm_room) {
                (Some(CallIntent::Video), true) => gettext("Incoming video call"),
                (Some(CallIntent::Video), false) => {
                    // Translators: Do NOT translate the content between '{' and '}', this
                    // is a variable name.
                    gettext_f("Incoming video call from {user}", &[("user", sender_name)])
                }
                (_, true) => gettext("Incoming call"),
                // Translators: Do NOT translate the content between '{' and '}', this
                // is a variable name.
                (_, false) => gettext_f("Incoming call from {user}", &[("user", sender_name)]),
            };
            Some(body)
        }
        _ => None,
    }
}
