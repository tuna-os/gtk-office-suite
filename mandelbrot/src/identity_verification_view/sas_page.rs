use std::collections::HashMap;

use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gio, glib, glib::clone};

use super::sas_emoji::SasEmoji;
use crate::{
    components::LoadingButton, gettext_f, prelude::*, session::IdentityVerification, toast,
    utils::BoundObjectWeakRef,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/identity_verification_view/sas_page.ui")]
    #[properties(wrapper_type = super::SasPage)]
    pub struct SasPage {
        /// The current identity verification.
        #[property(get, set = Self::set_verification, explicit_notify, nullable)]
        pub verification: BoundObjectWeakRef<IdentityVerification>,
        pub display_name_handler: RefCell<Option<glib::SignalHandlerId>>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub instructions: TemplateChild<gtk::Label>,
        #[template_child]
        pub row_1: TemplateChild<gtk::Box>,
        #[template_child]
        pub row_2: TemplateChild<gtk::Box>,
        #[template_child]
        pub mismatch_btn: TemplateChild<LoadingButton>,
        #[template_child]
        pub match_btn: TemplateChild<LoadingButton>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SasPage {
        const NAME: &'static str = "IdentityVerificationSasPage";
        type Type = super::SasPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::Type::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SasPage {
        fn dispose(&self) {
            if let Some(verification) = self.verification.obj()
                && let Some(handler) = self.display_name_handler.take()
            {
                verification.user().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for SasPage {
        fn grab_focus(&self) -> bool {
            self.match_btn.grab_focus()
        }
    }

    impl BinImpl for SasPage {}

    impl SasPage {
        /// Set the current identity verification.
        fn set_verification(&self, verification: Option<&IdentityVerification>) {
            let prev_verification = self.verification.obj();

            if prev_verification.as_ref() == verification {
                return;
            }
            let obj = self.obj();

            obj.reset();

            if let Some(verification) = prev_verification
                && let Some(handler) = self.display_name_handler.take()
            {
                verification.user().disconnect(handler);
            }
            self.verification.disconnect_signals();

            if let Some(verification) = verification {
                let display_name_handler = verification.user().connect_display_name_notify(clone!(
                    #[weak]
                    obj,
                    move |_| {
                        obj.update_labels();
                    }
                ));
                self.display_name_handler
                    .replace(Some(display_name_handler));

                let sas_data_changed_handler = verification.connect_sas_data_changed(clone!(
                    #[weak]
                    obj,
                    move |_| {
                        obj.update_labels();
                        obj.fill_rows();
                    }
                ));

                self.verification
                    .set(verification, vec![sas_data_changed_handler]);
            }

            obj.update_labels();
            obj.fill_rows();
            obj.notify_verification();
        }
    }
}

glib::wrapper! {
    /// A page to confirm if SAS verification data matches.
    pub struct SasPage(ObjectSubclass<imp::SasPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[gtk::template_callbacks]
impl SasPage {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Update the labels for the current verification.
    fn update_labels(&self) {
        let Some(verification) = self.verification() else {
            return;
        };
        let imp = self.imp();

        if verification.is_self_verification() {
            imp.title.set_label(&gettext("Verify Session"));
            if verification.sas_supports_emoji() {
                imp.instructions.set_label(&gettext(
                    "Check if the same emoji appear in the same order on the other client.",
                ));
            } else {
                imp.instructions.set_label(&gettext(
                    "Check if the same numbers appear in the same order on the other client.",
                ));
            }
        } else {
            let name = verification.user().display_name();
            imp.title.set_markup(&gettext("Verification Request"));
            if verification.sas_supports_emoji() {
                imp.instructions.set_markup(&gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "Ask {user} if they see the following emoji appear in the same order on their screen.",
                    &[("user", &format!("<b>{name}</b>"))]
                ));
            } else {
                imp.instructions.set_markup(&gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "Ask {user} if they see the following numbers appear in the same order on their screen.",
                    &[("user", &format!("<b>{name}</b>"))]
                ));
            }
        }
    }

    /// Reset the UI to its initial state.
    pub fn reset(&self) {
        self.reset_buttons();
        self.fill_rows();
    }

    /// Reset the buttons to their initial state.
    fn reset_buttons(&self) {
        let imp = self.imp();

        imp.mismatch_btn.set_is_loading(false);
        imp.match_btn.set_is_loading(false);
        self.set_sensitive(true);
    }

    /// Empty the rows.
    fn clean_rows(&self) {
        let imp = self.imp();

        while let Some(child) = imp.row_1.first_child() {
            imp.row_1.remove(&child);
        }

        while let Some(child) = imp.row_2.first_child() {
            imp.row_2.remove(&child);
        }
    }

    /// Fill the rows with the current SAS data.
    fn fill_rows(&self) {
        let Some(verification) = self.verification() else {
            return;
        };
        let imp = self.imp();

        // Make sure the rows are empty.
        self.clean_rows();

        if let Some(emoji_list) = verification.sas_emoji() {
            let emoji_i18n = sas_emoji_i18n();
            for (index, emoji) in emoji_list.iter().enumerate() {
                let emoji_name = emoji_i18n
                    .get(emoji.description)
                    .map_or(emoji.description, String::as_str);
                let emoji_widget = SasEmoji::new(emoji.symbol, emoji_name);

                if index < 4 {
                    imp.row_1.append(&emoji_widget);
                } else {
                    imp.row_2.append(&emoji_widget);
                }
            }
        } else if let Some((a, b, c)) = verification.sas_decimals() {
            let container = gtk::Box::builder()
                .spacing(24)
                .css_classes(["emoji"])
                .build();
            container.append(&gtk::Label::builder().label(a.to_string()).build());
            container.append(&gtk::Label::builder().label(b.to_string()).build());
            container.append(&gtk::Label::builder().label(c.to_string()).build());
            imp.row_1.append(&container);
        }
    }

    #[template_callback]
    async fn data_mismatch(&self) {
        let Some(verification) = self.verification() else {
            return;
        };

        self.imp().mismatch_btn.set_is_loading(true);
        self.set_sensitive(false);

        if verification.sas_mismatch().await.is_err() {
            toast!(self, gettext("Could not send that the data does not match"));
            self.reset_buttons();
        }
    }

    #[template_callback]
    async fn data_match(&self) {
        let Some(verification) = self.verification() else {
            return;
        };

        self.imp().match_btn.set_is_loading(true);
        self.set_sensitive(false);

        if verification.sas_match().await.is_err() {
            toast!(
                self,
                gettext("Could not send confirmation that the data matches")
            );
            self.reset_buttons();
        }
    }
}

/// Get the SAS emoji translations for the current locale.
///
/// Returns a map of emoji name to its translation.
fn sas_emoji_i18n() -> HashMap<String, String> {
    for lang in glib::language_names()
        .into_iter()
        .flat_map(|locale| glib::locale_variants(&locale))
    {
        if let Some(emoji_i18n) = gio::resources_lookup_data(
            &format!("/org/tunaos/mandelbrot/sas-emoji/{lang}.json"),
            gio::ResourceLookupFlags::NONE,
        )
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        {
            return emoji_i18n;
        }
    }

    HashMap::new()
}
