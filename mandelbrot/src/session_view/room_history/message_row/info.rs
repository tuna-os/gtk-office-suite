use adw::subclass::prelude::*;
use gtk::glib;

/// The icon to use for this message.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MessageInfoIcon {
    Info,
    Warning,
}

impl MessageInfoIcon {
    fn icon_name(self) -> &'static str {
        match self {
            MessageInfoIcon::Info => "about-symbolic",
            MessageInfoIcon::Warning => "warning-symbolic",
        }
    }
}

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_row/info.ui"
    )]
    #[properties(wrapper_type = super::MessageInfo)]
    pub struct MessageInfo {
        #[template_child]
        icon: TemplateChild<gtk::Image>,
        #[template_child]
        label: TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageInfo {
        const NAME: &'static str = "ContentMessageInfo";
        type Type = super::MessageInfo;
        type ParentType = gtk::Grid;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MessageInfo {}
    impl WidgetImpl for MessageInfo {}
    impl GridImpl for MessageInfo {}

    impl MessageInfo {
        /// Sets the necessary info for this message.
        pub(super) fn set_info(&self, icon: MessageInfoIcon, text: &str) {
            self.icon.set_icon_name(Some(icon.icon_name()));
            self.label.set_text(text);
        }
    }
}

glib::wrapper! {
    /// A widget presenting an informative event.
    pub struct MessageInfo(ObjectSubclass<imp::MessageInfo>)
        @extends gtk::Widget, gtk::Grid,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl MessageInfo {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Sets the necessary info for this message.
    pub(crate) fn set_info(&self, icon: MessageInfoIcon, text: &str) {
        self.imp().set_info(icon, text);
    }
}

impl Default for MessageInfo {
    fn default() -> Self {
        Self::new()
    }
}
