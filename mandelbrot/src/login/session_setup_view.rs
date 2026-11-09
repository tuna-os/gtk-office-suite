use adw::{prelude::*, subclass::prelude::*};
use gtk::{
    glib,
    glib::{clone, closure_local},
};

use crate::{
    components::crypto::{
        CryptoIdentitySetupNextStep, CryptoIdentitySetupView, CryptoRecoverySetupView,
    },
    session::{CryptoIdentityState, RecoveryState, Session, SessionVerificationState},
    spawn, spawn_tokio,
};

/// A page of the session setup stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionSetupPage {
    /// The loading page.
    Loading,
    /// The crypto identity setup view.
    CryptoIdentity,
    /// The recovery view.
    Recovery,
}

impl SessionSetupPage {
    /// Get the name of this page.
    const fn name(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::CryptoIdentity => "crypto-identity",
            Self::Recovery => "recovery",
        }
    }

    /// Get the page matching the given name.
    ///
    /// Panics if the name does not match any of the variants.
    fn from_name(name: &str) -> Self {
        match name {
            "loading" => Self::Loading,
            "crypto-identity" => Self::CryptoIdentity,
            "recovery" => Self::Recovery,
            _ => panic!("Unknown SessionSetupPage: {name}"),
        }
    }
}

mod imp {
    use std::{
        cell::{OnceCell, RefCell},
        sync::LazyLock,
    };

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/login/session_setup_view.ui")]
    #[properties(wrapper_type = super::SessionSetupView)]
    pub struct SessionSetupView {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        /// The current session.
        #[property(get, set = Self::set_session, construct_only)]
        session: glib::WeakRef<Session>,
        /// The crypto identity view.
        crypto_identity_view: OnceCell<CryptoIdentitySetupView>,
        /// The recovery view.
        recovery_view: OnceCell<CryptoRecoverySetupView>,
        session_handler: RefCell<Option<glib::SignalHandlerId>>,
        security_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SessionSetupView {
        const NAME: &'static str = "SessionSetupView";
        type Type = super::SessionSetupView;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("setup-view");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SessionSetupView {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    // The session setup is done.
                    Signal::builder("completed").build(),
                ]
            });
            SIGNALS.as_ref()
        }

        fn dispose(&self) {
            if let Some(session) = self.session.upgrade() {
                if let Some(handler) = self.session_handler.take() {
                    session.disconnect(handler);
                }
                if let Some(handler) = self.security_handler.take() {
                    session.security().disconnect(handler);
                }
            }
        }
    }

    impl WidgetImpl for SessionSetupView {
        fn grab_focus(&self) -> bool {
            match self.visible_stack_page() {
                SessionSetupPage::Loading => false,
                SessionSetupPage::CryptoIdentity => self.crypto_identity_view().grab_focus(),
                SessionSetupPage::Recovery => self.recovery_view().grab_focus(),
            }
        }
    }

    impl NavigationPageImpl for SessionSetupView {
        fn shown(&self) {
            self.grab_focus();
        }
    }

    #[gtk::template_callbacks]
    impl SessionSetupView {
        /// The visible page of the stack.
        fn visible_stack_page(&self) -> SessionSetupPage {
            SessionSetupPage::from_name(
                &self
                    .stack
                    .visible_child_name()
                    .expect("SessionSetupView stack should always have a visible child name"),
            )
        }

        /// The crypto identity view.
        fn crypto_identity_view(&self) -> &CryptoIdentitySetupView {
            self.crypto_identity_view.get_or_init(|| {
                let session = self
                    .session
                    .upgrade()
                    .expect("Session should still have a strong reference");
                let crypto_identity_view = CryptoIdentitySetupView::new(&session);

                crypto_identity_view.connect_completed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, next| {
                        match next {
                            CryptoIdentitySetupNextStep::None => imp.emit_completed(),
                            CryptoIdentitySetupNextStep::EnableRecovery => imp.check_recovery(true),
                            CryptoIdentitySetupNextStep::CompleteRecovery => {
                                imp.check_recovery(false);
                            }
                        }
                    }
                ));

                crypto_identity_view
            })
        }

        /// The recovery view.
        fn recovery_view(&self) -> &CryptoRecoverySetupView {
            self.recovery_view.get_or_init(|| {
                let session = self
                    .session
                    .upgrade()
                    .expect("Session should still have a strong reference");
                let recovery_view = CryptoRecoverySetupView::new(&session);

                recovery_view.connect_completed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.emit_completed();
                    }
                ));

                recovery_view
            })
        }

        /// Set the current session.
        fn set_session(&self, session: &Session) {
            self.session.set(Some(session));

            let ready_handler = session.connect_ready(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    spawn!(async move {
                        imp.load().await;
                    });
                }
            ));
            self.session_handler.replace(Some(ready_handler));
        }

        /// Load the session state.
        async fn load(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            // Make sure the encryption API is ready.
            let encryption = session.client().encryption();
            spawn_tokio!(async move {
                encryption.wait_for_e2ee_initialization_tasks().await;
            })
            .await
            .unwrap();

            self.check_session_setup();
        }

        /// Check whether we need to show the session setup.
        fn check_session_setup(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };
            let security = session.security();

            // Stop listening to notifications.
            if let Some(handler) = self.session_handler.take() {
                session.disconnect(handler);
            }
            if let Some(handler) = self.security_handler.take() {
                security.disconnect(handler);
            }

            // Wait if we don't know the crypto identity state.
            let crypto_identity_state = security.crypto_identity_state();
            if crypto_identity_state == CryptoIdentityState::Unknown {
                let handler = security.connect_crypto_identity_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.check_session_setup();
                    }
                ));
                self.security_handler.replace(Some(handler));
                return;
            }

            // Wait if we don't know the verification state.
            let verification_state = security.verification_state();
            if verification_state == SessionVerificationState::Unknown {
                let handler = security.connect_verification_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.check_session_setup();
                    }
                ));
                self.security_handler.replace(Some(handler));
                return;
            }

            // Wait if we don't know the recovery state.
            let recovery_state = security.recovery_state();
            if recovery_state == RecoveryState::Unknown {
                let handler = security.connect_recovery_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.check_session_setup();
                    }
                ));
                self.security_handler.replace(Some(handler));
                return;
            }

            if verification_state == SessionVerificationState::Verified
                && recovery_state == RecoveryState::Enabled
            {
                // No need for setup.
                self.emit_completed();
                return;
            }

            self.init();
        }

        /// Initialize this view.
        fn init(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let verification_state = session.security().verification_state();
            if verification_state == SessionVerificationState::Unverified {
                let crypto_identity_view = self.crypto_identity_view();

                self.stack.add_named(
                    crypto_identity_view,
                    Some(SessionSetupPage::CryptoIdentity.name()),
                );
                self.stack
                    .set_visible_child_name(SessionSetupPage::CryptoIdentity.name());
            } else {
                self.switch_to_recovery();
            }
        }

        /// Check whether we need to enable or set up recovery.
        fn check_recovery(&self, enable_only: bool) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            match session.security().recovery_state() {
                RecoveryState::Disabled => {
                    self.switch_to_recovery();
                }
                RecoveryState::Incomplete if !enable_only => {
                    self.switch_to_recovery();
                }
                _ => {
                    self.emit_completed();
                }
            }
        }

        /// Switch to the recovery view.
        fn switch_to_recovery(&self) {
            let recovery_view = self.recovery_view();

            self.stack
                .add_named(recovery_view, Some(SessionSetupPage::Recovery.name()));
            self.stack
                .set_visible_child_name(SessionSetupPage::Recovery.name());
        }

        /// Focus the proper widget for the current page.
        #[template_callback]
        fn focus_default_widget(&self) {
            if !self.stack.is_transition_running() {
                // Focus the default widget when the transition has ended.
                self.grab_focus();
            }
        }

        // Emit the `completed` signal.
        #[template_callback]
        fn emit_completed(&self) {
            self.obj().emit_by_name::<()>("completed", &[]);
        }
    }
}

glib::wrapper! {
    /// A view with the different flows to verify a session.
    pub struct SessionSetupView(ObjectSubclass<imp::SessionSetupView>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SessionSetupView {
    /// The tag for this page.
    pub(super) const TAG: &str = "session-setup";

    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }

    /// Connect to the signal emitted when the setup is completed.
    pub fn connect_completed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "completed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}
