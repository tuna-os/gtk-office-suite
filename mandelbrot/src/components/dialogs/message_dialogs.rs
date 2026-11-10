//! Common message dialogs.

use adw::prelude::*;
use gettextrs::gettext;

use crate::{
    i18n::gettext_f,
    ngettext_f,
    prelude::*,
    session::{Member, Membership, Room, RoomCategory, User},
};

/// Show a dialog to confirm leaving a room.
///
/// This supports both leaving a joined room and rejecting an invite.
///
/// Returns `None` if the user did not confirm.
pub(crate) async fn confirm_leave_room_dialog(
    room: &Room,
    parent: &impl IsA<gtk::Widget>,
) -> Option<ConfirmLeaveRoomResponse> {
    let (heading, body, response) = if room.category() == RoomCategory::Invited {
        // We are rejecting an invite.
        let heading = gettext("Decline Invite?");
        let body = if room.join_rule().we_can_join() {
            gettext(
                "Do you really want to decline this invite? You can join this room on your own later.",
            )
        } else {
            gettext(
                "Do you really want to decline this invite? You will not be able to join this room without it.",
            )
        };
        let response = gettext("Decline");

        (heading, body, response)
    } else {
        // We are leaving a room that was joined.
        let heading = gettext("Leave Room?");
        let body = if room.join_rule().we_can_join() {
            gettext("Do you really want to leave this room? You can come back later.")
        } else {
            gettext(
                "Do you really want to leave this room? You will not be able to come back without an invitation.",
            )
        };
        let response = gettext("Leave");

        (heading, body, response)
    };

    // Ask for confirmation.
    let confirm_dialog = adw::AlertDialog::builder()
        .default_response("cancel")
        .heading(heading)
        .body(body)
        .build();
    confirm_dialog.add_responses(&[("cancel", &gettext("Cancel")), ("leave", &response)]);
    confirm_dialog.set_response_appearance("leave", adw::ResponseAppearance::Destructive);

    let ignore_inviter_switch = if let Some(inviter) = room
        .inviter()
        .filter(|_| room.category() == RoomCategory::Invited)
    {
        let switch = adw::SwitchRow::builder()
            .title(gettext_f(
                "Ignore {user}",
                &[("user", inviter.user_id().as_str())],
            ))
            .subtitle(gettext(
                "All messages or invitations sent by this user will be ignored",
            ))
            .build();

        let list_box = gtk::ListBox::builder()
            .css_classes(["boxed-list"])
            .margin_top(6)
            .accessible_role(gtk::AccessibleRole::Group)
            .build();
        list_box.append(&switch);
        confirm_dialog.set_extra_child(Some(&list_box));

        Some(switch)
    } else {
        None
    };

    if confirm_dialog.choose_future(Some(parent)).await == "leave" {
        let mut response = ConfirmLeaveRoomResponse::default();

        if let Some(switch) = ignore_inviter_switch {
            response.ignore_inviter = switch.is_active();
        }

        Some(response)
    } else {
        None
    }
}

/// A response to the dialog to confirm leaving a room
#[derive(Debug, Default, Clone)]
pub(crate) struct ConfirmLeaveRoomResponse {
    /// If the room is an invite, whether the user wants to ignore the inviter.
    pub ignore_inviter: bool,
}

/// The room member destructive actions that need to be confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoomMemberDestructiveAction {
    /// Ban the member.
    ///
    /// The value is the number of events that can be redacted for the member.
    Ban(usize),
    /// Kick the member.
    Kick,
    /// Remove the member's messages.
    ///
    /// The value is the number of events that will be redacted.
    RemoveMessages(usize),
}

impl RoomMemberDestructiveAction {
    /// The content of the dialog for this action and the given member.
    ///
    /// Returns a `(heading, body, dialog)` tuple.
    fn dialog_content(self, member: &Member) -> (String, String, Option<String>) {
        match self {
            RoomMemberDestructiveAction::Ban(_) => {
                // Translators: Do NOT translate the content between '{' and '}',
                // this is a variable name.
                let heading = gettext_f("Ban {user}?", &[("user", &member.display_name())]);
                let body = gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}',
                    // this is a variable name.
                    "Are you sure you want to ban {user_id}? They will not be able to join the room again until someone unbans them.",
                    &[("user_id", member.user_id().as_str())],
                );
                let response = gettext("Ban");
                (heading, body, Some(response))
            }
            RoomMemberDestructiveAction::Kick => {
                let can_rejoin = member.room().join_rule().anyone_can_join();

                match member.membership() {
                    Membership::Invite => {
                        let heading = gettext_f(
                            // Translators: Do NOT translate the content between '{' and '}',
                            // this is a variable name.
                            "Revoke Invite for {user}?",
                            &[("user", &member.display_name())],
                        );
                        let body = if can_rejoin {
                            gettext_f(
                                // Translators: Do NOT translate the content between '{' and '}',
                                // this is a variable name.
                                "Are you sure you want to revoke the invite for {user_id}? They will still be able to join the room on their own.",
                                &[("user_id", member.user_id().as_str())],
                            )
                        } else {
                            gettext_f(
                                // Translators: Do NOT translate the content between '{' and '}',
                                // this is a variable name.
                                "Are you sure you want to revoke the invite for {user_id}? They will not be able to join the room again until someone reinvites them.",
                                &[("user_id", member.user_id().as_str())],
                            )
                        };
                        let response = gettext("Revoke Invite");
                        (heading, body, Some(response))
                    }
                    Membership::Knock => {
                        let heading = gettext_f(
                            // Translators: Do NOT translate the content between '{' and '}',
                            // this is a variable name.
                            "Deny Access to {user}?",
                            &[("user", &member.display_name())],
                        );
                        let body = gettext_f(
                            // Translators: Do NOT translate the content between '{' and '}',
                            // this is a variable name.
                            "Are you sure you want to deny access to {user_id}?",
                            &[("user_id", member.user_id().as_str())],
                        );
                        let response = gettext("Deny Access");
                        (heading, body, Some(response))
                    }
                    _ => {
                        // Translators: Do NOT translate the content between '{' and '}',
                        // this is a variable name.
                        let heading =
                            gettext_f("Kick {user}?", &[("user", &member.display_name())]);
                        let body = if can_rejoin {
                            gettext_f(
                                // Translators: Do NOT translate the content between '{' and '}',
                                // this is a variable name.
                                "Are you sure you want to kick {user_id}? They will still be able to join the room again on their own.",
                                &[("user_id", member.user_id().as_str())],
                            )
                        } else {
                            gettext_f(
                                // Translators: Do NOT translate the content between '{' and '}',
                                // this is a variable name.
                                "Are you sure you want to kick {user_id}? They will not be able to join the room again until someone invites them.",
                                &[("user_id", member.user_id().as_str())],
                            )
                        };
                        let response = gettext("Kick");
                        (heading, body, Some(response))
                    }
                }
            }
            RoomMemberDestructiveAction::RemoveMessages(count) => {
                let n = u32::try_from(count).unwrap_or(u32::MAX);
                if count > 0 {
                    let heading = gettext_f(
                        // Translators: Do NOT translate the content between '{' and '}',
                        // this is a variable name.
                        "Remove Messages Sent by {user}?",
                        &[("user", &member.display_name())],
                    );
                    let body = ngettext_f(
                        // Translators: Do NOT translate the content between '{' and '}',
                        // this is a variable name.
                        "This removes all the messages received from the homeserver. Are you sure you want to remove 1 message sent by {user_id}? This cannot be undone.",
                        "This removes all the messages received from the homeserver. Are you sure you want to remove {n} messages sent by {user_id}? This cannot be undone.",
                        n,
                        &[
                            ("n", &n.to_string()),
                            ("user_id", member.user_id().as_str()),
                        ],
                    );
                    let response = gettext("Remove");
                    (heading, body, Some(response))
                } else {
                    let heading = gettext_f(
                        // Translators: Do NOT translate the content between '{' and '}',
                        // this is a variable name.
                        "No Messages Sent by {user}",
                        &[("user", &member.display_name())],
                    );
                    let body = gettext_f(
                        // Translators: Do NOT translate the content between '{' and '}',
                        // this is a variable name.
                        "There are no messages received from the homeserver sent by {user_id}. You can try to load more by going further back in the room history.",
                        &[("user_id", member.user_id().as_str())],
                    );
                    (heading, body, None)
                }
            }
        }
    }
}

/// Show a dialog to confirm the given "destructive" action on the given room
/// member.
///
/// Returns `None` if the user did not confirm.
pub(crate) async fn confirm_room_member_destructive_action_dialog(
    member: &Member,
    action: RoomMemberDestructiveAction,
    parent: &impl IsA<gtk::Widget>,
) -> Option<ConfirmRoomMemberDestructiveActionResponse> {
    let (heading, body, response) = action.dialog_content(member);

    let child = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();

    // Add an entry for the optional reason.
    let reason_entry = adw::EntryRow::builder()
        .title(gettext("Reason (optional)"))
        .build();
    let list_box = gtk::ListBox::builder()
        .css_classes(["boxed-list"])
        .margin_top(6)
        .accessible_role(gtk::AccessibleRole::Group)
        .build();
    list_box.append(&reason_entry);
    child.append(&list_box);

    // Add a switch to ask the whether they want to also remove the latest events of
    // the user.
    let removable_events_count = if let RoomMemberDestructiveAction::Ban(count) = action {
        count
    } else {
        0
    };

    let remove_events_switch = if removable_events_count > 0 {
        let n = u32::try_from(removable_events_count).unwrap_or(u32::MAX);
        let switch = adw::SwitchRow::builder()
            .title(ngettext_f(
                // Translators: Do NOT translate the content between '{' and '}',
                // this is a variable name.
                "Remove the latest message sent by the user",
                "Remove the {n} latest messages sent by the user",
                n,
                &[("n", &n.to_string())],
            ))
            .build();

        let list_box = gtk::ListBox::builder()
            .css_classes(["boxed-list"])
            .margin_top(6)
            .accessible_role(gtk::AccessibleRole::Group)
            .build();
        list_box.append(&switch);
        child.append(&list_box);

        Some(switch)
    } else {
        None
    };

    // Ask for confirmation.
    let confirm_dialog = adw::AlertDialog::builder()
        .default_response("cancel")
        .heading(heading)
        .body(body)
        .extra_child(&child)
        .build();
    confirm_dialog.add_responses(&[("cancel", &gettext("Cancel"))]);

    if let Some(response) = response {
        confirm_dialog.add_responses(&[("confirm", &response)]);
        confirm_dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
    }

    if confirm_dialog.choose_future(Some(parent)).await != "confirm" {
        return None;
    }

    // Get the reason, and filter out if it is empty.
    let reason = Some(reason_entry.text().trim().to_owned()).filter(|s| !s.is_empty());

    let mut response = ConfirmRoomMemberDestructiveActionResponse {
        reason,
        ..Default::default()
    };

    if let Some(switch) = remove_events_switch {
        response.remove_events = switch.is_active();
    }

    Some(response)
}

/// A response to the dialog to confirm a "destructive" action on a room
/// member.
#[derive(Debug, Default, Clone)]
pub(crate) struct ConfirmRoomMemberDestructiveActionResponse {
    /// The reason of the action.
    pub reason: Option<String>,
    /// Whether we can remove the events.
    pub remove_events: bool,
}

/// Show a dialog to confirm muting one or several room members.
pub(crate) async fn confirm_mute_room_member_dialog(
    members: &[impl IsA<User>],
    parent: &impl IsA<gtk::Widget>,
) -> bool {
    if members.is_empty() {
        return false;
    }

    let first_member = members
        .first()
        .expect("there should be at least one member")
        .upcast_ref();
    let is_single_member = members.len() == 1;

    // We don't use the count in the strings so we use separate gettext calls for
    // singular and plural rather than using ngettext.
    let heading = if is_single_member {
        gettext_f(
            // Translators: Do NOT translate the content between '{' and '}',
            // this is a variable name.
            "Mute {user}?",
            &[("user", &first_member.display_name())],
        )
    } else {
        gettext("Mute Members?")
    };

    // We don't use the count in the strings so we use separate gettext calls for
    // singular and plural rather than using ngettext.
    let body = if is_single_member {
        gettext_f(
            // Translators: Do NOT translate the content between '{' and '}',
            // this is a variable name.
            "Are you sure you want to mute {user_id}? They will not be able to send new messages to this room.",
            &[("user_id", first_member.user_id().as_str())],
        )
    } else {
        gettext(
            "Are you sure you want to mute these members? They will not be able to send new messages to this room.",
        )
    };

    // Ask for confirmation.
    let confirm_dialog = adw::AlertDialog::builder()
        .default_response("cancel")
        .heading(heading)
        .body(body)
        .build();
    confirm_dialog.add_responses(&[
        ("cancel", &gettext("Cancel")),
        // Translators: In this string, 'Mute' is a verb, as in 'Mute room member'.
        ("mute", &gettext("Mute")),
    ]);
    confirm_dialog.set_response_appearance("mute", adw::ResponseAppearance::Destructive);

    confirm_dialog.choose_future(Some(parent)).await == "mute"
}

/// Show a dialog to confirm setting the power level of one or several room
/// members with the same value as our own.
pub(crate) async fn confirm_set_room_member_power_level_same_as_own_dialog(
    members: &[impl IsA<User>],
    parent: &impl IsA<gtk::Widget>,
) -> bool {
    if members.is_empty() {
        return false;
    }

    let first_member = members
        .first()
        .expect("there should be at least one member")
        .upcast_ref();
    let is_single_member = members.len() == 1;

    // We don't use the count in the strings so we use separate gettext calls for
    // singular and plural rather than using ngettext.
    let heading = if is_single_member {
        gettext_f(
            // Translators: Do NOT translate the content between '{' and '}',
            // this is a variable name.
            "Promote {user}?",
            &[("user", &first_member.display_name())],
        )
    } else {
        gettext("Promote Members?")
    };

    // We don't use the count in the strings so we use separate gettext calls for
    // singular and plural rather than using ngettext.
    let body = if is_single_member {
        gettext_f(
            // Translators: Do NOT translate the content between '{' and '}',
            // this is a variable name. The count cannot be zero.
            "If you promote {user_id} to the same level as yours, you will not be able to demote them in the future.",
            &[("user_id", first_member.user_id().as_str())],
        )
    } else {
        gettext(
            "If you promote these members to the same level as yours, you will not be able to demote them in the future.",
        )
    };

    // Ask for confirmation.
    let confirm_dialog = adw::AlertDialog::builder()
        .default_response("cancel")
        .heading(heading)
        .body(body)
        .build();
    confirm_dialog.add_responses(&[
        ("cancel", &gettext("Cancel")),
        ("promote", &gettext("Promote")),
    ]);
    confirm_dialog.set_response_appearance("promote", adw::ResponseAppearance::Destructive);

    confirm_dialog.choose_future(Some(parent)).await == "promote"
}

/// Show a dialog to confirm the demotion of our own user.
pub(crate) async fn confirm_own_demotion_dialog(parent: &impl IsA<gtk::Widget>) -> bool {
    let heading = gettext("Demote Yourself?");
    let body = gettext(
        "Are you sure you want to lower your power level? You will need to ask another member with a higher power level to undo this.",
    );

    // Ask for confirmation.
    let confirm_dialog = adw::AlertDialog::builder()
        .default_response("cancel")
        .heading(heading)
        .body(body)
        .build();
    confirm_dialog.add_responses(&[
        ("cancel", &gettext("Cancel")),
        ("demote", &gettext("Demote")),
    ]);
    confirm_dialog.set_response_appearance("demote", adw::ResponseAppearance::Destructive);

    confirm_dialog.choose_future(Some(parent)).await == "demote"
}

/// Show a dialog for the user to choose what to do about unsaved changes.
pub(crate) async fn unsaved_changes_dialog(
    parent: &impl IsA<gtk::Widget>,
) -> UnsavedChangesResponse {
    let title = gettext("Save Changes?");
    let description =
        gettext("This page contains unsaved changes. Changes which are not saved will be lost.");
    let dialog = adw::AlertDialog::builder()
        .title(title)
        .body(description)
        .default_response("cancel")
        .build();

    dialog.add_responses(&[
        ("cancel", &gettext("Cancel")),
        ("discard", &gettext("Discard")),
        ("save", &gettext("Save")),
    ]);
    dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);

    match dialog.choose_future(Some(parent)).await.as_str() {
        "discard" => UnsavedChangesResponse::Discard,
        "save" => UnsavedChangesResponse::Save,
        _ => UnsavedChangesResponse::Cancel,
    }
}

/// A response to the dialog to choose what to do about unsaved changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnsavedChangesResponse {
    /// Save the changes.
    Save,
    /// Discard the changes.
    Discard,
    /// Cancel the current action.
    Cancel,
}
