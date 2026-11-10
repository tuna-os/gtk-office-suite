use std::sync::Arc;

use ashpd::{desktop::settings::Settings as SettingsProxy, wire};
use futures_util::StreamExt;
use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};
use tracing::error;

use super::{ClockFormat, SystemSettings, SystemSettingsImpl};
use crate::{spawn, spawn_tokio};

const GNOME_DESKTOP_NAMESPACE: &str = "org.gnome.desktop.interface";
const CLOCK_FORMAT_KEY: &str = "clock-format";

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct LinuxSystemSettings {}

    #[glib::object_subclass]
    impl ObjectSubclass for LinuxSystemSettings {
        const NAME: &'static str = "LinuxSystemSettings";
        type Type = super::LinuxSystemSettings;
        type ParentType = SystemSettings;
    }

    impl ObjectImpl for LinuxSystemSettings {
        fn constructed(&self) {
            self.parent_constructed();

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.init().await;
                }
            ));
        }
    }

    impl SystemSettingsImpl for LinuxSystemSettings {}

    impl LinuxSystemSettings {
        /// Initialize the system settings.
        async fn init(&self) {
            let obj = self.obj();

            let proxy = match spawn_tokio!(async move { SettingsProxy::new().await })
                .await
                .expect("task was not aborted")
            {
                Ok(proxy) => proxy,
                Err(error) => {
                    error!("Could not access settings portal: {error}");
                    return;
                }
            };
            let proxy = Arc::new(proxy);

            let proxy_clone = proxy.clone();
            match spawn_tokio!(async move {
                proxy_clone
                    .read::<ClockFormat>(GNOME_DESKTOP_NAMESPACE, CLOCK_FORMAT_KEY)
                    .await
            })
            .await
            .expect("task was not aborted")
            {
                Ok(clock_format) => obj
                    .upcast_ref::<SystemSettings>()
                    .set_clock_format(clock_format),
                Err(error) => {
                    error!("Could not access clock format system setting: {error}");
                    return;
                }
            }

            let clock_format_changed_stream = match spawn_tokio!(async move {
                proxy
                    .receive_setting_changed_with_args::<ClockFormat>(
                        GNOME_DESKTOP_NAMESPACE,
                        CLOCK_FORMAT_KEY,
                    )
                    .await
            })
            .await
            .expect("task was not aborted")
            {
                Ok(stream) => stream,
                Err(error) => {
                    error!(
                        "Could not listen to changes of the clock format system setting: {error}"
                    );
                    return;
                }
            };

            let obj_weak = obj.downgrade();
            clock_format_changed_stream.for_each(move |value| {
                    let obj_weak = obj_weak.clone();
                    async move {
                        let clock_format = match value {
                            Ok(clock_format) => clock_format,
                            Err(error) => {
                                error!("Could not update clock format setting: {error}");
                                return;
                            }
                        };

                        if let Some(obj) = obj_weak.upgrade() {
                            obj.upcast_ref::<SystemSettings>().set_clock_format(clock_format);
                        } else {
                            error!("Could not update clock format setting: could not upgrade weak reference");
                        }
                    }
                }).await;
        }
    }
}

glib::wrapper! {
    /// API to access system settings on Linux.
    pub struct LinuxSystemSettings(ObjectSubclass<imp::LinuxSystemSettings>)
        @extends SystemSettings;
}

impl LinuxSystemSettings {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for LinuxSystemSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<&wire::OwnedVariant> for ClockFormat {
    type Error = wire::Error;

    fn try_from(value: &wire::OwnedVariant) -> Result<Self, Self::Error> {
        let Ok(s) = <&str>::try_from(value) else {
            return Err(wire::Error::IncorrectType);
        };

        match s {
            "12h" => Ok(Self::TwelveHours),
            "24h" => Ok(Self::TwentyFourHours),
            _ => Err(wire::Error::Failure(format!(
                "Invalid string `{s}`, expected `12h` or `24h`"
            ))),
        }
    }
}

impl TryFrom<wire::OwnedVariant> for ClockFormat {
    type Error = wire::Error;

    fn try_from(value: wire::OwnedVariant) -> Result<Self, Self::Error> {
        Self::try_from(&value)
    }
}
