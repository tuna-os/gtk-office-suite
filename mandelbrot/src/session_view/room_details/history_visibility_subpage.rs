use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use ruma::events::{
    StateEventType,
    room::{history_visibility::RoomHistoryVisibilityEventContent, power_levels::PowerLevelAction},
};
use tracing::error;

use crate::{
    components::{CheckLoadingRow, LoadingButton, UnsavedChangesResponse, unsaved_changes_dialog},
    session::{HistoryVisibilityValue, Room},
    spawn_tokio, toast,
    utils::BoundObjectWeakRef,
};

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/history_visibility_subpage.ui"
    )]
    #[properties(wrapper_type = super::HistoryVisibilitySubpage)]
    pub struct HistoryVisibilitySubpage {
        #[template_child]
        save_button: TemplateChild<LoadingButton>,
        /// The presented room.
        #[property(get, set = Self::set_room, explicit_notify, nullable)]
        room: BoundObjectWeakRef<Room>,
        /// The local value of the history visibility.
        #[property(get, set = Self::set_local_value, explicit_notify, builder(HistoryVisibilityValue::default()))]
        local_value: Cell<HistoryVisibilityValue>,
        /// Whether the history visibility was changed by the user.
        #[property(get)]
        changed: Cell<bool>,
        permissions_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for HistoryVisibilitySubpage {
        const NAME: &'static str = "RoomDetailsHistoryVisibilitySubpage";
        type Type = super::HistoryVisibilitySubpage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            CheckLoadingRow::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.install_property_action("history-visibility.set-value", "local-value");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for HistoryVisibilitySubpage {
        fn dispose(&self) {
            self.disconnect_signals();
        }
    }

    impl WidgetImpl for HistoryVisibilitySubpage {}
    impl NavigationPageImpl for HistoryVisibilitySubpage {}

    #[gtk::template_callbacks]
    impl HistoryVisibilitySubpage {
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

            let history_visibility_handler = room.connect_history_visibility_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update();
                }
            ));

            self.room.set(room, vec![history_visibility_handler]);

            self.update();
            self.obj().notify_room();
        }

        /// Update the subpage.
        fn update(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            self.set_local_value(room.history_visibility());

            self.save_button.set_is_loading(false);
            self.update_changed();
        }

        /// Set the local value of the history visibility.
        fn set_local_value(&self, value: HistoryVisibilityValue) {
            if self.local_value.get() == value {
                return;
            }

            self.local_value.set(value);

            self.update_changed();
            self.obj().notify_local_value();
        }

        /// Whether we can change the history visibility.
        fn can_change(&self) -> bool {
            let Some(room) = self.room.obj() else {
                return false;
            };

            if room.history_visibility() == HistoryVisibilityValue::Unsupported {
                return false;
            }

            room.permissions()
                .is_allowed_to(PowerLevelAction::SendState(
                    StateEventType::RoomHistoryVisibility,
                ))
        }

        /// Update whether the join rule was changed by the user.
        #[template_callback]
        fn update_changed(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let changed = if self.can_change() {
                let current_join_rule = room.history_visibility();
                let new_join_rule = self.local_value.get();

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

            let Some(room) = self.room.obj() else {
                return;
            };

            self.save_button.set_is_loading(true);

            let content = RoomHistoryVisibilityEventContent::new(self.local_value.get().into());

            let matrix_room = room.matrix_room().clone();
            let handle = spawn_tokio!(async move { matrix_room.send_state_event(content).await });

            if let Err(error) = handle.await.unwrap() {
                error!("Could not change room history visibility: {error}");
                toast!(self.obj(), gettext("Could not change who can read history"));
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
            if let Some(room) = self.room.obj()
                && let Some(handler) = self.permissions_handler.take()
            {
                room.permissions().disconnect(handler);
            }

            self.room.disconnect_signals();
        }
    }
}

glib::wrapper! {
    /// Subpage to select the history visibility of a room.
    pub struct HistoryVisibilitySubpage(ObjectSubclass<imp::HistoryVisibilitySubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl HistoryVisibilitySubpage {
    /// Construct a new `HistoryVisibilitySubpage` for the given room.
    pub fn new(room: &Room) -> Self {
        glib::Object::builder().property("room", room).build()
    }
}
