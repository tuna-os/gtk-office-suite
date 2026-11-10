use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use super::MemberTimestamp;
use crate::{
    Application, system_settings::ClockFormat, utils::matrix::seconds_since_unix_epoch_to_date,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/member_timestamp/row.ui"
    )]
    #[properties(wrapper_type = super::MemberTimestampRow)]
    pub struct MemberTimestampRow {
        #[template_child]
        timestamp: TemplateChild<gtk::Label>,
        /// The `MemberTimestamp` presented by this row.
        #[property(get, set = Self::set_data, explicit_notify, nullable)]
        data: glib::WeakRef<MemberTimestamp>,
        system_settings_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MemberTimestampRow {
        const NAME: &'static str = "ContentMemberTimestampRow";
        type Type = super::MemberTimestampRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MemberTimestampRow {
        fn constructed(&self) {
            self.parent_constructed();

            let system_settings = Application::default().system_settings();
            let system_settings_handler = system_settings.connect_clock_format_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_timestamp();
                }
            ));
            self.system_settings_handler
                .replace(Some(system_settings_handler));
        }

        fn dispose(&self) {
            if let Some(handler) = self.system_settings_handler.take() {
                Application::default().system_settings().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for MemberTimestampRow {}
    impl BinImpl for MemberTimestampRow {}

    impl MemberTimestampRow {
        /// Set the `MemberTimestamp` presented by this row.
        fn set_data(&self, data: Option<&MemberTimestamp>) {
            if self.data.upgrade().as_ref() == data {
                return;
            }

            self.data.set(data);

            self.obj().notify_data();
            self.update_timestamp();
        }

        /// The formatted date and time of this receipt.
        fn update_timestamp(&self) {
            let Some(timestamp) = self
                .data
                .upgrade()
                .map(|d| d.timestamp())
                .filter(|t| *t > 0)
            else {
                // No timestamp.
                self.timestamp.set_visible(false);
                return;
            };

            let timestamp = timestamp.try_into().unwrap_or(i64::MAX);
            let datetime = seconds_since_unix_epoch_to_date(timestamp);

            let clock_format = Application::default().system_settings().clock_format();

            let format = if clock_format == ClockFormat::TwelveHours {
                // Translators: this is a date and a time in 12h format.
                // For example, "May 5 at 01:20 PM".
                // Do not change the time format as it will follow the system settings.
                // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                gettext("%B %-e at %I:%M %p")
            } else {
                // Translators: this is a date and a time in 24h format.
                // For example, "May 5 at 13:20".
                // Do not change the time format as it will follow the system settings.
                // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                gettext("%B %-e at %H:%M")
            };
            let label = datetime.format(&format).unwrap();

            self.timestamp.set_label(&label);
            self.timestamp.set_visible(true);
        }
    }
}

glib::wrapper! {
    /// A row displaying a room member and timestamp.
    pub struct MemberTimestampRow(ObjectSubclass<imp::MemberTimestampRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MemberTimestampRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for MemberTimestampRow {
    fn default() -> Self {
        Self::new()
    }
}
