use adw::{prelude::*, subclass::prelude::*};
use gtk::{
    glib,
    glib::{clone, closure_local},
};

use crate::components::LoadingBin;

mod imp {
    use std::{marker::PhantomData, sync::LazyLock};

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/rows/loading_button_row.ui")]
    #[properties(wrapper_type = super::LoadingButtonRow)]
    pub struct LoadingButtonRow {
        #[template_child]
        loading_bin: TemplateChild<LoadingBin>,
        /// Whether the button row is loading.
        #[property(get = Self::is_loading, set = Self::set_is_loading)]
        is_loading: PhantomData<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LoadingButtonRow {
        const NAME: &'static str = "LoadingButtonRow";
        type Type = super::LoadingButtonRow;
        type ParentType = adw::PreferencesRow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_css_name("row");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for LoadingButtonRow {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("activated").build()]);
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.obj().connect_parent_notify(|obj| {
                if let Some(listbox) = obj.parent().and_downcast_ref::<gtk::ListBox>() {
                    listbox.connect_row_activated(clone!(
                        #[weak]
                        obj,
                        move |_, row| {
                            if *row == obj {
                                obj.emit_by_name::<()>("activated", &[]);
                            }
                        }
                    ));
                }
            });
        }
    }

    impl WidgetImpl for LoadingButtonRow {}
    impl ListBoxRowImpl for LoadingButtonRow {}
    impl PreferencesRowImpl for LoadingButtonRow {}

    impl LoadingButtonRow {
        /// Whether the row is loading.
        fn is_loading(&self) -> bool {
            self.loading_bin.is_loading()
        }

        /// Set whether the row is loading.
        fn set_is_loading(&self, loading: bool) {
            if self.is_loading() == loading {
                return;
            }

            self.loading_bin.set_is_loading(loading);

            let obj = self.obj();
            obj.set_activatable(!loading);
            obj.notify_is_loading();
        }
    }
}

glib::wrapper! {
    /// An `AdwPreferencesRow` usable as a button with a loading state.
    pub struct LoadingButtonRow(ObjectSubclass<imp::LoadingButtonRow>)
        @extends gtk::Widget, gtk::ListBoxRow, adw::PreferencesRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl LoadingButtonRow {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Connect to the signal emitted when the row is activated.
    pub fn connect_activated<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "activated",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}
