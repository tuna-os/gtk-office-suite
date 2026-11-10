use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gio, glib, glib::clone};

use crate::{
    components::{CheckLoadingRow, EntryAddRow, RemovableRow, SwitchLoadingRow},
    i18n::gettext_f,
    session::{NotificationsGlobalSetting, NotificationsSettings},
    spawn, toast,
    utils::{BoundObjectWeakRef, PlaceholderObject, SingleItemListModel},
};

mod imp {
    use std::{cell::Cell, marker::PhantomData};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/account_settings/notifications_page.ui")]
    #[properties(wrapper_type = super::NotificationsPage)]
    pub struct NotificationsPage {
        #[template_child]
        account_row: TemplateChild<SwitchLoadingRow>,
        #[template_child]
        session_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        global: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        global_all_row: TemplateChild<CheckLoadingRow>,
        #[template_child]
        global_direct_row: TemplateChild<CheckLoadingRow>,
        #[template_child]
        global_mentions_row: TemplateChild<CheckLoadingRow>,
        #[template_child]
        keywords: TemplateChild<gtk::ListBox>,
        #[template_child]
        keywords_add_row: TemplateChild<EntryAddRow>,
        /// The notifications settings of the current session.
        #[property(get, set = Self::set_notifications_settings, explicit_notify)]
        notifications_settings: BoundObjectWeakRef<NotificationsSettings>,
        /// Whether the account section is busy.
        #[property(get)]
        account_loading: Cell<bool>,
        /// Whether the global section is busy.
        #[property(get)]
        global_loading: Cell<bool>,
        /// The global notifications setting, as a string.
        #[property(get = Self::global_setting, set = Self::set_global_setting)]
        global_setting: PhantomData<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NotificationsPage {
        const NAME: &'static str = "NotificationsPage";
        type Type = super::NotificationsPage;
        type ParentType = adw::PreferencesPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.install_property_action("notifications.set-global-default", "global-setting");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for NotificationsPage {}

    impl WidgetImpl for NotificationsPage {}
    impl PreferencesPageImpl for NotificationsPage {}

    #[gtk::template_callbacks]
    impl NotificationsPage {
        /// Set the notifications settings of the current session.
        fn set_notifications_settings(
            &self,
            notifications_settings: Option<&NotificationsSettings>,
        ) {
            if self.notifications_settings.obj().as_ref() == notifications_settings {
                return;
            }

            self.notifications_settings.disconnect_signals();

            if let Some(settings) = notifications_settings {
                let account_enabled_handler = settings.connect_account_enabled_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_account();
                    }
                ));
                let session_enabled_handler = settings.connect_session_enabled_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_session();
                    }
                ));
                let global_setting_handler = settings.connect_global_setting_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_global();
                    }
                ));

                self.notifications_settings.set(
                    settings,
                    vec![
                        account_enabled_handler,
                        session_enabled_handler,
                        global_setting_handler,
                    ],
                );

                let extra_items = SingleItemListModel::new(Some(&PlaceholderObject::new("add")));

                let all_items = gio::ListStore::new::<glib::Object>();
                all_items.append(&settings.keywords_list());
                all_items.append(&extra_items);

                let flattened_list = gtk::FlattenListModel::new(Some(all_items));
                self.keywords.bind_model(
                    Some(&flattened_list),
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        #[upgrade_or_else]
                        || { adw::ActionRow::new().upcast() },
                        move |item| imp.create_keyword_row(item)
                    ),
                );
            } else {
                self.keywords.bind_model(
                    None::<&gio::ListModel>,
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        #[upgrade_or_else]
                        || { adw::ActionRow::new().upcast() },
                        move |item| imp.create_keyword_row(item)
                    ),
                );
            }

            self.update_account();
            self.obj().notify_notifications_settings();
        }

        /// Update the account row.
        fn update_account(&self) {
            let Some(settings) = self.notifications_settings.obj() else {
                return;
            };

            let checked = settings.account_enabled();
            self.account_row.set_is_active(checked);
            self.account_row.set_sensitive(!self.account_loading.get());

            // Other sections will be disabled or not.
            self.update_session();
        }

        /// Set the loading state of the account row.
        fn set_account_loading(&self, loading: bool) {
            self.account_loading.set(loading);
            self.obj().notify_account_loading();
        }

        /// Set the account setting.
        #[template_callback]
        async fn set_account_enabled(&self) {
            let Some(settings) = self.notifications_settings.obj() else {
                return;
            };

            let enabled = self.account_row.is_active();
            if enabled == settings.account_enabled() {
                // Nothing to do.
                return;
            }

            self.account_row.set_sensitive(false);
            self.set_account_loading(true);

            if settings.set_account_enabled(enabled).await.is_err() {
                let msg = if enabled {
                    gettext("Could not enable account notifications")
                } else {
                    gettext("Could not disable account notifications")
                };
                toast!(self.obj(), msg);
            }

            self.set_account_loading(false);
            self.update_account();
        }

        /// Update the session row.
        fn update_session(&self) {
            let Some(settings) = self.notifications_settings.obj() else {
                return;
            };

            self.session_row.set_active(settings.session_enabled());
            self.session_row.set_sensitive(settings.account_enabled());

            // Other sections will be disabled or not.
            self.update_global();
            self.update_keywords();
        }

        /// Set the session setting.
        #[template_callback]
        fn set_session_enabled(&self) {
            let Some(settings) = self.notifications_settings.obj() else {
                return;
            };

            settings.set_session_enabled(self.session_row.is_active());
        }

        /// The global notifications setting, as a string.
        fn global_setting(&self) -> String {
            let Some(settings) = self.notifications_settings.obj() else {
                return String::new();
            };

            settings.global_setting().as_str().to_owned()
        }

        /// Update the global section.
        fn update_global(&self) {
            let Some(settings) = self.notifications_settings.obj() else {
                return;
            };

            // Updates the active radio button.
            self.obj().notify_global_setting();

            let sensitive = settings.account_enabled()
                && settings.session_enabled()
                && !self.global_loading.get();
            self.global.set_sensitive(sensitive);
        }

        /// Set the global setting, as a string.
        fn set_global_setting(&self, default: &str) {
            let default = NotificationsGlobalSetting::from_str(default);

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.set_global_setting_inner(default).await;
                }
            ));
        }

        /// Propagate the global setting.
        async fn set_global_setting_inner(&self, setting: NotificationsGlobalSetting) {
            let Some(settings) = self.notifications_settings.obj() else {
                return;
            };

            if setting == settings.global_setting() {
                // Nothing to do.
                return;
            }

            self.global.set_sensitive(false);
            self.set_global_loading(true, setting);

            if settings.set_global_setting(setting).await.is_err() {
                toast!(
                    self.obj(),
                    gettext("Could not change global notifications setting"),
                );
            }

            self.set_global_loading(false, setting);
            self.update_global();
        }

        /// Set the loading state of the global section.
        fn set_global_loading(&self, loading: bool, setting: NotificationsGlobalSetting) {
            // Only show the spinner on the selected one.
            self.global_all_row
                .set_is_loading(loading && setting == NotificationsGlobalSetting::All);
            self.global_direct_row.set_is_loading(
                loading && setting == NotificationsGlobalSetting::DirectAndMentions,
            );
            self.global_mentions_row
                .set_is_loading(loading && setting == NotificationsGlobalSetting::MentionsOnly);

            self.global_loading.set(loading);
            self.obj().notify_global_loading();
        }

        /// Update the section about keywords.
        #[template_callback]
        fn update_keywords(&self) {
            let Some(settings) = self.notifications_settings.obj() else {
                return;
            };

            let sensitive = settings.account_enabled() && settings.session_enabled();
            self.keywords.set_sensitive(sensitive);

            if !sensitive {
                // Nothing else to update.
                return;
            }

            self.keywords_add_row
                .set_inhibit_add(!self.can_add_keyword());
        }

        /// Create a row in the keywords list for the given item.
        fn create_keyword_row(&self, item: &glib::Object) -> gtk::Widget {
            let Some(string_obj) = item.downcast_ref::<gtk::StringObject>() else {
                // It can only be the dummy item to add a new keyword.
                return self.keywords_add_row.clone().upcast();
            };

            let keyword = string_obj.string();
            let row = RemovableRow::new();
            row.set_title(&keyword);
            row.set_remove_button_tooltip_text(Some(gettext_f(
                "Remove “{keyword}”",
                &[("keyword", &keyword)],
            )));

            row.connect_remove(clone!(
                #[weak(rename_to = imp)]
                self,
                move |row| {
                    imp.remove_keyword(row);
                }
            ));

            row.upcast()
        }

        /// Remove the keyword from the given row.
        fn remove_keyword(&self, row: &RemovableRow) {
            let Some(settings) = self.notifications_settings.obj() else {
                return;
            };

            row.set_is_loading(true);

            let obj = self.obj();
            spawn!(clone!(
                #[weak]
                obj,
                #[weak]
                row,
                async move {
                    if settings.remove_keyword(row.title().into()).await.is_err() {
                        toast!(obj, gettext("Could not remove notification keyword"));
                    }

                    row.set_is_loading(false);
                }
            ));
        }

        /// Whether we can add the keyword that is currently in the entry.
        fn can_add_keyword(&self) -> bool {
            // Cannot add a keyword if section is disabled.
            if !self.keywords.is_sensitive() {
                return false;
            }

            // Cannot add a keyword if a keyword is already being added.
            if self.keywords_add_row.is_loading() {
                return false;
            }

            let text = self.keywords_add_row.text().to_lowercase();

            // Cannot add an empty keyword.
            if text.is_empty() {
                return false;
            }

            // Cannot add a keyword without the API.
            let Some(settings) = self.notifications_settings.obj() else {
                return false;
            };

            // Cannot add a keyword that already exists.
            let keywords_list = settings.keywords_list();
            for keyword_obj in keywords_list.iter::<glib::Object>() {
                let Ok(keyword_obj) = keyword_obj else {
                    break;
                };

                if keyword_obj
                    .downcast_ref::<gtk::StringObject>()
                    .map(gtk::StringObject::string)
                    .is_some_and(|keyword| keyword.to_lowercase() == text)
                {
                    return false;
                }
            }

            true
        }

        /// Add the keyword that is currently in the entry.
        #[template_callback]
        async fn add_keyword(&self) {
            if !self.can_add_keyword() {
                return;
            }

            let Some(settings) = self.notifications_settings.obj() else {
                return;
            };

            self.keywords_add_row.set_is_loading(true);

            let keyword = self.keywords_add_row.text().into();

            if settings.add_keyword(keyword).await.is_err() {
                toast!(self.obj(), gettext("Could not add notification keyword"));
            } else {
                // Adding the keyword was successful, reset the entry.
                self.keywords_add_row.set_text("");
            }

            self.keywords_add_row.set_is_loading(false);
            self.update_keywords();
        }
    }
}

glib::wrapper! {
    /// Preferences page to edit global notification settings.
    pub struct NotificationsPage(ObjectSubclass<imp::NotificationsPage>)
        @extends gtk::Widget, adw::PreferencesPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl NotificationsPage {
    pub fn new(notifications_settings: &NotificationsSettings) -> Self {
        glib::Object::builder()
            .property("notifications-settings", notifications_settings)
            .build()
    }
}
