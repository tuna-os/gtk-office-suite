use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};

use super::AccountSwitcherPopover;
use crate::{
    Window,
    components::Avatar,
    session_list::SessionInfo,
    utils::{BoundObjectWeakRef, TemplateCallbacks},
};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/account_switcher/account_switcher_button.ui")]
    #[properties(wrapper_type = super::AccountSwitcherButton)]
    pub struct AccountSwitcherButton {
        /// The popover of this button.
        #[property(get, set = Self::set_popover, explicit_notify, nullable)]
        popover: BoundObjectWeakRef<AccountSwitcherPopover>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AccountSwitcherButton {
        const NAME: &'static str = "AccountSwitcherButton";
        type Type = super::AccountSwitcherButton;
        type ParentType = gtk::ToggleButton;

        fn class_init(klass: &mut Self::Class) {
            Avatar::ensure_type();
            SessionInfo::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
            TemplateCallbacks::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AccountSwitcherButton {
        fn dispose(&self) {
            self.reset();
        }
    }

    impl WidgetImpl for AccountSwitcherButton {}
    impl ButtonImpl for AccountSwitcherButton {}
    impl ToggleButtonImpl for AccountSwitcherButton {}

    #[gtk::template_callbacks]
    impl AccountSwitcherButton {
        /// Set the popover of this button.
        fn set_popover(&self, popover: Option<&AccountSwitcherPopover>) {
            if self.popover.obj().as_ref() == popover {
                return;
            }

            // Reset the state.
            self.reset();
            let obj = self.obj();

            if let Some(popover) = popover {
                // We need to remove the popover from the previous button, if any.
                if let Some(parent) = popover
                    .parent()
                    .and_downcast::<super::AccountSwitcherButton>()
                {
                    parent.set_popover(None::<AccountSwitcherPopover>);
                }

                let closed_handler = popover.connect_closed(clone!(
                    #[weak]
                    obj,
                    move |_| {
                        obj.set_active(false);
                    }
                ));

                popover.set_parent(&*obj);
                self.popover.set(popover, vec![closed_handler]);
            }

            obj.notify_popover();
        }

        /// Toggle the popover of this button.
        #[template_callback]
        fn toggle_popover(&self) {
            let obj = self.obj();

            if obj.is_active() {
                let Some(window) = obj.root().and_downcast::<Window>() else {
                    return;
                };

                let popover = window.account_switcher();
                self.set_popover(Some(popover));

                popover.popup();
            } else if let Some(popover) = self.popover.obj() {
                popover.popdown();
            }
        }

        /// Reset the state of this button.
        fn reset(&self) {
            if let Some(popover) = self.popover.obj() {
                popover.unparent();
            }
            self.popover.disconnect_signals();
            self.obj().set_active(false);
        }
    }
}

glib::wrapper! {
    /// A button showing the currently selected session and opening the account switcher popover.
    pub struct AccountSwitcherButton(ObjectSubclass<imp::AccountSwitcherButton>)
        @extends gtk::Widget, gtk::Button, gtk::ToggleButton,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

#[gtk::template_callbacks]
impl AccountSwitcherButton {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for AccountSwitcherButton {
    fn default() -> Self {
        Self::new()
    }
}
