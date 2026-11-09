use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::closure_local};

#[derive(Debug, Default, Hash, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[repr(u32)]
#[enum_type(name = "ActionState")]
pub enum ActionState {
    #[default]
    Default = 0,
    Confirm = 1,
    Retry = 2,
    Loading = 3,
    Success = 4,
    Warning = 5,
    Error = 6,
}

impl AsRef<str> for ActionState {
    fn as_ref(&self) -> &str {
        match self {
            ActionState::Default => "default",
            ActionState::Confirm => "confirm",
            ActionState::Retry => "retry",
            ActionState::Loading => "loading",
            ActionState::Success => "success",
            ActionState::Warning => "warning",
            ActionState::Error => "error",
        }
    }
}

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
        sync::LazyLock,
    };

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/action_button.ui")]
    #[properties(wrapper_type = super::ActionButton)]
    pub struct ActionButton {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        button_default: TemplateChild<gtk::Button>,
        /// The icon used in the default state.
        #[property(get, set = Self::set_icon_name, explicit_notify)]
        icon_name: RefCell<String>,
        /// The extra CSS classes applied to the button in the default state.
        extra_classes: RefCell<Vec<&'static str>>,
        /// The action emitted by the button.
        #[property(get = Self::action_name, set = Self::set_action_name, override_interface = gtk::Actionable)]
        action_name: RefCell<Option<glib::GString>>,
        /// The target value of the action of the button.
        #[property(get = Self::action_target_value, set = Self::set_action_target, override_interface = gtk::Actionable)]
        action_target: RefCell<Option<glib::Variant>>,
        /// The state of the button.
        #[property(get, set = Self::set_state, explicit_notify, builder(ActionState::default()))]
        state: Cell<ActionState>,
        /// The tooltip text of the button of the default state.
        #[property(set = Self::set_default_state_tooltip_text)]
        default_state_tooltip_text: PhantomData<Option<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ActionButton {
        const NAME: &'static str = "ActionButton";
        type Type = super::ActionButton;
        type ParentType = adw::Bin;
        type Interfaces = (gtk::Actionable,);

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("action-button");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for ActionButton {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("clicked").build()]);
            SIGNALS.as_ref()
        }
    }

    impl WidgetImpl for ActionButton {}
    impl BinImpl for ActionButton {}

    impl ActionableImpl for ActionButton {
        fn action_name(&self) -> Option<glib::GString> {
            self.action_name.borrow().clone()
        }

        fn action_target_value(&self) -> Option<glib::Variant> {
            self.action_target.borrow().clone()
        }

        fn set_action_name(&self, name: Option<&str>) {
            self.action_name.replace(name.map(Into::into));
        }

        fn set_action_target_value(&self, value: Option<&glib::Variant>) {
            self.set_action_target(value.cloned());
        }
    }

    #[gtk::template_callbacks]
    impl ActionButton {
        /// Set the icon used in the default state.
        fn set_icon_name(&self, icon_name: &str) {
            if self.icon_name.borrow().as_str() == icon_name {
                return;
            }

            self.icon_name.replace(icon_name.to_owned());
            self.obj().notify_icon_name();
        }

        /// Set the extra CSS classes applied to the button in the default
        /// state.
        pub(super) fn set_extra_classes(&self, classes: &[&'static str]) {
            let mut extra_classes = self.extra_classes.borrow_mut();

            if *extra_classes == classes {
                // Nothing to do.
                return;
            }

            for class in extra_classes.drain(..) {
                self.button_default.remove_css_class(class);
            }

            for class in classes {
                self.button_default.add_css_class(class);
            }

            extra_classes.extend(classes);
        }

        /// Set the state of the button.
        fn set_state(&self, state: ActionState) {
            if self.state.get() == state {
                return;
            }

            self.stack.set_visible_child_name(state.as_ref());
            self.state.replace(state);
            self.obj().notify_state();
        }

        /// Set the target value of the action of the button.
        fn set_action_target(&self, value: Option<glib::Variant>) {
            self.action_target.replace(value);
        }

        /// Set the tooltip text of the button of the default state.
        fn set_default_state_tooltip_text(&self, text: Option<&str>) {
            self.button_default.set_tooltip_text(text);
        }

        #[template_callback]
        fn button_clicked(&self) {
            self.obj().emit_by_name::<()>("clicked", &[]);
        }
    }
}

glib::wrapper! {
    /// A button to emit an action and handle its different states.
    pub struct ActionButton(ObjectSubclass<imp::ActionButton>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

#[gtk::template_callbacks]
impl ActionButton {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the extra CSS classes applied to the button in the default state.
    pub(crate) fn set_extra_classes(&self, classes: &[&'static str]) {
        self.imp().set_extra_classes(classes);
    }

    /// Connect to the signal emitted when the button is clicked.
    pub fn connect_clicked<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "clicked",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}
