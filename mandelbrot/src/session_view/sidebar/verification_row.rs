use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use crate::session::IdentityVerification;

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/sidebar/verification_row.ui")]
    #[properties(wrapper_type = super::SidebarVerificationRow)]
    pub struct SidebarVerificationRow {
        /// The identity verification represented by this row.
        #[property(get, set = Self::set_identity_verification, explicit_notify, nullable)]
        identity_verification: RefCell<Option<IdentityVerification>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarVerificationRow {
        const NAME: &'static str = "SidebarVerificationRow";
        type Type = super::SidebarVerificationRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarVerificationRow {}

    impl WidgetImpl for SidebarVerificationRow {}
    impl BinImpl for SidebarVerificationRow {}

    impl SidebarVerificationRow {
        /// Set the identity verification represented by this row.
        fn set_identity_verification(&self, verification: Option<IdentityVerification>) {
            if *self.identity_verification.borrow() == verification {
                return;
            }

            self.identity_verification.replace(verification);
            self.obj().notify_identity_verification();
        }
    }
}

glib::wrapper! {
    /// A sidebar row representing an identity verification.
    pub struct SidebarVerificationRow(ObjectSubclass<imp::SidebarVerificationRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SidebarVerificationRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for SidebarVerificationRow {
    fn default() -> Self {
        Self::new()
    }
}
