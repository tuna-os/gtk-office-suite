use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use super::InviteItem;

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;
    use crate::utils::TemplateCallbacks;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/invite_subpage/row.ui"
    )]
    #[properties(wrapper_type = super::InviteRow)]
    pub struct InviteRow {
        #[template_child]
        check_button: TemplateChild<gtk::CheckButton>,
        /// The item displayed by this row.
        #[property(get, set = Self::set_item, explicit_notify, nullable)]
        item: RefCell<Option<InviteItem>>,
        binding: RefCell<Option<glib::Binding>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for InviteRow {
        const NAME: &'static str = "RoomDetailsInviteRow";
        type Type = super::InviteRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            TemplateCallbacks::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for InviteRow {}

    impl WidgetImpl for InviteRow {}
    impl BinImpl for InviteRow {}

    impl InviteRow {
        /// Set the item displayed by this row.
        fn set_item(&self, item: Option<InviteItem>) {
            if *self.item.borrow() == item {
                return;
            }

            if let Some(binding) = self.binding.take() {
                binding.unbind();
            }

            if let Some(item) = &item {
                // We can't use `gtk::Expression` because we need a bidirectional binding
                let binding = item
                    .bind_property("is-invitee", &*self.check_button, "active")
                    .sync_create()
                    .bidirectional()
                    .build();

                self.binding.replace(Some(binding));
            }

            self.item.replace(item);
            self.obj().notify_item();
        }
    }
}

glib::wrapper! {
    /// A row presenting an item of the result of a search in the user directory.
    pub struct InviteRow(ObjectSubclass<imp::InviteRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl InviteRow {
    pub fn new(item: &InviteItem) -> Self {
        glib::Object::builder().property("item", item).build()
    }
}
