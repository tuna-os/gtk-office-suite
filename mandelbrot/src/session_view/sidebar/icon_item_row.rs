use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use crate::session::{SidebarIconItem, SidebarIconItemType};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/sidebar/icon_item_row.ui")]
    #[properties(wrapper_type = super::SidebarIconItemRow)]
    pub struct SidebarIconItemRow {
        /// The [`SidebarIconItem`] of this row.
        #[property(get, set = Self::set_icon_item, explicit_notify, nullable)]
        icon_item: RefCell<Option<SidebarIconItem>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarIconItemRow {
        const NAME: &'static str = "SidebarIconItemRow";
        type Type = super::SidebarIconItemRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_css_name("icon-item");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarIconItemRow {}

    impl WidgetImpl for SidebarIconItemRow {}
    impl BinImpl for SidebarIconItemRow {}

    impl SidebarIconItemRow {
        /// Set the [`SidebarIconItem`] of this row.
        fn set_icon_item(&self, icon_item: Option<SidebarIconItem>) {
            if *self.icon_item.borrow() == icon_item {
                return;
            }
            let obj = self.obj();

            if icon_item
                .as_ref()
                .is_some_and(|i| i.item_type() == SidebarIconItemType::Forget)
            {
                obj.add_css_class("forget");
            } else {
                obj.remove_css_class("forget");
            }

            self.icon_item.replace(icon_item);
            obj.notify_icon_item();
        }
    }
}

glib::wrapper! {
    /// A row in the sidebar presenting a [`SidebarIconItem`].
    pub struct SidebarIconItemRow(ObjectSubclass<imp::SidebarIconItemRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SidebarIconItemRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for SidebarIconItemRow {
    fn default() -> Self {
        Self::new()
    }
}
