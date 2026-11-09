use adw::subclass::prelude::*;
use gtk::{glib, prelude::*};

pub mod row;

use crate::session::Member;

mod imp {
    use std::cell::Cell;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::MemberTimestamp)]
    pub struct MemberTimestamp {
        /// The room member.
        #[property(get, construct_only)]
        member: glib::WeakRef<Member>,
        /// The timestamp, in seconds since Unix Epoch.
        ///
        /// A value of 0 means no timestamp.
        #[property(get, construct_only)]
        timestamp: Cell<u64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MemberTimestamp {
        const NAME: &'static str = "ContentMemberTimestamp";
        type Type = super::MemberTimestamp;
    }

    #[glib::derived_properties]
    impl ObjectImpl for MemberTimestamp {}
}

glib::wrapper! {
    /// A room member and a timestamp.
    pub struct MemberTimestamp(ObjectSubclass<imp::MemberTimestamp>);
}

impl MemberTimestamp {
    /// Constructs a new `MemberTimestamp` with the given member and
    /// timestamp.
    pub fn new(member: &Member, timestamp: Option<u64>) -> Self {
        glib::Object::builder()
            .property("member", member)
            .property("timestamp", timestamp.unwrap_or_default())
            .build()
    }
}
