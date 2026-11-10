use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use crate::session::User;

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_row/reply.ui"
    )]
    #[properties(wrapper_type = super::MessageReply)]
    pub struct MessageReply {
        #[template_child]
        related_content_sender: TemplateChild<gtk::Label>,
        #[template_child]
        related_content: TemplateChild<adw::Bin>,
        #[template_child]
        content: TemplateChild<adw::Bin>,
        /// Whether to show the header of the related content.
        #[property(get, set = Self::set_show_related_content_header, explicit_notify)]
        show_related_content_header: Cell<bool>,
        related_display_name_binding: RefCell<Option<glib::Binding>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageReply {
        const NAME: &'static str = "ContentMessageReply";
        type Type = super::MessageReply;
        type ParentType = gtk::Grid;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MessageReply {
        fn dispose(&self) {
            if let Some(binding) = self.related_display_name_binding.take() {
                binding.unbind();
            }
        }
    }

    impl WidgetImpl for MessageReply {}
    impl GridImpl for MessageReply {}

    impl MessageReply {
        /// Set whether to show the header of the related content.
        fn set_show_related_content_header(&self, show: bool) {
            if self.show_related_content_header.get() == show {
                return;
            }

            self.show_related_content_header.set(show);
            self.obj().notify_show_related_content_header();
        }

        /// Set the sender of the replied-to event.
        pub(super) fn set_related_content_sender(&self, user: &User) {
            if let Some(binding) = self.related_display_name_binding.take() {
                binding.unbind();
            }

            let related_display_name_binding = user
                .bind_property("disambiguated-name", &*self.related_content_sender, "label")
                .sync_create()
                .build();
            self.related_display_name_binding
                .replace(Some(related_display_name_binding));
        }

        /// The widget containing the replied-to content.
        pub(super) fn related_content(&self) -> &adw::Bin {
            self.related_content.as_ref()
        }

        /// The widget containing the reply's content.
        pub(super) fn content(&self) -> &adw::Bin {
            self.content.as_ref()
        }
    }
}

glib::wrapper! {
    /// A widget displaying a reply to a message.
    pub struct MessageReply(ObjectSubclass<imp::MessageReply>)
        @extends gtk::Widget, gtk::Grid,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl MessageReply {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the sender of the replied-to event.
    pub(crate) fn set_related_content_sender(&self, user: &User) {
        self.imp().set_related_content_sender(user);
    }

    /// The widget containing the replied-to content.
    pub(crate) fn related_content(&self) -> &adw::Bin {
        self.imp().related_content()
    }

    /// The widget containing the reply's content.
    pub(crate) fn content(&self) -> &adw::Bin {
        self.imp().content()
    }
}
