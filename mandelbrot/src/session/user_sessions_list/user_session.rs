use gettextrs::gettext;
use gtk::{
    glib,
    glib::{clone, closure_local},
    prelude::*,
    subclass::prelude::*,
};
use matrix_sdk::{HttpError, encryption::identities::Device as CryptoDevice};
use ruma::{DeviceId, OwnedDeviceId, api::client::device::Device as DeviceData};
use tracing::{debug, error};

use crate::{
    Application,
    components::{AuthDialog, AuthError},
    prelude::*,
    session::Session,
    spawn_tokio,
    system_settings::ClockFormat,
    utils::matrix::timestamp_to_date,
};

/// The possible sources of the user data.
#[derive(Debug, Clone)]
pub(super) enum UserSessionData {
    /// The data comes from the `/devices` API.
    DevicesApi(DeviceData),
    /// The data comes from the crypto store.
    Crypto(CryptoDevice),
    /// The data comes from both sources.
    Both {
        api: DeviceData,
        crypto: CryptoDevice,
    },
}

impl UserSessionData {
    /// The ID of the user session.
    pub(super) fn device_id(&self) -> &DeviceId {
        match self {
            UserSessionData::DevicesApi(api) | UserSessionData::Both { api, .. } => &api.device_id,
            UserSessionData::Crypto(crypto) => crypto.device_id(),
        }
    }

    /// Set the display name of user session.
    fn set_display_name(&mut self, name: String) {
        match self {
            UserSessionData::DevicesApi(api) | UserSessionData::Both { api, .. } => {
                api.display_name = Some(name);
            }
            UserSessionData::Crypto(crypto) => {
                *self = UserSessionData::Both {
                    api: DeviceData::new(crypto.device_id().into()),
                    crypto: crypto.to_owned(),
                }
            }
        }
    }

    /// The `/devices` API data.
    fn api(&self) -> Option<&DeviceData> {
        match self {
            UserSessionData::DevicesApi(api) | UserSessionData::Both { api, .. } => Some(api),
            UserSessionData::Crypto(_) => None,
        }
    }

    /// The crypto API.
    fn crypto(&self) -> Option<&CryptoDevice> {
        match self {
            UserSessionData::Crypto(crypto) | UserSessionData::Both { crypto, .. } => Some(crypto),
            UserSessionData::DevicesApi(_) => None,
        }
    }
}

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        marker::PhantomData,
        sync::LazyLock,
    };

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::UserSession)]
    pub struct UserSession {
        /// The current session.
        #[property(get, construct_only)]
        session: glib::WeakRef<Session>,
        /// The ID of the user session.
        device_id: OnceCell<OwnedDeviceId>,
        /// The user session data.
        data: RefCell<Option<UserSessionData>>,
        /// Whether this is the current user session.
        #[property(get)]
        is_current: Cell<bool>,
        /// The ID of the user session, as a string.
        #[property(get = Self::device_id_string)]
        device_id_string: PhantomData<String>,
        /// The display name of the device.
        #[property(get = Self::display_name)]
        display_name: PhantomData<String>,
        /// The display name of the device, or the device id as a fallback.
        #[property(get = Self::display_name_or_device_id)]
        display_name_or_device_id: PhantomData<String>,
        /// The last IP address used by the user session.
        #[property(get = Self::last_seen_ip)]
        last_seen_ip: PhantomData<Option<String>>,
        /// The last time the user session was used, as the number of
        /// milliseconds since Unix EPOCH.
        #[property(get = Self::last_seen_ts)]
        last_seen_ts: PhantomData<u64>,
        /// The last time the user session was used, as a `GDateTime`.
        #[property(get = Self::last_seen_datetime)]
        last_seen_datetime: PhantomData<Option<glib::DateTime>>,
        /// The last time the user session was used, as a formatted string.
        #[property(get = Self::last_seen_datetime_string)]
        last_seen_datetime_string: PhantomData<Option<String>>,
        /// Whether this user session is verified.
        #[property(get = Self::verified)]
        verified: PhantomData<bool>,
        system_settings_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for UserSession {
        const NAME: &'static str = "UserSession";
        type Type = super::UserSession;
    }

    #[glib::derived_properties]
    impl ObjectImpl for UserSession {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            let system_settings = Application::default().system_settings();
            let system_settings_handler = system_settings.connect_clock_format_notify(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.notify_last_seen_datetime_string();
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

        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("disconnected").build()]);
            SIGNALS.as_ref()
        }
    }

    impl UserSession {
        /// She the ID of this user session.
        pub(super) fn set_device_id(&self, device_id: OwnedDeviceId) {
            let device_id = self.device_id.get_or_init(|| device_id);

            if let Some(session) = self.session.upgrade() {
                let is_current = session.device_id() == device_id;
                self.is_current.set(is_current);
            }
        }

        /// The ID of this user session.
        pub(super) fn device_id(&self) -> &OwnedDeviceId {
            self.device_id
                .get()
                .expect("device ID should be initialized")
        }

        /// Set the user session data.
        pub(super) fn set_data(&self, data: UserSessionData) {
            let old_display_name = self.display_name();
            let old_last_seen_ip = self.last_seen_ip();
            let old_last_seen_ts = self.last_seen_ts();
            let old_verified = self.verified();

            self.data.replace(Some(data));

            let obj = self.obj();
            if self.display_name() != old_display_name {
                obj.notify_display_name();
                obj.notify_display_name_or_device_id();
            }
            if self.last_seen_ip() != old_last_seen_ip {
                obj.notify_last_seen_ip();
            }
            if self.last_seen_ts() != old_last_seen_ts {
                obj.notify_last_seen_ts();
                obj.notify_last_seen_datetime();
                obj.notify_last_seen_datetime_string();
            }
            if self.verified() != old_verified {
                obj.notify_verified();
            }
        }

        /// The ID of this user session, as a string.
        fn device_id_string(&self) -> String {
            self.device_id().to_string()
        }

        /// The display name of the device.
        fn display_name(&self) -> String {
            self.data
                .borrow()
                .as_ref()
                .and_then(UserSessionData::api)
                .and_then(|d| d.display_name.clone())
                .unwrap_or_default()
        }

        /// Set the display name of the device.
        pub(super) fn set_display_name(&self, name: String) {
            if let Some(data) = &mut *self.data.borrow_mut() {
                data.set_display_name(name);
            }

            self.obj().notify_display_name();
            self.obj().notify_display_name_or_device_id();
        }

        /// The display name of the device, or the device id as a fallback.
        fn display_name_or_device_id(&self) -> String {
            if let Some(display_name) = self
                .data
                .borrow()
                .as_ref()
                .and_then(UserSessionData::api)
                .and_then(|d| d.display_name.as_ref().map(|s| s.trim()))
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
            {
                display_name
            } else {
                self.device_id_string()
            }
        }

        /// The last IP address used by the user session.
        fn last_seen_ip(&self) -> Option<String> {
            self.data.borrow().as_ref()?.api()?.last_seen_ip.clone()
        }

        /// The last time the user session was used, as the number of
        /// milliseconds since Unix EPOCH.
        ///
        /// Defaults to `0` if the timestamp is unknown.
        fn last_seen_ts(&self) -> u64 {
            self.data
                .borrow()
                .as_ref()
                .and_then(UserSessionData::api)
                .and_then(|s| s.last_seen_ts)
                .map(|ts| ts.0.into())
                .unwrap_or_default()
        }

        /// The last time the user session was used, as a `GDateTime`.
        fn last_seen_datetime(&self) -> Option<glib::DateTime> {
            self.data
                .borrow()
                .as_ref()?
                .api()?
                .last_seen_ts
                .map(timestamp_to_date)
        }

        /// The last time the user session was used, as a localized formatted
        /// string.
        pub(super) fn last_seen_datetime_string(&self) -> Option<String> {
            let datetime = self.last_seen_datetime()?;

            let clock_format = Application::default().system_settings().clock_format();
            let use_24 = clock_format == ClockFormat::TwentyFourHours;

            // This was ported from Nautilus and simplified for our use case.
            // See: https://gitlab.gnome.org/GNOME/nautilus/-/blob/1c5bd3614a35cfbb49de087bc10381cdef5a218f/src/nautilus-file.c#L5001
            let now = glib::DateTime::now_local().unwrap();
            let format;
            let days_ago = {
                let today_midnight = glib::DateTime::from_local(
                    now.year(),
                    now.month(),
                    now.day_of_month(),
                    0,
                    0,
                    0f64,
                )
                .expect("constructing GDateTime works");

                let date = glib::DateTime::from_local(
                    datetime.year(),
                    datetime.month(),
                    datetime.day_of_month(),
                    0,
                    0,
                    0f64,
                )
                .expect("constructing GDateTime works");

                today_midnight.difference(&date).as_days()
            };

            // Show only the time if date is on today
            if days_ago == 0 {
                if use_24 {
                    // Translators: Time in 24h format, i.e. "23:04".
                    // Do not change the time format as it will follow the system settings.
                    // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                    format = gettext("Last seen at %H:%M");
                } else {
                    // Translators: Time in 12h format, i.e. "11:04 PM".
                    // Do not change the time format as it will follow the system settings.
                    // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                    format = gettext("Last seen at %I:%M %p");
                }
            }
            // Show the word "Yesterday" and time if date is on yesterday
            else if days_ago == 1 {
                if use_24 {
                    // Translators: this a time in 24h format, i.e. "Last seen yesterday at 23:04".
                    // Do not change the time format as it will follow the system settings.
                    // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                    // xgettext:no-c-format
                    format = gettext("Last seen yesterday at %H:%M");
                } else {
                    // Translators: this is a time in 12h format, i.e. "Last seen Yesterday at 11:04
                    // PM".
                    // Do not change the time format as it will follow the system settings.
                    // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                    // xgettext:no-c-format
                    format = gettext("Last seen yesterday at %I:%M %p");
                }
            }
            // Show a week day and time if date is in the last week
            else if days_ago > 1 && days_ago < 7 {
                if use_24 {
                    // Translators: this is the name of the week day followed by a time in 24h
                    // format, i.e. "Last seen Monday at 23:04".
                    // Do not change the time format as it will follow the system settings.
                    //  See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                    // xgettext:no-c-format
                    format = gettext("Last seen %A at %H:%M");
                } else {
                    // Translators: this is the week day name followed by a time in 12h format, i.e.
                    // "Last seen Monday at 11:04 PM".
                    // Do not change the time format as it will follow the system settings.
                    // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                    // xgettext:no-c-format
                    format = gettext("Last seen %A at %I:%M %p");
                }
            } else if datetime.year() == now.year() {
                if use_24 {
                    // Translators: this is the month and day and the time in 24h format, i.e. "Last
                    // seen February 3 at 23:04".
                    // Do not change the time format as it will follow the system settings.
                    // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                    // xgettext:no-c-format
                    format = gettext("Last seen %B %-e at %H:%M");
                } else {
                    // Translators: this is the month and day and the time in 12h format, i.e. "Last
                    // seen February 3 at 11:04 PM".
                    // Do not change the time format as it will follow the system settings.
                    // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                    // xgettext:no-c-format
                    format = gettext("Last seen %B %-e at %I:%M %p");
                }
            } else if use_24 {
                // Translators: this is the full date and the time in 24h format, i.e. "Last
                // seen February 3 2015 at 23:04".
                // Do not change the time format as it will follow the system settings.
                // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                // xgettext:no-c-format
                format = gettext("Last seen %B %-e %Y at %H:%M");
            } else {
                // Translators: this is the full date and the time in 12h format, i.e. "Last
                // seen February 3 2015 at 11:04 PM".
                // Do not change the time format as it will follow the system settings.
                // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
                // xgettext:no-c-format
                format = gettext("Last seen %B %-e %Y at %I:%M %p");
            }

            Some(
                datetime
                    .format(&format)
                    .expect("formatting GDateTime works")
                    .into(),
            )
        }

        /// Whether this device is verified.
        fn verified(&self) -> bool {
            self.data
                .borrow()
                .as_ref()
                .and_then(UserSessionData::crypto)
                .is_some_and(CryptoDevice::is_verified)
        }
    }
}

glib::wrapper! {
    /// A user's session.
    pub struct UserSession(ObjectSubclass<imp::UserSession>);
}

impl UserSession {
    pub(super) fn new(session: &Session, device_id: OwnedDeviceId) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("session", session)
            .build();

        obj.imp().set_device_id(device_id);

        obj
    }

    /// The ID of this user session.
    pub(crate) fn device_id(&self) -> &OwnedDeviceId {
        self.imp().device_id()
    }

    /// Set the user session data.
    pub(super) fn set_data(&self, data: UserSessionData) {
        self.imp().set_data(data);
    }

    /// Renames the user session.
    pub(crate) async fn rename(&self, display_name: String) -> Result<(), HttpError> {
        let Some(client) = self.session().map(|s| s.client()) else {
            return Ok(());
        };
        let device_id = self.imp().device_id().clone();

        let cloned_display_name = display_name.clone();
        let res =
            spawn_tokio!(
                async move { client.rename_device(&device_id, &cloned_display_name).await }
            )
            .await
            .expect("task was not aborted");

        match res {
            Ok(_) => {
                self.imp().set_display_name(display_name);
                Ok(())
            }
            Err(error) => {
                let device_id = self.device_id();
                error!("Could not rename user session {device_id}: {error}");
                Err(error)
            }
        }
    }

    /// Deletes the `UserSession`.
    ///
    /// Requires a widget because it might show a dialog for UIAA.
    pub(crate) async fn delete(&self, parent: &impl IsA<gtk::Widget>) -> Result<(), AuthError> {
        let Some(session) = self.session() else {
            return Err(AuthError::Unknown);
        };
        let device_id = self.imp().device_id().clone();

        let dialog = AuthDialog::new(&session);

        let res = dialog
            .authenticate(parent, move |client, auth| {
                let device_id = device_id.clone();
                async move {
                    client
                        .delete_devices(&[device_id], auth)
                        .await
                        .map_err(Into::into)
                }
            })
            .await;

        match res {
            Ok(_) => Ok(()),
            Err(error) => {
                let device_id = self.imp().device_id();

                if matches!(error, AuthError::UserCancelled) {
                    debug!("Deletion of user session {device_id} cancelled by user");
                } else {
                    error!("Could not delete user session {device_id}: {error}");
                }
                Err(error)
            }
        }
    }

    /// Signal that this session was disconnected.
    pub(super) fn emit_disconnected(&self) {
        self.emit_by_name::<()>("disconnected", &[]);
    }

    /// Connect to the signal emitted when this session is disconnected.
    pub fn connect_disconnected<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "disconnected",
            true,
            closure_local!(|obj: Self| {
                f(&obj);
            }),
        )
    }
}
