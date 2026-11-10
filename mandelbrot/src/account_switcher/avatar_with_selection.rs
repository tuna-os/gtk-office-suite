use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use crate::components::{Avatar, AvatarData};

mod imp {
    use std::marker::PhantomData;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/account_switcher/avatar_with_selection.ui")]
    #[properties(wrapper_type = super::AvatarWithSelection)]
    pub struct AvatarWithSelection {
        #[template_child]
        child_avatar: TemplateChild<Avatar>,
        #[template_child]
        checkmark: TemplateChild<gtk::Image>,
        /// The [`AvatarData`] displayed by this widget.
        #[property(get = Self::data, set = Self::set_data, explicit_notify, nullable)]
        data: PhantomData<Option<AvatarData>>,
        /// The size of the Avatar.
        #[property(get = Self::size, set = Self::set_size, minimum = -1, default = -1)]
        size: PhantomData<i32>,
        /// Whether this avatar is selected.
        #[property(get = Self::is_selected, set = Self::set_selected, explicit_notify)]
        selected: PhantomData<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AvatarWithSelection {
        const NAME: &'static str = "AvatarWithSelection";
        type Type = super::AvatarWithSelection;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AvatarWithSelection {}

    impl WidgetImpl for AvatarWithSelection {}
    impl BinImpl for AvatarWithSelection {}

    impl AccessibleImpl for AvatarWithSelection {
        fn first_accessible_child(&self) -> Option<gtk::Accessible> {
            // Hide the children in the a11y tree.
            None
        }
    }

    impl AvatarWithSelection {
        /// Whether this avatar is selected.
        fn is_selected(&self) -> bool {
            self.checkmark.get_visible()
        }

        /// Set whether this avatar is selected.
        fn set_selected(&self, selected: bool) {
            if self.is_selected() == selected {
                return;
            }

            self.checkmark.set_visible(selected);

            if selected {
                self.child_avatar.add_css_class("selected-avatar");
            } else {
                self.child_avatar.remove_css_class("selected-avatar");
            }

            self.obj().notify_selected();
        }

        /// The [`AvatarData`] displayed by this widget.
        fn data(&self) -> Option<AvatarData> {
            self.child_avatar.data()
        }

        /// Set the [`AvatarData`] displayed by this widget.
        fn set_data(&self, data: Option<AvatarData>) {
            self.child_avatar.set_data(data);
        }

        /// The size of the Avatar.
        fn size(&self) -> i32 {
            self.child_avatar.size()
        }

        /// Set the size of the Avatar.
        fn set_size(&self, size: i32) {
            self.child_avatar.set_size(size);
        }
    }
}

glib::wrapper! {
    /// A widget displaying an [`Avatar`] and an optional selected effect.
    pub struct AvatarWithSelection(ObjectSubclass<imp::AvatarWithSelection>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AvatarWithSelection {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
