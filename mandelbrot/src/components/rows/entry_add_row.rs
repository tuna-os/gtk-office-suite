use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::closure_local};

use crate::components::LoadingButton;

mod imp {
    use std::{cell::Cell, marker::PhantomData, sync::LazyLock};

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/rows/entry_add_row.ui")]
    #[properties(wrapper_type = super::EntryAddRow)]
    pub struct EntryAddRow {
        #[template_child]
        add_button: TemplateChild<LoadingButton>,
        /// The tooltip text of the add button.
        #[property(get = Self::add_button_tooltip_text, set = Self::set_add_button_tooltip_text, explicit_notify, nullable)]
        add_button_tooltip_text: PhantomData<Option<glib::GString>>,
        /// Whether to prevent the add button from being activated.
        #[property(get, set = Self::set_inhibit_add, explicit_notify)]
        inhibit_add: Cell<bool>,
        /// Whether this row is loading.
        #[property(get = Self::is_loading, set = Self::set_is_loading, explicit_notify)]
        is_loading: PhantomData<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EntryAddRow {
        const NAME: &'static str = "EntryAddRow";
        type Type = super::EntryAddRow;
        type ParentType = adw::EntryRow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for EntryAddRow {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("add").build()]);
            SIGNALS.as_ref()
        }
    }

    impl WidgetImpl for EntryAddRow {}
    impl ListBoxRowImpl for EntryAddRow {}
    impl PreferencesRowImpl for EntryAddRow {}
    impl ActionRowImpl for EntryAddRow {}
    impl EntryRowImpl for EntryAddRow {}

    #[gtk::template_callbacks]
    impl EntryAddRow {
        /// The tooltip text of the add button.
        fn add_button_tooltip_text(&self) -> Option<glib::GString> {
            self.add_button.tooltip_text()
        }

        /// Set the tooltip text of the add button.
        fn set_add_button_tooltip_text(&self, tooltip_text: Option<&str>) {
            if self.add_button_tooltip_text().as_deref() == tooltip_text {
                return;
            }

            self.add_button.set_tooltip_text(tooltip_text);
            self.obj().notify_add_button_tooltip_text();
        }

        /// Set whether to prevent the add button from being activated.
        fn set_inhibit_add(&self, inhibit: bool) {
            if self.inhibit_add.get() == inhibit {
                return;
            }

            self.inhibit_add.set(inhibit);

            self.update_add_button();
            self.obj().notify_inhibit_add();
        }

        /// Whether this row is loading.
        fn is_loading(&self) -> bool {
            self.add_button.is_loading()
        }

        /// Set whether this row is loading.
        fn set_is_loading(&self, is_loading: bool) {
            if self.is_loading() == is_loading {
                return;
            }

            self.add_button.set_is_loading(is_loading);

            let obj = self.obj();
            obj.set_sensitive(!is_loading);
            obj.notify_is_loading();
        }

        /// Whether the add button can be activated.
        fn can_add(&self) -> bool {
            !self.inhibit_add.get() && !self.obj().text().is_empty()
        }

        /// Update the state of the add button.
        #[template_callback]
        fn update_add_button(&self) {
            self.add_button.set_sensitive(self.can_add());
        }

        /// Emit the `add` signal.
        #[template_callback]
        fn add(&self) {
            if !self.can_add() {
                return;
            }

            self.obj().emit_by_name::<()>("add", &[]);
        }
    }
}

glib::wrapper! {
    /// An `AdwEntryRow` with an "Add" button.
    pub struct EntryAddRow(ObjectSubclass<imp::EntryAddRow>)
        @extends gtk::Widget, gtk::ListBoxRow, adw::PreferencesRow, adw::ActionRow, adw::EntryRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable, gtk::Editable;
}

impl EntryAddRow {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Connect to the signal emitted when the "Add" button is activated.
    pub fn connect_add<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "add",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}
