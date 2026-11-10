use gtk::{
    glib,
    glib::{clone, closure_local},
    prelude::*,
    subclass::prelude::*,
};

use crate::components::LoadingBin;

mod imp {
    use std::{marker::PhantomData, sync::LazyLock};

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/rows/loading_row.ui")]
    #[properties(wrapper_type = super::LoadingRow)]
    pub struct LoadingRow {
        #[template_child]
        loading_bin: TemplateChild<LoadingBin>,
        #[template_child]
        error_label: TemplateChild<gtk::Label>,
        #[template_child]
        retry_button: TemplateChild<gtk::Button>,
        /// The error message to display.
        #[property(get = Self::error, set = Self::set_error, explicit_notify, nullable)]
        error: PhantomData<Option<glib::GString>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LoadingRow {
        const NAME: &'static str = "LoadingRow";
        type Type = super::LoadingRow;
        type ParentType = gtk::ListBoxRow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for LoadingRow {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("retry").build()]);
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.retry_button.connect_clicked(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.emit_by_name::<()>("retry", &[]);
                }
            ));
        }
    }

    impl WidgetImpl for LoadingRow {}
    impl ListBoxRowImpl for LoadingRow {}

    impl LoadingRow {
        /// The error message to display.
        fn error(&self) -> Option<glib::GString> {
            let message = self.error_label.text();
            if message.is_empty() {
                None
            } else {
                Some(message)
            }
        }

        /// Set the error message to display.
        ///
        /// If this is `Some`, the error will be shown, otherwise the spinner
        /// will be shown.
        fn set_error(&self, message: Option<&str>) {
            if let Some(message) = message {
                self.error_label.set_text(message);
                self.loading_bin.set_is_loading(false);
            } else {
                self.loading_bin.set_is_loading(true);
            }

            self.obj().notify_error();
        }
    }
}

glib::wrapper! {
    /// A `ListBoxRow` containing a loading spinner.
    ///
    /// It's also possible to set an error once the loading fails, including a retry button.
    pub struct LoadingRow(ObjectSubclass<imp::LoadingRow>)
        @extends gtk::Widget, gtk::ListBoxRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl LoadingRow {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Connect to the signal emitted when the retry button is clicked.
    pub fn connect_retry<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "retry",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}

impl Default for LoadingRow {
    fn default() -> Self {
        Self::new()
    }
}
