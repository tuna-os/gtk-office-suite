//! Collection of methods related to the Matrix specification.

use std::{borrow::Cow, fmt, str::FromStr};

use gettextrs::gettext;
use gtk::{glib, prelude::*};
use matrix_sdk::{
    AuthSession, Client, ClientBuildError, SessionMeta, SessionTokens,
    authentication::{
        matrix::MatrixSession,
        oauth::{OAuthSession, UserSession},
    },
    config::RequestConfig,
    deserialized_responses::RawAnySyncOrStrippedTimelineEvent,
    encryption::{BackupDownloadStrategy, EncryptionSettings},
};
use ruma::{
    EventId, IdParseError, MatrixToUri, MatrixUri, MatrixUriError, MilliSecondsSinceUnixEpoch,
    OwnedEventId, OwnedRoomAliasId, OwnedRoomId, OwnedRoomOrAliasId, OwnedServerName,
    OwnedTransactionId, OwnedUserId, RoomId, RoomOrAliasId, UserId,
    events::{AnyStrippedStateEvent, AnySyncTimelineEvent},
    html::{
        Children, Html, NodeRef, StrTendril,
        matrix::{AnchorUri, MatrixElement},
    },
    matrix_uri::MatrixId,
    serde::Raw,
};
use thiserror::Error;

pub(crate) mod ext_traits;
mod media_message;

pub(crate) use self::media_message::*;
use crate::{
    components::{AvatarImageSafetySetting, Pill},
    prelude::*,
    secret::StoredSession,
    session::Room,
};

/// The result of a password validation.
#[derive(Debug, Default, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct PasswordValidity {
    /// Whether the password includes at least one lowercase letter.
    pub(crate) has_lowercase: bool,
    /// Whether the password includes at least one uppercase letter.
    pub(crate) has_uppercase: bool,
    /// Whether the password includes at least one number.
    pub(crate) has_number: bool,
    /// Whether the password includes at least one symbol.
    pub(crate) has_symbol: bool,
    /// Whether the password is at least 8 characters long.
    pub(crate) has_length: bool,
    /// The percentage of checks passed for the password, between 0 and 100.
    ///
    /// If progress is 100, the password is valid.
    pub(crate) progress: u32,
}

impl PasswordValidity {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Validate a password according to the Matrix specification.
///
/// A password should include a lower-case letter, an upper-case letter, a
/// number and a symbol and be at a minimum 8 characters in length.
///
/// See: <https://spec.matrix.org/v1.1/client-server-api/#notes-on-password-management>
pub(crate) fn validate_password(password: &str) -> PasswordValidity {
    let mut validity = PasswordValidity::new();

    for char in password.chars() {
        if char.is_numeric() {
            validity.has_number = true;
        } else if char.is_lowercase() {
            validity.has_lowercase = true;
        } else if char.is_uppercase() {
            validity.has_uppercase = true;
        } else {
            validity.has_symbol = true;
        }
    }

    validity.has_length = password.len() >= 8;

    let mut passed = 0;
    if validity.has_number {
        passed += 1;
    }
    if validity.has_lowercase {
        passed += 1;
    }
    if validity.has_uppercase {
        passed += 1;
    }
    if validity.has_symbol {
        passed += 1;
    }
    if validity.has_length {
        passed += 1;
    }
    validity.progress = passed * 100 / 5;

    validity
}

/// An deserialized event received in a sync response.
#[derive(Debug, Clone)]
pub(crate) enum AnySyncOrStrippedTimelineEvent {
    /// An event from a joined or left room.
    Sync(Box<AnySyncTimelineEvent>),
    /// An event from an invited room.
    Stripped(Box<AnyStrippedStateEvent>),
}

impl AnySyncOrStrippedTimelineEvent {
    /// Deserialize the given raw event.
    pub(crate) fn from_raw(
        raw: &RawAnySyncOrStrippedTimelineEvent,
    ) -> Result<Self, serde_json::Error> {
        let ev = match raw {
            RawAnySyncOrStrippedTimelineEvent::Sync(ev) => Self::Sync(ev.deserialize()?.into()),
            RawAnySyncOrStrippedTimelineEvent::Stripped(ev) => {
                Self::Stripped(Box::new(ev.deserialize()?))
            }
        };

        Ok(ev)
    }

    /// The sender of the event.
    pub(crate) fn sender(&self) -> &UserId {
        match self {
            AnySyncOrStrippedTimelineEvent::Sync(ev) => ev.sender(),
            AnySyncOrStrippedTimelineEvent::Stripped(ev) => ev.sender(),
        }
    }

    /// The ID of the event, if it's not a stripped state event.
    pub(crate) fn event_id(&self) -> Option<&EventId> {
        match self {
            AnySyncOrStrippedTimelineEvent::Sync(ev) => Some(ev.event_id()),
            AnySyncOrStrippedTimelineEvent::Stripped(_) => None,
        }
    }
}

/// All errors that can occur when setting up the Matrix client.
#[derive(Error, Debug)]
pub(crate) enum ClientSetupError {
    /// An error when building the client.
    #[error("Matrix client build error: {0}")]
    Client(#[from] ClientBuildError),
    /// An error when using the client.
    #[error("Matrix client restoration error: {0}")]
    Sdk(#[from] matrix_sdk::Error),
    /// An error creating the unique local ID of the session.
    #[error("Could not generate unique session ID")]
    NoSessionId,
    /// An error accessing the session tokens.
    #[error("Could not access session tokens")]
    NoSessionTokens,
}

impl UserFacingError for ClientSetupError {
    fn to_user_facing(&self) -> String {
        match self {
            Self::Client(err) => err.to_user_facing(),
            Self::Sdk(err) => err.to_user_facing(),
            Self::NoSessionId => gettext("Could not generate unique session ID"),
            Self::NoSessionTokens => gettext("Could not access the session tokens"),
        }
    }
}

/// Create a [`Client`] with the given stored session.
pub(crate) async fn client_with_stored_session(
    session: StoredSession,
    tokens: SessionTokens,
) -> Result<Client, ClientSetupError> {
    let has_refresh_token = tokens.refresh_token.is_some();
    let data_path = session.data_path();
    let cache_path = session.cache_path();

    let StoredSession {
        homeserver,
        user_id,
        device_id,
        passphrase,
        client_id,
        ..
    } = session;

    let meta = SessionMeta { user_id, device_id };
    let session_data: AuthSession = if let Some(client_id) = client_id {
        OAuthSession {
            user: UserSession { meta, tokens },
            client_id,
        }
        .into()
    } else {
        MatrixSession { meta, tokens }.into()
    };

    let encryption_settings = EncryptionSettings {
        auto_enable_cross_signing: true,
        backup_download_strategy: BackupDownloadStrategy::AfterDecryptionFailure,
        // This only enables room keys backup and not recovery, which would leave us in an awkward
        // state, because we want both to be enabled at the same time.
        auto_enable_backups: false,
    };

    let mut client_builder = Client::builder()
        .homeserver_url(homeserver)
        .sqlite_store_with_cache_path(data_path, cache_path, Some(&passphrase))
        // force_auth option to solve an issue with some servers configuration to require
        // auth for profiles:
        // https://gitlab.gnome.org/World/fractal/-/issues/934
        .request_config(RequestConfig::new().retry_limit(2).force_auth())
        .with_encryption_settings(encryption_settings);

    if has_refresh_token {
        client_builder = client_builder.handle_refresh_tokens();
    }

    let client = client_builder.build().await?;

    client.restore_session(session_data).await?;

    Ok(client)
}

/// Find mentions in the given HTML string.
///
/// Returns a list of `(pill, mention_content)` tuples.
pub(crate) fn find_html_mentions(html: &str, room: &Room) -> Vec<(Pill, StrTendril)> {
    let mut mentions = Vec::new();
    let html = Html::parse(html);

    append_children_mentions(&mut mentions, html.children(), room);

    mentions
}

/// Find mentions in the given child nodes and append them to the given list.
fn append_children_mentions(
    mentions: &mut Vec<(Pill, StrTendril)>,
    children: Children,
    room: &Room,
) {
    for node in children {
        if let Some(mention) = node_as_mention(&node, room) {
            mentions.push(mention);
            continue;
        }

        append_children_mentions(mentions, node.children(), room);
    }
}

/// Try to convert the given node to a mention.
///
/// This does not recurse into children.
fn node_as_mention(node: &NodeRef, room: &Room) -> Option<(Pill, StrTendril)> {
    // Mentions are links.
    let MatrixElement::A(anchor) = node.as_element()?.to_matrix().element else {
        return None;
    };

    // Mentions contain Matrix URIs.
    let id = MatrixIdUri::try_from(anchor.href?).ok()?;

    // Mentions contain one text child node.
    let child = node.children().next()?;

    if child.next_sibling().is_some() {
        return None;
    }

    let content = child.as_text()?.borrow().clone();
    let pill = id.into_pill(room)?;

    Some((pill, content))
}

/// The textual representation of a room mention.
pub(crate) const AT_ROOM: &str = "@room";

/// Find `@room` in the given string.
///
/// This uses the same algorithm as the pushrules from the Matrix spec to detect
/// it in the `body`.
///
/// Returns the position of the first match.
pub(crate) fn find_at_room(s: &str) -> Option<usize> {
    for (pos, _) in s.match_indices(AT_ROOM) {
        let is_at_word_start = pos == 0 || s[..pos].ends_with(char_is_ascii_word_boundary);
        if !is_at_word_start {
            continue;
        }

        let pos_after_match = pos + 5;
        let is_at_word_end = pos_after_match == s.len()
            || s[pos_after_match..].starts_with(char_is_ascii_word_boundary);
        if is_at_word_end {
            return Some(pos);
        }
    }

    None
}

/// Whether the given `char` is a word boundary, according to the Matrix spec.
///
/// A word boundary is any character not in the sets `[A-Z]`, `[a-z]`, `[0-9]`
/// or `_`.
fn char_is_ascii_word_boundary(c: char) -> bool {
    !c.is_ascii_alphanumeric() && c != '_'
}

/// Compare two raw JSON sources.
pub(crate) fn raw_eq<T, U>(lhs: Option<&Raw<T>>, rhs: Option<&Raw<U>>) -> bool {
    let Some(lhs) = lhs else {
        // They are equal only if both are `None`.
        return rhs.is_none();
    };
    let Some(rhs) = rhs else {
        // They cannot be equal.
        return false;
    };

    lhs.json().get() == rhs.json().get()
}

/// A URI for a Matrix ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MatrixIdUri {
    /// A room.
    Room(MatrixRoomIdUri),
    /// A user.
    User(OwnedUserId),
    /// An event.
    Event(MatrixEventIdUri),
}

impl MatrixIdUri {
    /// Constructs a `MatrixIdUri` from the given ID and servers list.
    fn try_from_parts(id: MatrixId, via: &[OwnedServerName]) -> Result<Self, ()> {
        let uri = match id {
            MatrixId::Room(room_id) => Self::Room(MatrixRoomIdUri {
                id: room_id.into(),
                via: via.to_owned(),
            }),
            MatrixId::RoomAlias(room_alias) => Self::Room(MatrixRoomIdUri {
                id: room_alias.into(),
                via: via.to_owned(),
            }),
            MatrixId::User(user_id) => Self::User(user_id),
            MatrixId::Event(room_id, event_id) => Self::Event(MatrixEventIdUri {
                event_id,
                room_uri: MatrixRoomIdUri {
                    id: room_id,
                    via: via.to_owned(),
                },
            }),
            _ => return Err(()),
        };

        Ok(uri)
    }

    /// Try parsing a `&str` into a `MatrixIdUri`.
    pub(crate) fn parse(s: &str) -> Result<Self, MatrixIdUriParseError> {
        if let Ok(uri) = MatrixToUri::parse(s) {
            return uri.try_into();
        }

        MatrixUri::parse(s)?.try_into()
    }

    /// Try to construct a [`Pill`] from this ID in the given room.
    pub(crate) fn into_pill(self, room: &Room) -> Option<Pill> {
        match self {
            Self::Room(room_uri) => {
                let session = room.session()?;

                let pill =
                    if let Some(uri_room) = session.room_list().get_by_identifier(&room_uri.id) {
                        // We do not need to watch safety settings for local rooms, they will be
                        // watched automatically.
                        Pill::new(&uri_room, AvatarImageSafetySetting::None, None)
                    } else {
                        Pill::new(
                            &session.remote_cache().room(room_uri),
                            AvatarImageSafetySetting::MediaPreviews,
                            Some(room.clone()),
                        )
                    };

                Some(pill)
            }
            Self::User(user_id) => {
                // We should have a strong reference to the list wherever we show a user pill,
                // so we can use `get_or_create_members()`.
                let user = room.get_or_create_members().get_or_create(user_id);

                // We do not need to watch safety settings for users.
                Some(Pill::new(&user, AvatarImageSafetySetting::None, None))
            }
            Self::Event(_) => None,
        }
    }

    /// Get this ID as a `matrix:` URI.
    pub(crate) fn as_matrix_uri(&self) -> MatrixUri {
        match self {
            MatrixIdUri::Room(room_uri) => match <&RoomId>::try_from(&*room_uri.id) {
                Ok(room_id) => room_id.matrix_uri_via(room_uri.via.clone(), false),
                Err(room_alias) => room_alias.matrix_uri(false),
            },
            MatrixIdUri::User(user_id) => user_id.matrix_uri(false),
            MatrixIdUri::Event(event_uri) => {
                let room_id = <&RoomId>::try_from(&*event_uri.room_uri.id)
                    .expect("room alias should not be used to construct event URI");

                room_id.matrix_event_uri_via(
                    event_uri.event_id.clone(),
                    event_uri.room_uri.via.clone(),
                )
            }
        }
    }
}

impl fmt::Display for MatrixIdUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_matrix_uri().fmt(f)
    }
}

impl TryFrom<&MatrixUri> for MatrixIdUri {
    type Error = MatrixIdUriParseError;

    fn try_from(uri: &MatrixUri) -> Result<Self, Self::Error> {
        // We ignore the action, because we always offer to join a room or DM a user.
        Self::try_from_parts(uri.id().clone(), uri.via())
            .map_err(|()| MatrixIdUriParseError::UnsupportedId(uri.id().clone()))
    }
}

impl TryFrom<MatrixUri> for MatrixIdUri {
    type Error = MatrixIdUriParseError;

    fn try_from(uri: MatrixUri) -> Result<Self, Self::Error> {
        Self::try_from(&uri)
    }
}

impl TryFrom<&MatrixToUri> for MatrixIdUri {
    type Error = MatrixIdUriParseError;

    fn try_from(uri: &MatrixToUri) -> Result<Self, Self::Error> {
        Self::try_from_parts(uri.id().clone(), uri.via())
            .map_err(|()| MatrixIdUriParseError::UnsupportedId(uri.id().clone()))
    }
}

impl TryFrom<MatrixToUri> for MatrixIdUri {
    type Error = MatrixIdUriParseError;

    fn try_from(uri: MatrixToUri) -> Result<Self, Self::Error> {
        Self::try_from(&uri)
    }
}

impl FromStr for MatrixIdUri {
    type Err = MatrixIdUriParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<&str> for MatrixIdUri {
    type Error = MatrixIdUriParseError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::parse(s)
    }
}

impl TryFrom<&AnchorUri> for MatrixIdUri {
    type Error = MatrixIdUriParseError;

    fn try_from(value: &AnchorUri) -> Result<Self, Self::Error> {
        match value {
            AnchorUri::Matrix(uri) => MatrixIdUri::try_from(uri),
            AnchorUri::MatrixTo(uri) => MatrixIdUri::try_from(uri),
            // The same error that should be returned by `parse()` when parsing a non-Matrix URI.
            _ => Err(IdParseError::InvalidMatrixUri(MatrixUriError::WrongScheme).into()),
        }
    }
}

impl TryFrom<AnchorUri> for MatrixIdUri {
    type Error = MatrixIdUriParseError;

    fn try_from(value: AnchorUri) -> Result<Self, Self::Error> {
        Self::try_from(&value)
    }
}

impl StaticVariantType for MatrixIdUri {
    fn static_variant_type() -> Cow<'static, glib::VariantTy> {
        String::static_variant_type()
    }
}

impl ToVariant for MatrixIdUri {
    fn to_variant(&self) -> glib::Variant {
        self.to_string().to_variant()
    }
}

impl FromVariant for MatrixIdUri {
    fn from_variant(variant: &glib::Variant) -> Option<Self> {
        Self::parse(&variant.get::<String>()?).ok()
    }
}

/// A URI for a Matrix room ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MatrixRoomIdUri {
    /// The room ID.
    pub(crate) id: OwnedRoomOrAliasId,
    /// Matrix servers usable to route a `RoomId`.
    pub(crate) via: Vec<OwnedServerName>,
}

impl MatrixRoomIdUri {
    /// Try parsing a `&str` into a `MatrixRoomIdUri`.
    pub(crate) fn parse(s: &str) -> Option<MatrixRoomIdUri> {
        MatrixIdUri::parse(s)
            .ok()
            .and_then(|uri| match uri {
                MatrixIdUri::Room(room_uri) => Some(room_uri),
                _ => None,
            })
            .or_else(|| RoomOrAliasId::parse(s).ok().map(Into::into))
    }
}

impl From<OwnedRoomOrAliasId> for MatrixRoomIdUri {
    fn from(id: OwnedRoomOrAliasId) -> Self {
        Self {
            id,
            via: Vec::new(),
        }
    }
}

impl From<OwnedRoomId> for MatrixRoomIdUri {
    fn from(value: OwnedRoomId) -> Self {
        OwnedRoomOrAliasId::from(value).into()
    }
}

impl From<OwnedRoomAliasId> for MatrixRoomIdUri {
    fn from(value: OwnedRoomAliasId) -> Self {
        OwnedRoomOrAliasId::from(value).into()
    }
}

impl From<&MatrixRoomIdUri> for MatrixUri {
    fn from(value: &MatrixRoomIdUri) -> Self {
        match <&RoomId>::try_from(&*value.id) {
            Ok(room_id) => room_id.matrix_uri_via(value.via.clone(), false),
            Err(alias) => alias.matrix_uri(false),
        }
    }
}

/// A URI for a Matrix event ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MatrixEventIdUri {
    /// The event ID.
    pub event_id: OwnedEventId,
    /// The event's room ID URI.
    pub room_uri: MatrixRoomIdUri,
}

/// Errors encountered when parsing a Matrix ID URI.
#[derive(Debug, Clone, Error)]
pub(crate) enum MatrixIdUriParseError {
    /// Not a valid Matrix URI.
    #[error(transparent)]
    InvalidUri(#[from] IdParseError),
    /// Unsupported Matrix ID.
    #[error("unsupported Matrix ID: {0:?}")]
    UnsupportedId(MatrixId),
}

/// Convert the given timestamp to a `GDateTime`.
pub(crate) fn timestamp_to_date(ts: MilliSecondsSinceUnixEpoch) -> glib::DateTime {
    seconds_since_unix_epoch_to_date(ts.as_secs().into())
}

/// Convert the given number of seconds since Unix EPOCH to a `GDateTime`.
pub(crate) fn seconds_since_unix_epoch_to_date(secs: i64) -> glib::DateTime {
    glib::DateTime::from_unix_utc(secs)
        .and_then(|date| date.to_local())
        .expect("constructing GDateTime from timestamp should work")
}

/// The data used as a cache key for messages.
///
/// This is used when there is no reliable way to detect if the content of a
/// message changed. For example, the URI of a media file might change between a
/// local echo and a remote echo, but we do not need to reload the media in this
/// case, and we have no other way to know that both URIs point to the same
/// file.
#[derive(Debug, Clone, Default)]
pub(crate) struct MessageCacheKey {
    /// The transaction ID of the event.
    ///
    /// Local echo should keep its transaction ID after the message is sent, so
    /// we do not need to reload the message if it did not change.
    pub(crate) transaction_id: Option<OwnedTransactionId>,
    /// The global ID of the event.
    ///
    /// Local echo that was sent and remote echo should have the same event ID,
    /// so we do not need to reload the message if it did not change.
    pub(crate) event_id: Option<OwnedEventId>,
    /// Whether the message is edited.
    ///
    /// The message must be reloaded when it was edited.
    pub(crate) is_edited: bool,
}

impl MessageCacheKey {
    /// Whether the given new `MessageCacheKey` should trigger a reload of the
    /// message compared to this one.
    pub(crate) fn should_reload(&self, new: &MessageCacheKey) -> bool {
        if new.is_edited {
            return true;
        }

        let transaction_id_invalidated = self.transaction_id.is_none()
            || new.transaction_id.is_none()
            || self.transaction_id != new.transaction_id;
        let event_id_invalidated =
            self.event_id.is_none() || new.event_id.is_none() || self.event_id != new.event_id;

        transaction_id_invalidated && event_id_invalidated
    }
}
