use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use crate::toast;

/// The main title of an `AdwActionRow`.
#[derive(Debug, Default, Hash, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[repr(u32)]
#[enum_type(name = "ActionRowMainTitle")]
pub enum ActionRowMainTitle {
    /// The main title is the title.
    #[default]
    Title = 0,
    /// The main title is the subtitle.
    Subtitle = 1,
}

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
    };

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/rows/copyable_row.ui")]
    #[properties(wrapper_type = super::CopyableRow)]
    pub struct CopyableRow {
        #[template_child]
        copy_button: TemplateChild<gtk::Button>,
        #[template_child]
        extra_suffix_bin: TemplateChild<adw::Bin>,
        /// The tooltip text of the copy button.
        #[property(get = Self::copy_button_tooltip_text, set = Self::set_copy_button_tooltip_text, explicit_notify, nullable)]
        copy_button_tooltip_text: PhantomData<Option<glib::GString>>,
        /// The text to show in a toast when the copy button is activated.
        ///
        /// No toast is shown if this is `None`.
        #[property(get, set = Self::set_toast_text, explicit_notify, nullable)]
        toast_text: RefCell<Option<String>>,
        /// The main title of this row.
        ///
        /// This is used to decide the field to copy when the button is
        /// activated. Also, if the subtitle is the main title, the `property`
        /// CSS class is added.
        #[property(get, set = Self::set_main_title, explicit_notify, builder(ActionRowMainTitle::default()))]
        main_title: Cell<ActionRowMainTitle>,
        /// The extra suffix widget of this row.
        ///
        /// The widget is placed before the remove button.
        #[property(get = Self::extra_suffix, set = Self::set_extra_suffix, explicit_notify, nullable)]
        extra_suffix: PhantomData<Option<gtk::Widget>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CopyableRow {
        const NAME: &'static str = "CopyableRow";
        type Type = super::CopyableRow;
        type ParentType = adw::ActionRow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.install_action("copyable-row.copy", None, |obj, _, _| {
                let imp = obj.imp();

                let text = match imp.main_title.get() {
                    ActionRowMainTitle::Title => obj.title(),
                    ActionRowMainTitle::Subtitle => obj.subtitle().unwrap_or_default(),
                };

                obj.clipboard().set_text(&text);

                if let Some(toast_text) = imp.toast_text.borrow().clone() {
                    toast!(obj, toast_text);
                }
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for CopyableRow {}

    impl WidgetImpl for CopyableRow {}
    impl ListBoxRowImpl for CopyableRow {}
    impl PreferencesRowImpl for CopyableRow {}
    impl ActionRowImpl for CopyableRow {}

    impl CopyableRow {
        /// The tooltip text of the copy button.
        fn copy_button_tooltip_text(&self) -> Option<glib::GString> {
            self.copy_button.tooltip_text()
        }

        /// Set the tooltip text of the copy button.
        fn set_copy_button_tooltip_text(&self, tooltip_text: Option<&str>) {
            if self.copy_button_tooltip_text().as_deref() == tooltip_text {
                return;
            }

            self.copy_button.set_tooltip_text(tooltip_text);
            self.obj().notify_copy_button_tooltip_text();
        }

        /// Set the text to show in a toast when the copy button is activated.
        fn set_toast_text(&self, text: Option<String>) {
            if *self.toast_text.borrow() == text {
                return;
            }

            self.toast_text.replace(text);
            self.obj().notify_toast_text();
        }

        /// Set the main title of this row.
        fn set_main_title(&self, main_title: ActionRowMainTitle) {
            if self.main_title.get() == main_title {
                return;
            }
            let obj = self.obj();

            if main_title == ActionRowMainTitle::Title {
                obj.remove_css_class("property");
            } else {
                obj.add_css_class("property");
            }

            self.main_title.set(main_title);
            obj.notify_main_title();
        }

        /// The extra suffix widget of this row.
        fn extra_suffix(&self) -> Option<gtk::Widget> {
            self.extra_suffix_bin.child()
        }

        /// Set the extra suffix widget of this row.
        fn set_extra_suffix(&self, widget: Option<&gtk::Widget>) {
            if self.extra_suffix().as_ref() == widget {
                return;
            }

            self.extra_suffix_bin.set_child(widget);
            self.obj().notify_extra_suffix();
        }
    }
}

glib::wrapper! {
    /// An `AdwActionRow` with a button to copy the title or subtitle.
    pub struct CopyableRow(ObjectSubclass<imp::CopyableRow>)
        @extends gtk::Widget, gtk::ListBoxRow, adw::PreferencesRow, adw::ActionRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl CopyableRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
