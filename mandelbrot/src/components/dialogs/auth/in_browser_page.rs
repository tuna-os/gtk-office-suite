use std::fmt::Debug;

use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;
use tracing::error;

use crate::components::LoadingButton;

mod imp {
    use std::cell::OnceCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/dialogs/auth/in_browser_page.ui")]
    #[properties(wrapper_type = super::AuthDialogInBrowserPage)]
    pub struct AuthDialogInBrowserPage {
        #[template_child]
        pub(super) confirm_button: TemplateChild<LoadingButton>,
        /// The URL to launch.
        #[property(get, construct_only)]
        url: OnceCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AuthDialogInBrowserPage {
        const NAME: &'static str = "AuthDialogInBrowserPage";
        type Type = super::AuthDialogInBrowserPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AuthDialogInBrowserPage {}

    impl WidgetImpl for AuthDialogInBrowserPage {}
    impl BinImpl for AuthDialogInBrowserPage {}

    #[gtk::template_callbacks]
    impl AuthDialogInBrowserPage {
        /// Open the URL in the browser.
        #[template_callback]
        async fn launch_url(&self) {
            let url = self
                .url
                .get()
                .expect("URL should be set during construction");

            if let Err(error) = gtk::UriLauncher::new(url)
                .launch_future(self.obj().root().and_downcast_ref::<gtk::Window>())
                .await
            {
                error!("Could not launch authentication URI: {error}");
            }

            self.confirm_button.set_sensitive(true);
        }

        /// Proceed to authentication with the current password.
        #[template_callback]
        fn proceed(&self) {
            self.confirm_button.set_is_loading(true);
            let _ = self.obj().activate_action("auth-dialog.continue", None);
        }

        /// Retry this stage.
        pub(super) fn retry(&self) {
            self.confirm_button.set_is_loading(false);
        }
    }
}

glib::wrapper! {
    /// Page to pass a stage in the browser for the [`AuthDialog`].
    pub struct AuthDialogInBrowserPage(ObjectSubclass<imp::AuthDialogInBrowserPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AuthDialogInBrowserPage {
    /// Construct an `AuthDialogInBrowserPage` that will launch the given URL.
    pub fn new(url: String) -> Self {
        glib::Object::builder().property("url", url).build()
    }

    /// Get the default widget of this page.
    pub fn default_widget(&self) -> &gtk::Widget {
        self.imp().confirm_button.upcast_ref()
    }

    /// Retry this stage.
    pub fn retry(&self) {
        self.imp().retry();
    }
}
