use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use ruma::events::media_preview_config::MediaPreviews;
use tracing::error;

mod ignored_users_subpage;

pub(super) use self::ignored_users_subpage::IgnoredUsersSubpage;
use crate::{
    components::{ButtonCountRow, CheckLoadingRow, SwitchLoadingRow},
    session::Session,
    spawn, toast,
};

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
    };

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/account_settings/safety_page/mod.ui")]
    #[properties(wrapper_type = super::SafetyPage)]
    pub struct SafetyPage {
        #[template_child]
        public_read_receipts_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        typing_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        ignored_users_row: TemplateChild<ButtonCountRow>,
        #[template_child]
        media_previews: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        media_previews_on_row: TemplateChild<CheckLoadingRow>,
        #[template_child]
        media_previews_private_row: TemplateChild<CheckLoadingRow>,
        #[template_child]
        media_previews_off_row: TemplateChild<CheckLoadingRow>,
        #[template_child]
        invite_avatars_row: TemplateChild<SwitchLoadingRow>,
        /// The current session.
        #[property(get, set = Self::set_session, nullable)]
        session: glib::WeakRef<Session>,
        /// The media previews setting, as a string.
        #[property(get = Self::media_previews_enabled, set = Self::set_media_previews_enabled)]
        media_previews_enabled: PhantomData<String>,
        /// Whether the media previews section is busy.
        #[property(get)]
        media_previews_loading: Cell<bool>,
        /// Whether the invite avatars row is busy.
        #[property(get)]
        invite_avatars_loading: Cell<bool>,
        ignored_users_count_handler: RefCell<Option<glib::SignalHandlerId>>,
        global_account_data_handlers: RefCell<Vec<glib::SignalHandlerId>>,
        bindings: RefCell<Vec<glib::Binding>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SafetyPage {
        const NAME: &'static str = "SafetyPage";
        type Type = super::SafetyPage;
        type ParentType = adw::PreferencesPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.install_property_action(
                "safety.set-media-previews-enabled",
                "media-previews-enabled",
            );
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SafetyPage {
        fn dispose(&self) {
            self.disconnect_signals();
        }
    }

    impl WidgetImpl for SafetyPage {}
    impl PreferencesPageImpl for SafetyPage {}

    #[gtk::template_callbacks]
    impl SafetyPage {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            if self.session.upgrade().as_ref() == session {
                return;
            }

            self.disconnect_signals();

            if let Some(session) = session {
                let ignored_users = session.ignored_users();
                let ignored_users_count_handler = ignored_users.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |ignored_users, _, _, _| {
                        imp.ignored_users_row
                            .set_count(ignored_users.n_items().to_string());
                    }
                ));
                self.ignored_users_row
                    .set_count(ignored_users.n_items().to_string());

                self.ignored_users_count_handler
                    .replace(Some(ignored_users_count_handler));

                let global_account_data = session.global_account_data();

                let media_previews_handler = global_account_data
                    .connect_media_previews_enabled_changed(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_media_previews();
                        }
                    ));
                let invite_avatars_handler = global_account_data
                    .connect_invite_avatars_enabled_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_invite_avatars();
                        }
                    ));
                self.global_account_data_handlers
                    .replace(vec![media_previews_handler, invite_avatars_handler]);

                let session_settings = session.settings();

                let public_read_receipts_binding = session_settings
                    .bind_property(
                        "public-read-receipts-enabled",
                        &*self.public_read_receipts_row,
                        "active",
                    )
                    .bidirectional()
                    .sync_create()
                    .build();
                let typing_binding = session_settings
                    .bind_property("typing-enabled", &*self.typing_row, "active")
                    .bidirectional()
                    .sync_create()
                    .build();

                self.bindings
                    .replace(vec![public_read_receipts_binding, typing_binding]);
            }

            self.session.set(session);

            self.update_media_previews();
            self.update_invite_avatars();
            self.obj().notify_session();
        }

        /// The media previews setting, as a string.
        fn media_previews_enabled(&self) -> String {
            let Some(session) = self.session.upgrade() else {
                return String::new();
            };

            match session.global_account_data().media_previews_enabled() {
                MediaPreviews::Off => "off",
                MediaPreviews::Private => "private",
                _ => "on",
            }
            .to_owned()
        }

        /// Update the media previews section.
        fn update_media_previews(&self) {
            // Updates the active radio button.
            self.obj().notify_media_previews_enabled();

            self.media_previews
                .set_sensitive(!self.media_previews_loading.get());
        }

        /// Set the media previews setting, as a string.
        fn set_media_previews_enabled(&self, setting: &str) {
            if setting.is_empty() {
                error!("Invalid empty value to set media previews setting");
                return;
            }

            let setting = setting.into();

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.set_media_previews_enabled_inner(setting).await;
                }
            ));
        }

        /// Propagate the media previews setting.
        async fn set_media_previews_enabled_inner(&self, setting: MediaPreviews) {
            let Some(session) = self.session.upgrade() else {
                return;
            };
            let global_account_data = session.global_account_data();

            if setting == global_account_data.media_previews_enabled() {
                // Nothing to do.
                return;
            }

            self.media_previews.set_sensitive(false);
            self.set_media_previews_loading(true, &setting);

            if global_account_data
                .set_media_previews_enabled(setting.clone())
                .await
                .is_err()
            {
                toast!(
                    self.obj(),
                    gettext("Could not change media previews setting"),
                );
            }

            self.set_media_previews_loading(false, &setting);
            self.update_media_previews();
        }

        /// Set the loading state of the media previews section.
        fn set_media_previews_loading(&self, loading: bool, setting: &MediaPreviews) {
            // Only show the spinner on the selected one.
            self.media_previews_on_row
                .set_is_loading(loading && *setting == MediaPreviews::On);
            self.media_previews_private_row
                .set_is_loading(loading && *setting == MediaPreviews::Private);
            self.media_previews_off_row
                .set_is_loading(loading && *setting == MediaPreviews::Off);

            self.media_previews_loading.set(loading);
            self.obj().notify_media_previews_loading();
        }

        /// Update the invite avatars section.
        fn update_invite_avatars(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            self.invite_avatars_row
                .set_is_active(session.global_account_data().invite_avatars_enabled());
            self.invite_avatars_row
                .set_sensitive(!self.invite_avatars_loading.get());
        }

        /// Set the invite avatars setting.
        #[template_callback]
        async fn set_invite_avatars_enabled(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };
            let global_account_data = session.global_account_data();

            let enabled = self.invite_avatars_row.is_active();
            if enabled == global_account_data.invite_avatars_enabled() {
                // Nothing to do.
                return;
            }

            self.invite_avatars_row.set_sensitive(false);
            self.set_invite_avatars_loading(true);

            if global_account_data
                .set_invite_avatars_enabled(enabled)
                .await
                .is_err()
            {
                let msg = if enabled {
                    gettext("Could not enable avatars for invites")
                } else {
                    gettext("Could not disable avatars for invites")
                };
                toast!(self.obj(), msg);
            }

            self.set_invite_avatars_loading(false);
            self.update_invite_avatars();
        }

        /// Set the loading state of the invite avatars section.
        fn set_invite_avatars_loading(&self, loading: bool) {
            self.invite_avatars_loading.set(loading);
            self.obj().notify_invite_avatars_loading();
        }

        /// Disconnect the signal handlers and bindings.
        fn disconnect_signals(&self) {
            if let Some(session) = self.session.upgrade() {
                let global_account_data = session.global_account_data();
                for handler in self.global_account_data_handlers.take() {
                    global_account_data.disconnect(handler);
                }

                if let Some(handler) = self.ignored_users_count_handler.take() {
                    session.ignored_users().disconnect(handler);
                }
            }

            for binding in self.bindings.take() {
                binding.unbind();
            }
        }
    }
}

glib::wrapper! {
    /// Safety settings page.
    pub struct SafetyPage(ObjectSubclass<imp::SafetyPage>)
        @extends gtk::Widget, adw::PreferencesPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SafetyPage {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }
}
