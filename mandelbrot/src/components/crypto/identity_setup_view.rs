use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{
    glib,
    glib::{clone, closure_local},
};
use tracing::{debug, error};

use super::{CryptoRecoverySetupInitialPage, CryptoRecoverySetupView};
use crate::{
    components::{AuthDialog, AuthError, LoadingButton},
    identity_verification_view::IdentityVerificationView,
    session::{
        CryptoIdentityState, IdentityVerification, RecoveryState, Session, SessionVerificationState,
    },
    spawn, toast,
    utils::BoundObjectWeakRef,
};

/// A page of the crypto identity setup navigation stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CryptoIdentitySetupPage {
    /// Choose a verification method.
    ChooseMethod,
    /// In-progress verification.
    Verify,
    /// Bootstrap cross-signing.
    Bootstrap,
    /// Reset cross-signing.
    Reset,
    /// Use recovery or reset cross-signing and recovery.
    Recovery,
}

impl CryptoIdentitySetupPage {
    /// Get the tag for this page.
    const fn tag(self) -> &'static str {
        match self {
            Self::ChooseMethod => "choose-method",
            Self::Verify => "verify",
            Self::Bootstrap => "bootstrap",
            Self::Reset => "reset",
            Self::Recovery => "recovery",
        }
    }

    /// Get page matching the given tag.
    ///
    /// Panics if the tag does not match any of the variants.
    fn from_tag(tag: &str) -> Self {
        match tag {
            "choose-method" => Self::ChooseMethod,
            "verify" => Self::Verify,
            "bootstrap" => Self::Bootstrap,
            "reset" => Self::Reset,
            "recovery" => Self::Recovery,
            _ => panic!("Unknown CryptoIdentitySetupPage: {tag}"),
        }
    }
}

/// The result of the crypto identity setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, glib::Enum)]
#[enum_type(name = "CryptoIdentitySetupNextStep")]
pub enum CryptoIdentitySetupNextStep {
    /// No more steps should be needed.
    None,
    /// We should enable the recovery, if it is disabled.
    EnableRecovery,
    /// We should make sure that the recovery is fully set up.
    CompleteRecovery,
}

mod imp {
    use std::{
        cell::{OnceCell, RefCell},
        sync::LazyLock,
    };

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/crypto/identity_setup_view.ui")]
    #[properties(wrapper_type = super::CryptoIdentitySetupView)]
    pub struct CryptoIdentitySetupView {
        #[template_child]
        navigation: TemplateChild<adw::NavigationView>,
        #[template_child]
        send_request_btn: TemplateChild<LoadingButton>,
        #[template_child]
        use_recovery_btn: TemplateChild<gtk::Button>,
        #[template_child]
        verification_page: TemplateChild<IdentityVerificationView>,
        #[template_child]
        bootstrap_btn: TemplateChild<LoadingButton>,
        #[template_child]
        reset_btn: TemplateChild<gtk::Button>,
        /// The current session.
        #[property(get, set = Self::set_session, construct_only)]
        session: glib::WeakRef<Session>,
        /// The ongoing identity verification, if any.
        #[property(get)]
        verification: BoundObjectWeakRef<IdentityVerification>,
        verification_list_handler: RefCell<Option<glib::SignalHandlerId>>,
        /// The recovery view.
        recovery_view: OnceCell<CryptoRecoverySetupView>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CryptoIdentitySetupView {
        const NAME: &'static str = "CryptoIdentitySetupView";
        type Type = super::CryptoIdentitySetupView;
        type ParentType = adw::Bin;

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
    impl ObjectImpl for CryptoIdentitySetupView {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    // The crypto identity setup is done.
                    Signal::builder("completed")
                        .param_types([CryptoIdentitySetupNextStep::static_type()])
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }

        fn dispose(&self) {
            if let Some(verification) = self.verification.obj() {
                spawn!(clone!(
                    #[strong]
                    verification,
                    async move {
                        let _ = verification.cancel().await;
                    }
                ));
            }

            if let Some(session) = self.session.upgrade()
                && let Some(handler) = self.verification_list_handler.take()
            {
                session.verification_list().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for CryptoIdentitySetupView {
        fn grab_focus(&self) -> bool {
            match self.visible_page() {
                CryptoIdentitySetupPage::ChooseMethod => self.send_request_btn.grab_focus(),
                CryptoIdentitySetupPage::Verify => self.verification_page.grab_focus(),
                CryptoIdentitySetupPage::Bootstrap => self.bootstrap_btn.grab_focus(),
                CryptoIdentitySetupPage::Reset => self.reset_btn.grab_focus(),
                CryptoIdentitySetupPage::Recovery => self.recovery_view().grab_focus(),
            }
        }
    }

    impl BinImpl for CryptoIdentitySetupView {}

    #[gtk::template_callbacks]
    impl CryptoIdentitySetupView {
        /// The visible page of the view.
        fn visible_page(&self) -> CryptoIdentitySetupPage {
            CryptoIdentitySetupPage::from_tag(
                &self
                    .navigation
                    .visible_page()
                    .expect(
                        "CryptoIdentitySetupView navigation view should always have a visible page",
                    )
                    .tag()
                    .expect("CryptoIdentitySetupView navigation page should always have a tag"),
            )
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
                        imp.emit_completed(CryptoIdentitySetupNextStep::None);
                    }
                ));

                recovery_view
            })
        }

        /// Set the current session.
        fn set_session(&self, session: &Session) {
            self.session.set(Some(session));

            // Use received verification requests too.
            let verification_list = session.verification_list();
            let verification_list_handler = verification_list.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |verification_list, _, _, _| {
                    if imp.verification.obj().is_some() {
                        // We don't want to override the current verification.
                        return;
                    }

                    if let Some(verification) = verification_list.ongoing_session_verification() {
                        imp.set_verification(Some(verification));
                    }
                }
            ));
            self.verification_list_handler
                .replace(Some(verification_list_handler));

            self.init();
        }

        /// Initialize the view.
        fn init(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };
            let security = session.security();

            // If the session is already verified, offer to reset it.
            let verification_state = security.verification_state();
            if verification_state == SessionVerificationState::Verified {
                self.navigation
                    .replace_with_tags(&[CryptoIdentitySetupPage::Reset.tag()]);
                return;
            }

            let crypto_identity_state = security.crypto_identity_state();
            let recovery_state = security.recovery_state();

            // If there is no crypto identity, we need to bootstrap it.
            if crypto_identity_state == CryptoIdentityState::Missing {
                self.navigation
                    .replace_with_tags(&[CryptoIdentitySetupPage::Bootstrap.tag()]);
                return;
            }

            // If there is no other session available, we can only use recovery or reset.
            if crypto_identity_state == CryptoIdentityState::LastManStanding {
                let recovery_view = if recovery_state == RecoveryState::Disabled {
                    // If recovery is disabled, we can only reset.
                    self.recovery_page(CryptoRecoverySetupInitialPage::Reset)
                } else {
                    // We can recover or reset.
                    self.recovery_page(CryptoRecoverySetupInitialPage::Recover)
                };

                self.navigation.replace(&[recovery_view]);

                return;
            }

            if let Some(verification) = session.verification_list().ongoing_session_verification() {
                self.set_verification(Some(verification));
            }

            // Choose methods is the default page.
            self.update_choose_methods();
        }

        /// Update the choose methods page for the current state.
        fn update_choose_methods(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let can_recover = session.security().recovery_state() != RecoveryState::Disabled;
            self.use_recovery_btn.set_visible(can_recover);
        }

        /// Set the ongoing identity verification.
        ///
        /// Cancels the previous verification if it's not finished.
        fn set_verification(&self, verification: Option<IdentityVerification>) {
            let prev_verification = self.verification.obj();

            if prev_verification == verification {
                return;
            }

            if let Some(verification) = prev_verification {
                if !verification.is_finished() {
                    spawn!(clone!(
                        #[strong]
                        verification,
                        async move {
                            let _ = verification.cancel().await;
                        }
                    ));
                }

                self.verification.disconnect_signals();
            }

            if let Some(verification) = &verification {
                let replaced_handler = verification.connect_replaced(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, new_verification| {
                        imp.set_verification(Some(new_verification.clone()));
                    }
                ));
                let done_handler = verification.connect_done(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    #[upgrade_or]
                    glib::Propagation::Stop,
                    move |verification| {
                        imp.emit_completed(CryptoIdentitySetupNextStep::EnableRecovery);
                        imp.set_verification(None);
                        verification.remove_from_list();

                        glib::Propagation::Stop
                    }
                ));
                let remove_handler = verification.connect_dismiss(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.navigation.pop();
                        imp.set_verification(None);
                    }
                ));

                self.verification.set(
                    verification,
                    vec![replaced_handler, done_handler, remove_handler],
                );
            }

            let has_verification = verification.is_some();
            self.verification_page.set_verification(verification);

            if has_verification
                && self
                    .navigation
                    .visible_page()
                    .and_then(|p| p.tag())
                    .is_none_or(|t| t != CryptoIdentitySetupPage::Verify.tag())
            {
                self.navigation
                    .push_by_tag(CryptoIdentitySetupPage::Verify.tag());
            }

            self.obj().notify_verification();
        }

        /// Construct the recovery view and wrap it into a navigation page.
        fn recovery_page(
            &self,
            initial_page: CryptoRecoverySetupInitialPage,
        ) -> adw::NavigationPage {
            let recovery_view = self.recovery_view();
            recovery_view.set_initial_page(initial_page);

            let page = adw::NavigationPage::builder()
                .tag(CryptoIdentitySetupPage::Recovery.tag())
                .child(recovery_view)
                .build();
            page.connect_shown(clone!(
                #[weak]
                recovery_view,
                move |_| {
                    recovery_view.grab_focus();
                }
            ));

            page
        }

        /// Focus the proper widget for the current page.
        #[template_callback]
        fn grab_focus(&self) {
            <Self as WidgetImpl>::grab_focus(self);
        }

        /// Create a new verification request.
        #[template_callback]
        async fn send_request(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            self.send_request_btn.set_is_loading(true);

            if let Err(()) = session.verification_list().create(None).await {
                toast!(
                    self.obj(),
                    gettext("Could not send a new verification request")
                );
            }

            // On success, the verification should be shown automatically.

            self.send_request_btn.set_is_loading(false);
        }

        /// Reset cross-signing and optionally recovery.
        #[template_callback]
        fn reset(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let can_recover = session.security().recovery_state() != RecoveryState::Disabled;

            if can_recover {
                let recovery_view = self.recovery_page(CryptoRecoverySetupInitialPage::Reset);
                self.navigation.push(&recovery_view);
            } else {
                self.navigation
                    .push_by_tag(CryptoIdentitySetupPage::Bootstrap.tag());
            }
        }

        /// Create a new crypto user identity.
        #[template_callback]
        async fn bootstrap_cross_signing(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            self.bootstrap_btn.set_is_loading(true);

            let obj = self.obj();
            let dialog = AuthDialog::new(&session);

            let result = dialog
                .authenticate(&*obj, move |client, auth| async move {
                    client.encryption().bootstrap_cross_signing(auth).await
                })
                .await;

            match result {
                Ok(()) => self.emit_completed(CryptoIdentitySetupNextStep::CompleteRecovery),
                Err(AuthError::UserCancelled) => {
                    debug!("User cancelled authentication for cross-signing bootstrap");
                }
                Err(error) => {
                    error!("Could not bootstrap cross-signing: {error:?}");
                    toast!(obj, gettext("Could not create the crypto identity"));
                }
            }

            self.bootstrap_btn.set_is_loading(false);
        }

        /// Recover the data.
        #[template_callback]
        fn recover(&self) {
            let recovery_view = self.recovery_page(CryptoRecoverySetupInitialPage::Recover);
            self.navigation.push(&recovery_view);
        }

        // Emit the `completed` signal.
        #[template_callback]
        fn emit_completed(&self, next: CryptoIdentitySetupNextStep) {
            self.obj().emit_by_name::<()>("completed", &[&next]);
        }
    }
}

glib::wrapper! {
    /// A view with the different flows to setup a crypto identity.
    pub struct CryptoIdentitySetupView(ObjectSubclass<imp::CryptoIdentitySetupView>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CryptoIdentitySetupView {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }

    /// Connect to the signal emitted when the setup is completed.
    pub fn connect_completed<F: Fn(&Self, CryptoIdentitySetupNextStep) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "completed",
            true,
            closure_local!(move |obj: Self, next: CryptoIdentitySetupNextStep| {
                f(&obj, next);
            }),
        )
    }
}
