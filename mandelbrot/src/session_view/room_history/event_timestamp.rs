use adw::subclass::prelude::*;
use gtk::{glib, glib::clone, prelude::*};

use crate::{Application, gettext_f, system_settings::ClockFormat};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/event_timestamp.ui")]
    #[properties(wrapper_type = super::EventTimestamp)]
    pub struct EventTimestamp {
        /// Inner label that contains the timestamp text.
        #[template_child]
        label: TemplateChild<gtk::Label>,
        /// Underlying datetime object that's being rendered.
        #[property(get, set = Self::set_datetime, explicit_notify, nullable)]
        datetime: RefCell<Option<glib::DateTime>>,
        /// Handler for accessing 12h/24h time preference.
        settings_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EventTimestamp {
        const NAME: &'static str = "EventTimestamp";
        type Type = super::EventTimestamp;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for EventTimestamp {
        fn constructed(&self) {
            self.parent_constructed();

            let settings = Application::default().system_settings();
            let handler = settings.connect_clock_format_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_label();
                }
            ));
            self.settings_handler.replace(Some(handler));
        }

        fn dispose(&self) {
            if let Some(handler) = self.settings_handler.take() {
                Application::default().system_settings().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for EventTimestamp {}
    impl BinImpl for EventTimestamp {}

    impl EventTimestamp {
        /// Set the datetime that should be rendered.
        fn set_datetime(&self, datetime: Option<glib::DateTime>) {
            if *self.datetime.borrow() == datetime {
                return;
            }
            self.datetime.replace(datetime);
            self.update_label();
            self.obj().notify_datetime();
        }

        /// Update the label based on the underlying datetime.
        fn update_label(&self) {
            let Some(datetime) = self.datetime.borrow().clone() else {
                self.label.set_label("");
                self.obj().reset_property(gtk::AccessibleProperty::Label);
                return;
            };

            let clock_format = Application::default().system_settings().clock_format();
            let time = if clock_format == ClockFormat::TwelveHours {
                datetime.format("%I:%M %p").unwrap()
            } else {
                datetime.format("%R").unwrap()
            };

            self.label.set_label(&time);

            let accessible_label = gettext_f("Sent at {time}", &[("time", &time)]);
            self.obj()
                .update_property(&[gtk::accessible::Property::Label(&accessible_label)]);
        }
    }
}

glib::wrapper! {
    pub struct EventTimestamp(ObjectSubclass<imp::EventTimestamp>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl EventTimestamp {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for EventTimestamp {
    fn default() -> Self {
        Self::new()
    }
}
