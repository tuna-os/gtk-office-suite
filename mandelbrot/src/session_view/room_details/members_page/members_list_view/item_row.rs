use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use super::MembershipSubpageRow;
use crate::{
    prelude::*,
    session::Member,
    session_view::room_details::{MemberRow, MembershipSubpageItem},
};

mod imp {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::ItemRow)]
    pub struct ItemRow {
        /// The item represented by this row.
        ///
        /// It can be a `Member` or a `MemberSubpageItem`.
        #[property(get, set = Self::set_item, explicit_notify, nullable)]
        item: RefCell<Option<glib::Object>>,
        /// Whether this row can be activated.
        #[property(get)]
        activatable: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ItemRow {
        const NAME: &'static str = "ContentMemberItemRow";
        type Type = super::ItemRow;
        type ParentType = adw::Bin;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ItemRow {}

    impl WidgetImpl for ItemRow {}
    impl BinImpl for ItemRow {}

    impl ItemRow {
        /// Set the item represented by this row.
        ///
        /// It must be a `Member` or a `MemberSubpageItem`.
        fn set_item(&self, item: Option<glib::Object>) {
            if *self.item.borrow() == item {
                return;
            }
            let obj = self.obj();

            if let Some(item) = &item {
                if let Some(member) = item.downcast_ref::<Member>() {
                    let child = obj.child_or_else::<MemberRow>(|| MemberRow::new(true));
                    child.set_member(Some(member.clone()));
                    self.set_activatable(true);
                } else if let Some(item) = item.downcast_ref::<MembershipSubpageItem>() {
                    let child = obj.child_or_else::<MembershipSubpageRow>(|| {
                        let child = MembershipSubpageRow::new();
                        child.set_activatable(false);
                        child
                    });

                    child.set_item(Some(item.clone()));
                    self.set_activatable(true);
                } else if let Some(child) = item.downcast_ref::<gtk::Widget>() {
                    obj.set_child(Some(child));
                    self.set_activatable(false);
                } else {
                    unimplemented!("The object {item:?} doesn't have a widget implementation");
                }
            }

            self.item.replace(item);
            obj.notify_item();
        }

        /// Set whether this row can be activated.
        fn set_activatable(&self, activatable: bool) {
            if self.activatable.get() == activatable {
                return;
            }

            self.activatable.set(activatable);
            self.obj().notify_activatable();
        }
    }
}

glib::wrapper! {
    /// A row presenting an item in the list of room members.
    pub struct ItemRow(ObjectSubclass<imp::ItemRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ItemRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl IsABin for ItemRow {}
