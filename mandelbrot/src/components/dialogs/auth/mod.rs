use std::{fmt::Debug, future::Future};

use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use matrix_sdk::{Error, encryption::CrossSigningResetAuthType};
use ruma::{
    api::{
        MatrixVersion, OutgoingRequestExt, SupportedVersions,
        auth_scheme::SendAccessToken,
        client::uiaa::{
            AuthData, AuthType, Dummy, FallbackAcknowledgement, Password, UiaaInfo, UserIdentifier,
            get_uiaa_fallback_page,
        },
    },
    assign,
};
use thiserror::Error;
use tracing::{error, warn};

mod in_browser_page;
mod password_page;

use self::{in_browser_page::AuthDialogInBrowserPage, password_page::AuthDialogPasswordPage};
use crate::{
    components::ToastableDialog, prelude::*, session::Session, spawn_tokio, toast,
    utils::OneshotNotifier,
};

mod imp {
    use std::{
        borrow::Cow,
        cell::{Cell, OnceCell, RefCell},
        rc::Rc,
        sync::Arc,
    };

    use glib::subclass::InitializingObject;
    use tokio::task::JoinHandle;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/dialogs/auth/mod.ui")]
    #[properties(wrapper_type = super::AuthDialog)]
    pub struct AuthDialog {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        /// The parent session.
        #[property(get, set, construct_only)]
        session: glib::WeakRef<Session>,
        /// Whether this dialog is presented.
        is_presented: Cell<bool>,
        /// The current state of the authentication.
        ///
        /// `None` means that the authentication has not started yet.
        state: RefCell<Option<AuthState>>,
        /// The page for the current stage.
        current_page: RefCell<Option<gtk::Widget>>,
        /// The notifier to signal to perform the current stage.
        notifier: OnceCell<OneshotNotifier<Option<()>>>,
        /// The handle to abort the current future.
        abort_handle: RefCell<Option<tokio::task::AbortHandle>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AuthDialog {
        const NAME: &'static str = "AuthDialog";
        type Type = super::AuthDialog;
        type ParentType = ToastableDialog;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.install_action("auth-dialog.continue", None, |obj, _, _| {
                obj.imp().notifier().notify_value(Some(()));
            });

            klass.install_action("auth-dialog.close", None, |obj, _, _| {
                obj.imp().close();
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AuthDialog {
        fn dispose(&self) {
            if let Some(abort_handle) = self.abort_handle.take() {
                abort_handle.abort();
            }
        }
    }

    impl WidgetImpl for AuthDialog {}
    impl AdwDialogImpl for AuthDialog {}
    impl ToastableDialogImpl for AuthDialog {}

    impl AuthDialog {
        /// The notifier to signal to perform the current stage.
        fn notifier(&self) -> &OneshotNotifier<Option<()>> {
            self.notifier
                .get_or_init(|| OneshotNotifier::new("AuthDialog"))
        }

        /// Authenticate the user to the server via an interactive
        /// authentication flow.
        ///
        /// The type of flow and the required stages are negotiated during the
        /// authentication. Returns the last server response on success.
        pub(super) async fn authenticate<Response, Fut, FN>(
            &self,
            parent: &gtk::Widget,
            callback: FN,
        ) -> Result<Response, AuthError>
        where
            Response: Send + 'static,
            Fut: Future<Output = Result<Response, Error>> + Send + 'static,
            FN: Fn(matrix_sdk::Client, Option<AuthData>) -> Fut + Send + Sync + 'static + Clone,
        {
            let Some(client) = self.session.upgrade().map(|s| s.client()) else {
                return Err(AuthError::Unknown);
            };

            // Perform the request once, to see if UIAA if required.
            let callback_clone = callback.clone();
            let client_clone = client.clone();
            let handle = spawn_tokio!(async move { callback_clone(client_clone, None).await });
            let result = self.await_tokio_task(handle).await;

            // If this is a UIAA error, we need authentication.
            let Some(uiaa_info) = result.uiaa_info() else {
                return result;
            };

            let result = self
                .perform_uiaa(uiaa_info.clone(), parent, move |auth_data| {
                    let client = client.clone();
                    let callback = callback.clone();
                    async move { callback(client, Some(auth_data)).await }
                })
                .await;

            self.close();

            result
        }

        /// Reset the cross-signing keys while handling the interactive
        /// authentication flow.
        ///
        /// The type of flow and the required stages are negotiated during the
        /// authentication.
        ///
        /// Note that due to the implementation of the underlying SDK API, this
        /// will not work if there are several stages in the flow.
        ///
        /// Returns the last server response on success.
        pub(super) async fn reset_cross_signing(
            &self,
            parent: &gtk::Widget,
        ) -> Result<(), AuthError> {
            let Some(encryption) = self.session.upgrade().map(|s| s.client().encryption()) else {
                return Err(AuthError::Unknown);
            };

            let handle = spawn_tokio!(async move { encryption.reset_cross_signing().await });
            let result = self.await_tokio_task(handle).await?;

            let Some(cross_signing_reset_handle) = result else {
                // No authentication is needed.
                return Ok(());
            };

            let result = match cross_signing_reset_handle.auth_type().clone() {
                CrossSigningResetAuthType::Uiaa(uiaa_info) => {
                    let cross_signing_reset_handle = Arc::new(cross_signing_reset_handle);

                    self.perform_uiaa(uiaa_info, parent, move |auth_data| {
                        let cross_signing_reset_handle = cross_signing_reset_handle.clone();
                        async move { cross_signing_reset_handle.auth(Some(auth_data)).await }
                    })
                    .await
                }
                CrossSigningResetAuthType::OAuth(info) => {
                    // This is a special stage, which requires opening a URL in the browser.
                    let page = AuthDialogInBrowserPage::new(info.approval_url.to_string());
                    let default_widget = page.default_widget().clone();

                    self.show_page(page.upcast(), &default_widget, parent);

                    // The `CrossSigningResetHandle` will poll the endpoint until it succeeds.
                    let handle =
                        spawn_tokio!(async move { cross_signing_reset_handle.auth(None).await });
                    self.await_tokio_task(handle).await
                }
            };

            self.close();

            result
        }

        /// Await the given tokio task, handling if it is aborted.
        async fn await_tokio_task<Response>(
            &self,
            handle: JoinHandle<Result<Response, Error>>,
        ) -> Result<Response, AuthError>
        where
            Response: Send + 'static,
        {
            self.abort_handle.replace(Some(handle.abort_handle()));

            let Ok(result) = handle.await else {
                // The future was aborted, which means that the user closed the dialog.
                return Err(AuthError::UserCancelled);
            };

            self.abort_handle.take();

            Ok(result?)
        }

        /// Perform UIAA for the given callback, starting with the given UIAA
        /// info.
        async fn perform_uiaa<Response, Fut, FN>(
            &self,
            mut uiaa_info: UiaaInfo,
            parent: &gtk::Widget,
            callback: FN,
        ) -> Result<Response, AuthError>
        where
            Response: Send + 'static,
            Fut: Future<Output = Result<Response, Error>> + Send + 'static,
            FN: Fn(AuthData) -> Fut + Send + Sync + 'static + Clone,
        {
            loop {
                let callback = callback.clone();

                let auth_data = self.perform_next_stage(&uiaa_info, parent).await?;

                // Get the current state of the authentication.
                let handle = spawn_tokio!(async move { callback(auth_data).await });
                let result = self.await_tokio_task(handle).await;

                // If this is a UIAA error, authentication continues.
                let Some(next_uiaa_info) = result.uiaa_info() else {
                    return result;
                };

                uiaa_info = next_uiaa_info.clone();
            }
        }

        /// Perform the preferred next stage in the given UIAA info.
        ///
        /// Stages that are actually supported are preferred. If no stages are
        /// supported, we use the web-based fallback.
        ///
        /// When this function returns, the next stage is ready to be performed.
        async fn perform_next_stage(
            &self,
            uiaa_info: &UiaaInfo,
            parent: &gtk::Widget,
        ) -> Result<AuthData, AuthError> {
            let Some(next_state) = AuthState::next(uiaa_info) else {
                // There is no stage left, this should not happen.
                error!("Cannot perform next stage when flow is complete");
                return Err(AuthError::Unknown);
            };

            if matches!(next_state.stage, AuthType::Dummy) {
                // We can just do this stage without waiting for user input.
                self.state.replace(Some(next_state));
                return self.current_stage_auth_data();
            }

            let receiver = self.notifier().listen();

            // If the stage didn't succeed, we get the same state again.
            let is_same_state = self
                .state
                .borrow()
                .as_ref()
                .is_some_and(|state| *state == next_state);

            if is_same_state {
                self.retry_current_stage(&next_state.stage, uiaa_info);
            } else {
                let (next_page, default_widget) = self.page(&next_state).await?;
                self.show_page(next_page, &default_widget, parent);
                self.state.replace(Some(next_state));
            }

            if receiver.await.is_none() {
                // The sender was dropped, which means that the user closed the dialog.
                return Err(AuthError::UserCancelled);
            }

            self.current_stage_auth_data()
        }

        // Retry the current stage.
        fn retry_current_stage(&self, stage: &AuthType, uiaa_info: &UiaaInfo) {
            // Show the authentication error, if there is one.
            if let Some(error) = &uiaa_info.auth_error {
                warn!("Could not perform authentication stage: {}", error.message);

                if matches!(stage, AuthType::Password) {
                    toast!(self.stack, gettext("The password is invalid."));
                } else {
                    toast!(self.stack, gettext("An unexpected error occurred."));
                }
            }

            // Reset the loading state of the page.
            if let Some(page) = self.current_page.borrow().as_ref() {
                if let Some(password_page) = page.downcast_ref::<AuthDialogPasswordPage>() {
                    password_page.retry();
                } else if let Some(in_browser_page) = page.downcast_ref::<AuthDialogInBrowserPage>()
                {
                    in_browser_page.retry();
                }
            }
        }

        /// Show the given page.
        fn show_page(&self, page: gtk::Widget, default_widget: &gtk::Widget, parent: &gtk::Widget) {
            self.stack.add_child(&page);
            self.stack.set_visible_child(&page);
            self.obj().set_default_widget(Some(default_widget));

            let prev_page = self.current_page.replace(Some(page));

            // Remove the previous page from the stack when the transition is over.
            if let Some(page) = prev_page {
                let cell = Rc::new(RefCell::new(None));

                let handler = self.stack.connect_transition_running_notify(clone!(
                    #[strong]
                    cell,
                    #[strong]
                    page,
                    move |stack| {
                        if !stack.is_transition_running()
                            && stack.visible_child().is_some_and(|child| child != page)
                        {
                            stack.remove(&page);

                            if let Some(handler) = cell.take() {
                                stack.disconnect(handler);
                            }
                        }
                    }
                ));

                cell.replace(Some(handler));
            }

            // Present the dialog if it is not already the case.
            if !self.is_presented.get() {
                self.obj().present(Some(parent));
                self.is_presented.set(true);
            }
        }

        /// Get the page for the given state.
        ///
        /// Returns a `(page, default_widget)` tuple.
        async fn page(&self, state: &AuthState) -> Result<(gtk::Widget, gtk::Widget), AuthError> {
            if state.stage == AuthType::Password {
                let page = AuthDialogPasswordPage::new();
                let default_widget = page.default_widget().clone();
                Ok((page.upcast(), default_widget))
            } else {
                let fallback_url = self.fallback_url(state).await?;
                let page = AuthDialogInBrowserPage::new(fallback_url);
                let default_widget = page.default_widget().clone();
                Ok((page.upcast(), default_widget))
            }
        }

        /// Get the fallback URL for the given state.
        async fn fallback_url(&self, state: &AuthState) -> Result<String, AuthError> {
            let Some(session) = self.session.upgrade() else {
                return Err(AuthError::Unknown);
            };

            let uiaa_session = state.session.clone().ok_or(AuthError::MissingSessionId)?;

            let request =
                get_uiaa_fallback_page::v3::Request::new(state.stage.clone(), uiaa_session);

            let client = session.client();
            let homeserver = client.homeserver();

            let handle =
                spawn_tokio!(async move { client.supported_versions().await.map_err(Into::into) });
            let result = self.await_tokio_task(handle).await;

            let supported_versions = match result {
                Ok(supported_versions) => supported_versions,
                Err(AuthError::ServerResponse(server_error)) => {
                    warn!("Could not get Matrix versions supported by homeserver: {server_error}");
                    // Default to the v3 endpoint.
                    SupportedVersions {
                        versions: [MatrixVersion::V1_1].into(),
                        features: Default::default(),
                    }
                }
                Err(error) => {
                    return Err(error);
                }
            };

            let http_request = match request.try_into_http_request::<Vec<u8>>(
                homeserver.as_ref(),
                SendAccessToken::None,
                Cow::Owned(supported_versions),
            ) {
                Ok(http_request) => http_request,
                Err(error) => {
                    error!("Could not construct fallback UIAA URL: {error}");
                    return Err(AuthError::Unknown);
                }
            };

            Ok(http_request.uri().to_string())
        }

        /// Get the authentication data for the current stage.
        fn current_stage_auth_data(&self) -> Result<AuthData, AuthError> {
            let Some(state) = self.state.borrow().clone() else {
                error!("Could not get current authentication state");
                return Err(AuthError::Unknown);
            };

            let auth_data = match state.stage {
                AuthType::Password => {
                    let password = self
                        .current_page
                        .borrow()
                        .as_ref()
                        .and_then(|page| page.downcast_ref::<AuthDialogPasswordPage>())
                        .ok_or_else(|| {
                            error!(
                                "Could not get password because current page is not password page"
                            );
                            AuthError::Unknown
                        })?
                        .password();

                    let user_id = self
                        .session
                        .upgrade()
                        .ok_or(AuthError::Unknown)?
                        .user_id()
                        .clone();

                    AuthData::Password(assign!(
                        Password::new(UserIdentifier::Matrix(user_id.into()), password),
                        { session: state.session }
                    ))
                }
                AuthType::Dummy => AuthData::Dummy(assign!(Dummy::new(), {
                    session: state.session
                })),
                _ => {
                    let uiaa_session = state.session.ok_or(AuthError::MissingSessionId)?;

                    AuthData::FallbackAcknowledgement(FallbackAcknowledgement::new(uiaa_session))
                }
            };

            Ok(auth_data)
        }

        // Close the dialog and cancel any ongoing task.
        fn close(&self) {
            if self.is_presented.get() {
                self.obj().close();
            }

            if let Some(abort_handle) = self.abort_handle.take() {
                abort_handle.abort();
            }

            self.notifier().notify();
        }
    }
}

glib::wrapper! {
    /// Dialog to guide the user through the [User-Interactive Authentication API] (UIAA).
    ///
    /// [User-Interactive Authentication API]: https://spec.matrix.org/latest/client-server-api/#user-interactive-authentication-api
    pub struct AuthDialog(ObjectSubclass<imp::AuthDialog>)
        @extends gtk::Widget, adw::Dialog, ToastableDialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl AuthDialog {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }

    /// Authenticate the user to the server via an interactive authentication
    /// flow.
    ///
    /// The type of flow and the required stages are negotiated during the
    /// authentication. Returns the last server response on success.
    pub(crate) async fn authenticate<Response, Fut, FN>(
        &self,
        parent: &impl IsA<gtk::Widget>,
        callback: FN,
    ) -> Result<Response, AuthError>
    where
        Response: Send + 'static,
        Fut: Future<Output = Result<Response, Error>> + Send + 'static,
        FN: Fn(matrix_sdk::Client, Option<AuthData>) -> Fut + Send + Sync + 'static + Clone,
    {
        self.imp().authenticate(parent.upcast_ref(), callback).await
    }

    /// Reset the cross-signing keys while handling the interactive
    /// authentication flow.
    ///
    /// The type of flow and the required stages are negotiated during the
    /// authentication. Returns the last server response on success.
    pub(crate) async fn reset_cross_signing(
        &self,
        parent: &impl IsA<gtk::Widget>,
    ) -> Result<(), AuthError> {
        self.imp().reset_cross_signing(parent.upcast_ref()).await
    }
}

/// Data about the current authentication state.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthState {
    /// The completed stages.
    completed: Vec<AuthType>,

    /// The current stage.
    stage: AuthType,

    /// The ID of the authentication session.
    session: Option<String>,
}

impl AuthState {
    /// Try to construct the next `AuthState` from the given UIAA info.
    ///
    /// Returns `None` if the next stage could not be determined.
    fn next(uiaa_info: &UiaaInfo) -> Option<Self> {
        // Find the possible next stages.
        // These are the next stage in flows that have the same stages as the ones we
        // have completed.
        let stages = uiaa_info
            .flows
            .iter()
            .filter_map(|flow| flow.stages.strip_prefix(uiaa_info.completed.as_slice()))
            .filter_map(|stages_left| stages_left.first());

        // Now get the first stage that we support.
        let mut next_stage = None;
        for stage in stages {
            if matches!(stage, AuthType::Password | AuthType::Sso | AuthType::Dummy) {
                // We found a supported stage.
                next_stage = Some(stage);
                break;
            } else if next_stage.is_none() {
                // We will default to the first stage if we do not find one that we support.
                next_stage = Some(stage);
            }
        }

        let stage = next_stage?.clone();

        Some(Self {
            completed: uiaa_info.completed.clone(),
            stage,
            session: uiaa_info.session.clone(),
        })
    }
}

/// An error during UIAA interaction.
#[derive(Debug, Error)]
pub enum AuthError {
    /// The server returned a non-UIAA error.
    #[error(transparent)]
    ServerResponse(#[from] Error),

    /// The ID of the UIAA session is missing for a stage that requires it.
    #[error("The ID of the session is missing")]
    MissingSessionId,

    /// The user cancelled the authentication.
    #[error("The user cancelled the authentication")]
    UserCancelled,

    /// An unexpected error occurred.
    #[error("An unexpected error occurred")]
    Unknown,
}

/// Helper trait to extract [`UiaaInfo`].
trait ExtractUiaa {
    /// Extract the [`UiaaInfo`] from this type, if it contains one.
    fn uiaa_info(&self) -> Option<&UiaaInfo>;
}

impl ExtractUiaa for AuthError {
    fn uiaa_info(&self) -> Option<&UiaaInfo> {
        if let Self::ServerResponse(server_error) = self {
            server_error.as_uiaa_response()
        } else {
            None
        }
    }
}

impl ExtractUiaa for Error {
    fn uiaa_info(&self) -> Option<&UiaaInfo> {
        self.as_uiaa_response()
    }
}

impl<T, Err> ExtractUiaa for Result<T, Err>
where
    Err: ExtractUiaa,
{
    fn uiaa_info(&self) -> Option<&UiaaInfo> {
        match self {
            Ok(_) => None,
            Err(error) => error.uiaa_info(),
        }
    }
}
