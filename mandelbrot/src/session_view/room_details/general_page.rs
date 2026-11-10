use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{
    gio,
    glib::{self, clone},
    pango,
};
use ruma::{
    api::client::{
        directory::{get_room_visibility, set_room_visibility},
        discovery::get_capabilities::v3::RoomVersionsCapability,
        room::{Visibility, upgrade_room},
    },
    events::{
        StateEventType,
        room::{
            guest_access::{GuestAccess, RoomGuestAccessEventContent},
            power_levels::PowerLevelAction,
        },
    },
};
use tracing::error;

use super::{MemberRow, RoomDetails, UpgradeDialog, UpgradeInfo};
use crate::{
    Window,
    components::{
        Avatar, ButtonCountRow, CheckLoadingRow, CopyableRow, LoadingButton, SwitchLoadingRow,
    },
    gettext_f,
    prelude::*,
    session::{
        HistoryVisibilityValue, Member, MemberList, MembershipListKind, NotificationsRoomSetting,
        Room, RoomCategory,
    },
    spawn, spawn_tokio, toast,
    utils::{BoundObjectWeakRef, TemplateCallbacks, expression, matrix::MatrixIdUri},
};

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
    };

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/general_page.ui")]
    #[properties(wrapper_type = super::GeneralPage)]
    pub struct GeneralPage {
        #[template_child]
        avatar: TemplateChild<Avatar>,
        #[template_child]
        room_topic: TemplateChild<gtk::Label>,
        #[template_child]
        edit_details_btn: TemplateChild<gtk::Button>,
        #[template_child]
        direct_members_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        direct_members_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        no_direct_members_label: TemplateChild<gtk::Label>,
        #[template_child]
        members_row_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        members_row: TemplateChild<ButtonCountRow>,
        #[template_child]
        notifications: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        notifications_global_row: TemplateChild<CheckLoadingRow>,
        #[template_child]
        notifications_all_row: TemplateChild<CheckLoadingRow>,
        #[template_child]
        notifications_mentions_row: TemplateChild<CheckLoadingRow>,
        #[template_child]
        notifications_mute_row: TemplateChild<CheckLoadingRow>,
        #[template_child]
        addresses_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        edit_addresses_button: TemplateChild<gtk::Button>,
        #[template_child]
        no_addresses_label: TemplateChild<gtk::Label>,
        canonical_alias_row: RefCell<Option<CopyableRow>>,
        alt_aliases_rows: RefCell<Vec<CopyableRow>>,
        #[template_child]
        join_rule: TemplateChild<ButtonCountRow>,
        #[template_child]
        guest_access: TemplateChild<SwitchLoadingRow>,
        #[template_child]
        publish: TemplateChild<SwitchLoadingRow>,
        #[template_child]
        history_visibility: TemplateChild<ButtonCountRow>,
        #[template_child]
        encryption: TemplateChild<SwitchLoadingRow>,
        #[template_child]
        upgrade_button: TemplateChild<LoadingButton>,
        #[template_child]
        room_federated: TemplateChild<adw::ActionRow>,
        /// The presented room.
        #[property(get, set = Self::set_room, construct_only)]
        room: BoundObjectWeakRef<Room>,
        /// The lists of members in the room.
        #[property(get, set = Self::set_members, construct_only)]
        members: glib::WeakRef<MemberList>,
        /// The notifications setting for the room.
        #[property(get = Self::notifications_setting, set = Self::set_notifications_setting, explicit_notify, builder(NotificationsRoomSetting::default()))]
        notifications_setting: PhantomData<NotificationsRoomSetting>,
        /// Whether the notifications section is busy.
        #[property(get)]
        notifications_loading: Cell<bool>,
        /// Whether the room is published in the directory.
        #[property(get)]
        is_published: Cell<bool>,
        supported_room_versions: RefCell<RoomVersionsCapability>,
        upgrade_info: RefCell<Option<UpgradeInfo>>,
        direct_members_list_has_bound_model: Cell<bool>,
        expr_watch: RefCell<Option<gtk::ExpressionWatch>>,
        notifications_settings_handlers: RefCell<Vec<glib::SignalHandlerId>>,
        membership_handler: RefCell<Option<glib::SignalHandlerId>>,
        permissions_handler: RefCell<Option<glib::SignalHandlerId>>,
        canonical_alias_handler: RefCell<Option<glib::SignalHandlerId>>,
        alt_aliases_handler: RefCell<Option<glib::SignalHandlerId>>,
        join_rule_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GeneralPage {
        const NAME: &'static str = "RoomDetailsGeneralPage";
        type Type = super::GeneralPage;
        type ParentType = adw::PreferencesPage;

        fn class_init(klass: &mut Self::Class) {
            CopyableRow::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
            TemplateCallbacks::bind_template_callbacks(klass);

            klass
                .install_property_action("room.set-notifications-setting", "notifications-setting");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for GeneralPage {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            self.room_topic.connect_activate_link(clone!(
                #[weak]
                obj,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, uri| {
                    let Ok(uri) = MatrixIdUri::parse(uri) else {
                        return glib::Propagation::Proceed;
                    };
                    let Some(room_details) = obj
                        .ancestor(RoomDetails::static_type())
                        .and_downcast::<RoomDetails>()
                    else {
                        return glib::Propagation::Proceed;
                    };
                    let Some(parent_window) = room_details.transient_for().and_downcast::<Window>()
                    else {
                        return glib::Propagation::Proceed;
                    };

                    parent_window.session_view().show_matrix_uri(uri);
                    room_details.close();

                    glib::Propagation::Stop
                }
            ));
        }

        fn dispose(&self) {
            self.disconnect_all();
        }
    }

    impl WidgetImpl for GeneralPage {
        /// Avoid the room topic rendering with all text selected by instead
        /// focusing on the next available widget.
        fn map(&self) {
            self.parent_map();

            if !self.room_topic.get_visible() {
                return;
            }

            // Call `grab_focus()` outside of `debug_assert!`, which is not
            // evaluated at all in release builds.
            let focused = if self.edit_details_btn.get_visible() {
                self.edit_details_btn.grab_focus()
            } else if self.direct_members_group.get_visible() {
                self.direct_members_list.grab_focus()
            } else if self.members_row_group.get_visible() {
                self.members_row.grab_focus()
            } else {
                error!("No widget to focus on besides the topic!");
                return;
            };
            debug_assert!(focused);
        }
    }
    impl PreferencesPageImpl for GeneralPage {}

    #[gtk::template_callbacks]
    impl GeneralPage {
        /// Set the presented room.
        #[allow(clippy::too_many_lines)]
        fn set_room(&self, room: &Room) {
            let obj = self.obj();

            let membership_handler = room.own_member().connect_membership_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_notifications();
                }
            ));
            self.membership_handler.replace(Some(membership_handler));

            let permissions_handler = room.permissions().connect_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_upgrade_button();
                    imp.update_edit_addresses_button();
                    imp.update_join_rule();
                    imp.update_guest_access();
                    imp.update_history_visibility();
                    imp.update_encryption();

                    spawn!(async move {
                        imp.update_publish().await;
                    });
                }
            ));
            self.permissions_handler.replace(Some(permissions_handler));

            let aliases = room.aliases();
            let canonical_alias_handler = aliases.connect_canonical_alias_string_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_addresses();
                }
            ));
            self.canonical_alias_handler
                .replace(Some(canonical_alias_handler));

            let alt_aliases_handler = aliases.alt_aliases_model().connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _, _, _| {
                    imp.update_addresses();
                }
            ));
            self.alt_aliases_handler.replace(Some(alt_aliases_handler));

            let join_rule_handler = room.join_rule().connect_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_join_rule();
                }
            ));
            self.join_rule_handler.replace(Some(join_rule_handler));

            let room_handler_ids = vec![
                room.connect_joined_members_count_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_members();
                    }
                )),
                room.connect_is_direct_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_members();
                    }
                )),
                room.connect_category_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_members();
                    }
                )),
                room.connect_notifications_setting_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_notifications();
                    }
                )),
                room.connect_is_tombstoned_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_upgrade_button();
                    }
                )),
                room.connect_guests_allowed_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_guest_access();
                    }
                )),
                room.connect_history_visibility_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_history_visibility();
                    }
                )),
                room.connect_is_encrypted_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_encryption();
                    }
                )),
            ];

            self.room.set(room, room_handler_ids);
            obj.notify_room();

            if let Some(session) = room.session() {
                let notifications_settings = session.notifications().settings();
                let notifications_settings_handlers = vec![
                    notifications_settings.connect_account_enabled_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_notifications();
                        }
                    )),
                    notifications_settings.connect_session_enabled_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_notifications();
                        }
                    )),
                ];

                self.notifications_settings_handlers
                    .replace(notifications_settings_handlers);
            }

            self.init_edit_details();
            self.update_members();
            self.update_notifications();
            self.update_edit_addresses_button();
            self.update_addresses();
            self.update_federated();
            self.update_join_rule();
            self.update_guest_access();
            self.update_publish_title();
            self.update_history_visibility();
            self.update_encryption();
            self.update_upgrade_button();

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.update_publish().await;
                }
            ));

            self.load_capabilities();
        }

        /// Set the lists of members in the room.
        fn set_members(&self, members: &MemberList) {
            self.members.set(Some(members));
            self.update_members();
        }

        /// The notifications setting for the room.
        fn notifications_setting(&self) -> NotificationsRoomSetting {
            self.room
                .obj()
                .map(|r| r.notifications_setting())
                .unwrap_or_default()
        }

        /// Set the notifications setting for the room.
        fn set_notifications_setting(&self, setting: NotificationsRoomSetting) {
            if self.notifications_setting() == setting {
                return;
            }

            self.notifications_setting_changed(setting);
        }

        /// Fetch the capabilities of the homeserver.
        fn load_capabilities(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };
            let client = room.matrix_room().client();

            spawn!(
                glib::Priority::LOW,
                clone!(
                    #[weak(rename_to = imp)]
                    self,
                    async move {
                        let handle = spawn_tokio!(async move {
                            client.homeserver_capabilities().room_versions().await
                        });
                        match handle.await.expect("task was not aborted") {
                            Ok(room_versions) => {
                                imp.supported_room_versions.replace(room_versions);
                            }
                            Err(error) => {
                                error!("Could not get supported room versions: {error}");
                                imp.supported_room_versions.take();
                            }
                        }

                        imp.update_upgrade_info();
                    }
                )
            );
        }

        /// Initialize the button to edit details.
        fn init_edit_details(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            // Hide edit button when the user cannot edit any detail or when the room is
            // direct.
            let permissions = room.permissions();
            let can_change_avatar = permissions.property_expression("can-change-avatar");
            let can_change_name = permissions.property_expression("can-change-name");
            let can_change_topic = permissions.property_expression("can-change-topic");

            let can_change_name_or_topic = expression::or(can_change_name, can_change_topic);
            let can_edit_at_least_one_detail =
                expression::or(can_change_name_or_topic, can_change_avatar);

            let is_direct_expr = room.property_expression("is-direct");

            let expr_watch = expression::and(
                expression::not(is_direct_expr),
                can_edit_at_least_one_detail,
            )
            .bind(&*self.edit_details_btn, "visible", gtk::Widget::NONE);
            self.expr_watch.replace(Some(expr_watch));
        }

        /// Update the members section.
        fn update_members(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };
            let Some(members) = self.members.upgrade() else {
                return;
            };

            let joined_members = members.membership_list(MembershipListKind::Join);
            let joined_members_count = joined_members.n_items();

            // When the room is direct there should only be 2 members in most cases, but use
            // the members count to make sure we do not show a list that is too long.
            let is_direct_with_few_members = room.is_direct() && joined_members_count < 5;
            if is_direct_with_few_members {
                // We don't use the count in the strings so we use separate gettext calls for
                // singular and plural rather than using ngettext.
                let title = if joined_members_count == 1 {
                    gettext("Member")
                } else {
                    gettext("Members")
                };
                self.direct_members_group.set_title(&title);

                // Set model of direct members list dynamically to avoid creating unnecessary
                // widgets in the background.
                if !self.direct_members_list_has_bound_model.get() {
                    self.direct_members_list
                        .bind_model(Some(&joined_members), |item| {
                            let member = item
                                .downcast_ref::<Member>()
                                .expect("joined members list contains members");
                            let member_row = MemberRow::new(false);
                            member_row.set_member(Some(member));

                            gtk::ListBoxRow::builder()
                                .selectable(false)
                                .child(&member_row)
                                .action_name("details.show-member")
                                .action_target(&member.user_id().as_str().to_variant())
                                .build()
                                .upcast()
                        });
                    self.direct_members_list_has_bound_model.set(true);
                }

                let has_members = joined_members_count > 0;
                self.direct_members_list.set_visible(has_members);
                self.no_direct_members_label.set_visible(!has_members);
            } else {
                let mut server_joined_members_count = room.joined_members_count();

                if room.category() == RoomCategory::Left {
                    // The number of joined members count from the homeserver is only updated when
                    // we are joined, so we must at least remove ourself from the count after we
                    // left.
                    server_joined_members_count = server_joined_members_count.saturating_sub(1);
                }

                // Use the maximum between the count of joined members in the local list, and
                // the one provided by the homeserver. The homeserver is usually right, except
                // when we just joined a room, where it will be 0 for a while.
                let joined_members_count =
                    server_joined_members_count.max(joined_members_count.into());
                self.members_row.set_count(joined_members_count.to_string());

                // We don't use the count in the strings so we use separate gettext calls for
                // singular and plural rather than using ngettext.
                let title = if joined_members_count == 1 {
                    gettext("Member")
                } else {
                    gettext("Members")
                };
                self.members_row.set_title(&title);

                if self.direct_members_list_has_bound_model.get() {
                    self.direct_members_list
                        .bind_model(None::<&gio::ListModel>, |_item| {
                            gtk::ListBoxRow::new().upcast()
                        });
                    self.direct_members_list_has_bound_model.set(false);
                }
            }

            self.direct_members_group
                .set_visible(is_direct_with_few_members);
            self.members_row_group
                .set_visible(!is_direct_with_few_members);
        }

        /// Disconnect all the signals.
        fn disconnect_all(&self) {
            if let Some(room) = self.room.obj() {
                if let Some(session) = room.session() {
                    for handler in self.notifications_settings_handlers.take() {
                        session.notifications().settings().disconnect(handler);
                    }
                }

                if let Some(handler) = self.membership_handler.take() {
                    room.own_member().disconnect(handler);
                }

                if let Some(handler) = self.permissions_handler.take() {
                    room.permissions().disconnect(handler);
                }

                let aliases = room.aliases();
                if let Some(handler) = self.canonical_alias_handler.take() {
                    aliases.disconnect(handler);
                }
                if let Some(handler) = self.alt_aliases_handler.take() {
                    aliases.alt_aliases_model().disconnect(handler);
                }

                if let Some(handler) = self.join_rule_handler.take() {
                    room.join_rule().disconnect(handler);
                }
            }

            self.room.disconnect_signals();

            if let Some(watch) = self.expr_watch.take() {
                watch.unwatch();
            }
        }

        /// Update the section about notifications.
        fn update_notifications(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            if !room.is_joined() {
                self.notifications.set_visible(false);
                return;
            }

            let Some(session) = room.session() else {
                return;
            };

            // Updates the active radio button.
            self.obj().notify_notifications_setting();

            let settings = session.notifications().settings();
            let sensitive = settings.account_enabled()
                && settings.session_enabled()
                && !self.notifications_loading.get();
            self.notifications.set_sensitive(sensitive);
            self.notifications.set_visible(true);
        }

        /// Update the loading state in the notifications section.
        fn set_notifications_loading(&self, loading: bool, setting: NotificationsRoomSetting) {
            // Only show the spinner on the selected one.
            self.notifications_global_row
                .set_is_loading(loading && setting == NotificationsRoomSetting::Global);
            self.notifications_all_row
                .set_is_loading(loading && setting == NotificationsRoomSetting::All);
            self.notifications_mentions_row
                .set_is_loading(loading && setting == NotificationsRoomSetting::MentionsOnly);
            self.notifications_mute_row
                .set_is_loading(loading && setting == NotificationsRoomSetting::Mute);

            self.notifications_loading.set(loading);
            self.obj().notify_notifications_loading();
        }

        /// Handle a change of the notifications setting.
        fn notifications_setting_changed(&self, setting: NotificationsRoomSetting) {
            let Some(room) = self.room.obj() else {
                return;
            };
            let Some(session) = room.session() else {
                return;
            };

            if setting == room.notifications_setting() {
                // Nothing to do.
                return;
            }

            self.notifications.set_sensitive(false);
            self.set_notifications_loading(true, setting);

            let settings = session.notifications().settings();
            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    if settings
                        .set_per_room_setting(room.room_id().to_owned(), setting)
                        .await
                        .is_err()
                    {
                        toast!(imp.obj(), gettext("Could not change notifications setting"));
                    }

                    imp.set_notifications_loading(false, setting);
                    imp.update_notifications();
                }
            ));
        }

        /// Update the button to edit addresses.
        fn update_edit_addresses_button(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let can_edit = room.is_joined()
                && room
                    .permissions()
                    .is_allowed_to(PowerLevelAction::SendState(StateEventType::RoomPowerLevels));
            self.edit_addresses_button.set_visible(can_edit);
        }

        /// Update the addresses group.
        fn update_addresses(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };
            let aliases = room.aliases();

            let canonical_alias_string = aliases.canonical_alias_string();
            let has_canonical_alias = canonical_alias_string.is_some();

            if let Some(canonical_alias_string) = canonical_alias_string {
                let mut row_borrow = self.canonical_alias_row.borrow_mut();
                let row = row_borrow.get_or_insert_with(|| {
                    // We want the main alias always at the top but cannot add a row at the top so
                    // we have to remove the other rows first.
                    self.remove_alt_aliases_rows();

                    let row = CopyableRow::new();
                    row.set_copy_button_tooltip_text(Some(gettext("Copy address")));
                    row.set_toast_text(Some(gettext("Address copied to clipboard")));

                    // Mark the main alias with a tag.
                    let label = gtk::Label::builder()
                        .label(gettext("Main Address"))
                        .ellipsize(pango::EllipsizeMode::End)
                        .css_classes(["public-address-tag"])
                        .valign(gtk::Align::Center)
                        .build();
                    row.update_relation(&[gtk::accessible::Relation::DescribedBy(&[
                        label.upcast_ref()
                    ])]);
                    row.set_extra_suffix(Some(label));

                    self.addresses_group.add(&row);

                    row
                });

                row.set_title(&canonical_alias_string);
            } else if let Some(row) = self.canonical_alias_row.take() {
                self.addresses_group.remove(&row);
            }

            let alt_aliases = aliases.alt_aliases_model();
            let alt_aliases_count = alt_aliases.n_items() as usize;
            if alt_aliases_count == 0 {
                self.remove_alt_aliases_rows();
            } else {
                let mut rows = self.alt_aliases_rows.borrow_mut();

                for (pos, alt_alias) in alt_aliases.iter::<glib::Object>().enumerate() {
                    let Some(alt_alias) = alt_alias.ok().and_downcast::<gtk::StringObject>() else {
                        break;
                    };

                    let row = rows.get(pos).cloned().unwrap_or_else(|| {
                        let row = CopyableRow::new();
                        row.set_copy_button_tooltip_text(Some(gettext("Copy address")));
                        row.set_toast_text(Some(gettext("Address copied to clipboard")));

                        self.addresses_group.add(&row);
                        rows.push(row.clone());

                        row
                    });

                    row.set_title(&alt_alias.string());
                }

                let rows_count = rows.len();
                if alt_aliases_count < rows_count {
                    for _ in alt_aliases_count..rows_count {
                        if let Some(row) = rows.pop() {
                            self.addresses_group.remove(&row);
                        }
                    }
                }
            }

            self.no_addresses_label
                .set_visible(!has_canonical_alias && alt_aliases_count == 0);
        }

        fn remove_alt_aliases_rows(&self) {
            for row in self.alt_aliases_rows.take() {
                self.addresses_group.remove(&row);
            }
        }

        /// Copy the room's permalink to the clipboard.
        #[template_callback]
        async fn copy_permalink(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let permalink = room.matrix_to_uri().await;

            let obj = self.obj();
            obj.clipboard().set_text(&permalink.to_string());
            toast!(obj, gettext("Room link copied to clipboard"));
        }

        /// Update the join rule row.
        fn update_join_rule(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let can_change = room.join_rule().value().can_be_edited()
                && room
                    .permissions()
                    .is_allowed_to(PowerLevelAction::SendState(StateEventType::RoomJoinRules));
            self.join_rule.set_activatable(can_change);
        }

        /// Update the guest access row.
        fn update_guest_access(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let row = &self.guest_access;
            row.set_is_active(room.guests_allowed());
            row.set_is_loading(false);

            let can_change = room
                .permissions()
                .is_allowed_to(PowerLevelAction::SendState(StateEventType::RoomGuestAccess));
            row.set_read_only(!can_change);
        }

        /// Toggle the guest access.
        #[template_callback]
        async fn toggle_guest_access(&self) {
            let Some(room) = self.room.obj() else { return };

            let row = &self.guest_access;
            let guests_allowed = row.is_active();

            if room.guests_allowed() == guests_allowed {
                return;
            }

            row.set_is_loading(true);
            row.set_read_only(true);

            let guest_access = if guests_allowed {
                GuestAccess::CanJoin
            } else {
                GuestAccess::Forbidden
            };
            let content = RoomGuestAccessEventContent::new(guest_access);

            let matrix_room = room.matrix_room().clone();
            let handle = spawn_tokio!(async move { matrix_room.send_state_event(content).await });

            if let Err(error) = handle.await.unwrap() {
                error!("Could not change guest access: {error}");
                toast!(self.obj(), gettext("Could not change guest access"));
                self.update_guest_access();
            }
        }

        /// Update the title of the publish row.
        fn update_publish_title(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let own_member = room.own_member();
            let server_name = own_member.user_id().server_name();

            let title = gettext_f(
                // Translators: Do NOT translate the content between '{' and '}',
                // this is a variable name.
                "Publish in the {homeserver} directory",
                &[("homeserver", server_name.as_str())],
            );
            self.publish.set_title(&title);
        }

        /// Update the publish row.
        async fn update_publish(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let row = &self.publish;

            // There is no clear definition of who is allowed to publish a room to the
            // directory in the Matrix spec. Let's assume it doesn't make sense unless the
            // user can change the public addresses.
            let can_change = room
                .permissions()
                .is_allowed_to(PowerLevelAction::SendState(
                    StateEventType::RoomCanonicalAlias,
                ));
            row.set_read_only(!can_change);

            let matrix_room = room.matrix_room();
            let client = matrix_room.client();
            let request = get_room_visibility::v3::Request::new(matrix_room.room_id().to_owned());

            let handle = spawn_tokio!(async move { client.send(request).await });

            match handle.await.unwrap() {
                Ok(response) => {
                    let is_published = response.visibility == Visibility::Public;
                    self.is_published.set(is_published);
                    row.set_is_active(is_published);
                }
                Err(error) => {
                    error!("Could not get directory visibility of room: {error}");
                }
            }

            row.set_is_loading(false);
        }

        /// Toggle whether the room is published in the room directory.
        #[template_callback]
        async fn toggle_publish(&self) {
            let Some(room) = self.room.obj() else { return };

            let row = &self.publish;
            let publish = row.is_active();

            if self.is_published.get() == publish {
                return;
            }

            row.set_is_loading(true);
            row.set_read_only(true);

            let visibility = if publish {
                Visibility::Public
            } else {
                Visibility::Private
            };

            let matrix_room = room.matrix_room();
            let client = matrix_room.client();
            let request =
                set_room_visibility::v3::Request::new(matrix_room.room_id().to_owned(), visibility);

            let handle = spawn_tokio!(async move { client.send(request).await });

            if let Err(error) = handle.await.unwrap() {
                error!("Could not change directory visibility of room: {error}");
                let text = if publish {
                    gettext("Could not publish room in directory")
                } else {
                    gettext("Could not unpublish room from directory")
                };
                toast!(self.obj(), text);
            }

            self.update_publish().await;
        }

        /// Update the history visibility row.
        fn update_history_visibility(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let history_visibility = room.history_visibility();

            let text = match history_visibility {
                HistoryVisibilityValue::WorldReadable => {
                    gettext("Anyone, even if they are not in the room")
                }
                HistoryVisibilityValue::Shared => {
                    gettext("Members only, since this option was selected")
                }
                HistoryVisibilityValue::Invited => gettext("Members only, since they were invited"),
                HistoryVisibilityValue::Joined => {
                    gettext("Members only, since they joined the room")
                }
                HistoryVisibilityValue::Unsupported => gettext("Unsupported rule"),
            };
            self.history_visibility.set_subtitle(&text);

            let can_change = history_visibility != HistoryVisibilityValue::Unsupported
                && room
                    .permissions()
                    .is_allowed_to(PowerLevelAction::SendState(
                        StateEventType::RoomHistoryVisibility,
                    ));

            self.history_visibility.set_activatable(can_change);
        }

        /// Update the encryption row.
        fn update_encryption(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let row = &self.encryption;
            row.set_is_loading(false);

            let is_encrypted = room.is_encrypted();
            row.set_is_active(is_encrypted);

            let can_change = !is_encrypted
                && room
                    .permissions()
                    .is_allowed_to(PowerLevelAction::SendState(StateEventType::RoomEncryption));
            row.set_read_only(!can_change);
        }

        /// Enable encryption in the room.
        #[template_callback]
        async fn enable_encryption(&self) {
            let Some(room) = self.room.obj() else { return };

            let row = &self.encryption;

            if room.is_encrypted() || !row.is_active() {
                // Nothing to do.
                return;
            }

            row.set_is_loading(true);
            row.set_read_only(true);

            // Ask for confirmation.
            let dialog = adw::AlertDialog::builder()
                        .heading(gettext("Enable Encryption?"))
                        .body(gettext("Enabling encryption will prevent new members to read the history before they arrived. This cannot be disabled later."))
                        .default_response("cancel")
                        .build();
            dialog.add_responses(&[
                ("cancel", &gettext("Cancel")),
                ("enable", &gettext("Enable")),
            ]);
            dialog.set_response_appearance("enable", adw::ResponseAppearance::Destructive);

            let obj = self.obj();
            if dialog.choose_future(Some(&*obj)).await != "enable" {
                self.update_encryption();
                return;
            }

            if room.enable_encryption().await.is_err() {
                toast!(obj, gettext("Could not enable encryption"));
                self.update_encryption();
            }
        }

        /// Update the room upgrade info.
        fn update_upgrade_info(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let room_info = room.matrix_room().clone_info();

            let upgrade_info = room_info.create().map(|create_content| {
                let privileged_creators = room_info
                    .room_version_rules_or_default()
                    .authorization
                    .explicitly_privilege_room_creators
                    .then(|| room_info.creators())
                    .flatten();

                UpgradeInfo::new(room.join_rule().value())
                    .with_room_versions(
                        &create_content.room_version,
                        &self.supported_room_versions.borrow(),
                    )
                    .with_privileged_creators(
                        room.own_member().user_id(),
                        &privileged_creators.unwrap_or_default(),
                    )
            });

            self.upgrade_info.replace(upgrade_info);

            self.update_upgrade_button();
        }

        /// Update the room upgrade button.
        fn update_upgrade_button(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let can_upgrade = !room.is_direct()
                && !room.is_tombstoned()
                && room
                    .permissions()
                    .is_allowed_to(PowerLevelAction::SendState(StateEventType::RoomTombstone))
                && self.upgrade_info.borrow().is_some();
            self.upgrade_button.set_visible(can_upgrade);
        }

        /// Update the room federation row.
        fn update_federated(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let subtitle = if room.federated() {
                // Translators: As in, 'Room federated'.
                gettext("Federated")
            } else {
                // Translators: As in, 'Room not federated'.
                gettext("Not federated")
            };

            self.room_federated.set_subtitle(&subtitle);
        }

        /// Upgrade the room to a new version.
        #[template_callback]
        async fn upgrade(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };
            let Some(upgrade_info) = self.upgrade_info.borrow().clone() else {
                return;
            };

            let obj = self.obj();
            self.upgrade_button.set_is_loading(true);

            let Some(new_version) = UpgradeDialog::new()
                .confirm_upgrade(&upgrade_info, &*obj)
                .await
            else {
                self.upgrade_button.set_is_loading(false);
                return;
            };

            let client = room.matrix_room().client();
            let request = upgrade_room::v3::Request::new(room.room_id().to_owned(), new_version);

            let handle = spawn_tokio!(async move { client.send(request).await });

            match handle.await.unwrap() {
                Ok(_) => {
                    toast!(obj, gettext("Room upgraded successfully"));
                }
                Err(error) => {
                    error!("Could not upgrade room: {error}");
                    toast!(obj, gettext("Could not upgrade room"));
                    self.upgrade_button.set_is_loading(false);
                }
            }
        }
    }
}

glib::wrapper! {
    /// Preference Window to display and update room details.
    pub struct GeneralPage(ObjectSubclass<imp::GeneralPage>)
        @extends gtk::Widget, adw::PreferencesPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl GeneralPage {
    pub fn new(room: &Room, members: &MemberList) -> Self {
        glib::Object::builder()
            .property("room", room)
            .property("members", members)
            .build()
    }
}
