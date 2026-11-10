use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use crate::{
    session::{HighlightFlags, RoomCategory, SidebarSection, SidebarSectionName},
    utils::{BoundObject, TemplateCallbacks},
};

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
    };

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/sidebar/section_row.ui")]
    #[properties(wrapper_type = super::SidebarSectionRow)]
    pub struct SidebarSectionRow {
        #[template_child]
        pub(super) display_name: TemplateChild<gtk::Label>,
        #[template_child]
        notification_count: TemplateChild<gtk::Label>,
        /// The section of this row.
        #[property(get, set = Self::set_section, explicit_notify, nullable)]
        section: BoundObject<SidebarSection>,
        section_binding: RefCell<Option<glib::Binding>>,
        /// Whether this row is expanded.
        #[property(get, set = Self::set_is_expanded, explicit_notify, construct, default = true)]
        is_expanded: Cell<bool>,
        /// The label to show for this row.
        #[property(get = Self::label)]
        label: PhantomData<Option<String>>,
        /// The room category to show a label for during a drag-and-drop
        /// operation.
        ///
        /// This will change the label according to the action that can be
        /// performed when dropping a room with the given category.
        show_label_for_room_category: Cell<Option<RoomCategory>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarSectionRow {
        const NAME: &'static str = "SidebarSectionRow";
        type Type = super::SidebarSectionRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
            TemplateCallbacks::bind_template_callbacks(klass);

            klass.set_css_name("sidebar-section");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarSectionRow {
        fn dispose(&self) {
            if let Some(binding) = self.section_binding.take() {
                binding.unbind();
            }
        }
    }

    impl WidgetImpl for SidebarSectionRow {}
    impl BinImpl for SidebarSectionRow {}

    #[gtk::template_callbacks]
    impl SidebarSectionRow {
        /// Set the section represented by this row.
        fn set_section(&self, section: Option<SidebarSection>) {
            if self.section.obj() == section {
                return;
            }

            if let Some(binding) = self.section_binding.take() {
                binding.unbind();
            }
            self.section.disconnect_signals();

            let obj = self.obj();

            if let Some(section) = section {
                let section_binding = section
                    .bind_property("is-expanded", &*obj, "is-expanded")
                    .sync_create()
                    .build();
                self.section_binding.replace(Some(section_binding));

                let highlight_handler = section.connect_highlight_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_highlight();
                    }
                ));

                self.section.set(section, vec![highlight_handler]);
            }

            self.update_highlight();
            obj.notify_section();
            obj.notify_label();
        }

        /// The label to show for this row.
        fn label(&self) -> Option<String> {
            let target_section_name = self.section.obj().as_ref()?.name();
            let source_room_category = self.show_label_for_room_category.get();

            let label = match source_room_category {
                Some(RoomCategory::Invited) => match target_section_name {
                    // Translators: This is an action to join a room and put it in the "Favorites"
                    // section.
                    SidebarSectionName::Favorite => gettext("Join Room as Favorite"),
                    SidebarSectionName::Normal => gettext("Join Room"),
                    // Translators: This is an action to join a room and put it in the "Low
                    // Priority" section.
                    SidebarSectionName::LowPriority => gettext("Join Room as Low Priority"),
                    SidebarSectionName::Left => gettext("Reject Invite"),
                    _ => target_section_name.to_string(),
                },
                Some(RoomCategory::Favorite) => match target_section_name {
                    SidebarSectionName::Normal => gettext("Move to Rooms"),
                    SidebarSectionName::LowPriority => gettext("Move to Low Priority"),
                    SidebarSectionName::Left => gettext("Leave Room"),
                    _ => target_section_name.to_string(),
                },
                Some(RoomCategory::Normal) => match target_section_name {
                    SidebarSectionName::Favorite => gettext("Move to Favorites"),
                    SidebarSectionName::LowPriority => gettext("Move to Low Priority"),
                    SidebarSectionName::Left => gettext("Leave Room"),
                    _ => target_section_name.to_string(),
                },
                Some(RoomCategory::LowPriority) => match target_section_name {
                    SidebarSectionName::Favorite => gettext("Move to Favorites"),
                    SidebarSectionName::Normal => gettext("Move to Rooms"),
                    SidebarSectionName::Left => gettext("Leave Room"),
                    _ => target_section_name.to_string(),
                },
                Some(RoomCategory::Left) => match target_section_name {
                    // Translators: This is an action to rejoin a room and put it in the "Favorites"
                    // section.
                    SidebarSectionName::Favorite => gettext("Rejoin Room as Favorite"),
                    SidebarSectionName::Normal => gettext("Rejoin Room"),
                    // Translators: This is an action to rejoin a room and put it in the "Low
                    // Priority" section.
                    SidebarSectionName::LowPriority => gettext("Rejoin Room as Low Priority"),
                    _ => target_section_name.to_string(),
                },
                _ => target_section_name.to_string(),
            };

            Some(label)
        }

        /// Set whether this row is expanded.
        fn set_is_expanded(&self, is_expanded: bool) {
            if self.is_expanded.get() == is_expanded {
                return;
            }
            let obj = self.obj();

            if is_expanded {
                obj.set_state_flags(gtk::StateFlags::CHECKED, false);
            } else {
                obj.unset_state_flags(gtk::StateFlags::CHECKED);
            }

            self.is_expanded.set(is_expanded);
            self.update_expanded_accessibility_state();
            self.update_highlight();
            obj.notify_is_expanded();
        }

        /// Update how this row is highlighted according to the current state of
        /// rooms in this section.
        fn update_highlight(&self) {
            let highlight = self
                .section
                .obj()
                .as_ref()
                .map(SidebarSection::highlight)
                .unwrap_or_default();

            if !self.is_expanded.get() && highlight.contains(HighlightFlags::HIGHLIGHT) {
                self.notification_count.add_css_class("highlight");
            } else {
                self.notification_count.remove_css_class("highlight");
            }
        }

        /// Update the expanded state of this row for a11y.
        #[template_callback]
        fn update_expanded_accessibility_state(&self) {
            if let Some(row) = self.obj().parent() {
                row.update_state(&[gtk::accessible::State::Expanded(Some(
                    self.is_expanded.get(),
                ))]);
            }
        }

        /// Set the room category to show the label for.
        pub(super) fn set_show_label_for_room_category(&self, category: Option<RoomCategory>) {
            if self.show_label_for_room_category.get() == category {
                return;
            }

            self.show_label_for_room_category.set(category);

            self.obj().notify_label();
        }
    }
}

glib::wrapper! {
    /// A sidebar row representing a category.
    pub struct SidebarSectionRow(ObjectSubclass<imp::SidebarSectionRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SidebarSectionRow {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the room category to show the label for.
    pub(crate) fn set_show_label_for_room_category(&self, category: Option<RoomCategory>) {
        self.imp().set_show_label_for_room_category(category);
    }

    /// The descendant that labels this row for a11y.
    pub(crate) fn labelled_by(&self) -> &gtk::Accessible {
        self.imp().display_name.upcast_ref()
    }
}
