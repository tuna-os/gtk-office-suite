use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gio, glib, glib::clone};
use matrix_sdk::RoomState;
use ruma::{OwnedMxcUri, assign, events::room::avatar::ImageInfo};
use tracing::error;

use crate::{
    components::{
        ActionButton, ActionState, AvatarData, AvatarImage, EditableAvatar, LoadingButton,
        UnsavedChangesResponse, unsaved_changes_dialog,
    },
    prelude::*,
    session::Room,
    spawn_tokio, toast,
    utils::{
        BoundObjectWeakRef, OngoingAsyncAction, TemplateCallbacks,
        media::{FileInfo, image::ImageInfoLoader},
    },
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/edit_details_subpage.ui"
    )]
    #[properties(wrapper_type = super::EditDetailsSubpage)]
    pub struct EditDetailsSubpage {
        #[template_child]
        avatar: TemplateChild<EditableAvatar>,
        #[template_child]
        name_entry_row: TemplateChild<adw::EntryRow>,
        #[template_child]
        name_button: TemplateChild<ActionButton>,
        #[template_child]
        topic_text_view: TemplateChild<gtk::TextView>,
        #[template_child]
        topic_buffer: TemplateChild<gtk::TextBuffer>,
        #[template_child]
        save_topic_button: TemplateChild<LoadingButton>,
        /// The presented room.
        #[property(get, set = Self::set_room, explicit_notify, nullable)]
        room: BoundObjectWeakRef<Room>,
        changing_avatar: RefCell<Option<OngoingAsyncAction<OwnedMxcUri>>>,
        changing_name: RefCell<Option<OngoingAsyncAction<String>>>,
        changing_topic: RefCell<Option<OngoingAsyncAction<String>>>,
        expr_watch: RefCell<Option<gtk::ExpressionWatch>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EditDetailsSubpage {
        const NAME: &'static str = "RoomDetailsEditDetailsSubpage";
        type Type = super::EditDetailsSubpage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
            TemplateCallbacks::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for EditDetailsSubpage {
        fn dispose(&self) {
            self.disconnect_all();
        }
    }

    impl WidgetImpl for EditDetailsSubpage {}
    impl NavigationPageImpl for EditDetailsSubpage {}

    #[gtk::template_callbacks]
    impl EditDetailsSubpage {
        /// Set the presented room.
        fn set_room(&self, room: Option<&Room>) {
            let Some(room) = room else {
                // Just ignore when room is missing.
                return;
            };

            self.disconnect_all();

            let avatar_data = room.avatar_data();
            let expr_watch = AvatarData::this_expression("image")
                .chain_property::<AvatarImage>("uri-string")
                .watch(
                    Some(&avatar_data),
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        #[weak]
                        avatar_data,
                        move || {
                            imp.avatar_changed(avatar_data.image().and_then(|i| i.uri()).as_ref());
                        }
                    ),
                );
            self.expr_watch.replace(Some(expr_watch));

            let name_handler = room.connect_name_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |room| {
                    imp.name_changed(room.name().as_deref());
                }
            ));
            let topic_handler = room.connect_topic_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |room| {
                    imp.topic_changed(room.topic().as_deref());
                }
            ));

            self.room.set(room, vec![name_handler, topic_handler]);
            self.obj().notify_room();
        }

        /// Handle when we receive an avatar URI change from the homeserver.
        fn avatar_changed(&self, uri: Option<&OwnedMxcUri>) {
            if let Some(action) = self.changing_avatar.borrow().as_ref() {
                if uri != action.as_value() {
                    // This is not the change we expected, maybe another device did a change too.
                    // Let's wait for another change.
                    return;
                }
            } else {
                // No action is ongoing, we don't need to do anything.
                return;
            }

            // Reset the state.
            self.changing_avatar.take();
            self.avatar.success();

            let obj = self.obj();
            if uri.is_none() {
                toast!(obj, gettext("Avatar removed successfully"));
            } else {
                toast!(obj, gettext("Avatar changed successfully"));
            }
        }

        /// Change the avatar.
        #[template_callback]
        async fn change_avatar(&self, file: gio::File) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let matrix_room = room.matrix_room();
            if matrix_room.state() != RoomState::Joined {
                error!("Cannot change avatar of room not joined");
                return;
            }

            let obj = self.obj();
            let avatar = &self.avatar;
            avatar.edit_in_progress();

            let info = match FileInfo::try_from_file(&file).await {
                Ok(info) => info,
                Err(error) => {
                    error!("Could not load room avatar file info: {error}");
                    toast!(obj, gettext("Could not load file"));
                    avatar.reset();
                    return;
                }
            };

            let data = match file.load_contents_future().await {
                Ok((data, _)) => data,
                Err(error) => {
                    error!("Could not load room avatar file: {error}");
                    toast!(obj, gettext("Could not load file"));
                    avatar.reset();
                    return;
                }
            };

            let base_image_info = ImageInfoLoader::from(file).load_info().await;
            let image_info = assign!(ImageInfo::new(), {
                width: base_image_info.width,
                height: base_image_info.height,
                size: info.size.map(Into::into),
                mimetype: Some(info.mime.to_string()),
            });

            let Some(session) = room.session() else {
                return;
            };
            let client = session.client();
            let handle =
                spawn_tokio!(
                    async move { client.media().upload(&info.mime, data.into(), None).await }
                );

            let uri = match handle.await.unwrap() {
                Ok(res) => res.content_uri,
                Err(error) => {
                    error!("Could not upload room avatar: {error}");
                    toast!(obj, gettext("Could not upload avatar"));
                    avatar.reset();
                    return;
                }
            };

            let (action, weak_action) = OngoingAsyncAction::set(uri.clone());
            self.changing_avatar.replace(Some(action));

            let matrix_room = matrix_room.clone();
            let handle =
                spawn_tokio!(
                    async move { matrix_room.set_avatar_url(&uri, Some(image_info)).await }
                );

            // We don't need to handle the success of the request, we should receive the
            // change via sync.
            if let Err(error) = handle.await.unwrap() {
                // Because this action can finish in avatar_changed, we must only act if this is
                // still the current action.
                if weak_action.is_ongoing() {
                    self.changing_avatar.take();
                    error!("Could not change room avatar: {error}");
                    toast!(obj, gettext("Could not change avatar"));
                    avatar.reset();
                }
            }
        }

        /// Remove the avatar.
        #[template_callback]
        async fn remove_avatar(&self) {
            let Some(room) = self.room.obj() else {
                error!("Cannot remove avatar with missing room");
                return;
            };

            let matrix_room = room.matrix_room();
            if matrix_room.state() != RoomState::Joined {
                error!("Cannot remove avatar of room not joined");
                return;
            }

            let obj = self.obj();

            // Ask for confirmation.
            let confirm_dialog = adw::AlertDialog::builder()
                .default_response("cancel")
                .heading(gettext("Remove Avatar?"))
                .body(gettext(
                    "Do you really want to remove the avatar for this room?",
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

            let avatar = &self.avatar;
            avatar.removal_in_progress();

            let (action, weak_action) = OngoingAsyncAction::remove();
            self.changing_avatar.replace(Some(action));

            let matrix_room = matrix_room.clone();
            let handle = spawn_tokio!(async move { matrix_room.remove_avatar().await });

            // We don't need to handle the success of the request, we should receive the
            // change via sync.
            if let Err(error) = handle.await.unwrap() {
                // Because this action can finish in avatar_changed, we must only act if this is
                // still the current action.
                if weak_action.is_ongoing() {
                    self.changing_avatar.take();
                    error!("Could not remove room avatar: {error}");
                    toast!(obj, gettext("Could not remove avatar"));
                    avatar.reset();
                }
            }
        }

        /// Reset the name entry and button.
        fn reset_name(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            self.name_entry_row
                .set_sensitive(room.permissions().can_change_name());
            self.name_entry_row
                .set_text(&room.name().unwrap_or_default());
            self.name_button.set_visible(false);
            self.name_button.set_state(ActionState::Confirm);
        }

        /// Handle when we receive a name change from the homeserver.
        fn name_changed(&self, name: Option<&str>) {
            if let Some(action) = self.changing_name.borrow().as_ref() {
                if name != action.as_value().map(String::as_str) {
                    // This is not the change we expected, maybe another device did a change too.
                    // Let's wait for another change.
                    return;
                }
            } else {
                // No action is ongoing, we don't need to do anything.
                return;
            }

            toast!(self.obj(), gettext("Room name saved successfully"));

            // Reset state.
            self.changing_name.take();
            self.reset_name();
        }

        /// Whether the room name was edited.
        fn was_name_edited(&self) -> bool {
            let Some(room) = self.room.obj() else {
                return false;
            };

            let text = Some(self.name_entry_row.text()).filter(|t| !t.is_empty());
            // Do not send text if it has just more whitespaces at the beginning or end.
            let trimmed_text = text.as_deref().map(str::trim).filter(|t| !t.is_empty());

            let name = room.name();
            name.as_deref() != text.as_deref() && name.as_deref() != trimmed_text
        }

        /// Handle when the name was edited in the entry.
        #[template_callback]
        fn name_edited(&self) {
            self.name_button.set_visible(self.was_name_edited());
        }

        /// Change the name of the room.
        #[template_callback]
        async fn change_name(&self) {
            if !self.was_name_edited() {
                // No change to send.
                return;
            }

            let Some(room) = self.room.obj() else {
                return;
            };

            let matrix_room = room.matrix_room().clone();
            if matrix_room.state() != RoomState::Joined {
                error!("Cannot change name of room not joined");
                return;
            }

            self.name_entry_row.set_sensitive(false);
            self.name_button.set_state(ActionState::Loading);

            // Trim whitespaces.
            let name = Some(self.name_entry_row.text().trim())
                .filter(|t| !t.is_empty())
                .map(ToOwned::to_owned);

            let (action, weak_action) = if let Some(name) = name.clone() {
                OngoingAsyncAction::set(name)
            } else {
                OngoingAsyncAction::remove()
            };
            self.changing_name.replace(Some(action));

            let handle =
                spawn_tokio!(async move { matrix_room.set_name(name.unwrap_or_default()).await });

            // We don't need to handle the success of the request, we should receive the
            // change via sync.
            if let Err(error) = handle.await.unwrap() {
                // Because this action can finish in name_changed, we must only act if this is
                // still the current action.
                if weak_action.is_ongoing() {
                    self.changing_name.take();
                    error!("Could not change room name: {error}");
                    toast!(self.obj(), gettext("Could not change room name"));
                    self.name_entry_row.set_sensitive(true);
                    self.name_button.set_state(ActionState::Retry);
                }
            }
        }

        /// Reset the topic text view and button.
        fn reset_topic(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            self.topic_text_view
                .set_sensitive(room.permissions().can_change_topic());
            self.topic_buffer
                .set_text(&room.topic().unwrap_or_default());
            self.save_topic_button.set_is_loading(false);
            self.save_topic_button.set_sensitive(false);
        }

        /// Handle when we receive a topic change from the homeserver.
        fn topic_changed(&self, topic: Option<&str>) {
            // It is not possible to remove a topic so we process the empty string as
            // `None`. We need to cancel that here.
            let topic = topic.unwrap_or_default();

            if let Some(action) = self.changing_topic.borrow().as_ref() {
                if Some(topic) != action.as_value().map(String::as_str) {
                    // This is not the change we expected, maybe another device did a change too.
                    // Let's wait for another change.
                    return;
                }
            } else {
                // No action is ongoing, we don't need to do anything.
                return;
            }

            toast!(self.obj(), gettext("Room description saved successfully"));

            // Reset state.
            self.changing_topic.take();
            self.reset_topic();
        }

        /// Whether the room topic was edited.
        fn was_topic_edited(&self) -> bool {
            let Some(room) = self.room.obj() else {
                return false;
            };

            let (start_iter, end_iter) = self.topic_buffer.bounds();
            let text = Some(self.topic_buffer.text(&start_iter, &end_iter, false))
                .filter(|t| !t.is_empty());
            // Do not send text if it has just more whitespaces at the beginning or end.
            let trimmed_text = text.as_deref().map(str::trim).filter(|t| !t.is_empty());

            let topic = room.topic();
            topic.as_deref() != text.as_deref() && topic.as_deref() != trimmed_text
        }

        /// Handle when the topic was edited in the text view.
        #[template_callback]
        fn topic_edited(&self) {
            self.save_topic_button
                .set_sensitive(self.was_topic_edited());
        }

        /// Change the topic of the room.
        #[template_callback]
        async fn change_topic(&self) {
            if !self.was_topic_edited() {
                // No change to send.
                return;
            }

            let Some(room) = self.room.obj() else {
                return;
            };

            let matrix_room = room.matrix_room().clone();
            if matrix_room.state() != RoomState::Joined {
                error!("Cannot change description of room not joined");
                return;
            }

            self.topic_text_view.set_sensitive(false);
            self.save_topic_button.set_is_loading(true);

            // Trim whitespaces.
            let (start_iter, end_iter) = self.topic_buffer.bounds();
            let topic = Some(self.topic_buffer.text(&start_iter, &end_iter, false).trim())
                .filter(|t| !t.is_empty())
                .map(ToOwned::to_owned);

            let (action, weak_action) = if let Some(topic) = topic.clone() {
                OngoingAsyncAction::set(topic)
            } else {
                OngoingAsyncAction::remove()
            };
            self.changing_topic.replace(Some(action));

            let handle = spawn_tokio!(async move {
                matrix_room.set_room_topic(&topic.unwrap_or_default()).await
            });

            // We don't need to handle the success of the request, we should receive the
            // change via sync.
            if let Err(error) = handle.await.unwrap() {
                // Because this action can finish in topic_changed, we must only act if this is
                // still the current action.
                if weak_action.is_ongoing() {
                    self.changing_topic.take();
                    error!("Could not change room description: {error}");
                    toast!(self.obj(), gettext("Could not change room description"));
                    self.topic_text_view.set_sensitive(true);
                    self.save_topic_button.set_is_loading(false);
                }
            }
        }

        /// Go back to the previous page in the room details.
        ///
        /// If there are changes in the page, ask the user to confirm.
        #[template_callback]
        async fn go_back(&self) {
            let obj = self.obj();
            let mut reset_after = false;

            let name_was_edited = self.was_name_edited() && self.changing_name.borrow().is_none();
            let topic_was_edited =
                self.was_topic_edited() && self.changing_topic.borrow().is_none();

            if name_was_edited || topic_was_edited {
                match unsaved_changes_dialog(&*obj).await {
                    UnsavedChangesResponse::Save => {
                        if name_was_edited {
                            self.change_name().await;
                        }

                        if topic_was_edited {
                            self.change_topic().await;
                        }
                    }
                    UnsavedChangesResponse::Discard => reset_after = true,
                    UnsavedChangesResponse::Cancel => return,
                }
            }

            obj.activate_action("navigation.pop", None).unwrap();

            if reset_after {
                self.reset_name();
                self.reset_topic();
            }
        }

        /// Disconnect all the signals.
        fn disconnect_all(&self) {
            self.room.disconnect_signals();

            if let Some(watch) = self.expr_watch.take() {
                watch.unwatch();
            }
        }
    }
}

glib::wrapper! {
    /// Subpage to edit the room main details (avatar, name and topic).
    pub struct EditDetailsSubpage(ObjectSubclass<imp::EditDetailsSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl EditDetailsSubpage {
    /// Construct a new `EditDetailsSubpage` for the given room.
    pub fn new(room: &Room) -> Self {
        glib::Object::builder().property("room", room).build()
    }
}
