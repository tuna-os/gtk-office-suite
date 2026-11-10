use std::fmt::Write;

use gtk::prelude::*;
use matrix_sdk::{ComposerDraft, ComposerDraftType};
use ruma::{
    OwnedUserId,
    events::{
        Mentions,
        room::message::{
            EmoteMessageEventContent, FormattedBody, MessageType,
            RoomMessageEventContentWithoutRelation,
        },
    },
};

use super::{
    ComposerState, RelationInfo,
    composer_state::{MENTION_END_TAG, MENTION_START_TAG},
};
use crate::{
    components::{AtRoom, Pill, PillSource},
    prelude::*,
    session::{Member, Room},
    utils::matrix::AT_ROOM,
};

/// A message composer parser.
pub(super) struct ComposerParser<'a> {
    /// The composer state associated with the buffer.
    composer_state: &'a ComposerState,
    /// The current position of the iterator in the buffer.
    iter: gtk::TextIter,
    /// The position of the end of the buffer.
    end: gtk::TextIter,
}

impl<'a> ComposerParser<'a> {
    /// Construct a `ComposerParser` to parse the message composer with the
    /// given state, and between the given bounds.
    ///
    /// If no bounds are provided, the whole content of the composer will be
    /// parsed.
    pub(super) fn new(
        composer_state: &'a ComposerState,
        bounds: Option<(gtk::TextIter, gtk::TextIter)>,
    ) -> Self {
        let (iter, end) = bounds.unwrap_or_else(|| composer_state.buffer().bounds());
        Self {
            composer_state,
            iter,
            end,
        }
    }

    /// The length of the message between the two iterators.
    fn message_len(&self) -> usize {
        self.end
            .offset()
            .saturating_sub(self.iter.offset())
            .try_into()
            .unwrap_or_default()
    }

    /// Get the next chunk of the composer and update the iterator position.
    fn next_chunk(&mut self) -> Option<ComposerChunk> {
        if self.iter == self.end {
            // We reached the end.
            return None;
        }

        if let Some(source) = self
            .iter
            .child_anchor()
            .and_then(|anchor| self.composer_state.widget_at_anchor(&anchor))
            .and_then(|widget| widget.downcast::<Pill>().ok())
            .and_then(|p| p.source())
        {
            self.iter.forward_cursor_position();

            return Some(ComposerChunk::Mention(source));
        }

        // This chunk is not a mention. Go forward until the next mention or the
        // end and return the text in between.
        let start = self.iter;
        while self.iter.forward_cursor_position() && self.iter != self.end {
            if self
                .iter
                .child_anchor()
                .and_then(|anchor| self.composer_state.widget_at_anchor(&anchor))
                .and_then(|widget| widget.downcast::<Pill>().ok())
                .is_some()
            {
                break;
            }
        }

        let text = self.iter.buffer().text(&start, &self.iter, false);
        // We might somehow have an empty string before the end, or at the end,
        // because of hidden `char`s in the buffer, so we must only return
        // `None` when we have an empty string at the end.
        if self.iter == self.end && text.is_empty() {
            None
        } else {
            Some(ComposerChunk::Text(text.into()))
        }
    }

    /// Parse the content of the message composer into the content of a message
    /// event.
    pub(super) async fn into_message_event_content(
        mut self,
        markdown_enabled: bool,
    ) -> Option<RoomMessageEventContentWithoutRelation> {
        let message_len = self.message_len();

        let mut has_rich_mentions = false;
        let mut plain_body = String::with_capacity(message_len);
        // This is Markdown if markdown is enabled, otherwise it is HTML.
        let mut formatted_body = String::with_capacity(message_len);
        let mut mentions = Mentions::new();

        while let Some(chunk) = self.next_chunk() {
            match chunk {
                ComposerChunk::Text(text) => {
                    plain_body.push_str(&text);
                    formatted_body.push_str(&text);
                }
                ComposerChunk::Mention(source) => match Mention::from_source(&source).await {
                    Mention::Rich { name, uri, user_id } => {
                        has_rich_mentions = true;
                        plain_body.push_str(&name);
                        if markdown_enabled {
                            let _ = write!(formatted_body, "[{name}]({uri})");
                        } else {
                            let _ = write!(formatted_body, "<a href=\"{uri}\">{name}</a>");
                        }

                        if let Some(user_id) = user_id {
                            mentions.user_ids.insert(user_id);
                        }
                    }
                    Mention::AtRoom => {
                        plain_body.push_str(AT_ROOM);
                        formatted_body.push_str(AT_ROOM);

                        mentions.room = true;
                    }
                },
            }
        }

        // Remove the command of the emote.
        let is_emote = plain_body.starts_with("/me ");
        if is_emote {
            plain_body.replace_range(.."/me ".len(), "");
            formatted_body.replace_range(.."/me ".len(), "");
        }

        if plain_body.trim().is_empty() {
            // Do not send empty message.
            return None;
        }

        let html_body = if markdown_enabled {
            FormattedBody::markdown(formatted_body).map(|b| b.body)
        } else if has_rich_mentions {
            // Already formatted with HTML.
            Some(formatted_body)
        } else {
            None
        };

        let mut content = if is_emote {
            MessageType::Emote(if let Some(html_body) = html_body {
                EmoteMessageEventContent::html(plain_body, html_body)
            } else {
                EmoteMessageEventContent::plain(plain_body)
            })
            .into()
        } else if let Some(html_body) = html_body {
            RoomMessageEventContentWithoutRelation::text_html(plain_body, html_body)
        } else {
            RoomMessageEventContentWithoutRelation::text_plain(plain_body)
        };

        // To avoid triggering legacy pushrules, we must always include the mentions,
        // even if they are empty.
        content = content.add_mentions(mentions);

        Some(content)
    }

    /// Parse the content of the message composer into a string.
    pub(super) fn into_plain_text(mut self) -> String {
        let mut body = String::with_capacity(self.message_len());

        while let Some(chunk) = self.next_chunk() {
            match chunk {
                ComposerChunk::Text(text) => {
                    body.push_str(&text);
                }
                ComposerChunk::Mention(source) => {
                    if let Some(user) = source.downcast_ref::<Member>() {
                        body.push_str(&user.display_name());
                    } else if let Some(room) = source.downcast_ref::<Room>() {
                        body.push_str(
                            room.aliases()
                                .alias()
                                .as_ref()
                                .map_or_else(|| room.room_id().as_ref(), AsRef::as_ref),
                        );
                    } else if source.is::<AtRoom>() {
                        body.push_str(AT_ROOM);
                    } else {
                        unreachable!()
                    }
                }
            }
        }

        body
    }

    /// Parse the content of the message composer into a [`ComposerDraft`].
    pub(super) fn into_composer_draft(mut self) -> Option<ComposerDraft> {
        let draft_type = self
            .composer_state
            .related_to()
            .as_ref()
            .map_or(ComposerDraftType::NewMessage, RelationInfo::as_draft_type);

        let mut plain_text = String::with_capacity(self.message_len());

        while let Some(chunk) = self.next_chunk() {
            match chunk {
                ComposerChunk::Text(text) => {
                    plain_text.push_str(&text);
                }
                ComposerChunk::Mention(source) => {
                    plain_text.push_str(MENTION_START_TAG);

                    if let Some(user) = source.downcast_ref::<Member>() {
                        plain_text.push_str(user.user_id().as_ref());
                    } else if let Some(room) = source.downcast_ref::<Room>() {
                        plain_text.push_str(
                            room.aliases()
                                .alias()
                                .as_ref()
                                .map_or_else(|| room.room_id().as_ref(), AsRef::as_ref),
                        );
                    } else if source.is::<AtRoom>() {
                        plain_text.push_str(AT_ROOM);
                    } else {
                        unreachable!()
                    }

                    plain_text.push_str(MENTION_END_TAG);
                }
            }
        }

        if draft_type == ComposerDraftType::NewMessage && plain_text.trim().is_empty() {
            None
        } else {
            Some(ComposerDraft {
                plain_text,
                html_text: None,
                draft_type,
                attachments: Vec::new(),
            })
        }
    }
}

/// A chunk of content in a message composer.
enum ComposerChunk {
    /// Some text.
    Text(String),
    /// A mention as a `Pill`.
    Mention(PillSource),
}

/// A mention that can be sent in a message.
enum Mention {
    /// A mention that has a HTML representation.
    Rich {
        /// The string representation of the mention.
        name: String,
        /// The URI of the mention.
        uri: String,
        /// The user ID, if this is a user mention.
        user_id: Option<OwnedUserId>,
    },
    /// An `@room` mention.
    AtRoom,
}

impl Mention {
    /// Construct a `Mention` from the given pill source.
    async fn from_source(source: &PillSource) -> Self {
        if let Some(user) = source.downcast_ref::<Member>() {
            let name = if user.has_display_name() {
                user.display_name()
            } else {
                user.user_id().to_string()
            };

            Self::Rich {
                name,
                uri: user.matrix_to_uri().to_string(),
                user_id: Some(user.user_id().clone()),
            }
        } else if let Some(room) = source.downcast_ref::<Room>() {
            let matrix_to_uri = room.matrix_to_uri().await;
            let name = room
                .aliases()
                .alias_string()
                .unwrap_or_else(|| room.room_id_string());

            Self::Rich {
                name,
                uri: matrix_to_uri.to_string(),
                user_id: None,
            }
        } else if source.is::<AtRoom>() {
            Self::AtRoom
        } else {
            unreachable!()
        }
    }
}
