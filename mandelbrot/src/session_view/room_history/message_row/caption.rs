use gtk::{glib, prelude::*, subclass::prelude::*};
use ruma::events::room::message::FormattedBody;

use super::{ContentFormat, text::MessageText};
use crate::{prelude::*, session::Room};

mod imp {
    use std::marker::PhantomData;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::MessageCaption)]
    pub struct MessageCaption {
        /// The widget displaying the file alongside the caption.
        #[property(get = Self::child, set = Self::set_child, explicit_notify, nullable)]
        child: PhantomData<Option<gtk::Widget>>,
        caption_widget: MessageText,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageCaption {
        const NAME: &'static str = "ContentMessageCaption";
        type Type = super::MessageCaption;
        type ParentType = gtk::Grid;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("message-caption");

            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MessageCaption {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            obj.attach(&self.caption_widget, 0, 1, 1, 1);
            obj.set_row_spacing(6);
        }
    }

    impl WidgetImpl for MessageCaption {}
    impl GridImpl for MessageCaption {}

    impl MessageCaption {
        /// The widget displaying the file alongside the caption.
        fn child(&self) -> Option<gtk::Widget> {
            self.obj().child_at(0, 0)
        }

        /// Set the widget displaying the file alongside the caption.
        fn set_child(&self, widget: Option<gtk::Widget>) {
            let prev_widget = self.child();

            if prev_widget == widget {
                return;
            }
            let obj = self.obj();

            if let Some(widget) = prev_widget {
                obj.remove(&widget);
            }

            if let Some(widget) = widget {
                obj.attach(&widget, 0, 0, 1, 1);
            }

            obj.notify_child();
        }

        /// Set the caption.
        pub(super) fn set_caption(
            &self,
            caption: String,
            formatted_caption: Option<FormattedBody>,
            room: &Room,
            format: ContentFormat,
            detect_at_room: bool,
        ) {
            self.caption_widget.with_markup(
                formatted_caption,
                caption,
                room,
                format,
                detect_at_room,
            );
        }
    }
}

glib::wrapper! {
    /// A widget displaying a caption alongside a file message.
    pub struct MessageCaption(ObjectSubclass<imp::MessageCaption>)
        @extends gtk::Widget, gtk::Grid,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl MessageCaption {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the caption.
    pub(crate) fn set_caption(
        &self,
        caption: String,
        formatted_caption: Option<FormattedBody>,
        room: &Room,
        format: ContentFormat,
        detect_at_room: bool,
    ) {
        self.imp()
            .set_caption(caption, formatted_caption, room, format, detect_at_room);
    }
}

impl Default for MessageCaption {
    fn default() -> Self {
        Self::new()
    }
}

impl ChildPropertyExt for MessageCaption {
    fn child_property(&self) -> Option<gtk::Widget> {
        self.child()
    }

    fn set_child_property(&self, child: Option<&impl IsA<gtk::Widget>>) {
        self.set_child(child.map(Cast::upcast_ref));
    }
}
