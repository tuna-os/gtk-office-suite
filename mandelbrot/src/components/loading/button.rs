use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, pango};

use super::LoadingBin;
use crate::prelude::*;

mod imp {
    use std::marker::PhantomData;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::LoadingButton)]
    pub struct LoadingButton {
        loading_bin: LoadingBin,
        /// The label of the content of the button.
        ///
        /// If an icon was set, it is removed.
        #[property(get = Self::content_label, set = Self::set_content_label, explicit_notify)]
        content_label: PhantomData<Option<glib::GString>>,
        /// The name of the icon of the content of the button.
        ///
        /// If a label was set, it is removed.
        #[property(get = Self::content_icon_name, set = Self::set_content_icon_name, explicit_notify)]
        content_icon_name: PhantomData<Option<glib::GString>>,
        /// Whether to display the loading spinner.
        ///
        /// If this is `false`, the text or icon will be displayed.
        #[property(get = Self::is_loading, set = Self::set_is_loading, explicit_notify)]
        is_loading: PhantomData<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LoadingButton {
        const NAME: &'static str = "LoadingButton";
        type Type = super::LoadingButton;
        type ParentType = gtk::Button;
    }

    #[glib::derived_properties]
    impl ObjectImpl for LoadingButton {
        fn constructed(&self) {
            self.parent_constructed();

            self.obj().set_child(Some(&self.loading_bin));
        }
    }

    impl WidgetImpl for LoadingButton {}
    impl ButtonImpl for LoadingButton {}

    impl LoadingButton {
        /// The label of the content of the button.
        fn content_label(&self) -> Option<glib::GString> {
            self.loading_bin
                .child()
                .and_downcast::<gtk::Label>()
                .map(|l| l.label())
                .filter(|s| !s.is_empty())
        }

        /// Set the label of the content of the button.
        fn set_content_label(&self, label: &str) {
            if self.content_label().as_deref() == Some(label) {
                return;
            }
            let obj = self.obj();

            let child_label = self.loading_bin.child_or_else::<gtk::Label>(|| {
                let child_label = gtk::Label::builder()
                    .ellipsize(pango::EllipsizeMode::End)
                    .use_underline(true)
                    .mnemonic_widget(&*obj)
                    .css_classes(["text-button"])
                    .build();

                // In case it was an image before.
                obj.remove_css_class("image-button");
                obj.update_relation(&[gtk::accessible::Relation::LabelledBy(&[
                    child_label.upcast_ref()
                ])]);

                child_label
            });

            child_label.set_label(label);

            obj.notify_content_label();
        }

        /// The name of the icon of the content of the button.
        fn content_icon_name(&self) -> Option<glib::GString> {
            self.loading_bin
                .child()
                .and_downcast::<gtk::Image>()
                .and_then(|i| i.icon_name())
        }

        /// Set the name of the icon of the content of the button.
        fn set_content_icon_name(&self, icon_name: &str) {
            if self.content_icon_name().as_deref() == Some(icon_name) {
                return;
            }
            let obj = self.obj();

            let child_image = self.loading_bin.child_or_else::<gtk::Image>(|| {
                obj.add_css_class("image-button");

                gtk::Image::builder()
                    .accessible_role(gtk::AccessibleRole::Presentation)
                    .build()
            });

            child_image.set_icon_name(Some(icon_name));

            obj.notify_content_icon_name();
        }

        /// Whether to display the loading spinner.
        ///
        /// If this is `false`, the text will be displayed.
        fn is_loading(&self) -> bool {
            self.loading_bin.is_loading()
        }

        /// Set whether to display the loading spinner.
        fn set_is_loading(&self, is_loading: bool) {
            if self.is_loading() == is_loading {
                return;
            }
            let obj = self.obj();

            // The action should have been enabled or disabled so the sensitive
            // state should update itself.
            if obj.action_name().is_none() {
                obj.set_sensitive(!is_loading);
            }

            self.loading_bin.set_is_loading(is_loading);

            obj.notify_is_loading();
        }
    }
}

glib::wrapper! {
    /// Button showing either a spinner or a label.
    ///
    /// Use the `content-label` and `content-icon-name` properties instead of `label` and
    /// `icon-name` respectively, otherwise the spinner will not appear.
    pub struct LoadingButton(ObjectSubclass<imp::LoadingButton>)
        @extends gtk::Widget, gtk::Button,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl LoadingButton {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for LoadingButton {
    fn default() -> Self {
        Self::new()
    }
}
