use std::slice;

use adw::{prelude::*, subclass::prelude::*};
use gettextrs::{gettext, ngettext, pgettext};
use gtk::{
    glib,
    glib::{clone, closure_local},
};
use ruma::{
    OwnedEventId,
    events::room::power_levels::{PowerLevelUserAction, UserPowerLevel},
};

use super::{Avatar, LoadingButton, LoadingButtonRow, PowerLevelSelectionRow};
use crate::{
    Window,
    components::{
        RoomMemberDestructiveAction, confirm_mute_room_member_dialog, confirm_own_demotion_dialog,
        confirm_room_member_destructive_action_dialog,
        confirm_set_room_member_power_level_same_as_own_dialog,
    },
    gettext_f,
    prelude::*,
    session::{Member, Membership, Permissions, Room, User},
    toast,
    utils::BoundObject,
};

mod imp {
    use std::{cell::RefCell, sync::LazyLock};

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/user_page.ui")]
    #[properties(wrapper_type = super::UserPage)]
    pub struct UserPage {
        #[template_child]
        avatar: TemplateChild<Avatar>,
        #[template_child]
        direct_chat_box: TemplateChild<gtk::ListBox>,
        #[template_child]
        direct_chat_button: TemplateChild<LoadingButtonRow>,
        #[template_child]
        verified_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        verified_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        verify_button: TemplateChild<LoadingButton>,
        #[template_child]
        room_box: TemplateChild<gtk::Box>,
        #[template_child]
        room_title: TemplateChild<gtk::Label>,
        #[template_child]
        membership_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        membership_label: TemplateChild<gtk::Label>,
        #[template_child]
        power_level_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        invite_button: TemplateChild<LoadingButtonRow>,
        #[template_child]
        kick_button: TemplateChild<LoadingButtonRow>,
        #[template_child]
        ban_button: TemplateChild<LoadingButtonRow>,
        #[template_child]
        unban_button: TemplateChild<LoadingButtonRow>,
        #[template_child]
        remove_messages_button: TemplateChild<LoadingButtonRow>,
        #[template_child]
        ignored_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        ignored_button: TemplateChild<LoadingButton>,
        /// The current user.
        #[property(get, set = Self::set_user, explicit_notify, nullable)]
        user: BoundObject<User>,
        bindings: RefCell<Vec<glib::Binding>>,
        permissions_handler: RefCell<Option<glib::SignalHandlerId>>,
        room_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for UserPage {
        const NAME: &'static str = "UserPage";
        type Type = super::UserPage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("user-page");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for UserPage {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("close").build()]);
            SIGNALS.as_ref()
        }

        fn dispose(&self) {
            self.disconnect_signals();
        }
    }

    impl WidgetImpl for UserPage {}
    impl NavigationPageImpl for UserPage {}

    #[gtk::template_callbacks]
    impl UserPage {
        /// Set the current user.
        fn set_user(&self, user: Option<User>) {
            if self.user.obj() == user {
                return;
            }
            let obj = self.obj();

            self.disconnect_signals();
            self.power_level_row.set_permissions(None::<Permissions>);

            if let Some(user) = user {
                let title_binding = user
                    .bind_property("display-name", &*obj, "title")
                    .sync_create()
                    .build();
                let avatar_binding = user
                    .bind_property("avatar-data", &*self.avatar, "data")
                    .sync_create()
                    .build();
                let bindings = vec![title_binding, avatar_binding];

                let verified_handler = user.connect_is_verified_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_verified();
                    }
                ));
                let ignored_handler = user.connect_is_ignored_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_direct_chat();
                        imp.update_ignored();
                    }
                ));
                let mut handlers = vec![verified_handler, ignored_handler];

                if let Some(member) = user.downcast_ref::<Member>() {
                    let room = member.room();

                    let permissions = room.permissions();
                    let permissions_handler = permissions.connect_changed(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_room();
                        }
                    ));
                    self.permissions_handler.replace(Some(permissions_handler));
                    self.power_level_row.set_permissions(Some(permissions));

                    let room_display_name_handler = room.connect_display_name_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_room();
                        }
                    ));
                    let room_direct_member_handler = room.connect_direct_member_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_direct_chat();
                        }
                    ));
                    self.room_handlers
                        .replace(vec![room_display_name_handler, room_direct_member_handler]);

                    let membership_handler = member.connect_membership_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |member| {
                            if member.membership() == Membership::Leave {
                                imp.obj().emit_by_name::<()>("close", &[]);
                            } else {
                                imp.update_room();
                            }
                        }
                    ));
                    let power_level_handler = member.connect_power_level_changed(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_room();
                        }
                    ));
                    handlers.extend([membership_handler, power_level_handler]);
                }

                // We do not need to listen to changes of the property, it never changes after
                // construction.
                let is_own_user = user.is_own_user();
                self.ignored_row.set_visible(!is_own_user);

                self.user.set(user, handlers);
                self.bindings.replace(bindings);
            }

            self.load_direct_chat();
            self.update_direct_chat();
            self.update_room();
            self.update_verified();
            self.update_ignored();
            obj.notify_user();
        }

        /// Disconnect all the signals.
        fn disconnect_signals(&self) {
            if let Some(member) = self.user.obj().and_downcast::<Member>() {
                let room = member.room();

                for handler in self.room_handlers.take() {
                    room.disconnect(handler);
                }
                if let Some(handler) = self.permissions_handler.take() {
                    room.permissions().disconnect(handler);
                }
            }

            for binding in self.bindings.take() {
                binding.unbind();
            }

            self.user.disconnect_signals();
        }

        /// Copy the user ID to the clipboard.
        #[template_callback]
        fn copy_user_id(&self) {
            let Some(user) = self.user.obj() else {
                return;
            };

            let obj = self.obj();
            obj.clipboard().set_text(user.user_id().as_str());
            toast!(obj, gettext("Matrix user ID copied to clipboard"));
        }

        /// Update the visibility of the direct chat button.
        fn update_direct_chat(&self) {
            let user = self.user.obj();
            let is_other_user = user
                .as_ref()
                .is_some_and(|u| !u.is_own_user() && !u.is_ignored());
            let is_direct_chat = user
                .and_downcast::<Member>()
                .is_some_and(|m| m.room().direct_member().is_some());
            self.direct_chat_box
                .set_visible(is_other_user && !is_direct_chat);
        }

        /// Load whether the current user has a direct chat or not.
        fn load_direct_chat(&self) {
            self.direct_chat_button.set_is_loading(true);

            let Some(user) = self.user.obj() else {
                return;
            };

            let direct_chat = user.direct_chat();

            let title = if direct_chat.is_some() {
                gettext("Open Direct Chat")
            } else {
                gettext("Create Direct Chat")
            };
            self.direct_chat_button.set_title(&title);

            self.direct_chat_button.set_is_loading(false);
        }

        /// Open a direct chat with the current user.
        ///
        /// If one doesn't exist already, it is created.
        #[template_callback]
        async fn open_direct_chat(&self) {
            let Some(user) = self.user.obj() else {
                return;
            };

            self.direct_chat_button.set_is_loading(true);
            let obj = self.obj();

            let Ok(room) = user.get_or_create_direct_chat().await else {
                toast!(obj, &gettext("Could not create a new Direct Chat"));
                self.direct_chat_button.set_is_loading(false);

                return;
            };

            let Some(parent_window) = obj.root().and_downcast::<gtk::Window>() else {
                return;
            };

            if let Some(main_window) = parent_window.transient_for().and_downcast::<Window>() {
                main_window.session_view().select_room(room);
            }

            parent_window.close();
        }

        /// Update the room section.
        fn update_room(&self) {
            let Some(member) = self.user.obj().and_downcast::<Member>() else {
                self.room_box.set_visible(false);
                return;
            };

            let membership = member.membership();
            if membership == Membership::Leave {
                self.room_box.set_visible(false);
                return;
            }

            let room = member.room();
            let room_title = gettext_f("In {room_name}", &[("room_name", &room.display_name())]);
            self.room_title.set_label(&room_title);

            let label = match membership {
                Membership::Leave => unreachable!(),
                Membership::Join => {
                    // Nothing to update, it should show the role row.
                    None
                }
                Membership::Invite => {
                    // Translators: As in, 'The room member was invited'.
                    Some(pgettext("member", "Invited"))
                }
                Membership::Ban => {
                    // Translators: As in, 'The room member was banned'.
                    Some(pgettext("member", "Banned"))
                }
                Membership::Knock => {
                    // Translators: As in, 'The room member requested an invite'.
                    Some(pgettext("member", "Requested an Invite"))
                }
                Membership::Unsupported => {
                    // Translators: As in, 'The room member has an unknown role'.
                    Some(pgettext("member", "Unknown"))
                }
            };
            if let Some(label) = label {
                self.membership_label.set_label(&label);
            }

            let is_role = membership == Membership::Join;
            self.membership_row.set_visible(!is_role);
            self.power_level_row.set_visible(is_role);

            let permissions = room.permissions();
            let user_id = member.user_id();

            self.power_level_row.set_is_loading(false);
            self.power_level_row
                .set_selected_power_level(member.power_level());

            let can_change_power_level =
                permissions.can_do_to_user(user_id, PowerLevelUserAction::ChangePowerLevel);
            self.power_level_row.set_read_only(!can_change_power_level);

            let can_invite = matches!(membership, Membership::Knock) && permissions.can_invite();
            self.invite_button.set_visible(can_invite);

            let can_kick = matches!(
                membership,
                Membership::Join | Membership::Invite | Membership::Knock
            ) && permissions.can_do_to_user(user_id, PowerLevelUserAction::Kick);
            if can_kick {
                let label = match membership {
                    Membership::Invite => gettext("Revoke Invite"),
                    Membership::Knock => gettext("Deny Request"),
                    // Translators: As in, 'Kick room member'.
                    _ => gettext("Kick"),
                };
                self.kick_button.set_title(&label);
            }
            self.kick_button.set_visible(can_kick);

            let can_ban = membership != Membership::Ban
                && permissions.can_do_to_user(user_id, PowerLevelUserAction::Ban);
            self.ban_button.set_visible(can_ban);

            let can_unban = matches!(membership, Membership::Ban)
                && permissions.can_do_to_user(user_id, PowerLevelUserAction::Unban);
            self.unban_button.set_visible(can_unban);

            let can_redact = !member.is_own_user() && permissions.can_redact_other();
            self.remove_messages_button.set_visible(can_redact);

            self.room_box.set_visible(true);
        }

        /// Reset the initial state of the buttons of the room section.
        fn reset_room(&self) {
            self.kick_button.set_is_loading(false);
            self.kick_button.set_sensitive(true);

            self.invite_button.set_is_loading(false);
            self.invite_button.set_sensitive(true);

            self.ban_button.set_is_loading(false);
            self.ban_button.set_sensitive(true);

            self.unban_button.set_is_loading(false);
            self.unban_button.set_sensitive(true);

            self.remove_messages_button.set_is_loading(false);
            self.remove_messages_button.set_sensitive(true);
        }

        /// Set the power level of the user.
        #[template_callback]
        async fn set_power_level(&self) {
            let Some(member) = self.user.obj().and_downcast::<Member>() else {
                return;
            };

            let row = &self.power_level_row;
            let UserPowerLevel::Int(power_level) = row.selected_power_level() else {
                // We cannot set the power level to infinite.
                return;
            };

            let UserPowerLevel::Int(old_power_level) = member.power_level() else {
                // We cannot change the power level if it is currently infinite.
                return;
            };

            if old_power_level == power_level {
                // Nothing to do.
                return;
            }

            row.set_is_loading(true);
            row.set_read_only(true);

            let obj = self.obj();
            let permissions = member.room().permissions();

            if member.is_own_user() {
                // Warn that demoting oneself is irreversible.
                if !confirm_own_demotion_dialog(&*obj).await {
                    self.update_room();
                    return;
                }
            } else {
                // Warn if user is muted but was not before.
                let mute_power_level = permissions.mute_power_level();
                let is_muted = i64::from(power_level) <= mute_power_level
                    && i64::from(old_power_level) > mute_power_level;
                if is_muted
                    && !confirm_mute_room_member_dialog(slice::from_ref(&member), &*obj).await
                {
                    self.update_room();
                    return;
                }

                // Warn if power level is set at same level as own power level.
                let is_own_power_level = power_level == permissions.own_power_level();
                if is_own_power_level
                    && !confirm_set_room_member_power_level_same_as_own_dialog(
                        slice::from_ref(&member),
                        &*obj,
                    )
                    .await
                {
                    self.update_room();
                    return;
                }
            }

            let user_id = member.user_id().clone();

            if permissions
                .set_user_power_level(user_id, power_level)
                .await
                .is_err()
            {
                toast!(obj, gettext("Could not change the role"));
                self.update_room();
            }
        }

        /// Invite the user to the room.
        #[template_callback]
        async fn invite_user(&self) {
            let Some(member) = self.user.obj().and_downcast::<Member>() else {
                return;
            };

            self.invite_button.set_is_loading(true);
            self.kick_button.set_sensitive(false);
            self.ban_button.set_sensitive(false);
            self.unban_button.set_sensitive(false);

            let room = member.room();
            let user_id = member.user_id().clone();

            if room.invite(&[user_id]).await.is_err() {
                toast!(self.obj(), gettext("Could not invite user"));
            }

            self.reset_room();
        }

        /// Kick the user from the room.
        #[template_callback]
        async fn kick_user(&self) {
            let Some(member) = self.user.obj().and_downcast::<Member>() else {
                return;
            };
            let obj = self.obj();

            self.kick_button.set_is_loading(true);
            self.invite_button.set_sensitive(false);
            self.ban_button.set_sensitive(false);
            self.unban_button.set_sensitive(false);

            let Some(response) = confirm_room_member_destructive_action_dialog(
                &member,
                RoomMemberDestructiveAction::Kick,
                &*obj,
            )
            .await
            else {
                self.reset_room();
                return;
            };

            let room = member.room();
            let user_id = member.user_id().clone();
            if room.kick(&[(user_id, response.reason)]).await.is_err() {
                let error = match member.membership() {
                    Membership::Invite => gettext("Could not revoke invite of user"),
                    Membership::Knock => gettext("Could not deny access to user"),
                    _ => gettext("Could not kick user"),
                };
                toast!(obj, error);

                self.reset_room();
            }
        }

        /// Ban the room member.
        #[template_callback]
        async fn ban_user(&self) {
            let Some(member) = self.user.obj().and_downcast::<Member>() else {
                return;
            };
            let obj = self.obj();

            self.ban_button.set_is_loading(true);
            self.invite_button.set_sensitive(false);
            self.kick_button.set_sensitive(false);
            self.unban_button.set_sensitive(false);

            let permissions = member.room().permissions();
            let redactable_events = if permissions.can_redact_other() {
                member.redactable_events()
            } else {
                vec![]
            };

            let Some(response) = confirm_room_member_destructive_action_dialog(
                &member,
                RoomMemberDestructiveAction::Ban(redactable_events.len()),
                &*obj,
            )
            .await
            else {
                self.reset_room();
                return;
            };

            let room = member.room();
            let user_id = member.user_id().clone();
            if room
                .ban(&[(user_id, response.reason.clone())])
                .await
                .is_err()
            {
                toast!(obj, gettext("Could not ban user"));
            }

            if response.remove_events {
                self.remove_known_messages_inner(
                    &member.room(),
                    redactable_events,
                    response.reason,
                )
                .await;
            }

            self.reset_room();
        }

        /// Unban the room member.
        #[template_callback]
        async fn unban_user(&self) {
            let Some(member) = self.user.obj().and_downcast::<Member>() else {
                return;
            };

            self.unban_button.set_is_loading(true);
            self.invite_button.set_sensitive(false);
            self.kick_button.set_sensitive(false);
            self.ban_button.set_sensitive(false);

            let room = member.room();
            let user_id = member.user_id().clone();

            if room.unban(&[(user_id, None)]).await.is_err() {
                toast!(self.obj(), gettext("Could not unban user"));
            }

            self.reset_room();
        }

        /// Remove the known events of the room member.
        #[template_callback]
        async fn remove_messages(&self) {
            let Some(member) = self.user.obj().and_downcast::<Member>() else {
                return;
            };

            self.remove_messages_button.set_is_loading(true);

            let redactable_events = member.redactable_events();

            let Some(response) = confirm_room_member_destructive_action_dialog(
                &member,
                RoomMemberDestructiveAction::RemoveMessages(redactable_events.len()),
                &*self.obj(),
            )
            .await
            else {
                self.reset_room();
                return;
            };

            self.remove_known_messages_inner(&member.room(), redactable_events, response.reason)
                .await;

            self.reset_room();
        }

        async fn remove_known_messages_inner(
            &self,
            room: &Room,
            events: Vec<OwnedEventId>,
            reason: Option<String>,
        ) {
            if let Err(events) = room.redact(&events, reason).await {
                let n = u32::try_from(events.len()).unwrap_or(u32::MAX);

                toast!(
                    self.obj(),
                    ngettext(
                        // Translators: Do NOT translate the content between '{' and '}',
                        // this is a variable name.
                        "Could not remove 1 message sent by the user",
                        "Could not remove {n} messages sent by the user",
                        n,
                    ),
                    n,
                );
            }
        }

        /// Update the verified row.
        fn update_verified(&self) {
            let Some(user) = self.user.obj() else {
                return;
            };

            if user.is_verified() {
                self.verified_row.set_title(&gettext("Identity verified"));
                self.verified_stack.set_visible_child_name("icon");
                self.verify_button.set_sensitive(false);
            } else {
                self.verify_button.set_sensitive(true);
                self.verified_stack.set_visible_child_name("button");
                self.verified_row
                    .set_title(&gettext("Identity not verified"));
            }
        }

        /// Launch the verification for the current user.
        #[template_callback]
        async fn verify_user(&self) {
            let Some(user) = self.user.obj() else {
                return;
            };
            let obj = self.obj();

            self.verify_button.set_is_loading(true);

            let Ok(verification) = user.verify_identity().await else {
                toast!(obj, gettext("Could not start user verification"));
                self.verify_button.set_is_loading(false);
                return;
            };

            let Some(parent_window) = obj.root().and_downcast::<gtk::Window>() else {
                return;
            };

            if let Some(main_window) = parent_window.transient_for().and_downcast::<Window>() {
                main_window
                    .session_view()
                    .select_identity_verification(verification);
            }

            parent_window.close();
        }

        /// Update the ignored row.
        fn update_ignored(&self) {
            let Some(user) = self.user.obj() else {
                return;
            };

            if user.is_ignored() {
                self.ignored_row.set_title(&gettext("Ignored"));
                self.ignored_button
                    .set_content_label(gettext("Stop Ignoring"));
                self.ignored_button.remove_css_class("destructive-action");
            } else {
                self.ignored_row.set_title(&gettext("Not Ignored"));
                self.ignored_button.set_content_label(gettext("Ignore"));
                self.ignored_button.add_css_class("destructive-action");
            }
        }

        /// Toggle whether the user is ignored or not.
        #[template_callback]
        async fn toggle_ignored(&self) {
            let Some(user) = self.user.obj() else {
                return;
            };

            let obj = self.obj();
            self.ignored_button.set_is_loading(true);

            if user.is_ignored() {
                if user.stop_ignoring().await.is_err() {
                    toast!(obj, gettext("Could not stop ignoring user"));
                }
            } else if user.ignore().await.is_err() {
                toast!(obj, gettext("Could not ignore user"));
            }

            self.ignored_button.set_is_loading(false);
        }
    }
}

glib::wrapper! {
    /// Page to view details about a user.
    pub struct UserPage(ObjectSubclass<imp::UserPage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl UserPage {
    /// Construct a new `UserPage` for the given user.
    pub fn new(user: &impl IsA<User>) -> Self {
        glib::Object::builder().property("user", user).build()
    }

    /// Connect to the signal emitted when the page should be closed.
    pub fn connect_close<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "close",
            true,
            closure_local!(|obj: Self| {
                f(&obj);
            }),
        )
    }
}
