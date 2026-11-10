use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gio, glib, glib::clone};
use ruma::{
    OwnedMxcUri,
    api::client::discovery::get_authorization_server_metadata::v1::{
        AccountManagementActionData, AuthorizationServerMetadata,
    },
};
use tracing::error;

mod change_password_subpage;
mod deactivate_account_subpage;
mod log_out_subpage;

pub use self::{
    change_password_subpage::ChangePasswordSubpage,
    deactivate_account_subpage::DeactivateAccountSubpage, log_out_subpage::LogOutSubpage,
};
use super::AccountSettings;
use crate::{
    components::{ActionButton, ActionState, ButtonCountRow, CopyableRow, EditableAvatar},
    prelude::*,
    session::Session,
    spawn, spawn_tokio, toast,
    utils::{OngoingAsyncAction, TemplateCallbacks, media::FileInfo},
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/account_settings/general_page/mod.ui")]
    #[properties(wrapper_type = super::GeneralPage)]
    pub struct GeneralPage {
        #[template_child]
        avatar: TemplateChild<EditableAvatar>,
        #[template_child]
        display_name: TemplateChild<adw::EntryRow>,
        #[template_child]
        display_name_button: TemplateChild<ActionButton>,
        #[template_child]
        user_id: TemplateChild<CopyableRow>,
        #[template_child]
        user_sessions_row: TemplateChild<ButtonCountRow>,
        #[template_child]
        change_password_row: TemplateChild<adw::ButtonRow>,
        #[template_child]
        manage_account_row: TemplateChild<adw::ButtonRow>,
        #[template_child]
        homeserver: TemplateChild<CopyableRow>,
        #[template_child]
        session_id: TemplateChild<CopyableRow>,
        #[template_child]
        deactivate_account_button: TemplateChild<adw::ButtonRow>,
        /// The current session.
        #[property(get, set = Self::set_session, nullable)]
        session: glib::WeakRef<Session>,
        /// The ancestor [`AccountSettings`].
        #[property(get, set = Self::set_account_settings, nullable)]
        account_settings: glib::WeakRef<AccountSettings>,
        capabilities_data: RefCell<CapabilitiesData>,
        changing_avatar: RefCell<Option<OngoingAsyncAction<OwnedMxcUri>>>,
        changing_display_name: RefCell<Option<OngoingAsyncAction<String>>>,
        avatar_uri_handler: RefCell<Option<glib::SignalHandlerId>>,
        display_name_handler: RefCell<Option<glib::SignalHandlerId>>,
        user_sessions_count_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GeneralPage {
        const NAME: &'static str = "AccountSettingsGeneralPage";
        type Type = super::GeneralPage;
        type ParentType = adw::PreferencesPage;

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
    impl ObjectImpl for GeneralPage {}

    impl WidgetImpl for GeneralPage {}
    impl PreferencesPageImpl for GeneralPage {}

    #[gtk::template_callbacks]
    impl GeneralPage {
        /// Set the current session.
        fn set_session(&self, session: Option<Session>) {
            let prev_session = self.session.upgrade();
            if prev_session == session {
                return;
            }

            if let Some(session) = prev_session {
                let user = session.user();

                if let Some(handler) = self.avatar_uri_handler.take() {
                    user.avatar_data()
                        .image()
                        .expect("user of session always has an avatar image")
                        .disconnect(handler);
                }
                if let Some(handler) = self.display_name_handler.take() {
                    user.disconnect(handler);
                }
                if let Some(handler) = self.user_sessions_count_handler.take() {
                    session.user_sessions().other_sessions().disconnect(handler);
                }
            }

            self.session.set(session.as_ref());
            self.obj().notify_session();

            let Some(session) = session else {
                return;
            };

            self.user_id.set_subtitle(session.user_id().as_str());
            self.homeserver.set_subtitle(session.homeserver().as_str());
            self.session_id.set_subtitle(session.session_id());

            let user = session.user();
            let avatar_uri_handler = user
                .avatar_data()
                .image()
                .expect("user of session always has an avatar image")
                .connect_uri_string_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |avatar_image| {
                        imp.user_avatar_changed(avatar_image.uri().as_ref());
                    }
                ));
            self.avatar_uri_handler.replace(Some(avatar_uri_handler));

            let display_name_handler = user.connect_display_name_notify(clone!(
                #[weak(rename_to=imp)]
                self,
                move |user| {
                    imp.user_display_name_changed(&user.display_name());
                }
            ));
            self.display_name_handler
                .replace(Some(display_name_handler));

            let other_user_sessions = session.user_sessions().other_sessions();
            let user_sessions_count_handler = other_user_sessions.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |other_user_sessions, _, _, _| {
                    imp.user_sessions_row
                        .set_count((other_user_sessions.n_items() + 1).to_string());
                }
            ));
            self.user_sessions_row
                .set_count((other_user_sessions.n_items() + 1).to_string());
            self.user_sessions_count_handler
                .replace(Some(user_sessions_count_handler));

            spawn!(
                glib::Priority::LOW,
                clone!(
                    #[weak(rename_to = imp)]
                    self,
                    async move {
                        imp.load_capabilities().await;
                    }
                )
            );
        }

        /// Set the ancestor [`AccountSettings`].
        fn set_account_settings(&self, account_settings: Option<&AccountSettings>) {
            self.account_settings.set(account_settings);

            if let Some(account_settings) = account_settings {
                account_settings.connect_oauth_server_metadata_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_capabilities();
                    }
                ));
            }

            self.update_capabilities();
        }

        /// The OAuth 2.0 authorization server metadata, if any.
        fn oauth_server_metadata(&self) -> Option<AuthorizationServerMetadata> {
            self.account_settings
                .upgrade()
                .and_then(|s| s.oauth_server_metadata())
        }

        /// Load the possible changes on the user account.
        async fn load_capabilities(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let client = session.client();
            let handle = spawn_tokio!(async move {
                let capabilities = client.homeserver_capabilities();
                tokio::try_join!(
                    capabilities.can_change_avatar(),
                    capabilities.can_change_displayname(),
                    capabilities.can_change_password(),
                )
            });

            let capabilities = match handle.await.expect("task was not aborted") {
                Ok((can_change_avatar, can_change_displayname, can_change_password)) => {
                    CapabilitiesData {
                        can_change_avatar,
                        can_change_displayname,
                        can_change_password,
                    }
                }
                Err(error) => {
                    error!("Could not fetch capabilities: {error}");
                    CapabilitiesData::default()
                }
            };

            self.capabilities_data.replace(capabilities);
            self.update_capabilities();
        }

        /// Update the possible changes on the user account with the current
        /// state.
        fn update_capabilities(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let uses_oauth_api = session.uses_oauth_api();
            let has_account_management_url = self
                .oauth_server_metadata()
                .is_some_and(|metadata| metadata.account_management_uri.is_some());
            let capabilities_data = self.capabilities_data.borrow();

            self.avatar
                .set_editable(capabilities_data.can_change_avatar);
            self.display_name
                .set_editable(capabilities_data.can_change_displayname);
            self.change_password_row
                .set_visible(!has_account_management_url && capabilities_data.can_change_password);
            self.manage_account_row
                .set_visible(has_account_management_url);
            self.deactivate_account_button
                .set_visible(!uses_oauth_api || has_account_management_url);
        }

        /// Open the URL to manage the account.
        #[template_callback]
        async fn manage_account(&self) {
            let Some(metadata) = self.oauth_server_metadata() else {
                error!("Could not find OAuth 2.0 authorization server metadata");
                return;
            };

            let Some(url) =
                metadata.account_management_url_with_action(AccountManagementActionData::Profile)
            else {
                error!("Could not build OAuth 2.0 account management URL");
                return;
            };

            if let Err(error) = gtk::UriLauncher::new(url.as_str())
                .launch_future(self.obj().root().and_downcast_ref::<gtk::Window>())
                .await
            {
                error!("Could not launch OAuth 2.0 account management URL: {error}");
            }
        }

        /// Update the view when the user's avatar changed.
        fn user_avatar_changed(&self, uri: Option<&OwnedMxcUri>) {
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

        /// Change the avatar of the user with the one in the given file.
        #[template_callback]
        async fn change_avatar(&self, file: gio::File) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let avatar = &self.avatar;
            avatar.edit_in_progress();

            let info = match FileInfo::try_from_file(&file).await {
                Ok(info) => info,
                Err(error) => {
                    error!("Could not load user avatar file info: {error}");
                    toast!(self.obj(), gettext("Could not load file"));
                    avatar.reset();
                    return;
                }
            };

            let data = match file.load_contents_future().await {
                Ok((data, _)) => data,
                Err(error) => {
                    error!("Could not load user avatar file: {error}");
                    toast!(self.obj(), gettext("Could not load file"));
                    avatar.reset();
                    return;
                }
            };

            let client = session.client();
            let client_clone = client.clone();
            let handle = spawn_tokio!(async move {
                client_clone
                    .media()
                    .upload(&info.mime, data.into(), None)
                    .await
            });

            let uri = match handle.await.expect("task was not aborted") {
                Ok(res) => res.content_uri,
                Err(error) => {
                    error!("Could not upload user avatar: {error}");
                    toast!(self.obj(), gettext("Could not upload avatar"));
                    avatar.reset();
                    return;
                }
            };

            let (action, weak_action) = OngoingAsyncAction::set(uri.clone());
            self.changing_avatar.replace(Some(action));

            let uri_clone = uri.clone();
            let handle =
                spawn_tokio!(
                    async move { client.account().set_avatar_url(Some(&uri_clone)).await }
                );

            match handle.await.expect("task was not aborted") {
                Ok(()) => {
                    // If the user is in no rooms, we won't receive the update via sync, so change
                    // the avatar manually if this request succeeds before the avatar is updated.
                    // Because this action can finish in user_avatar_changed, we must only act if
                    // this is still the current action.
                    if weak_action.is_ongoing() {
                        session.user().set_avatar_url(Some(uri));
                    }
                }
                Err(error) => {
                    // Because this action can finish in user_avatar_changed, we must only act if
                    // this is still the current action.
                    if weak_action.is_ongoing() {
                        self.changing_avatar.take();
                        error!("Could not change user avatar: {error}");
                        toast!(self.obj(), gettext("Could not change avatar"));
                        avatar.reset();
                    }
                }
            }
        }

        /// Remove the current avatar of the user.
        #[template_callback]
        async fn remove_avatar(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            // Ask for confirmation.
            let confirm_dialog = adw::AlertDialog::builder()
                .default_response("cancel")
                .heading(gettext("Remove Avatar?"))
                .body(gettext("Do you really want to remove your avatar?"))
                .build();
            confirm_dialog.add_responses(&[
                ("cancel", &gettext("Cancel")),
                ("remove", &gettext("Remove")),
            ]);
            confirm_dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);

            let obj = self.obj();
            if confirm_dialog.choose_future(Some(&*obj)).await != "remove" {
                return;
            }

            let avatar = &*self.avatar;
            avatar.removal_in_progress();

            let (action, weak_action) = OngoingAsyncAction::remove();
            self.changing_avatar.replace(Some(action));

            let client = session.client();
            let handle = spawn_tokio!(async move { client.account().set_avatar_url(None).await });

            match handle.await.expect("task was not aborted") {
                Ok(()) => {
                    // If the user is in no rooms, we won't receive the update via sync, so change
                    // the avatar manually if this request succeeds before the avatar is updated.
                    // Because this action can finish in avatar_changed, we must only act if this is
                    // still the current action.
                    if weak_action.is_ongoing() {
                        session.user().set_avatar_url(None);
                    }
                }
                Err(error) => {
                    // Because this action can finish in avatar_changed, we must only act if this is
                    // still the current action.
                    if weak_action.is_ongoing() {
                        self.changing_avatar.take();
                        error!("Could not remove user avatar: {error}");
                        toast!(obj, gettext("Could not remove avatar"));
                        avatar.reset();
                    }
                }
            }
        }

        /// Update the view when the text of the display name changed.
        #[template_callback]
        fn display_name_changed(&self) {
            self.display_name_button
                .set_visible(self.has_display_name_changed());
        }

        /// Whether the display name in the entry row is different than the
        /// user's.
        fn has_display_name_changed(&self) -> bool {
            let Some(session) = self.session.upgrade() else {
                return false;
            };
            let text = self.display_name.text();
            let display_name = session.user().display_name();

            text != display_name
        }

        /// Update the view when the user's display name changed.
        fn user_display_name_changed(&self, name: &str) {
            if let Some(action) = self.changing_display_name.borrow().as_ref() {
                if action.as_value().map(String::as_str) == Some(name) {
                    // This is not the change we expected, maybe another device did a change too.
                    // Let's wait for another change.
                    return;
                }
            } else {
                // No action is ongoing, we don't need to do anything.
                return;
            }

            // Reset state.
            self.changing_display_name.take();

            let entry = &self.display_name;
            let button = &self.display_name_button;

            entry.remove_css_class("error");
            entry.set_sensitive(true);
            button.set_visible(false);
            button.set_state(ActionState::Confirm);
            toast!(self.obj(), gettext("Name changed successfully"));
        }

        /// Change the display name of the user.
        #[template_callback]
        async fn change_display_name(&self) {
            if !self.has_display_name_changed() {
                // Nothing to do.
                return;
            }
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let entry = &self.display_name;
            let button = &self.display_name_button;

            entry.set_sensitive(false);
            button.set_state(ActionState::Loading);

            let display_name = entry.text().trim().to_string();

            let (action, weak_action) = OngoingAsyncAction::set(display_name.clone());
            self.changing_display_name.replace(Some(action));

            let client = session.client();
            let display_name_clone = display_name.clone();
            let handle = spawn_tokio!(async move {
                client
                    .account()
                    .set_display_name(Some(&display_name_clone))
                    .await
            });

            match handle.await.expect("task was not aborted") {
                Ok(()) => {
                    // If the user is in no rooms, we won't receive the update via sync, so change
                    // the avatar manually if this request succeeds before the avatar is updated.
                    // Because this action can finish in user_display_name_changed, we must only act
                    // if this is still the current action.
                    if weak_action.is_ongoing() {
                        session.user().set_name(Some(display_name));
                    }
                }
                Err(error) => {
                    // Because this action can finish in user_display_name_changed, we must only act
                    // if this is still the current action.
                    if weak_action.is_ongoing() {
                        self.changing_display_name.take();
                        error!("Could not change user display name: {error}");
                        toast!(self.obj(), gettext("Could not change display name"));
                        button.set_state(ActionState::Retry);
                        entry.add_css_class("error");
                        entry.set_sensitive(true);
                    }
                }
            }
        }
    }
}

glib::wrapper! {
    /// Account settings page about the user and the session.
    pub struct GeneralPage(ObjectSubclass<imp::GeneralPage>)
        @extends gtk::Widget, adw::PreferencesPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl GeneralPage {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }
}

/// The data necessary to compute the capabilities of the server.
#[derive(Debug, Default, Clone)]
struct CapabilitiesData {
    /// Whether changing the user's avatar is allowed by the homeserver.
    can_change_avatar: bool,
    /// Whether changing the user's display name is allowed by the homeserver.
    can_change_displayname: bool,
    /// Whether changing the user's password is allowed by the homeserver.
    can_change_password: bool,
}
