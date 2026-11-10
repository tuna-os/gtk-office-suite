use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::closure_local};
use matrix_sdk::encryption::{
    recovery::{RecoveryError, RecoveryState as SdkRecoveryState},
    secret_storage::SecretStorageError,
};
use tracing::{debug, error, warn};

use crate::{
    components::{AuthDialog, AuthError, LoadingButton, SwitchLoadingRow},
    session::{RecoveryState, Session},
    spawn_tokio, toast,
};

/// A page of the [`CryptoRecoverySetupView`] navigation stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CryptoRecoverySetupPage {
    /// Use account recovery.
    Recover,
    /// Reset the recovery and optionally the cross-signing.
    Reset,
    /// Enable recovery.
    Enable,
    /// The recovery was successfully enabled.
    Success,
    /// The recovery was successful but is still incomplete.
    Incomplete,
}

impl CryptoRecoverySetupPage {
    /// Get the tag for this page.
    const fn tag(self) -> &'static str {
        match self {
            Self::Recover => "recover",
            Self::Reset => "reset",
            Self::Enable => "enable",
            Self::Success => "success",
            Self::Incomplete => "incomplete",
        }
    }

    /// Get the page matching the given tag.
    ///
    /// Panics if the tag does not match any variant.
    fn from_tag(tag: &str) -> Self {
        match tag {
            "recover" => Self::Recover,
            "reset" => Self::Reset,
            "enable" => Self::Enable,
            "success" => Self::Success,
            "incomplete" => Self::Incomplete,
            _ => panic!("Unknown CryptoRecoverySetupPage: {tag}"),
        }
    }
}

/// The initial page of the [`CryptoRecoverySetupView`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CryptoRecoverySetupInitialPage {
    /// Use account recovery.
    #[default]
    Recover,
    /// Reset the account recovery recovery.
    Reset,
    /// Enable recovery.
    Enable,
}

impl From<CryptoRecoverySetupInitialPage> for CryptoRecoverySetupPage {
    fn from(value: CryptoRecoverySetupInitialPage) -> Self {
        match value {
            CryptoRecoverySetupInitialPage::Recover => Self::Recover,
            CryptoRecoverySetupInitialPage::Reset => Self::Reset,
            CryptoRecoverySetupInitialPage::Enable => Self::Enable,
        }
    }
}

mod imp {
    use std::sync::LazyLock;

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/crypto/recovery_setup_view.ui")]
    #[properties(wrapper_type = super::CryptoRecoverySetupView)]
    pub struct CryptoRecoverySetupView {
        #[template_child]
        navigation: TemplateChild<adw::NavigationView>,
        #[template_child]
        recover_entry: TemplateChild<adw::PasswordEntryRow>,
        #[template_child]
        recover_btn: TemplateChild<LoadingButton>,
        #[template_child]
        reset_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        reset_identity_row: TemplateChild<SwitchLoadingRow>,
        #[template_child]
        reset_backup_row: TemplateChild<SwitchLoadingRow>,
        #[template_child]
        reset_entry: TemplateChild<adw::PasswordEntryRow>,
        #[template_child]
        reset_btn: TemplateChild<LoadingButton>,
        #[template_child]
        enable_entry: TemplateChild<adw::PasswordEntryRow>,
        #[template_child]
        enable_btn: TemplateChild<LoadingButton>,
        #[template_child]
        success_description: TemplateChild<gtk::Label>,
        #[template_child]
        success_key_box: TemplateChild<gtk::Box>,
        #[template_child]
        success_key_label: TemplateChild<gtk::Label>,
        #[template_child]
        success_key_copy_btn: TemplateChild<gtk::Button>,
        #[template_child]
        success_confirm_btn: TemplateChild<gtk::Button>,
        #[template_child]
        incomplete_confirm_btn: TemplateChild<gtk::Button>,
        /// The current session.
        #[property(get, set = Self::set_session, construct_only)]
        session: glib::WeakRef<Session>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CryptoRecoverySetupView {
        const NAME: &'static str = "CryptoRecoverySetupView";
        type Type = super::CryptoRecoverySetupView;
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
    impl ObjectImpl for CryptoRecoverySetupView {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    // Recovery is enabled.
                    Signal::builder("completed").build(),
                ]
            });
            SIGNALS.as_ref()
        }
    }

    impl WidgetImpl for CryptoRecoverySetupView {
        fn grab_focus(&self) -> bool {
            match self.visible_page() {
                CryptoRecoverySetupPage::Recover => self.recover_entry.grab_focus(),
                CryptoRecoverySetupPage::Reset => self.reset_entry.grab_focus(),
                CryptoRecoverySetupPage::Enable => self.enable_entry.grab_focus(),
                CryptoRecoverySetupPage::Success => self.success_confirm_btn.grab_focus(),
                CryptoRecoverySetupPage::Incomplete => self.incomplete_confirm_btn.grab_focus(),
            }
        }
    }

    impl BinImpl for CryptoRecoverySetupView {}

    #[gtk::template_callbacks]
    impl CryptoRecoverySetupView {
        /// The visible page of the view.
        fn visible_page(&self) -> CryptoRecoverySetupPage {
            CryptoRecoverySetupPage::from_tag(
                &self
                    .navigation
                    .visible_page()
                    .expect(
                        "CryptoRecoverySetupView navigation view should always have a visible page",
                    )
                    .tag()
                    .expect("CryptoRecoverySetupView navigation page should always have a tag"),
            )
        }

        /// Set the current session.
        fn set_session(&self, session: &Session) {
            self.session.set(Some(session));

            let security = session.security();
            let recovery_state = security.recovery_state();
            let initial_page = match recovery_state {
                RecoveryState::Unknown | RecoveryState::Disabled
                    if !security.backup_exists_on_server() =>
                {
                    CryptoRecoverySetupInitialPage::Enable
                }
                RecoveryState::Unknown | RecoveryState::Disabled | RecoveryState::Enabled => {
                    CryptoRecoverySetupInitialPage::Reset
                }
                RecoveryState::Incomplete => CryptoRecoverySetupInitialPage::Recover,
            };

            self.update_reset();
            self.set_initial_page(initial_page);
        }

        /// Update the reset page for the current state.
        fn update_reset(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let security = session.security();
            let (required, description) = if security.cross_signing_keys_available() {
                (
                    false,
                    gettext("Invalidates the verifications of all users and sessions"),
                )
            } else {
                (
                    true,
                    gettext(
                        "Required because the crypto identity in the recovery data is incomplete. Invalidates the verifications of all users and sessions.",
                    ),
                )
            };
            self.reset_identity_row.set_read_only(required);
            self.reset_identity_row.set_is_active(required);
            self.reset_identity_row.set_subtitle(&description);

            let (required, description) = if security.backup_enabled() {
                (
                    false,
                    gettext("You might not be able to read your past encrypted messages anymore"),
                )
            } else {
                (
                    true,
                    gettext(
                        "Required because the backup is not set up properly. You might not be able to read your past encrypted messages anymore.",
                    ),
                )
            };
            self.reset_backup_row.set_read_only(required);
            self.reset_backup_row.set_is_active(required);
            self.reset_backup_row.set_subtitle(&description);
        }

        /// Set the initial page of this view.
        pub(super) fn set_initial_page(&self, initial_page: CryptoRecoverySetupInitialPage) {
            self.navigation
                .replace_with_tags(&[CryptoRecoverySetupPage::from(initial_page).tag()]);
        }

        /// Update the success page for the given recovery key.
        fn update_success(&self, key: Option<String>) {
            let has_key = key.is_some();

            let description = if has_key {
                gettext(
                    "Make sure to store this recovery key in a safe place. You will need it to recover your account if you lose access to all your sessions.",
                )
            } else {
                gettext(
                    "Make sure to remember your passphrase or to store it in a safe place. You will need it to recover your account if you lose access to all your sessions.",
                )
            };
            self.success_description.set_label(&description);

            if let Some(key) = key {
                self.success_key_label.set_label(&key);
            }
            self.success_key_box.set_visible(has_key);
        }

        /// Focus the proper widget for the current page.
        #[template_callback]
        fn grab_focus(&self) {
            <Self as WidgetImpl>::grab_focus(self);
        }

        /// The content of the recover entry changed.
        #[template_callback]
        fn recover_entry_changed(&self) {
            let can_recover = !self.recover_entry.text().is_empty();
            self.recover_btn.set_sensitive(can_recover);
        }

        /// Recover the data.
        #[template_callback]
        async fn recover(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let key = self.recover_entry.text();

            if key.is_empty() {
                return;
            }

            self.recover_btn.set_is_loading(true);

            let encryption = session.client().encryption();
            let recovery = encryption.recovery();
            let handle = spawn_tokio!(async move { recovery.recover(&key).await });

            match handle.await.unwrap() {
                Ok(()) => {
                    // Even if recovery was successful, the recovery data may not have been
                    // complete. Because the SDK uses multiple threads, we are only
                    // sure of the SDK's recovery state at this point, not the Session's.
                    if encryption.recovery().state() == SdkRecoveryState::Incomplete {
                        self.navigation
                            .push_by_tag(CryptoRecoverySetupPage::Incomplete.tag());
                    } else {
                        self.emit_completed();
                    }
                }
                Err(error) => {
                    error!("Could not recover account: {error}");
                    let obj = self.obj();

                    match error {
                        RecoveryError::SecretStorage(SecretStorageError::SecretStorageKey(_)) => {
                            toast!(obj, gettext("The recovery passphrase or key is invalid"));
                        }
                        _ => {
                            toast!(obj, gettext("Could not access recovery data"));
                        }
                    }
                }
            }

            self.recover_btn.set_is_loading(false);
        }

        /// Reset recovery and optionally cross-signing and room keys backup.
        #[template_callback]
        async fn reset(&self) {
            self.reset_btn.set_is_loading(true);

            let reset_identity = self.reset_identity_row.is_active();
            if reset_identity && self.reset_cross_signing().await.is_err() {
                self.reset_btn.set_is_loading(false);
                return;
            }

            let passphrase = self.reset_entry.text();

            let reset_backup = self.reset_backup_row.is_active();
            if reset_backup {
                self.reset_backup_and_recovery(passphrase).await;
            } else {
                self.reset_recovery(passphrase).await;
            }

            self.reset_btn.set_is_loading(false);
        }

        /// Reset the cross-signing identity.
        async fn reset_cross_signing(&self) -> Result<(), ()> {
            let Some(session) = self.session.upgrade() else {
                return Err(());
            };

            let dialog = AuthDialog::new(&session);
            let obj = self.obj();

            let result = dialog.reset_cross_signing(&*obj).await;

            match result {
                Ok(()) => Ok(()),
                Err(AuthError::UserCancelled) => {
                    debug!("User cancelled authentication for cross-signing bootstrap");
                    Err(())
                }
                Err(error) => {
                    error!("Could not bootstrap cross-signing: {error}");
                    toast!(obj, gettext("Could not reset the crypto identity"));
                    Err(())
                }
            }
        }

        /// Reset the room keys backup and the account recovery key.
        async fn reset_backup_and_recovery(&self, passphrase: glib::GString) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let passphrase = Some(passphrase).filter(|s| !s.is_empty());
            let has_passphrase = passphrase.is_some();

            let obj = self.obj();
            let encryption = session.client().encryption();

            // There is no method to reset the room keys backup, so we need to disable
            // recovery and re-enable it.
            // If backups are not enabled locally, we cannot disable recovery, the API will
            // return an error. If a backup exists on the homeserver but backups are not
            // enabled locally, we need to delete the backup manually.
            // In any case, `Recovery::enable` will reset the secret storage.
            let backups = encryption.backups();
            let (backups_are_enabled, backup_exists_on_server) = spawn_tokio!(async move {
            let backups_are_enabled = backups.are_enabled().await;

            let backup_exists_on_server = if backups_are_enabled {
                true
            } else {
                // Let's use up-to-date data instead of relying on the last time that we updated it.
                match backups.exists_on_server().await {
                    Ok(exists) => exists,
                    Err(error) => {
                        warn!("Could not request whether recovery backup exists on homeserver: {error}");
                        // If the request failed, we have to try to delete the backup to avoid unsolvable errors.
                        true
                    }
                }
            };

            (backups_are_enabled, backup_exists_on_server)
        })
        .await
        .expect("task was not aborted");

            if !backups_are_enabled && backup_exists_on_server {
                let backups = encryption.backups();
                let handle = spawn_tokio!(async move { backups.disable_and_delete().await });

                if let Err(error) = handle.await.expect("task was not aborted") {
                    error!("Could not disable backups: {error}");
                    toast!(obj, gettext("Could not reset account recovery"));
                    return;
                }
            } else if backups_are_enabled {
                let recovery = encryption.recovery();
                let handle = spawn_tokio!(async move { recovery.disable().await });

                if let Err(error) = handle.await.expect("task was not aborted") {
                    error!("Could not disable recovery: {error}");
                    toast!(obj, gettext("Could not reset account recovery"));
                    return;
                }
            }

            let recovery = encryption.recovery();
            let handle = spawn_tokio!(async move {
                let mut enable = recovery.enable();
                if let Some(passphrase) = passphrase.as_deref() {
                    enable = enable.with_passphrase(passphrase);
                }

                enable.await
            });

            match handle.await.unwrap() {
                Ok(key) => {
                    let key = (!has_passphrase).then_some(key);

                    self.update_success(key);
                    self.navigation
                        .push_by_tag(CryptoRecoverySetupPage::Success.tag());
                }
                Err(error) => {
                    error!("Could not re-enable account recovery: {error}");
                    toast!(obj, gettext("Could not reset account recovery"));
                }
            }
        }

        /// Reset the account recovery key.
        async fn reset_recovery(&self, passphrase: glib::GString) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let passphrase = Some(passphrase).filter(|s| !s.is_empty());
            let has_passphrase = passphrase.is_some();

            let recovery = session.client().encryption().recovery();
            let handle = spawn_tokio!(async move {
                let mut reset = recovery.reset_key();
                if let Some(passphrase) = passphrase.as_deref() {
                    reset = reset.with_passphrase(passphrase);
                }

                reset.await
            });

            match handle.await.unwrap() {
                Ok(key) => {
                    let key = (!has_passphrase).then_some(key);

                    self.update_success(key);
                    self.navigation
                        .push_by_tag(CryptoRecoverySetupPage::Success.tag());
                }
                Err(error) => {
                    error!("Could not reset account recovery key: {error}");
                    let obj = self.obj();
                    toast!(obj, gettext("Could not reset account recovery key"));
                }
            }
        }

        /// Enable recovery.
        #[template_callback]
        async fn enable(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            self.enable_btn.set_is_loading(true);

            let passphrase = Some(self.enable_entry.text()).filter(|s| !s.is_empty());
            let has_passphrase = passphrase.is_some();

            let recovery = session.client().encryption().recovery();
            let handle = spawn_tokio!(async move {
                let mut enable = recovery.enable();
                if let Some(passphrase) = passphrase.as_deref() {
                    enable = enable.with_passphrase(passphrase);
                }

                enable.await
            });

            match handle.await.unwrap() {
                Ok(key) => {
                    let key = if has_passphrase { None } else { Some(key) };

                    self.update_success(key);
                    self.navigation
                        .push_by_tag(CryptoRecoverySetupPage::Success.tag());
                }
                Err(error) => {
                    error!("Could not enable account recovery: {error}");
                    let obj = self.obj();
                    toast!(obj, gettext("Could not enable account recovery"));
                }
            }

            self.enable_btn.set_is_loading(false);
        }

        /// Copy the recovery key to the clipboard.
        #[template_callback]
        fn copy_key(&self) {
            let obj = self.obj();
            let key = self.success_key_label.label();

            let clipboard = obj.clipboard();
            clipboard.set_text(&key);

            toast!(obj, "Recovery key copied to clipboard");
        }

        // Emit the `completed` signal.
        #[template_callback]
        fn emit_completed(&self) {
            self.obj().emit_by_name::<()>("completed", &[]);
        }

        // Show the reset page, after updating it.
        #[template_callback]
        fn show_reset(&self) {
            self.update_reset();
            self.navigation
                .push_by_tag(CryptoRecoverySetupPage::Reset.tag());
        }
    }
}

glib::wrapper! {
    /// A view with the different flows to use or set up account recovery.
    pub struct CryptoRecoverySetupView(ObjectSubclass<imp::CryptoRecoverySetupView>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CryptoRecoverySetupView {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }

    /// Set the initial page of this view.
    pub(crate) fn set_initial_page(&self, initial_page: CryptoRecoverySetupInitialPage) {
        self.imp().set_initial_page(initial_page);
    }

    /// Connect to the signal emitted when the recovery was successfully
    /// enabled.
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
