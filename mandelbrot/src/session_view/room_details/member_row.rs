use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::{
    components::{Avatar, RoleBadge},
    session::Member,
    utils::expression,
};

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/member_row.ui")]
    #[properties(wrapper_type = super::MemberRow)]
    pub struct MemberRow {
        #[template_child]
        role_badge: TemplateChild<RoleBadge>,
        /// The room member presented by this row.
        #[property(get, set = Self::set_member, explicit_notify, nullable)]
        member: RefCell<Option<Member>>,
        /// Whether we should present the role of the user.
        #[property(get, construct_only)]
        show_role: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MemberRow {
        const NAME: &'static str = "ContentMemberRow";
        type Type = super::MemberRow;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            Avatar::ensure_type();

            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MemberRow {
        fn constructed(&self) {
            self.parent_constructed();

            // Only show the role badge when we explicitly want to show the badge, and it is
            // not the default role.
            let show_role_expr = self.obj().property_expression("show-role");
            let is_default_role_expr = self.role_badge.property_expression("is-default_role");
            expression::and(show_role_expr, expression::not(is_default_role_expr)).bind(
                &*self.role_badge,
                "visible",
                None::<&glib::Object>,
            );
        }
    }

    impl WidgetImpl for MemberRow {}
    impl BoxImpl for MemberRow {}

    impl MemberRow {
        /// Set the member displayed by this row.
        fn set_member(&self, member: Option<Member>) {
            if *self.member.borrow() == member {
                return;
            }

            self.member.replace(member);
            self.obj().notify_member();
        }
    }
}

glib::wrapper! {
    /// A row presenting a room member.
    pub struct MemberRow(ObjectSubclass<imp::MemberRow>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl MemberRow {
    /// Construct an empty `MemberRow`.
    pub fn new(show_role: bool) -> Self {
        glib::Object::builder()
            .property("show-role", show_role)
            .build()
    }
}
