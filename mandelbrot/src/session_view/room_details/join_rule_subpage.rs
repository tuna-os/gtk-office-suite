use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use ruma::events::{
    StateEventType,
    room::{join_rules::JoinRule as MatrixJoinRule, power_levels::PowerLevelAction},
};

use crate::{
    components::{CheckLoadingRow, LoadingButton, UnsavedChangesResponse, unsaved_changes_dialog},
    session::{JoinRuleValue, Room},
    toast,
};

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/join_rule_subpage.ui"
    )]
    #[properties(wrapper_type = super::JoinRuleSubpage)]
    pub struct JoinRuleSubpage {
        #[template_child]
        save_button: TemplateChild<LoadingButton>,
        #[template_child]
        info_box: TemplateChild<gtk::Box>,
        #[template_child]
        info_image: TemplateChild<gtk::Image>,
        #[template_child]
        info_description: TemplateChild<gtk::Label>,
        #[template_child]
        knock_box: TemplateChild<gtk::ListBox>,
        #[template_child]
        knock_row: TemplateChild<adw::SwitchRow>,
        /// The presented room.
        #[property(get, set = Self::set_room, explicit_notify, nullable)]
        room: glib::WeakRef<Room>,
        /// The local value of the join rule.
        #[property(get, set = Self::set_local_value, explicit_notify, builder(JoinRuleValue::default()))]
        local_value: Cell<JoinRuleValue>,
        /// Whether the join rule was changed by the user.
        #[property(get)]
        changed: Cell<bool>,
        permissions_handler: RefCell<Option<glib::SignalHandlerId>>,
        join_rule_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for JoinRuleSubpage {
        const NAME: &'static str = "RoomDetailsJoinRuleSubpage";
        type Type = super::JoinRuleSubpage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            CheckLoadingRow::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.install_property_action("join-rule.set-value", "local-value");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for JoinRuleSubpage {
        fn dispose(&self) {
            self.disconnect_signals();
        }
    }

    impl WidgetImpl for JoinRuleSubpage {}
    impl NavigationPageImpl for JoinRuleSubpage {}

    #[gtk::template_callbacks]
    impl JoinRuleSubpage {
        /// Set the presented room.
        fn set_room(&self, room: Option<&Room>) {
            let Some(room) = room else {
                // Just ignore when room is missing.
                return;
            };

            self.disconnect_signals();

            let permissions_handler = room.permissions().connect_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update();
                }
            ));
            self.permissions_handler.replace(Some(permissions_handler));

            let join_rule_handler = room.join_rule().connect_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update();
                }
            ));
            self.join_rule_handler.replace(Some(join_rule_handler));

            let supports_knocking = room.rules().authorization.knocking;
            if !supports_knocking {
                self.info_description.set_label(&gettext("The version of this room does not support all possibilities. Upgrade this room to the latest version to see more options."));
                self.info_image.set_icon_name(Some("about-symbolic"));
            }

            self.info_box.set_visible(!supports_knocking);
            self.knock_box.set_visible(supports_knocking);

            self.room.set(Some(room));

            self.update();
            self.obj().notify_room();
        }

        /// Update the subpage.
        fn update(&self) {
            let Some(room) = self.room.upgrade() else {
                return;
            };

            let join_rule = room.join_rule();
            self.set_local_value(join_rule.value());
            self.knock_row.set_active(join_rule.can_knock());

            self.save_button.set_is_loading(false);
            self.update_changed();
        }

        /// Set the local value of the join rule.
        fn set_local_value(&self, value: JoinRuleValue) {
            if self.local_value.get() == value {
                return;
            }

            self.local_value.set(value);

            let can_knock = matches!(value, JoinRuleValue::Invite | JoinRuleValue::RoomMembership);
            self.knock_box.set_sensitive(can_knock);

            self.update_changed();
            self.obj().notify_local_value();
        }

        /// Whether we can change the join rule.
        fn can_change(&self) -> bool {
            let Some(room) = self.room.upgrade() else {
                return false;
            };

            if !room.join_rule().value().can_be_edited() {
                return false;
            }

            room.permissions()
                .is_allowed_to(PowerLevelAction::SendState(StateEventType::RoomJoinRules))
        }

        /// Whether users can request invites.
        fn can_knock(&self) -> bool {
            self.knock_box.is_visible()
                && self.knock_box.is_sensitive()
                && self.knock_row.is_active()
        }

        /// Compute the new join rule from the current state.
        fn new_join_rule(&self) -> MatrixJoinRule {
            match self.local_value.get() {
                JoinRuleValue::Invite => {
                    if self.can_knock() {
                        MatrixJoinRule::Knock
                    } else {
                        MatrixJoinRule::Invite
                    }
                }
                JoinRuleValue::Public => MatrixJoinRule::Public,
                _ => unimplemented!(),
            }
        }

        /// Update whether the join rule was changed by the user.
        #[template_callback]
        fn update_changed(&self) {
            let Some(room) = self.room.upgrade() else {
                return;
            };

            let changed = if self.can_change() {
                let current_join_rule = room
                    .join_rule()
                    .matrix_join_rule()
                    .unwrap_or(MatrixJoinRule::Invite);
                let new_join_rule = self.new_join_rule();

                current_join_rule != new_join_rule
            } else {
                false
            };

            self.changed.set(changed);
            self.obj().notify_changed();
        }

        /// Save the changes of this page.
        #[template_callback]
        async fn save(&self) {
            if !self.changed.get() {
                // Nothing to do.
                return;
            }

            let Some(room) = self.room.upgrade() else {
                return;
            };

            self.save_button.set_is_loading(true);

            let rule = self.new_join_rule();

            if room.join_rule().set_matrix_join_rule(rule).await.is_err() {
                toast!(self.obj(), gettext("Could not change who can join"));
                self.save_button.set_is_loading(false);
            }
        }

        /// Go back to the previous page in the room details.
        ///
        /// If there are changes in the page, ask the user to confirm.
        #[template_callback]
        async fn go_back(&self) {
            let obj = self.obj();
            let mut reset_after = false;

            if self.changed.get() {
                match unsaved_changes_dialog(&*obj).await {
                    UnsavedChangesResponse::Save => self.save().await,
                    UnsavedChangesResponse::Discard => reset_after = true,
                    UnsavedChangesResponse::Cancel => return,
                }
            }

            let _ = obj.activate_action("navigation.pop", None);

            if reset_after {
                self.update();
            }
        }

        /// Disconnect all the signal handlers.
        fn disconnect_signals(&self) {
            if let Some(room) = self.room.upgrade() {
                if let Some(handler) = self.permissions_handler.take() {
                    room.permissions().disconnect(handler);
                }

                if let Some(handler) = self.join_rule_handler.take() {
                    room.join_rule().disconnect(handler);
                }
            }
        }
    }
}

glib::wrapper! {
    /// Subpage to select the join rule of a room.
    pub struct JoinRuleSubpage(ObjectSubclass<imp::JoinRuleSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl JoinRuleSubpage {
    /// Construct a new `JoinRuleSubpage` for the given room.
    pub fn new(room: &Room) -> Self {
        glib::Object::builder().property("room", room).build()
    }
}
