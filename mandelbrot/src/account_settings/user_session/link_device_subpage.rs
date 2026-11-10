use adw::{prelude::*, subclass::prelude::*};
use futures_util::StreamExt;
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use matrix_sdk::authentication::oauth::qrcode::{
    CheckCodeSender, ContinuationMessageSender, GeneratedQrProgress, GrantLoginProgress,
    QRCodeGrantLoginError,
};
use tokio::task::AbortHandle;
use tracing::{error, warn};
use url::Url;

use crate::{
    components::LoadingButton, contrib::QRCode, session::Session, spawn, spawn_tokio, toast,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/account_settings/user_session/link_device_subpage.ui"
    )]
    #[properties(wrapper_type = super::LinkDeviceSubpage)]
    pub struct LinkDeviceSubpage {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        qrcode: TemplateChild<QRCode>,
        #[template_child]
        check_code_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        confirm_check_code_button: TemplateChild<LoadingButton>,
        #[template_child]
        open_browser_button: TemplateChild<LoadingButton>,
        #[template_child]
        waiting_label: TemplateChild<gtk::Label>,
        #[template_child]
        error_label: TemplateChild<gtk::Label>,
        #[template_child]
        retry_button: TemplateChild<gtk::Button>,
        /// The current session.
        #[property(get, set, construct_only)]
        session: glib::WeakRef<Session>,
        /// The sender to submit the check code entered by the user.
        check_code_sender: RefCell<Option<CheckCodeSender>>,
        /// The URI to open in the browser to allow the new sign-in.
        verification_uri: RefCell<Option<Url>>,
        /// The sender to let the grant proceed once the verification URI was
        /// opened.
        continuation_sender: RefCell<Option<ContinuationMessageSender>>,
        /// The abort handle for the ongoing grant task.
        abort_handle: RefCell<Option<AbortHandle>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LinkDeviceSubpage {
        const NAME: &'static str = "LinkDeviceSubpage";
        type Type = super::LinkDeviceSubpage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for LinkDeviceSubpage {
        fn dispose(&self) {
            // Make sure to cancel the process even if the dialog was closed
            // without navigating back.
            self.clean();
        }
    }

    impl WidgetImpl for LinkDeviceSubpage {}

    impl NavigationPageImpl for LinkDeviceSubpage {
        fn shown(&self) {
            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.start().await;
                }
            ));
        }

        fn hidden(&self) {
            self.clean();
        }
    }

    #[gtk::template_callbacks]
    impl LinkDeviceSubpage {
        /// Start the process to grant the login of a new device.
        async fn start(&self) {
            self.clean();

            let Some(session) = self.session.upgrade() else {
                return;
            };

            let client = session.client();
            let (sender, mut receiver) = futures_channel::mpsc::unbounded();

            let handle = spawn_tokio!(async move {
                let oauth = client.oauth();
                let grant = oauth.grant_login_with_qr_code().generate();
                let mut progress = grant.subscribe_to_progress();

                let progress_task = tokio::spawn(async move {
                    while let Some(state) = progress.next().await {
                        if sender.unbounded_send(state).is_err() {
                            break;
                        }
                    }
                });

                let result = grant.await;
                progress_task.abort();

                result
            });

            self.abort_handle.replace(Some(handle.abort_handle()));

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    while let Some(progress) = receiver.next().await {
                        imp.update_progress(progress);
                    }
                }
            ));

            let Ok(result) = handle.await else {
                // The task was aborted.
                self.abort_handle.take();
                return;
            };

            self.abort_handle.take();

            match result {
                Ok(()) => {
                    self.stack.set_visible_child_name("success");

                    // Refresh the list of sessions to show the new device.
                    let _ = self
                        .obj()
                        .activate_action("account-settings.reload-user-sessions", None);
                }
                Err(grant_error) => {
                    warn!("Could not link new device: {grant_error}");
                    self.show_error(&grant_error_message(&grant_error));
                }
            }
        }

        /// Try to link a new device again after an error.
        #[template_callback]
        async fn retry(&self) {
            self.start().await;
        }

        /// Update the UI according to the progress of the process.
        fn update_progress(&self, progress: GrantLoginProgress<GeneratedQrProgress>) {
            match progress {
                GrantLoginProgress::Starting => {
                    self.stack.set_visible_child_name("loading");
                }
                GrantLoginProgress::EstablishingSecureChannel(GeneratedQrProgress::QrReady(
                    qr_code_data,
                )) => {
                    self.qrcode.set_bytes(&qr_code_data.to_bytes());
                    self.stack.set_visible_child_name("qr");
                }
                GrantLoginProgress::EstablishingSecureChannel(GeneratedQrProgress::QrScanned(
                    check_code_sender,
                )) => {
                    self.check_code_sender.replace(Some(check_code_sender));
                    self.stack.set_visible_child_name("check-code");
                    self.check_code_entry.grab_focus();
                }
                GrantLoginProgress::WaitingForAuth {
                    verification_uri,
                    continuation_sender,
                } => {
                    self.verification_uri.replace(Some(verification_uri));
                    self.continuation_sender.replace(Some(continuation_sender));
                    self.stack.set_visible_child_name("confirm-browser");
                    self.open_browser_button.grab_focus();
                }
                GrantLoginProgress::SyncingSecrets => {
                    self.waiting_label
                        .set_label(&gettext("Transferring the encryption keys…"));
                    self.stack.set_visible_child_name("waiting");
                }
                GrantLoginProgress::Done => {}
            }
        }

        /// Confirm the check code entered by the user.
        #[template_callback]
        async fn confirm_check_code(&self) {
            let text = self.check_code_entry.text();
            let Ok(check_code) = text.trim().parse::<u8>() else {
                toast!(
                    self.obj(),
                    gettext("The check code must be a 2-digit number")
                );
                return;
            };

            let Some(check_code_sender) = self.check_code_sender.take() else {
                return;
            };

            self.confirm_check_code_button.set_is_loading(true);

            let handle = spawn_tokio!(async move { check_code_sender.send(check_code).await });

            if let Err(send_error) = handle.await.expect("task was not aborted") {
                // The grant task will error out as well, just log this.
                error!("Could not send check code: {send_error}");
            }

            self.confirm_check_code_button.set_is_loading(false);
            self.waiting_label
                .set_label(&gettext("Waiting for the new device…"));
            self.stack.set_visible_child_name("waiting");
        }

        /// Open the verification URI in the browser to allow the new sign-in.
        #[template_callback]
        async fn open_browser(&self) {
            let Some(verification_uri) = self.verification_uri.borrow().clone() else {
                return;
            };

            self.open_browser_button.set_is_loading(true);

            if let Err(launch_error) = gtk::UriLauncher::new(verification_uri.as_str())
                .launch_future(self.obj().root().and_downcast_ref::<gtk::Window>())
                .await
            {
                error!("Could not launch URI: {launch_error}");
                toast!(self.obj(), gettext("Could not open URL"));
                self.open_browser_button.set_is_loading(false);
                return;
            }

            self.open_browser_button.set_is_loading(false);

            // The grant only proceeds once we confirm that the verification URI was opened.
            if let Some(continuation_sender) = self.continuation_sender.take() {
                let handle = spawn_tokio!(async move { continuation_sender.confirm().await });

                if let Err(send_error) = handle.await.expect("task was not aborted") {
                    // The grant task will error out as well, just log this.
                    error!("Could not confirm the sign-in continuation: {send_error}");
                }
            }

            self.waiting_label
                .set_label(&gettext("Waiting for the new device…"));
            self.stack.set_visible_child_name("waiting");
        }

        /// Show the given error message.
        fn show_error(&self, message: &str) {
            self.error_label.set_label(message);
            self.stack.set_visible_child_name("error");
            self.retry_button.grab_focus();
        }

        /// Reset this page.
        fn clean(&self) {
            if let Some(handle) = self.abort_handle.take() {
                handle.abort();
            }

            self.check_code_sender.take();
            self.verification_uri.take();
            self.continuation_sender.take();
            self.check_code_entry.set_text("");
            self.confirm_check_code_button.set_is_loading(false);
            self.open_browser_button.set_is_loading(false);
            self.stack.set_visible_child_name("loading");
        }
    }
}

glib::wrapper! {
    /// Subpage to link a new device to the account by displaying a QR code, as
    /// defined in MSC4108.
    pub struct LinkDeviceSubpage(ObjectSubclass<imp::LinkDeviceSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LinkDeviceSubpage {
    /// Construct a new `LinkDeviceSubpage` for the given session.
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }
}

/// Get a user-facing error message for the given QR code grant login error.
fn grant_error_message(error: &QRCodeGrantLoginError) -> String {
    match error {
        QRCodeGrantLoginError::MissingSecretsBackup(_) => gettext(
            "The crypto identity of this session is not set up, it is needed to link a new device",
        ),
        QRCodeGrantLoginError::InvalidCheckCode => gettext("The check code was incorrect"),
        QRCodeGrantLoginError::NotFound => {
            gettext("The session has expired, try again with a new QR code")
        }
        QRCodeGrantLoginError::LoginFailure { .. } => {
            gettext("The sign-in failed or was cancelled on the other device")
        }
        QRCodeGrantLoginError::SecureChannel(_) => {
            gettext("Could not establish a secure connection to the other device")
        }
        _ => gettext("Could not link the new device"),
    }
}
