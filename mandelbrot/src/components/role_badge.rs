use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use crate::session::MemberRole;

mod imp {
    use std::cell::Cell;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::RoleBadge)]
    pub struct RoleBadge {
        label: gtk::Label,
        /// The role displayed by this badge.
        #[property(get, set = Self::set_role, explicit_notify, builder(MemberRole::default()))]
        role: Cell<MemberRole>,
        /// Whether the role displayed by this badge is the default role.
        #[property(get)]
        is_default_role: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RoleBadge {
        const NAME: &'static str = "RoleBadge";
        type Type = super::RoleBadge;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("role-badge");
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for RoleBadge {
        fn constructed(&self) {
            self.parent_constructed();

            self.obj().set_child(Some(&self.label));
            self.update_badge();
            self.update_is_default_role();
        }
    }

    impl WidgetImpl for RoleBadge {}
    impl BinImpl for RoleBadge {}

    impl RoleBadge {
        /// Set the role displayed by this badge.
        fn set_role(&self, role: MemberRole) {
            if self.role.get() == role {
                return;
            }

            self.role.set(role);
            self.update_badge();
            self.update_is_default_role();
            self.obj().notify_role();
        }

        /// Update whether the role displayed by this badge is the default role.
        fn update_is_default_role(&self) {
            let is_default = self.role.get() == MemberRole::Default;

            if self.is_default_role.get() == is_default {
                return;
            }

            self.is_default_role.set(is_default);
            self.obj().notify_is_default_role();
        }

        /// Update the badge for the current state.
        fn update_badge(&self) {
            let obj = self.obj();
            let role = self.role.get();

            self.label.set_text(&role.to_string());

            if role == MemberRole::Creator {
                obj.add_css_class("creator");
            } else {
                obj.remove_css_class("creator");
            }

            if role == MemberRole::Administrator {
                obj.add_css_class("admin");
            } else {
                obj.remove_css_class("admin");
            }

            if role == MemberRole::Moderator {
                obj.add_css_class("mod");
            } else {
                obj.remove_css_class("mod");
            }

            if role == MemberRole::Muted {
                obj.add_css_class("muted");
            } else {
                obj.remove_css_class("muted");
            }
        }
    }
}

glib::wrapper! {
    /// Inline widget displaying a badge with the role of a room member.
    pub struct RoleBadge(ObjectSubclass<imp::RoleBadge>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl RoleBadge {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
