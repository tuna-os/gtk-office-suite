use std::ops::Deref;

use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gdk, gio, glib, glib::clone};
use matrix_sdk_ui::timeline::MembershipChange;
use ruma::events::{
    AnyMessageLikeEventContent,
    poll::unstable_end::UnstablePollEndEventContent,
    room::{message::MessageType, power_levels::PowerLevelUserAction},
};
use tracing::error;

use super::EventPropertiesDialog;
use crate::{
    components::{RoomMemberDestructiveAction, confirm_room_member_destructive_action_dialog},
    prelude::*,
    session::{Event, Membership, MessageState, Room},
    spawn, spawn_tokio, toast,
};

/// Trait to help a row that presents an `Event` to provide the proper actions.
pub(crate) trait EventActionsGroup: ObjectSubclass {
    /// The current event of the row, if any.
    fn event(&self) -> Option<Event>;

    /// The current `GdkTexture` of the row, if any.
    fn texture(&self) -> Option<gdk::Texture>;

    /// The current `GtkPopoverMenu` of the row, if any.
    fn popover(&self) -> Option<gtk::PopoverMenu>;

    /// Get the `GActionGroup` with the proper actions for the current event.
    fn event_actions_group(&self) -> Option<gio::SimpleActionGroup>
    where
        Self: glib::clone::Downgrade,
        Self::Type: IsA<gtk::Widget>,
        Self::Weak: 'static,
        <Self::Weak as glib::clone::Upgrade>::Strong: Deref,
        <<Self::Weak as glib::clone::Upgrade>::Strong as Deref>::Target: EventActionsGroup,
        <<<Self::Weak as glib::clone::Upgrade>::Strong as Deref>::Target as ObjectSubclass>::Type:
            IsA<gtk::Widget>,
    {
        let event = self.event()?;
        let action_group = gio::SimpleActionGroup::new();
        let room = event.room();
        let has_event_id = event.event_id().is_some();

        if has_event_id {
            action_group.add_action_entries([
                // Create a permalink.
                gio::ActionEntry::builder("permalink")
                    .activate(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_, _, _| {
                            spawn!(async move {
                                let Some(event) = imp.event() else {
                                    return;
                                };
                                let Some(permalink) = event.matrix_to_uri().await else {
                                    return;
                                };

                                let obj = imp.obj();
                                obj.clipboard().set_text(&permalink.to_string());
                                toast!(obj, gettext("Message link copied to clipboard"));
                            });
                        }
                    ))
                    .build(),
                // View event properties.
                gio::ActionEntry::builder("properties")
                    .activate(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_, _, _| {
                            let Some(event) = imp.event() else {
                                return;
                            };

                            let dialog = EventPropertiesDialog::new(&event);
                            dialog.present(Some(&*imp.obj()));
                        }
                    ))
                    .build(),
            ]);

            if room.is_joined() {
                action_group.add_action_entries([
                    // Report the event.
                    gio::ActionEntry::builder("report")
                        .activate(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            move |_, _, _| {
                                spawn!(async move {
                                    imp.report_event().await;
                                });
                            }
                        ))
                        .build(),
                ]);
            }
        } else {
            let state = event.state();

            if matches!(
                state,
                MessageState::Sending
                    | MessageState::RecoverableError
                    | MessageState::PermanentError
            ) {
                // Cancel the event.
                action_group.add_action_entries([gio::ActionEntry::builder("cancel-send")
                    .activate(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_, _, _| {
                            spawn!(async move {
                                imp.cancel_send().await;
                            });
                        }
                    ))
                    .build()]);
            }
        }

        self.add_message_like_actions(&action_group, &room, &event);
        self.add_state_actions(&action_group, &room, &event);

        Some(action_group)
    }

    /// Add actions to the given action group for the given event, if it is
    /// message-like.
    ///
    /// See [`Event::is_message_like()`] for the definition of a message
    /// event.
    #[allow(clippy::too_many_lines)]
    fn add_message_like_actions(
        &self,
        action_group: &gio::SimpleActionGroup,
        room: &Room,
        event: &Event,
    ) where
        Self: glib::clone::Downgrade,
        Self::Type: IsA<gtk::Widget>,
        Self::Weak: 'static,
        <Self::Weak as glib::clone::Upgrade>::Strong: Deref,
        <<Self::Weak as glib::clone::Upgrade>::Strong as Deref>::Target: EventActionsGroup,
        <<<Self::Weak as glib::clone::Upgrade>::Strong as Deref>::Target as ObjectSubclass>::Type:
            IsA<gtk::Widget>,
    {
        if !event.is_message_like() {
            return;
        }

        let own_member = room.own_member();
        let own_user_id = own_member.user_id();
        let is_from_own_user = event.sender_id() == *own_user_id;
        let permissions = room.permissions();
        let has_event_id = event.event_id().is_some();

        // Redact/remove the event.
        if has_event_id
            && ((is_from_own_user && permissions.can_redact_own())
                || permissions.can_redact_other())
        {
            action_group.add_action_entries([gio::ActionEntry::builder("remove")
                .activate(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, _, _| {
                        spawn!(async move {
                            imp.redact_message().await;
                        });
                    }
                ))
                .build()]);
        }

        // Send/redact a reaction.
        if event.can_be_reacted_to() {
            action_group.add_action_entries([
                gio::ActionEntry::builder("toggle-reaction")
                    .parameter_type(Some(&String::static_variant_type()))
                    .activate(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_, _, variant| {
                            let Some(key) = variant
                                .expect("toggle-reaction action should have a parameter")
                                .get::<String>()
                            else {
                                error!("Could not parse reaction to toggle");
                                return;
                            };

                            spawn!(async move {
                                imp.toggle_reaction(key).await;
                            });
                        }
                    ))
                    .build(),
                gio::ActionEntry::builder("show-reactions-chooser")
                    .activate(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_, _, _| {
                            imp.show_reactions_chooser();
                        }
                    ))
                    .build(),
            ]);
        }

        // Reply.
        if event.can_be_replied_to() {
            action_group.add_action_entries([gio::ActionEntry::builder("reply")
                .activate(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, _, _| {
                        let Some(event) = imp.event() else {
                            error!("Could not reply to timeline item that is not an event");
                            return;
                        };
                        let Some(event_id) = event.event_id() else {
                            error!("Event to reply to does not have an event ID");
                            return;
                        };

                        if imp
                            .obj()
                            .activate_action(
                                "room-history.reply",
                                Some(&event_id.as_str().to_variant()),
                            )
                            .is_err()
                        {
                            error!("Could not activate `room-history.reply` action");
                        }
                    }
                ))
                .build()]);
        }

        // Reply in a thread.
        //
        // Only offered in the main timeline, the thread panel always replies
        // in its thread.
        if event.can_be_replied_to() && !event.timeline().is_thread() {
            action_group.add_action_entries([gio::ActionEntry::builder("reply-in-thread")
                .activate(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, _, _| {
                        let Some(event) = imp.event() else {
                            error!(
                                "Could not reply in thread to timeline item that is not an event"
                            );
                            return;
                        };
                        let Some(event_id) = event.event_id() else {
                            error!("Event to reply to in thread does not have an event ID");
                            return;
                        };

                        if imp
                            .obj()
                            .activate_action(
                                "room-history.reply-in-thread",
                                Some(&event_id.as_str().to_variant()),
                            )
                            .is_err()
                        {
                            error!("Could not activate `room-history.reply-in-thread` action");
                        }
                    }
                ))
                .build()]);
        }

        // End the poll.
        if has_event_id
            && event
                .poll()
                .is_some_and(|poll| poll.results().end_time.is_none())
            && (is_from_own_user || permissions.can_redact_other())
            && permissions.can_send_message()
        {
            action_group.add_action_entries([gio::ActionEntry::builder("end-poll")
                .activate(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, _, _| {
                        spawn!(async move {
                            imp.end_poll().await;
                        });
                    }
                ))
                .build()]);
        }

        self.add_message_actions(action_group, room, event);
    }

    /// Add actions to the given action group for the given event, if it
    /// is a message.
    #[allow(clippy::too_many_lines)]
    fn add_message_actions(&self, action_group: &gio::SimpleActionGroup, room: &Room, event: &Event)
    where
        Self: glib::clone::Downgrade,
        Self::Type: IsA<gtk::Widget>,
        Self::Weak: 'static,
        <Self::Weak as glib::clone::Upgrade>::Strong: Deref,
        <<Self::Weak as glib::clone::Upgrade>::Strong as Deref>::Target: EventActionsGroup,
        <<<Self::Weak as glib::clone::Upgrade>::Strong as Deref>::Target as ObjectSubclass>::Type:
            IsA<gtk::Widget>,
    {
        let Some(message) = event.message() else {
            return;
        };

        let own_member = room.own_member();
        let own_user_id = own_member.user_id();
        let is_from_own_user = event.sender_id() == *own_user_id;
        let permissions = room.permissions();
        let has_event_id = event.event_id().is_some();

        match message.msgtype() {
            MessageType::Text(_) | MessageType::Emote(_) => {
                // Copy text.
                action_group.add_action_entries([gio::ActionEntry::builder("copy-text")
                    .activate(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_, _, _| {
                            imp.copy_text();
                        }
                    ))
                    .build()]);

                // Edit message.
                if has_event_id && is_from_own_user && permissions.can_send_message() {
                    action_group.add_action_entries([gio::ActionEntry::builder("edit")
                        .activate(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            move |_, _, _| {
                                imp.edit_message();
                            }
                        ))
                        .build()]);
                }
            }
            MessageType::File(_) => {
                // Save message's file.
                action_group.add_action_entries([gio::ActionEntry::builder("file-save")
                    .activate(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_, _, _| {
                            spawn!(async move {
                                imp.save_file().await;
                            });
                        }
                    ))
                    .build()]);
            }
            MessageType::Notice(_) => {
                // Copy text.
                action_group.add_action_entries([gio::ActionEntry::builder("copy-text")
                    .activate(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_, _, _| {
                            imp.copy_text();
                        }
                    ))
                    .build()]);
            }
            MessageType::Image(_) => {
                action_group.add_action_entries([
                    // Copy the texture to the clipboard.
                    gio::ActionEntry::builder("copy-image")
                        .activate(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            move |_, _, _| {
                                let Some(texture) = imp.texture() else {
                                    error!("Could not find texture to copy");
                                    return;
                                };

                                let obj = imp.obj();
                                obj.clipboard().set_texture(&texture);
                                toast!(obj, gettext("Thumbnail copied to clipboard"));
                            }
                        ))
                        .build(),
                    // Save the image to a file.
                    gio::ActionEntry::builder("save-image")
                        .activate(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            move |_, _, _| {
                                spawn!(async move {
                                    imp.save_file().await;
                                });
                            }
                        ))
                        .build(),
                ]);
            }
            MessageType::Video(_) => {
                // Save the video to a file.
                action_group.add_action_entries([gio::ActionEntry::builder("save-video")
                    .activate(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_, _, _| {
                            spawn!(async move {
                                imp.save_file().await;
                            });
                        }
                    ))
                    .build()]);
            }
            MessageType::Audio(_) => {
                // Save the audio to a file.
                action_group.add_action_entries([gio::ActionEntry::builder("save-audio")
                    .activate(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_, _, _| {
                            spawn!(async move {
                                imp.save_file().await;
                            });
                        }
                    ))
                    .build()]);
            }
            _ => {}
        }

        if event
            .media_message()
            .is_some_and(|media_message| media_message.caption().is_some())
        {
            // Copy caption.
            action_group.add_action_entries([gio::ActionEntry::builder("copy-text")
                .activate(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, _, _| {
                        imp.copy_text();
                    }
                ))
                .build()]);
        }
    }

    /// Add actions to the given action group for the given event, if it is a
    /// state event.
    fn add_state_actions(&self, action_group: &gio::SimpleActionGroup, room: &Room, event: &Event)
    where
        Self: glib::clone::Downgrade,
        Self::Weak: 'static,
        <Self::Weak as glib::clone::Upgrade>::Strong: Deref,
        <<Self::Weak as glib::clone::Upgrade>::Strong as Deref>::Target: EventActionsGroup,
        <<<Self::Weak as glib::clone::Upgrade>::Strong as Deref>::Target as ObjectSubclass>::Type:
            IsA<gtk::Widget>,
    {
        let Some(membership_change) = event.membership_change() else {
            return;
        };
        let Some(target_user) = event.target_user() else {
            return;
        };

        let permissions = room.permissions();

        // Revoke invite.
        if membership_change == MembershipChange::Invited
            && target_user.membership() == Membership::Invite
            && permissions.can_do_to_user(target_user.user_id(), PowerLevelUserAction::Kick)
        {
            action_group.add_action_entries([gio::ActionEntry::builder("revoke-invite")
                .activate(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, _, _| {
                        spawn!(async move {
                            imp.revoke_invite().await;
                        });
                    }
                ))
                .build()]);
        }
    }

    /// Replace the context menu with an emoji chooser for reactions.
    fn show_reactions_chooser(&self)
    where
        Self::Type: IsA<gtk::Widget>,
    {
        let Some(popover) = self.popover() else {
            return;
        };

        let obj = self.obj();
        let (_, rectangle) = popover.pointing_to();

        let emoji_chooser = gtk::EmojiChooser::builder()
            .has_arrow(false)
            .pointing_to(&rectangle)
            .build();

        emoji_chooser.connect_emoji_picked(clone!(
            #[strong]
            obj,
            move |_, emoji| {
                let _ = obj.activate_action("event.toggle-reaction", Some(&emoji.to_variant()));
            }
        ));
        emoji_chooser.connect_closed(|emoji_chooser| {
            emoji_chooser.unparent();
        });
        emoji_chooser.set_parent(&*obj);

        popover.popdown();
        emoji_chooser.popup();
    }

    /// Copy the text of this row.
    fn copy_text(&self)
    where
        Self::Type: IsA<gtk::Widget>,
    {
        let Some(event) = self.event() else {
            error!("Could not copy text of timeline item that is not an event");
            return;
        };
        let Some(message) = event.message() else {
            error!("Could not copy text of event that is not a message");
            return;
        };

        let text = match message.msgtype() {
            MessageType::Text(text_message) => text_message.body.clone(),
            MessageType::Emote(emote_message) => {
                let display_name = event.sender().display_name();
                format!("{display_name} {}", emote_message.body)
            }
            MessageType::Notice(notice_message) => notice_message.body.clone(),
            _ => {
                if let Some(caption) = event
                    .media_message()
                    .and_then(|m| m.caption().map(|(caption, _)| caption))
                {
                    caption
                } else {
                    error!("Could not copy text of event that is not a textual message");
                    return;
                }
            }
        };

        let obj = self.obj();
        obj.clipboard().set_text(&text);
        toast!(obj, gettext("Text copied to clipboard"));
    }

    /// Edit the message of this row.
    fn edit_message(&self)
    where
        Self::Type: IsA<gtk::Widget>,
    {
        let Some(event) = self.event() else {
            error!("Could not edit timeline item that is not an event");
            return;
        };
        let Some(event_id) = event.event_id() else {
            error!("Could not edit event without an event ID");
            return;
        };

        if self
            .obj()
            .activate_action("room-history.edit", Some(&event_id.as_str().to_variant()))
            .is_err()
        {
            error!("Could not activate `room-history.edit` action");
        }
    }

    /// Save the media file of this row.
    async fn save_file(&self)
    where
        Self::Type: IsA<gtk::Widget>,
    {
        let Some(event) = self.event() else {
            error!("Could not save file of timeline item that is not an event");
            return;
        };
        let Some(session) = event.room().session() else {
            // Should only happen if the process is being closed.
            return;
        };
        let Some(media_message) = event.media_message() else {
            error!("Could not save file for non-media event");
            return;
        };

        let client = session.client();
        media_message
            .save_to_file(&event.timestamp(), &client, &*self.obj())
            .await;
    }

    /// Redact the event of this row.
    async fn redact_message(&self)
    where
        Self::Type: IsA<gtk::Widget>,
    {
        let Some(event) = self.event() else {
            error!("Could not redact timeline item that is not an event");
            return;
        };
        let Some(event_id) = event.event_id() else {
            error!("Event to redact does not have an event ID");
            return;
        };
        let obj = self.obj();

        let confirm_dialog = adw::AlertDialog::builder()
            .default_response("cancel")
            .heading(gettext("Remove Message?"))
            .body(gettext(
                "Do you really want to remove this message? This cannot be undone.",
            ))
            .build();
        confirm_dialog.add_responses(&[
            ("cancel", &gettext("Cancel")),
            ("remove", &gettext("Remove")),
        ]);
        confirm_dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);

        if confirm_dialog.choose_future(Some(&*obj)).await != "remove" {
            return;
        }

        if event.room().redact(&[event_id], None).await.is_err() {
            toast!(obj, gettext("Could not remove message"));
        }
    }

    /// End the poll of this row.
    async fn end_poll(&self)
    where
        Self::Type: IsA<gtk::Widget>,
    {
        let Some(event) = self.event() else {
            error!("Could not end poll of timeline item that is not an event");
            return;
        };
        let Some(event_id) = event.event_id() else {
            error!("Poll to end does not have an event ID");
            return;
        };
        let obj = self.obj();

        let confirm_dialog = adw::AlertDialog::builder()
            .default_response("cancel")
            .heading(gettext("End Poll?"))
            .body(gettext(
                "Do you really want to end this poll? This will reveal the final results and voting will be closed. This cannot be undone.",
            ))
            .build();
        confirm_dialog.add_responses(&[
            ("cancel", &gettext("Cancel")),
            // Translators: This is a verb, as in 'End Poll'.
            ("end", &gettext("End")),
        ]);
        confirm_dialog.set_response_appearance("end", adw::ResponseAppearance::Destructive);

        if confirm_dialog.choose_future(Some(&*obj)).await != "end" {
            return;
        }

        let content = UnstablePollEndEventContent::new(gettext("The poll has ended"), event_id);
        let matrix_timeline = event.timeline().matrix_timeline();
        let handle = spawn_tokio!(async move {
            matrix_timeline
                .send(AnyMessageLikeEventContent::UnstablePollEnd(content))
                .await
        });

        if let Err(error) = handle.await.expect("task was not aborted") {
            error!("Could not end poll: {error}");
            toast!(obj, gettext("Could not end poll"));
        }
    }

    /// Toggle the reaction with the given key for the event of this row.
    async fn toggle_reaction(&self, key: String)
    where
        Self::Type: IsA<gtk::Widget>,
    {
        let Some(event) = self.event() else {
            error!("Could not toggle reaction on timeline item that is not an event");
            return;
        };

        if event.room().toggle_reaction(key, &event).await.is_err() {
            toast!(self.obj(), gettext("Could not toggle reaction"));
        }
    }

    /// Report the current event.
    async fn report_event(&self)
    where
        Self::Type: IsA<gtk::Widget>,
    {
        let Some(event) = self.event() else {
            error!("Could not report timeline item that is not an event");
            return;
        };
        let Some(event_id) = event.event_id() else {
            error!("Event to report does not have an event ID");
            return;
        };
        let obj = self.obj();

        // Ask the user to confirm, and provide optional reason.
        let reason_entry = adw::EntryRow::builder()
            .title(gettext("Reason (optional)"))
            .build();
        let list_box = gtk::ListBox::builder()
            .css_classes(["boxed-list"])
            .margin_top(6)
            .accessible_role(gtk::AccessibleRole::Group)
            .build();
        list_box.append(&reason_entry);

        let confirm_dialog = adw::AlertDialog::builder()
            .default_response("cancel")
            .heading(gettext("Report Event?"))
            .body(gettext(
                "Reporting an event will send its unique ID to the administrator of your homeserver. The administrator will not be able to see the content of the event if it is encrypted or redacted.",
            ))
            .extra_child(&list_box)
            .build();
        confirm_dialog.add_responses(&[
            ("cancel", &gettext("Cancel")),
            // Translators: This is a verb, as in 'Report Event'.
            ("report", &gettext("Report")),
        ]);
        confirm_dialog.set_response_appearance("report", adw::ResponseAppearance::Destructive);

        if confirm_dialog.choose_future(Some(&*obj)).await != "report" {
            return;
        }

        let reason = Some(reason_entry.text())
            .filter(|s| !s.is_empty())
            .map(Into::into);

        if event
            .room()
            .report_events(&[(event_id, reason)])
            .await
            .is_err()
        {
            toast!(obj, gettext("Could not report event"));
        }
    }

    /// Cancel sending the event of this row.
    async fn cancel_send(&self)
    where
        Self::Type: IsA<gtk::Widget>,
    {
        let Some(event) = self.event() else {
            error!("Could not discard timeline item that is not an event");
            return;
        };

        let matrix_timeline = event.timeline().matrix_timeline();
        let identifier = event.identifier();
        let handle = spawn_tokio!(async move { matrix_timeline.redact(&identifier, None).await });

        if let Err(error) = handle.await.unwrap() {
            error!("Could not discard local event: {error}");
            toast!(self.obj(), gettext("Could not discard message"));
        }
    }

    /// Revoke the invite of the target user of the current event.
    async fn revoke_invite(&self)
    where
        Self::Type: IsA<gtk::Widget>,
    {
        let Some(event) = self.event() else {
            error!("Could not revoke invite for timeline item that is not an event");
            return;
        };
        let Some(target_user) = event.target_user() else {
            error!("Could not revoke invite for event without a target user");
            return;
        };
        let obj = self.obj();

        let Some(response) = confirm_room_member_destructive_action_dialog(
            &target_user,
            RoomMemberDestructiveAction::Kick,
            &*obj,
        )
        .await
        else {
            return;
        };

        toast!(obj, gettext("Revoking invite…"));

        let room = target_user.room();
        let user_id = target_user.user_id().clone();
        if room.kick(&[(user_id, response.reason)]).await.is_err() {
            toast!(obj, gettext("Could not revoke invite of user"));
        }
    }
}
