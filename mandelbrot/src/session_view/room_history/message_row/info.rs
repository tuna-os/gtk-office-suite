use adw::subclass::prelude::*;
use gtk::{glib, prelude::*};

use crate::utils::matrix::utd::UtdAction;

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
        #[template_child]
        action_button: TemplateChild<gtk::Button>,
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
            self.set_action(UtdAction::None);
        }

        /// Sets the action offered for this message, if any.
        pub(super) fn set_action(&self, action: UtdAction) {
            match (action.action_name(), action.button_label()) {
                (Some(name), Some(label)) => {
                    self.action_button.set_action_name(Some(name));
                    self.action_button.set_label(&label);
                    self.action_button.set_visible(true);
                }
                _ => {
                    self.action_button.set_action_name(None);
                    self.action_button.set_visible(false);
                }
            }
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

impl MessageInfo {
    /// Offers an action that can fix the situation described by the message.
    pub(crate) fn set_action(&self, action: UtdAction) {
        self.imp().set_action(action);
    }
}

impl Default for MessageInfo {
    fn default() -> Self {
        Self::new()
    }
}
