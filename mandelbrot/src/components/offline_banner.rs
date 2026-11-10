use adw::{prelude::*, subclass::prelude::*};
use gtk::{gio, glib, glib::clone};

use crate::{session::Session, utils::BoundObjectWeakRef};

mod imp {
    use std::cell::RefCell;

    use gettextrs::gettext;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::OfflineBanner)]
    pub struct OfflineBanner {
        banner: adw::Banner,
        /// The session to check.
        #[property(get, set = Self::set_session, explicit_notify, nullable)]
        session: BoundObjectWeakRef<Session>,
        monitor_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OfflineBanner {
        const NAME: &'static str = "OfflineBanner";
        type Type = super::OfflineBanner;
        type ParentType = adw::Bin;
    }

    #[glib::derived_properties]
    impl ObjectImpl for OfflineBanner {
        fn constructed(&self) {
            self.parent_constructed();

            self.obj().set_child(Some(&self.banner));
            self.update();
        }

        fn dispose(&self) {
            if let Some(handler) = self.monitor_handler.take() {
                gio::NetworkMonitor::default().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for OfflineBanner {}
    impl BinImpl for OfflineBanner {}

    impl OfflineBanner {
        /// Set the session to check.
        fn set_session(&self, session: Option<&Session>) {
            if self.session.obj().as_ref() == session {
                return;
            }

            if let Some(handler) = self.monitor_handler.take() {
                gio::NetworkMonitor::default().disconnect(handler);
            }
            self.session.disconnect_signals();

            if let Some(session) = session {
                let offline_handler = session.connect_is_offline_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update();
                    }
                ));

                self.session.set(session, vec![offline_handler]);
            }

            self.update();
        }

        /// Make sure that we watch the connection, if there is no session.
        fn ensure_connection_watched(&self) {
            if self.session.obj().is_some() {
                // No need to watch.
                return;
            }

            if self.monitor_handler.borrow().is_some() {
                // Already watching.
                return;
            }

            let monitor = gio::NetworkMonitor::default();
            let monitor_handler = monitor.connect_network_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _| {
                    imp.update();
                }
            ));
            self.monitor_handler.replace(Some(monitor_handler));
        }

        /// Update this banner.
        fn update(&self) {
            if let Some(session) = self.session.obj() {
                self.banner.set_title(&gettext("Offline"));
                self.banner.set_revealed(session.is_offline());
            } else {
                self.ensure_connection_watched();
                let monitor = gio::NetworkMonitor::default();

                if !monitor.is_network_available() {
                    self.banner.set_title(&gettext("No network connection"));
                    self.banner.set_revealed(true);
                } else if monitor.connectivity() != gio::NetworkConnectivity::Full {
                    self.banner.set_title(&gettext("No Internet connection"));
                    self.banner.set_revealed(true);
                } else {
                    self.banner.set_revealed(false);
                }
            }
        }
    }
}

glib::wrapper! {
    /// Banner displaying the state of the connectivity.
    ///
    /// If a session is set, it watches the offline status of the session, otherwise it watches the
    /// network connection with [`gio::NetworkMonitor`].
    pub struct OfflineBanner(ObjectSubclass<imp::OfflineBanner>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl OfflineBanner {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
