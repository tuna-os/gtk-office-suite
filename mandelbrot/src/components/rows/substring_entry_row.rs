use adw::{prelude::*, subclass::prelude::*};
use gtk::{
    glib,
    glib::{clone, closure_local},
    pango,
};

use crate::components::LoadingButton;

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
        sync::LazyLock,
    };

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/rows/substring_entry_row.ui")]
    #[properties(wrapper_type = super::SubstringEntryRow)]
    pub struct SubstringEntryRow {
        #[template_child]
        header: TemplateChild<gtk::Box>,
        #[template_child]
        main_content: TemplateChild<gtk::Box>,
        #[template_child]
        entry_box: TemplateChild<gtk::Box>,
        #[template_child]
        text: TemplateChild<gtk::Text>,
        #[template_child]
        title: TemplateChild<gtk::Label>,
        #[template_child]
        edit_icon: TemplateChild<gtk::Image>,
        #[template_child]
        entry_prefix_label: TemplateChild<gtk::Label>,
        #[template_child]
        entry_suffix_label: TemplateChild<gtk::Label>,
        #[template_child]
        add_button: TemplateChild<LoadingButton>,
        /// The input hints of the entry.
        #[property(get = Self::input_hints, set = Self::set_input_hints, explicit_notify)]
        input_hints: PhantomData<gtk::InputHints>,
        /// The input purpose of the entry.
        #[property(get = Self::input_purpose, set = Self::set_input_purpose, explicit_notify, builder(gtk::InputPurpose::FreeForm))]
        input_purpose: PhantomData<gtk::InputPurpose>,
        /// A list of Pango attributes to apply to the text of the entry.
        #[property(get = Self::attributes, set = Self::set_attributes, explicit_notify, nullable)]
        attributes: PhantomData<Option<pango::AttrList>>,
        /// The placeholder text of the entry.
        #[property(get = Self::placeholder_text, set = Self::set_placeholder_text, explicit_notify, nullable)]
        placeholder_text: PhantomData<Option<glib::GString>>,
        /// The length of the text of the entry.
        #[property(get = Self::text_length)]
        text_length: PhantomData<u32>,
        /// The prefix text of the entry.
        #[property(get = Self::prefix_text, set = Self::set_prefix_text, explicit_notify)]
        prefix_text: PhantomData<glib::GString>,
        /// The suffix text of the entry.
        #[property(get = Self::suffix_text, set = Self::set_suffix_text, explicit_notify)]
        suffix_text: PhantomData<glib::GString>,
        /// Set the accessible description of the entry.
        ///
        /// If it is not set, the placeholder text will be used.
        #[property(get, set = Self::set_accessible_description, explicit_notify, nullable)]
        accessible_description: RefCell<Option<String>>,
        /// Whether the add button is hidden.
        #[property(get = Self::hide_add_button, set = Self::set_hide_add_button, explicit_notify)]
        hide_add_button: PhantomData<bool>,
        /// The tooltip text of the add button.
        #[property(get = Self::add_button_tooltip_text, set = Self::set_add_button_tooltip_text, explicit_notify, nullable)]
        add_button_tooltip_text: PhantomData<Option<glib::GString>>,
        /// The accessible label of the add button.
        #[property(get, set = Self::set_add_button_accessible_label, explicit_notify, nullable)]
        add_button_accessible_label: RefCell<Option<String>>,
        /// Whether to prevent the add button from being activated.
        #[property(get, set = Self::set_inhibit_add, explicit_notify)]
        inhibit_add: Cell<bool>,
        /// Whether this row is loading.
        #[property(get = Self::is_loading, set = Self::set_is_loading, explicit_notify)]
        is_loading: PhantomData<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SubstringEntryRow {
        const NAME: &'static str = "SubstringEntryRow";
        type Type = super::SubstringEntryRow;
        type ParentType = adw::PreferencesRow;
        type Interfaces = (gtk::Editable,);

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SubstringEntryRow {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("add").build()]);
            SIGNALS.as_ref()
        }

        fn properties() -> &'static [glib::ParamSpec] {
            Self::derived_properties()
        }

        fn set_property(&self, id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            // In case this is a property that's automatically added for Editable
            // implementations.
            if !self.delegate_set_property(id, value, pspec) {
                self.derived_set_property(id, value, pspec);
            }
        }

        fn property(&self, id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            // In case this is a property that's automatically added for Editable
            // implementations.
            if let Some(value) = self.delegate_get_property(id, pspec) {
                value
            } else {
                self.derived_property(id, pspec)
            }
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            obj.init_delegate();

            self.text.buffer().connect_length_notify(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.notify_text_length();
                }
            ));
        }

        fn dispose(&self) {
            self.obj().finish_delegate();
        }
    }

    impl WidgetImpl for SubstringEntryRow {
        fn grab_focus(&self) -> bool {
            self.text.grab_focus()
        }
    }

    impl ListBoxRowImpl for SubstringEntryRow {}
    impl PreferencesRowImpl for SubstringEntryRow {}

    impl EditableImpl for SubstringEntryRow {
        fn delegate(&self) -> Option<gtk::Editable> {
            Some(self.text.clone().upcast())
        }
    }

    #[gtk::template_callbacks]
    impl SubstringEntryRow {
        /// The input hints of the entry.
        fn input_hints(&self) -> gtk::InputHints {
            self.text.input_hints()
        }

        /// Set the input hints of the entry.
        fn set_input_hints(&self, input_hints: gtk::InputHints) {
            if self.input_hints() == input_hints {
                return;
            }

            self.text.set_input_hints(input_hints);
            self.obj().notify_input_hints();
        }

        /// The input purpose of the entry.
        fn input_purpose(&self) -> gtk::InputPurpose {
            self.text.input_purpose()
        }

        /// Set the input purpose of the entry.
        fn set_input_purpose(&self, input_purpose: gtk::InputPurpose) {
            if self.input_purpose() == input_purpose {
                return;
            }

            self.text.set_input_purpose(input_purpose);
            self.obj().notify_input_purpose();
        }

        /// A list of Pango attributes to apply to the text of the entry.
        fn attributes(&self) -> Option<pango::AttrList> {
            self.text.attributes()
        }

        /// Set the list of Pango attributes to apply to the text of the entry.
        fn set_attributes(&self, attributes: Option<&pango::AttrList>) {
            if self.attributes().as_ref() == attributes {
                return;
            }

            self.text.set_attributes(attributes);
            self.obj().notify_attributes();
        }

        /// The placeholder text of the entry.
        fn placeholder_text(&self) -> Option<glib::GString> {
            self.text.placeholder_text()
        }

        /// Set the placeholder text of the entry.
        fn set_placeholder_text(&self, text: Option<&str>) {
            if self.placeholder_text().as_deref() == text {
                return;
            }

            self.text.set_placeholder_text(text);

            self.update_accessible_description();
            self.obj().notify_placeholder_text();
        }

        /// The length of the text of the entry.
        fn text_length(&self) -> u32 {
            self.text.text_length().into()
        }

        /// The prefix text of the entry.
        fn prefix_text(&self) -> glib::GString {
            self.entry_prefix_label.label()
        }

        /// Set the prefix text of the entry.
        fn set_prefix_text(&self, text: &str) {
            if self.prefix_text() == text {
                return;
            }

            self.entry_prefix_label.set_label(text);
            self.obj().notify_prefix_text();
        }

        /// The suffix text of the entry.
        fn suffix_text(&self) -> glib::GString {
            self.entry_suffix_label.label()
        }

        /// Set the suffix text of the entry.
        fn set_suffix_text(&self, text: &str) {
            if self.suffix_text() == text {
                return;
            }

            self.entry_suffix_label.set_label(text);
            self.obj().notify_suffix_text();
        }

        /// Set the accessible description of the entry.
        fn set_accessible_description(&self, description: Option<String>) {
            if *self.accessible_description.borrow() == description {
                return;
            }

            self.accessible_description.replace(description);

            self.update_accessible_description();
            self.obj().notify_accessible_description();
        }

        /// Whether the add button is hidden.
        fn hide_add_button(&self) -> bool {
            !self.add_button.is_visible()
        }

        /// Set whether the add button is hidden.
        fn set_hide_add_button(&self, hide: bool) {
            if self.hide_add_button() == hide {
                return;
            }

            self.add_button.set_visible(!hide);
            self.obj().notify_hide_add_button();
        }

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

        /// Set the accessible label of the add button.
        fn set_add_button_accessible_label(&self, label: Option<String>) {
            if *self.add_button_accessible_label.borrow() == label {
                return;
            }

            if let Some(label) = &label {
                self.add_button
                    .update_property(&[gtk::accessible::Property::Label(label)]);
            } else {
                self.add_button
                    .reset_property(gtk::AccessibleProperty::Label);
            }

            self.add_button_accessible_label.replace(label);
            self.obj().notify_add_button_accessible_label();
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

        /// Update the accessible description.
        fn update_accessible_description(&self) {
            let description = self
                .accessible_description
                .borrow()
                .clone()
                .or(self.placeholder_text().map(Into::into));

            if let Some(description) = description {
                self.text
                    .update_property(&[gtk::accessible::Property::Description(&description)]);
            } else {
                self.text
                    .reset_property(gtk::AccessibleProperty::Description);
            }
        }

        /// Whether the text input is focused.
        fn is_text_focused(&self) -> bool {
            self.text
                .state_flags()
                .contains(gtk::StateFlags::FOCUS_WITHIN)
        }

        /// Update this row when the text input flags changed.
        #[template_callback]
        fn text_state_flags_changed(&self) {
            let obj = self.obj();
            let editing = self.is_text_focused();

            if editing {
                obj.add_css_class("focused");
            } else {
                obj.remove_css_class("focused");
            }

            self.edit_icon.set_visible(!editing);
        }

        /// Handle when the key navigation in the text input failed.
        #[template_callback]
        fn text_keynav_failed(&self, direction: gtk::DirectionType) -> bool {
            if matches!(
                direction,
                gtk::DirectionType::Left | gtk::DirectionType::Right
            ) {
                return self.obj().child_focus(direction);
            }

            // gdk::EVENT_PROPAGATE == 0;
            false
        }

        /// Handle when this row is pressed.
        #[template_callback]
        fn pressed(&self, _n_press: i32, x: f64, y: f64, gesture: &gtk::Gesture) {
            let obj = self.obj();
            let picked = obj.pick(x, y, gtk::PickFlags::DEFAULT);

            if picked.is_some_and(|w| {
                w != *obj || w != *self.header || w != *self.main_content || w != *self.entry_box
            }) {
                gesture.set_state(gtk::EventSequenceState::Denied);

                return;
            }

            self.text.grab_focus_without_selecting();

            gesture.set_state(gtk::EventSequenceState::Claimed);
        }

        /// Whether we can activate the add button.
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
    /// A `AdwPreferencesRow` with an embedded text entry, and a fixed text suffix and prefix.
    ///
    /// It also has a built-in "Add" button, making it an almost drop-in replacement to `EntryAddRow`.
    pub struct SubstringEntryRow(ObjectSubclass<imp::SubstringEntryRow>)
        @extends gtk::Widget, gtk::ListBoxRow, adw::PreferencesRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable, gtk::Editable;
}

impl SubstringEntryRow {
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
