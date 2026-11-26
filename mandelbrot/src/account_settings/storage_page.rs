use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use tracing::error;

use crate::{
    session::{Session, media_cache_limit_choice_index, media_cache_limit_choice_mib},
    spawn, toast,
};

mod imp {
    use std::cell::Cell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/account_settings/storage_page.ui")]
    #[properties(wrapper_type = super::StoragePage)]
    pub struct StoragePage {
        #[template_child]
        media_size_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        limit_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        clear_row: TemplateChild<adw::ButtonRow>,
        #[template_child]
        event_cache_size_row: TemplateChild<adw::ActionRow>,
        /// The current session.
        #[property(get, set = Self::set_session, nullable)]
        session: glib::WeakRef<Session>,
        /// Whether we are updating the limit row from the settings.
        syncing_limit: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StoragePage {
        const NAME: &'static str = "StoragePage";
        type Type = super::StoragePage;
        type ParentType = adw::PreferencesPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for StoragePage {}

    impl WidgetImpl for StoragePage {}
    impl PreferencesPageImpl for StoragePage {}

    #[gtk::template_callbacks]
    impl StoragePage {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            if self.session.upgrade().as_ref() == session {
                return;
            }

            self.session.set(session);

            if let Some(session) = session {
                let labels = crate::session::MEDIA_CACHE_LIMIT_CHOICES_MIB
                    .iter()
                    .map(|mib| format_bytes(u64::from(*mib) * 1024 * 1024))
                    .collect::<Vec<_>>();
                let labels = labels.iter().map(String::as_str).collect::<Vec<_>>();

                self.syncing_limit.set(true);
                self.limit_row.set_model(Some(&gtk::StringList::new(&labels)));
                self.limit_row.set_selected(
                    media_cache_limit_choice_index(session.settings().media_cache_limit_mib())
                        as u32,
                );
                self.syncing_limit.set(false);
            }

            self.obj().notify_session();
            self.refresh_sizes();
        }

        /// Show the current size of the stores.
        fn refresh_sizes(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    match session.store_sizes().await {
                        Ok((media, event_cache)) => {
                            imp.media_size_row.set_subtitle(&size_label(media));
                            imp.event_cache_size_row.set_subtitle(&size_label(event_cache));
                        }
                        Err(error) => {
                            error!("Could not get the size of the stores: {error}");
                            imp.media_size_row.set_subtitle(&gettext("Unknown"));
                            imp.event_cache_size_row.set_subtitle(&gettext("Unknown"));
                        }
                    }
                }
            ));
        }

        /// The cache limit was changed by the user.
        #[template_callback]
        fn limit_selected(&self) {
            if self.syncing_limit.get() {
                return;
            }
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let limit = media_cache_limit_choice_mib(self.limit_row.selected() as usize);
            session.settings().set_media_cache_limit_mib(limit);

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    if let Err(error) = session.apply_media_cache_limit().await {
                        error!("Could not apply the media cache limit: {error}");
                        toast!(imp.obj(), gettext("Could not apply the media cache limit"));
                    }
                    imp.refresh_sizes();
                }
            ));
        }

        /// Remove all the cached media.
        #[template_callback]
        fn clear_media_cache(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            self.clear_row.set_sensitive(false);

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    if let Err(error) = session.clear_media_cache().await {
                        error!("Could not clear the media cache: {error}");
                        toast!(imp.obj(), gettext("Could not clear the media cache"));
                    }
                    imp.clear_row.set_sensitive(true);
                    imp.refresh_sizes();
                }
            ));
        }
    }
}

glib::wrapper! {
    /// Account settings page about the data kept on this device.
    pub struct StoragePage(ObjectSubclass<imp::StoragePage>)
        @extends gtk::Widget, adw::PreferencesPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// Format a size in bytes with binary units, like the limits we offer.
fn format_bytes(bytes: u64) -> String {
    glib::format_size_full(bytes, glib::FormatSizeFlags::IEC_UNITS).into()
}

/// The label for a store size, if known.
fn size_label(size: Option<u64>) -> String {
    size.map_or_else(|| gettext("Unknown"), format_bytes)
}
