use gtk::{glib, prelude::*, subclass::prelude::*};
use tracing::error;

#[cfg(target_os = "linux")]
mod linux;

/// The clock format setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, glib::Enum)]
#[enum_type(name = "ClockFormat")]
pub enum ClockFormat {
    /// The 12h format, i.e. AM/PM.
    TwelveHours,
    /// The 24h format.
    TwentyFourHours,
}

impl Default for ClockFormat {
    fn default() -> Self {
        // Use the locale's default clock format as a fallback.
        let local_formatted_time = glib::DateTime::now_local()
            .and_then(|d| d.format("%X"))
            .map(|s| s.to_ascii_lowercase());
        match &local_formatted_time {
            Ok(s) if s.ends_with("am") || s.ends_with("pm") => ClockFormat::TwelveHours,
            Ok(_) => ClockFormat::TwentyFourHours,
            Err(error) => {
                error!("Could not get local formatted time: {error}");
                ClockFormat::TwelveHours
            }
        }
    }
}

mod imp {
    use std::cell::Cell;

    use super::*;

    #[repr(C)]
    pub struct SystemSettingsClass {
        parent_class: glib::object::Class<glib::Object>,
    }

    unsafe impl ClassStruct for SystemSettingsClass {
        type Type = SystemSettings;
    }

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::SystemSettings)]
    pub struct SystemSettings {
        /// The clock format setting.
        #[property(get, builder(ClockFormat::default()))]
        pub(super) clock_format: Cell<ClockFormat>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SystemSettings {
        const NAME: &'static str = "SystemSettings";
        type Type = super::SystemSettings;
        type Class = SystemSettingsClass;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SystemSettings {}
}

glib::wrapper! {
    /// A sublassable API to access system settings.
    pub struct SystemSettings(ObjectSubclass<imp::SystemSettings>);
}

impl SystemSettings {
    pub fn new() -> Self {
        #[cfg(target_os = "linux")]
        let obj = linux::LinuxSystemSettings::new().upcast();

        #[cfg(not(target_os = "linux"))]
        let obj = glib::Object::new();

        obj
    }

    /// Set the clock format setting.
    fn set_clock_format(&self, clock_format: ClockFormat) {
        if self.clock_format() == clock_format {
            return;
        }

        self.imp().clock_format.set(clock_format);
        self.notify_clock_format();
    }
}

impl Default for SystemSettings {
    fn default() -> Self {
        Self::new()
    }
}

/// Public trait that must be implemented for everything that derives from
/// `SystemSettings`.
pub trait SystemSettingsImpl: ObjectImpl {}

unsafe impl<T> IsSubclassable<T> for SystemSettings
where
    T: SystemSettingsImpl,
    T::Type: IsA<SystemSettings>,
{
}
