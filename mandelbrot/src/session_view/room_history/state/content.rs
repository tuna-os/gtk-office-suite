use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone, pango};
use matrix_sdk_ui::timeline::{
    AnyOtherStateEventContentChange, MemberProfileChange, MembershipChange, OtherState,
    RoomMembershipChange, TimelineItemContent,
};
use ruma::{
    UserId,
    events::{StateEventContentChange, room::member::MembershipState},
};
use tracing::warn;

use super::StateCreation;
use crate::{
    gettext_f,
    prelude::*,
    session::{Event, Member},
    utils::BoundObjectWeakRef,
};

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::StateContent)]
    pub struct StateContent {
        /// The state event displayed by this widget.
        #[property(get, set = Self::set_event, nullable)]
        event: glib::WeakRef<Event>,
        /// The sender of the event.
        sender: BoundObjectWeakRef<Member>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StateContent {
        const NAME: &'static str = "ContentStateContent";
        type Type = super::StateContent;
        type ParentType = adw::Bin;
    }

    #[glib::derived_properties]
    impl ObjectImpl for StateContent {}

    impl WidgetImpl for StateContent {}
    impl BinImpl for StateContent {}

    impl StateContent {
        /// Set the event presented by this row.
        fn set_event(&self, event: Option<&Event>) {
            let Some(event) = event else {
                // Only handle when an event is set.
                return;
            };

            let sender = event.sender();
            let disambiguated_name_handler = sender.connect_disambiguated_name_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_content();
                }
            ));
            self.sender.set(&sender, vec![disambiguated_name_handler]);

            self.event.set(Some(event));
            self.update_content();
        }

        /// Update the content for the current state.
        fn update_content(&self) {
            let Some(event) = self.event.upgrade() else {
                return;
            };
            let Some(sender) = self.sender.obj() else {
                return;
            };

            match event.content() {
                TimelineItemContent::MembershipChange(membership_change) => {
                    self.update_with_membership_change(&membership_change, &sender);
                }
                TimelineItemContent::ProfileChange(profile_change) => {
                    self.update_with_profile_change(&profile_change, &sender);
                }
                TimelineItemContent::OtherState(other_state) => {
                    self.update_with_other_state(&other_state, &sender);
                }
                _ => unreachable!(),
            }
        }

        /// Update this row with the given [`OtherState`].
        fn update_with_other_state(&self, other_state: &OtherState, sender: &Member) {
            let widget = match other_state.content() {
                AnyOtherStateEventContentChange::RoomCreate(content) => {
                    WidgetType::Creation(StateCreation::new(content))
                }
                AnyOtherStateEventContentChange::RoomEncryption(_) => {
                    WidgetType::Text(gettext("This room is encrypted from this point on."))
                }
                AnyOtherStateEventContentChange::RoomThirdPartyInvite(content) => {
                    let display_name = match content {
                        StateEventContentChange::Original { content, .. } => {
                            match &content.display_name {
                                s if s.is_empty() => other_state.state_key(),
                                s => s,
                            }
                        }
                        StateEventContentChange::Redacted(_) => other_state.state_key(),
                    };
                    WidgetType::Text(gettext_f(
                        // Translators: Do NOT translate the content between '{' and '}', this is a
                        // variable name.
                        "{sender} invited {user}.",
                        &[
                            ("sender", &sender.disambiguated_name()),
                            ("user", display_name),
                        ],
                    ))
                }
                _ => {
                    warn!(
                        "Unsupported state event: {}",
                        other_state.content().event_type()
                    );
                    WidgetType::Text(gettext("An unsupported state event was received."))
                }
            };

            let obj = self.obj();
            match widget {
                WidgetType::Text(message) => {
                    let child = obj.child_or_else::<gtk::Label>(text);
                    child.set_label(&message);
                }
                WidgetType::Creation(widget) => obj.set_child(Some(&widget)),
            }
        }

        /// Update this row for the given membership change.
        fn update_with_membership_change(
            &self,
            membership_change: &RoomMembershipChange,
            sender: &Member,
        ) {
            let sender_display_name = sender.disambiguated_name();
            let target_display_name = match membership_change.content() {
                StateEventContentChange::Original { content, .. } => content
                    .displayname
                    .clone()
                    .unwrap_or_else(|| membership_change.user_id().to_string()),
                StateEventContentChange::Redacted(_) => membership_change.user_id().to_string(),
            };

            let supported_membership_change =
                Self::to_supported_membership_change(membership_change, sender.user_id());

            let message = match supported_membership_change {
                MembershipChange::Joined => {
                    // Translators: Do NOT translate the content between '{' and '}', this
                    // is a variable name.
                    gettext_f(
                        "{user} joined this room.",
                        &[("user", &target_display_name)],
                    )
                }
                MembershipChange::Left => {
                    // Translators: Do NOT translate the content between '{' and '}',
                    // this is a variable name.
                    gettext_f("{user} left the room.", &[("user", &target_display_name)])
                }
                MembershipChange::Banned | MembershipChange::KickedAndBanned => gettext_f(
                    // Translators: Do NOT translate the content between
                    // '{' and '}', these are variable names.
                    "{sender} banned {user}.",
                    &[
                        ("sender", &sender_display_name),
                        ("user", &target_display_name),
                    ],
                ),
                MembershipChange::Unbanned => gettext_f(
                    // Translators: Do NOT translate the content between
                    // '{' and '}', these are variable names.
                    "{sender} unbanned {user}.",
                    &[
                        ("sender", &sender_display_name),
                        ("user", &target_display_name),
                    ],
                ),
                MembershipChange::Kicked => gettext_f(
                    // Translators: Do NOT translate the content between '{' and
                    // '}', these are variable names.
                    "{sender} kicked {user} out.",
                    &[
                        ("sender", &sender_display_name),
                        ("user", &target_display_name),
                    ],
                ),
                MembershipChange::Invited | MembershipChange::KnockAccepted => gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', these are
                    // variable names.
                    "{sender} invited {user}.",
                    &[
                        ("sender", &sender_display_name),
                        ("user", &target_display_name),
                    ],
                ),
                MembershipChange::InvitationAccepted => gettext_f(
                    // Translators: Do NOT translate the content between
                    // '{' and '}', this is a variable name.
                    "{user} accepted the invite.",
                    &[("user", &target_display_name)],
                ),
                MembershipChange::InvitationRejected => gettext_f(
                    // Translators: Do NOT translate the content between
                    // '{' and '}', these are variable names.
                    "{user} declined the invite.",
                    &[
                        ("sender", &sender_display_name),
                        ("user", &target_display_name),
                    ],
                ),
                MembershipChange::InvitationRevoked => gettext_f(
                    // Translators: Do NOT translate the content between
                    // '{' and '}', these are variable names.
                    "{sender} revoked the invitation for {user}.",
                    &[
                        ("sender", &sender_display_name),
                        ("user", &target_display_name),
                    ],
                ),
                MembershipChange::Knocked =>
                // TODO: Add button to invite the user.
                {
                    gettext_f(
                        // Translators: Do NOT translate the content between '{' and '}', this
                        // is a variable name.
                        "{user} requested to be invited to this room.",
                        &[("user", &target_display_name)],
                    )
                }
                MembershipChange::KnockRetracted => gettext_f(
                    // Translators: Do NOT translate the content between
                    // '{' and '}', this is a variable name.
                    "{user} retracted their request to be invited to this room.",
                    &[("user", &target_display_name)],
                ),
                MembershipChange::KnockDenied => gettext_f(
                    // Translators: Do NOT translate the content between
                    // '{' and '}', these are variable names.
                    "{sender} denied {user}’s request to be invited to this room.",
                    &[
                        ("sender", &sender_display_name),
                        ("user", &target_display_name),
                    ],
                ),
                _ => {
                    warn!(
                        "Unsupported membership change event: {:?}",
                        membership_change.content()
                    );
                    gettext("An unsupported room member event was received.")
                }
            };

            let child = self.obj().child_or_else::<gtk::Label>(text);
            child.set_label(&message);
        }

        /// Convert a received membership change to a supported membership
        /// change.
        ///
        /// This is used to fallback to showing the membership when we do not
        /// know or do not want to show the change.
        fn to_supported_membership_change(
            membership_change: &RoomMembershipChange,
            sender: &UserId,
        ) -> MembershipChange {
            match membership_change.change().unwrap_or(MembershipChange::None) {
                MembershipChange::Joined => MembershipChange::Joined,
                MembershipChange::Left => MembershipChange::Left,
                MembershipChange::Banned => MembershipChange::Banned,
                MembershipChange::Unbanned => MembershipChange::Unbanned,
                MembershipChange::Kicked => MembershipChange::Kicked,
                MembershipChange::Invited => MembershipChange::Invited,
                MembershipChange::KickedAndBanned => MembershipChange::KickedAndBanned,
                MembershipChange::InvitationAccepted => MembershipChange::InvitationAccepted,
                MembershipChange::InvitationRejected => MembershipChange::InvitationRejected,
                MembershipChange::InvitationRevoked => MembershipChange::InvitationRevoked,
                MembershipChange::Knocked => MembershipChange::Knocked,
                MembershipChange::KnockAccepted => MembershipChange::KnockAccepted,
                MembershipChange::KnockRetracted => MembershipChange::KnockRetracted,
                MembershipChange::KnockDenied => MembershipChange::KnockDenied,
                _ => {
                    let membership = match membership_change.content() {
                        StateEventContentChange::Original { content, .. } => &content.membership,
                        StateEventContentChange::Redacted(content) => &content.membership,
                    };

                    match membership {
                        MembershipState::Ban => MembershipChange::Banned,
                        MembershipState::Invite => MembershipChange::Invited,
                        MembershipState::Join => MembershipChange::Joined,
                        MembershipState::Knock => MembershipChange::Knocked,
                        MembershipState::Leave => {
                            if membership_change.user_id() == sender {
                                MembershipChange::Left
                            } else {
                                MembershipChange::Kicked
                            }
                        }
                        _ => MembershipChange::NotImplemented,
                    }
                }
            }
        }

        fn update_with_profile_change(
            &self,
            profile_change: &MemberProfileChange,
            sender: &Member,
        ) {
            let message = if let Some(displayname) = profile_change.displayname_change() {
                if let Some(prev_name) = &displayname.old {
                    if let Some(new_name) = &displayname.new {
                        gettext_f(
                            // Translators: Do NOT translate the content between
                            // '{' and '}', this is a variable name.
                            "{previous_user_name} changed their display name to {new_user_name}.",
                            &[
                                ("previous_user_name", prev_name),
                                ("new_user_name", new_name),
                            ],
                        )
                    } else {
                        gettext_f(
                            // Translators: Do NOT translate the content between
                            // '{' and '}', this is a variable name.
                            "{previous_user_name} removed their display name.",
                            &[("previous_user_name", prev_name)],
                        )
                    }
                } else {
                    let new_name = displayname
                        .new
                        .as_ref()
                        .expect("At least one displayname is set in a display name change");
                    gettext_f(
                        // Translators: Do NOT translate the content between
                        // '{' and '}', this is a variable name.
                        "{user_id} set their display name to {new_user_name}.",
                        &[
                            ("user_id", profile_change.user_id().as_ref()),
                            ("new_user_name", new_name),
                        ],
                    )
                }
            } else if let Some(avatar_url) = profile_change.avatar_url_change() {
                let display_name = sender.disambiguated_name();

                if avatar_url.old.is_none() {
                    gettext_f(
                        // Translators: Do NOT translate the content between
                        // '{' and '}', this is a variable name.
                        "{user} set their avatar.",
                        &[("user", &display_name)],
                    )
                } else if avatar_url.new.is_none() {
                    gettext_f(
                        // Translators: Do NOT translate the content between
                        // '{' and '}', this is a variable name.
                        "{user} removed their avatar.",
                        &[("user", &display_name)],
                    )
                } else {
                    gettext_f(
                        // Translators: Do NOT translate the content between
                        // '{' and '}', this is a variable name.
                        "{user} changed their avatar.",
                        &[("user", &display_name)],
                    )
                }
            } else {
                // We don't know what changed so fall back to the membership.
                // Translators: Do NOT translate the content between '{' and '}', this
                // is a variable name.
                gettext_f(
                    "{user} joined this room.",
                    &[("user", &sender.disambiguated_name())],
                )
            };

            let child = self.obj().child_or_else::<gtk::Label>(text);
            child.set_label(&message);
        }
    }
}

glib::wrapper! {
    /// A row presenting a state event.
    pub struct StateContent(ObjectSubclass<imp::StateContent>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl StateContent {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for StateContent {
    fn default() -> Self {
        Self::new()
    }
}

impl IsABin for StateContent {}

enum WidgetType {
    Text(String),
    Creation(StateCreation),
}

/// Construct a `GtkLabel` for presenting a state content.
fn text() -> gtk::Label {
    gtk::Label::builder()
        .css_classes(["dimmed"])
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .xalign(0.0)
        .build()
}
