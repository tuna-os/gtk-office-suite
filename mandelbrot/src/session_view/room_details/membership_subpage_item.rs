use gtk::{
    gio, glib,
    glib::{prelude::*, subclass::prelude::*},
};

use crate::session::MembershipListKind;

mod imp {
    use std::cell::{Cell, OnceCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::MembershipSubpageItem)]
    pub struct MembershipSubpageItem {
        /// The kind of membership list.
        #[property(get, construct_only, builder(MembershipListKind::default()))]
        kind: Cell<MembershipListKind>,
        /// The model used for the subpage.
        #[property(get, construct_only)]
        model: OnceCell<gio::ListModel>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MembershipSubpageItem {
        const NAME: &'static str = "MembersPageMembershipSubpageItem";
        type Type = super::MembershipSubpageItem;
    }

    #[glib::derived_properties]
    impl ObjectImpl for MembershipSubpageItem {}
}

glib::wrapper! {
    /// An item representing a subpage for a subset of the list of room members filtered by membership.
    pub struct MembershipSubpageItem(ObjectSubclass<imp::MembershipSubpageItem>);
}

impl MembershipSubpageItem {
    /// Construct a `MembershipSubpageItem` for the given membership list kind.
    pub fn new(kind: MembershipListKind, model: &gio::ListModel) -> Self {
        glib::Object::builder()
            .property("kind", kind)
            .property("model", model)
            .build()
    }
}
