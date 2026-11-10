use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use crate::{session::Member, utils::bool_to_accessible_tristate};

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/permissions/select_member_row.ui"
    )]
    #[properties(wrapper_type = super::PermissionsSelectMemberRow)]
    pub struct PermissionsSelectMemberRow {
        /// The room member displayed by this row.
        #[property(get, set = Self::set_member, explicit_notify, nullable)]
        member: RefCell<Option<Member>>,
        /// Whether this row is selected.
        #[property(get, set = Self::set_selected, explicit_notify)]
        selected: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PermissionsSelectMemberRow {
        const NAME: &'static str = "RoomDetailsPermissionsSelectMemberRow";
        type Type = super::PermissionsSelectMemberRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for PermissionsSelectMemberRow {}

    impl WidgetImpl for PermissionsSelectMemberRow {}
    impl BinImpl for PermissionsSelectMemberRow {}

    impl PermissionsSelectMemberRow {
        /// Set the room member displayed by this row.
        fn set_member(&self, member: Option<Member>) {
            if *self.member.borrow() == member {
                return;
            }

            self.member.replace(member);
            self.obj().notify_member();
        }

        /// Set whether this row is selected.
        fn set_selected(&self, selected: bool) {
            if self.selected.get() == selected {
                return;
            }

            self.selected.set(selected);

            let obj = self.obj();
            obj.update_state(&[gtk::accessible::State::Checked(
                bool_to_accessible_tristate(selected),
            )]);
            obj.notify_selected();
        }
    }
}

glib::wrapper! {
    /// A row presenting a room member that can be selected.
    pub struct PermissionsSelectMemberRow(ObjectSubclass<imp::PermissionsSelectMemberRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PermissionsSelectMemberRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
