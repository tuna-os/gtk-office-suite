use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};

use crate::{
    components::Avatar,
    i18n::{gettext_f, ngettext_f},
    session::{HighlightFlags, SidebarSpaceItem},
    utils::{BoundObject, TemplateCallbacks},
};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/sidebar/space_row.ui")]
    #[properties(wrapper_type = super::SidebarSpaceRow)]
    pub struct SidebarSpaceRow {
        #[template_child]
        notification_count: TemplateChild<gtk::Label>,
        /// The item represented by this row.
        #[property(get, set = Self::set_item, explicit_notify, nullable)]
        item: BoundObject<SidebarSpaceItem>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarSpaceRow {
        const NAME: &'static str = "SidebarSpaceRow";
        type Type = super::SidebarSpaceRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Avatar::ensure_type();

            Self::bind_template(klass);
            TemplateCallbacks::bind_template_callbacks(klass);

            klass.set_css_name("space-rail-item");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarSpaceRow {}

    impl WidgetImpl for SidebarSpaceRow {}
    impl BinImpl for SidebarSpaceRow {}

    impl SidebarSpaceRow {
        /// Set the item represented by this row.
        fn set_item(&self, item: Option<SidebarSpaceItem>) {
            if self.item.obj() == item {
                return;
            }

            self.item.disconnect_signals();

            if let Some(item) = item {
                let name_handler = item.connect_display_name_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_accessibility_label();
                    }
                ));
                let count_handler = item.connect_notification_count_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_accessibility_label();
                    }
                ));
                let read_handler = item.connect_is_read_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_accessibility_label();
                    }
                ));
                let highlight_handler = item.connect_highlight_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_highlight();
                        imp.update_accessibility_label();
                    }
                ));

                self.item.set(
                    item,
                    vec![name_handler, count_handler, read_handler, highlight_handler],
                );
            }

            self.update_highlight();
            self.update_accessibility_label();
            self.obj().notify_item();
        }

        /// Update the style of the badge according to the highlight state.
        fn update_highlight(&self) {
            let is_mention = self
                .item
                .obj()
                .is_some_and(|item| item.highlight().contains(HighlightFlags::HIGHLIGHT));

            if is_mention {
                self.notification_count.add_css_class("highlight");
            } else {
                self.notification_count.remove_css_class("highlight");
            }
        }

        /// Update the accessibility label of the parent list item.
        fn update_accessibility_label(&self) {
            let Some(parent) = self.obj().parent() else {
                return;
            };
            parent.update_property(&[gtk::accessible::Property::Label(&self.accessible_label())]);
        }

        /// Compute the accessibility label of this row.
        ///
        /// Since the row only shows an avatar, the label must convey the name
        /// and the unread state.
        fn accessible_label(&self) -> String {
            let Some(item) = self.item.obj() else {
                return String::new();
            };
            let name = item.display_name();

            if item.has_notifications() {
                let count = item.notification_count();
                let notifications = ngettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name. Presented to screen readers when a room has notifications
                    // for unread messages.
                    "1 notification",
                    "{count} notifications",
                    count.try_into().unwrap_or(u32::MAX),
                    &[("count", &count.to_string())],
                );
                let label = format!("{name} {notifications}");

                if item.highlight().contains(HighlightFlags::HIGHLIGHT) {
                    gettext_f(
                        // Translators: Do NOT translate the content between '{' and '}', this
                        // is a variable name. Presented to screen readers when a space in the
                        // space switcher contains a message that mentions the user.
                        "{space_and_notifications}, mentioned",
                        &[("space_and_notifications", &label)],
                    )
                } else {
                    label
                }
            } else if !item.is_read() {
                gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name. Presented to screen readers when a space in the space
                    // switcher contains rooms with unread messages.
                    "{name}, unread messages",
                    &[("name", &name)],
                )
            } else {
                name
            }
        }
    }
}

glib::wrapper! {
    /// A row of the space switcher of the sidebar, presenting a
    /// [`SidebarSpaceItem`].
    pub struct SidebarSpaceRow(ObjectSubclass<imp::SidebarSpaceRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SidebarSpaceRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for SidebarSpaceRow {
    fn default() -> Self {
        Self::new()
    }
}
