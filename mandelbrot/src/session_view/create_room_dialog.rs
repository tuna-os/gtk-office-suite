use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::glib;
use matrix_sdk::{
    Error,
    ruma::{
        api::{
            client::room::{Visibility, create_room},
            error::ErrorKind,
        },
        assign,
    },
};
use ruma::events::{InitialStateEvent, room::encryption::RoomEncryptionEventContent};
use tracing::error;

use crate::{
    Window,
    components::{LoadingButton, SubstringEntryRow, ToastableDialog},
    prelude::*,
    session::Session,
    spawn_tokio, toast,
};

// MAX length of room addresses
const MAX_BYTES: usize = 255;

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/create_room_dialog.ui")]
    #[properties(wrapper_type = super::CreateRoomDialog)]
    pub struct CreateRoomDialog {
        #[template_child]
        create_button: TemplateChild<LoadingButton>,
        #[template_child]
        content: TemplateChild<gtk::Box>,
        #[template_child]
        room_name: TemplateChild<adw::EntryRow>,
        #[template_child]
        topic_text_view: TemplateChild<gtk::TextView>,
        #[template_child]
        visibility_private: TemplateChild<gtk::CheckButton>,
        #[template_child]
        encryption: TemplateChild<adw::SwitchRow>,
        #[template_child]
        room_address: TemplateChild<SubstringEntryRow>,
        #[template_child]
        room_address_error_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        room_address_error: TemplateChild<gtk::Label>,
        /// The current session.
        #[property(get, set = Self::set_session, explicit_notify, nullable)]
        session: glib::WeakRef<Session>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CreateRoomDialog {
        const NAME: &'static str = "CreateRoomDialog";
        type Type = super::CreateRoomDialog;
        type ParentType = ToastableDialog;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for CreateRoomDialog {}

    impl WidgetImpl for CreateRoomDialog {}
    impl AdwDialogImpl for CreateRoomDialog {}
    impl ToastableDialogImpl for CreateRoomDialog {}

    #[gtk::template_callbacks]
    impl CreateRoomDialog {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            if self.session.upgrade().as_ref() == session {
                return;
            }

            if let Some(session) = session {
                let server_name = session.user_id().server_name();
                self.room_address.set_suffix_text(format!(":{server_name}"));
            }

            self.session.set(session);
            self.obj().notify_session();
        }

        /// Check whether a room can be created with the current input.
        ///
        /// This will also change the UI elements to reflect why the room can't
        /// be created.
        fn can_create_room(&self) -> bool {
            if self.room_name.text().trim().is_empty() {
                return false;
            }

            // Only public rooms have an address.
            if self.visibility_private.is_active() {
                return true;
            }

            let mut can_create = true;
            let room_address = self.room_address.text();

            // We don't allow #, : in the room address
            let address_error = if room_address.contains(':') {
                can_create = false;
                Some(gettext("Cannot contain “:”"))
            } else if room_address.contains('#') {
                can_create = false;
                Some(gettext("Cannot contain “#”"))
            } else if room_address.len() > MAX_BYTES {
                can_create = false;
                Some(gettext("Too long. Use a shorter address."))
            } else if room_address.trim().is_empty() {
                can_create = false;
                None
            } else {
                None
            };

            let reveal_address_error = address_error.is_some();

            if let Some(error) = address_error {
                self.room_address_error.set_text(&error);
                self.room_address.add_css_class("error");
            } else {
                self.room_address.remove_css_class("error");
            }
            self.room_address_error_revealer
                .set_reveal_child(reveal_address_error);

            can_create
        }

        /// Validate the form and change the corresponding UI elements.
        #[template_callback]
        fn validate_form(&self) {
            self.create_button.set_sensitive(self.can_create_room());
        }

        /// Create the room, if it is allowed.
        #[template_callback]
        async fn create_room(&self) {
            if !self.can_create_room() {
                return;
            }

            let Some(session) = self.session.upgrade() else {
                return;
            };

            self.create_button.set_is_loading(true);
            self.content.set_sensitive(false);

            let name = Some(self.room_name.text().trim())
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned);

            let buffer = self.topic_text_view.buffer();
            let (start_iter, end_iter) = buffer.bounds();
            let topic = Some(buffer.text(&start_iter, &end_iter, false).trim())
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned);

            let mut request = assign!(
                create_room::v3::Request::new(),
                {
                    name,
                    topic,
                }
            );

            if self.visibility_private.is_active() {
                // The room is private.
                request.visibility = Visibility::Private;

                if self.encryption.is_active() {
                    let event = InitialStateEvent::with_empty_state_key(
                        RoomEncryptionEventContent::with_recommended_defaults(),
                    );
                    request.initial_state = vec![event.to_raw_any()];
                }
            } else {
                // The room is public.
                request.visibility = Visibility::Public;
                request.room_alias_name = Some(self.room_address.text().trim().to_owned());
            }

            let client = session.client();
            let handle = spawn_tokio!(async move { client.create_room(request).await });

            match handle.await.expect("task was not aborted") {
                Ok(matrix_room) => {
                    let obj = self.obj();

                    let Some(window) = obj.root().and_downcast::<Window>() else {
                        return;
                    };
                    if let Some(room) = session
                        .room_list()
                        .get_wait(matrix_room.room_id(), None)
                        .await
                    {
                        window.session_view().select_room(room);
                    }

                    obj.close();
                }
                Err(error) => {
                    error!("Could not create a new room: {error}");
                    self.handle_error(&error);
                }
            }
        }

        /// Display the error that occurred during creation.
        fn handle_error(&self, error: &Error) {
            self.create_button.set_is_loading(false);
            self.content.set_sensitive(true);

            // Handle the room address already taken error.
            if error
                .client_api_error_kind()
                .is_some_and(|kind| *kind == ErrorKind::RoomInUse)
            {
                self.room_address.add_css_class("error");
                self.room_address_error
                    .set_text(&gettext("The address is already taken."));
                self.room_address_error_revealer.set_reveal_child(true);

                return;
            }

            toast!(self.obj(), error.to_user_facing());
        }
    }
}

glib::wrapper! {
    /// Dialog to create a new room.
    pub struct CreateRoomDialog(ObjectSubclass<imp::CreateRoomDialog>)
        @extends gtk::Widget, adw::Dialog, ToastableDialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl CreateRoomDialog {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }
}
