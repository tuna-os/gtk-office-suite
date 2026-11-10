use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use crate::{
    session::{VirtualItem, VirtualItemKind},
    utils::BoundObject,
};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/divider_row.ui")]
    #[properties(wrapper_type = super::DividerRow)]
    pub struct DividerRow {
        #[template_child]
        inner_label: TemplateChild<gtk::Label>,
        /// The virtual item presented by this row.
        #[property(get, set = Self::set_virtual_item, explicit_notify, nullable)]
        virtual_item: BoundObject<VirtualItem>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DividerRow {
        const NAME: &'static str = "ContentDividerRow";
        type Type = super::DividerRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_css_name("divider-row");
            klass.set_accessible_role(gtk::AccessibleRole::ListItem);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for DividerRow {}

    impl WidgetImpl for DividerRow {}
    impl BinImpl for DividerRow {}

    impl DividerRow {
        /// Set the virtual item presented by this row.
        fn set_virtual_item(&self, virtual_item: Option<VirtualItem>) {
            if self.virtual_item.obj() == virtual_item {
                return;
            }

            self.virtual_item.disconnect_signals();

            if let Some(virtual_item) = virtual_item {
                let kind_handler = virtual_item.connect_kind_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update();
                    }
                ));

                self.virtual_item.set(virtual_item, vec![kind_handler]);
            }

            self.update();
            self.obj().notify_virtual_item();
        }

        /// Update this row for the current kind.
        ///
        /// Panics if the kind is not `TimelineStart`, `DayDivider` or
        /// `NewMessages`.
        fn update(&self) {
            let Some(kind) = self
                .virtual_item
                .obj()
                .map(|virtual_item| virtual_item.kind())
            else {
                return;
            };

            let label = match &kind {
                VirtualItemKind::TimelineStart => {
                    gettext("This is the start of the visible history")
                }
                VirtualItemKind::DayDivider(date) => {
                    let fmt = if date.year()
                        == glib::DateTime::now_local()
                            .expect("we should be able to get the local datetime")
                            .year()
                    {
                        // Translators: This is a date format in the day divider without the
                        // year. For example, "Friday, May 5".
                        // Please use `-` before specifiers that add spaces on single
                        // digits. See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                        gettext("%A, %B %-e")
                    } else {
                        // Translators: This is a date format in the day divider with the
                        // year. For ex. "Friday, May 5, 2023".
                        // Please use `-` before specifiers that add spaces on single
                        // digits. See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                        gettext("%A, %B %-e, %Y")
                    };

                    date.format(&fmt)
                        .expect("we should be able to format the datetime")
                        .into()
                }
                VirtualItemKind::NewMessages => gettext("New Messages"),
                _ => unimplemented!(),
            };

            let obj = self.obj();
            if matches!(kind, VirtualItemKind::NewMessages) {
                obj.add_css_class("new-messages");
            } else {
                obj.remove_css_class("new-messages");
            }

            self.inner_label.set_label(&label);
        }
    }
}

glib::wrapper! {
    /// A row presenting a divider in the timeline.
    pub struct DividerRow(ObjectSubclass<imp::DividerRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl DividerRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for DividerRow {
    fn default() -> Self {
        Self::new()
    }
}
