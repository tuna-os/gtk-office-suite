//! Extension traits for Matrix types.

use std::borrow::Cow;

use gtk::{glib, prelude::*};
use matrix_sdk_ui::timeline::{
    EventTimelineItem, MembershipChange, Message, MsgLikeKind, TimelineEventItemId,
    TimelineItemContent,
};
use ruma::{
    UserId,
    events::{
        AnySyncTimelineEvent,
        room::message::{FormattedBody, MessageType},
    },
    serde::Raw,
};
use serde::Deserialize;

use crate::utils::string::StrMutExt;

/// Helper trait for types possibly containing an `@room` mention.
pub(crate) trait AtMentionExt {
    /// Whether this event might contain an `@room` mention.
    ///
    /// This means that either it does not have intentional mentions, or it has
    /// intentional mentions and `room` is set to `true`.
    fn can_contain_at_room(&self) -> bool;
}

impl AtMentionExt for TimelineItemContent {
    fn can_contain_at_room(&self) -> bool {
        match self {
            TimelineItemContent::MsgLike(msg_like) => match &msg_like.kind {
                MsgLikeKind::Message(message) => message.can_contain_at_room(),
                _ => false,
            },
            _ => false,
        }
    }
}

impl AtMentionExt for Message {
    fn can_contain_at_room(&self) -> bool {
        self.mentions().is_none_or(|mentions| mentions.room)
    }
}

/// Extension trait for [`TimelineEventItemId`].
pub(crate) trait TimelineEventItemIdExt: Sized {
    /// The type used to represent a [`TimelineEventItemId`] as a `GVariant`.
    fn static_variant_type() -> Cow<'static, glib::VariantTy>;

    /// Convert this [`TimelineEventItemId`] to a `GVariant`.
    fn to_variant(&self) -> glib::Variant;

    /// Try to convert a `GVariant` to a [`TimelineEventItemId`].
    fn from_variant(variant: &glib::Variant) -> Option<Self>;
}

impl TimelineEventItemIdExt for TimelineEventItemId {
    fn static_variant_type() -> Cow<'static, glib::VariantTy> {
        Cow::Borrowed(glib::VariantTy::STRING)
    }

    fn to_variant(&self) -> glib::Variant {
        let s = match self {
            Self::TransactionId(txn_id) => format!("transaction_id:{txn_id}"),
            Self::EventId(event_id) => format!("event_id:{event_id}"),
        };

        s.to_variant()
    }

    fn from_variant(variant: &glib::Variant) -> Option<Self> {
        let s = variant.str()?;

        if let Some(s) = s.strip_prefix("transaction_id:") {
            Some(Self::TransactionId(s.into()))
        } else if let Some(s) = s.strip_prefix("event_id:") {
            s.try_into().ok().map(Self::EventId)
        } else {
            None
        }
    }
}

/// Extension trait for [`TimelineItemContent`].
pub(crate) trait TimelineItemContentExt {
    /// Whether this content can count as an unread message.
    ///
    /// This follows the algorithm in [MSC2654], excluding events that we do not
    /// show in the timeline.
    ///
    /// [MSC2654]: https://github.com/matrix-org/matrix-spec-proposals/pull/2654
    fn counts_as_unread(&self) -> bool;

    /// Whether this content can count as the latest activity in a room.
    ///
    /// This includes content that counts as unread, plus membership changes for
    /// our own user towards joining a room, so that freshly joined rooms are at
    /// the top of the list.
    fn counts_as_activity(&self, own_user_id: &UserId) -> bool;

    /// Whether we can show the header for this content.
    fn can_show_header(&self) -> bool;

    /// Whether this content is edited.
    fn is_edited(&self) -> bool;
}

impl TimelineItemContentExt for TimelineItemContent {
    fn counts_as_unread(&self) -> bool {
        match self {
            TimelineItemContent::MsgLike(msg_like) => match &msg_like.kind {
                MsgLikeKind::Message(message) => {
                    !matches!(message.msgtype(), MessageType::Notice(_))
                }
                MsgLikeKind::Sticker(_) => true,
                _ => false,
            },
            _ => false,
        }
    }

    fn counts_as_activity(&self, own_user_id: &UserId) -> bool {
        if self.counts_as_unread() {
            return true;
        }

        match self {
            TimelineItemContent::MembershipChange(membership) => {
                if membership.user_id() != own_user_id {
                    return false;
                }

                // We need to bump the room for every meaningful change towards joining a room.
                //
                // The change cannot be computed in two cases:
                // - This is the first membership event for our user in the room: we need to
                //   count it.
                // - The event was redacted: we do not know if we should count it or not, so we
                //   count it too for simplicity.
                membership.change().is_none_or(|change| {
                    matches!(
                        change,
                        MembershipChange::Joined
                            | MembershipChange::Unbanned
                            | MembershipChange::Invited
                            | MembershipChange::InvitationAccepted
                            | MembershipChange::KnockAccepted
                            | MembershipChange::Knocked
                    )
                })
            }
            _ => false,
        }
    }

    fn can_show_header(&self) -> bool {
        match self {
            TimelineItemContent::MsgLike(msg_like) => match &msg_like.kind {
                MsgLikeKind::Message(message) => {
                    matches!(
                        message.msgtype(),
                        MessageType::Audio(_)
                            | MessageType::File(_)
                            | MessageType::Image(_)
                            | MessageType::Location(_)
                            | MessageType::Notice(_)
                            | MessageType::Text(_)
                            | MessageType::Video(_)
                    )
                }
                MsgLikeKind::Sticker(_)
                | MsgLikeKind::Poll(_)
                | MsgLikeKind::UnableToDecrypt(_) => true,
                _ => false,
            },
            _ => false,
        }
    }

    fn is_edited(&self) -> bool {
        match self {
            TimelineItemContent::MsgLike(msg_like) => {
                matches!(&msg_like.kind, MsgLikeKind::Message(message) if message.is_edited())
            }
            _ => false,
        }
    }
}

/// Extension trait for [`EventTimelineItem`].
pub(crate) trait EventTimelineItemExt {
    /// The JSON source for the latest edit of this item, if any.
    fn latest_edit_raw(&self) -> Option<Raw<AnySyncTimelineEvent>>;
}

impl EventTimelineItemExt for EventTimelineItem {
    /// The JSON source for the latest edit of this event, if any.
    fn latest_edit_raw(&self) -> Option<Raw<AnySyncTimelineEvent>> {
        if let Some(raw) = self.latest_edit_json() {
            return Some(raw.clone());
        }

        self.original_json()?
            .get_field::<RawUnsigned>("unsigned")
            .ok()
            .flatten()?
            .relations?
            .replace
    }
}

/// Raw unsigned event data.
///
/// Used as a fallback to get the JSON of the latest edit.
#[derive(Debug, Clone, Deserialize)]
struct RawUnsigned {
    #[serde(rename = "m.relations")]
    relations: Option<RawBundledRelations>,
}

/// Raw bundled event relations.
///
/// Used as a fallback to get the JSON of the latest edit.
#[derive(Debug, Clone, Deserialize)]
struct RawBundledRelations {
    #[serde(rename = "m.replace")]
    replace: Option<Raw<AnySyncTimelineEvent>>,
}

/// Extension trait for `Option<FormattedBody>`.
pub(crate) trait FormattedBodyExt {
    /// Clean the body in the `FormattedBody`.
    ///
    /// Replaces it with `None` if the body is empty after being cleaned.
    fn clean_string(&mut self);
}

impl FormattedBodyExt for Option<FormattedBody> {
    fn clean_string(&mut self) {
        self.take_if(|formatted| {
            formatted.body.clean_string();
            formatted.body.is_empty()
        });
    }
}
