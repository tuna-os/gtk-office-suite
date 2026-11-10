use futures_util::StreamExt;
use gtk::{
    glib,
    glib::{clone, closure_local},
    prelude::*,
    subclass::prelude::*,
};
use matrix_sdk::encryption::verification::{
    CancelInfo, Emoji, QrVerification, QrVerificationData, QrVerificationState, SasState,
    SasVerification, Verification, VerificationRequest, VerificationRequestState,
};
use qrcode::QrCode;
use ruma::{
    OwnedDeviceId,
    events::key::verification::{REQUEST_RECEIVED_TIMEOUT, VerificationMethod, cancel::CancelCode},
};
use tracing::{debug, error};

use super::{VerificationKey, load_supported_verification_methods};
use crate::{
    components::QrCodeScanner,
    prelude::*,
    session::{Member, Membership, Room, User},
    spawn, spawn_tokio,
    utils::BoundConstructOnlyObject,
};

#[glib::flags(name = "VerificationSupportedMethods")]
pub enum VerificationSupportedMethods {
    SAS = 0b0000_0001,
    QR_SHOW = 0b0000_0010,
    QR_SCAN = 0b0000_0100,
}

impl<'a> From<&'a [VerificationMethod]> for VerificationSupportedMethods {
    fn from(methods: &'a [VerificationMethod]) -> Self {
        let mut result = Self::empty();

        for method in methods {
            match method {
                VerificationMethod::SasV1 => result.insert(Self::SAS),
                VerificationMethod::QrCodeScanV1 => result.insert(Self::QR_SCAN),
                VerificationMethod::QrCodeShowV1 => result.insert(Self::QR_SHOW),
                _ => {}
            }
        }

        result
    }
}

impl Default for VerificationSupportedMethods {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Debug, Default, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[enum_type(name = "VerificationState")]
pub enum VerificationState {
    /// We created and sent the request.
    ///
    /// We must wait for the other user/device to accept it.
    #[default]
    Created,
    /// The other user/device sent us a request.
    ///
    /// We should ask the user if they want to accept it.
    Requested,
    /// We support none of the other user's verification methods.
    NoSupportedMethods,
    /// The request was accepted.
    ///
    /// We should ask the user to choose a method.
    Ready,
    /// An SAS verification was started.
    ///
    /// We should show the emojis and ask the user to confirm that they match.
    SasConfirm,
    /// The user wants to scan a QR Code.
    QrScan,
    /// The user scanned a QR Code.
    QrScanned,
    /// Our QR Code was scanned.
    ///
    /// We should ask the user to confirm that the QR Code was scanned
    /// successfully.
    QrConfirm,
    /// The verification was successful.
    Done,
    /// The verification was cancelled.
    Cancelled,
    /// The verification was automatically dismissed.
    ///
    /// Happens when a received request is not accepted by us after 2 minutes.
    Dismissed,
    /// The verification was happening in-room but the room was left.
    RoomLeft,
    /// An unexpected error happened.
    Error,
}

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        marker::PhantomData,
        sync::LazyLock,
    };

    use glib::subclass::Signal;

    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::IdentityVerification)]
    pub struct IdentityVerification {
        /// The SDK's verification request.
        request: OnceCell<VerificationRequest>,
        request_changes_abort_handle: RefCell<Option<tokio::task::AbortHandle>>,
        /// The SDK's verification, if one was started.
        verification: RefCell<Option<Verification>>,
        verification_changes_abort_handle: RefCell<Option<tokio::task::AbortHandle>>,
        /// The user to verify.
        #[property(get, set = Self::set_user, construct_only)]
        user: BoundConstructOnlyObject<User>,
        /// The room of this verification, if any.
        #[property(get, set = Self::set_room, construct_only)]
        room: glib::WeakRef<Room>,
        membership_handler: RefCell<Option<glib::SignalHandlerId>>,
        /// The state of this verification
        #[property(get, set = Self::set_state, construct_only, builder(VerificationState::default()))]
        state: Cell<VerificationState>,
        /// Whether the verification request was accepted.
        ///
        /// It means that the verification reached at least the `Ready` state.
        #[property(get)]
        was_accepted: Cell<bool>,
        /// Whether this verification is finished.
        #[property(get = Self::is_finished)]
        is_finished: PhantomData<bool>,
        /// The supported methods of the verification request.
        #[property(get = Self::supported_methods, type = VerificationSupportedMethods)]
        supported_methods: RefCell<Vec<VerificationMethod>>,
        /// The flow ID of this verification.
        #[property(get = Self::flow_id)]
        flow_id: PhantomData<String>,
        /// The time and date when the verification request was received.
        #[property(get)]
        received_time: OnceCell<glib::DateTime>,
        received_timeout_source: RefCell<Option<glib::SourceId>>,
        /// The display name of this verification.
        #[property(get = Self::display_name)]
        display_name: PhantomData<String>,
        /// The QR Code, if the `QrCodeShowV1` method is supported.
        pub(super) qr_code: RefCell<Option<QrCode>>,
        /// The QR code scanner, if the user wants to scan a QR Code and we
        /// have access to the camera.
        #[property(get)]
        pub(super) qrcode_scanner: RefCell<Option<QrCodeScanner>>,
        /// Whether this verification was viewed by the user.
        #[property(get, set = Self::set_was_viewed, explicit_notify)]
        was_viewed: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for IdentityVerification {
        const NAME: &'static str = "IdentityVerification";
        type Type = super::IdentityVerification;
    }

    #[glib::derived_properties]
    impl ObjectImpl for IdentityVerification {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    // The SAS data changed.
                    Signal::builder("sas-data-changed").build(),
                    // The cancel info changed.
                    Signal::builder("cancel-info-changed").build(),
                    // The verification has been replaced by a new one.
                    Signal::builder("replaced")
                        .param_types([super::IdentityVerification::static_type()])
                        .build(),
                    // The verification is done, but has not changed its state yes.
                    //
                    // Return `glib::Propagation::Stop` in a signal handler to prevent the state
                    // from changing to `VerificationState::Done`. Can be used to replace the last
                    // screen of `IdentityVerificationView`.
                    Signal::builder("done").return_type::<bool>().build(),
                    // The verification can be dismissed.
                    Signal::builder("dismiss").build(),
                    // The verification should be removed from the verification list.
                    Signal::builder("remove-from-list").build(),
                ]
            });
            SIGNALS.as_ref()
        }

        fn dispose(&self) {
            if let Some(handler) = self.membership_handler.take()
                && let Some(room) = self.room.upgrade()
            {
                room.own_member().disconnect(handler);
            }
            if let Some(handle) = self.request_changes_abort_handle.take() {
                handle.abort();
            }
            if let Some(handle) = self.verification_changes_abort_handle.take() {
                handle.abort();
            }
            if let Some(source) = self.received_timeout_source.take() {
                source.remove();
            }

            let request = self.request().clone();
            if !request.is_done() && !request.is_passive() && !request.is_cancelled() {
                spawn_tokio!(async move {
                    if let Err(error) = request.cancel().await {
                        error!("Could not cancel verification request on dispose: {error}");
                    }
                });
            }
        }
    }

    impl IdentityVerification {
        /// Set the SDK's verification request.
        pub(super) async fn set_request(&self, request: VerificationRequest) {
            let request = self.request.get_or_init(|| request);

            let Ok(datetime) = glib::DateTime::now_local() else {
                error!("Could not get current GDateTime");
                return;
            };

            // Set up the timeout if we received the request and it is not accepted yet.
            if matches!(request.state(), VerificationRequestState::Requested { .. }) {
                let source_id = glib::timeout_add_local_once(
                    REQUEST_RECEIVED_TIMEOUT,
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move || {
                            imp.received_timeout_source.take();

                            imp.set_state(VerificationState::Dismissed);
                            imp.obj().dismiss();
                        }
                    ),
                );
                self.received_timeout_source.replace(Some(source_id));
            }

            self.received_time
                .set(datetime)
                .expect("received time should be uninitialized");

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let fut = request.changes().for_each(move |state| {
                let obj_weak = obj_weak.clone();
                async move {
                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.imp().handle_request_state(state).await;
                            }
                        });
                    });
                }
            });

            self.request_changes_abort_handle
                .replace(Some(spawn_tokio!(fut).abort_handle()));

            let state = request.state();
            self.handle_request_state(state).await;
        }

        /// The SDK's verification request.
        pub(super) fn request(&self) -> &VerificationRequest {
            self.request.get().expect("request should be initialized")
        }

        /// Set the user to verify.
        fn set_user(&self, user: User) {
            let mut handlers = Vec::new();

            // If the user is a room member, it means it's an in-room verification, we need
            // to keep track of their name since it's used as the display name.
            if user.is::<Member>() {
                let obj = self.obj();
                let display_name_handler = user.connect_display_name_notify(clone!(
                    #[weak]
                    obj,
                    move |_| {
                        obj.notify_display_name();
                    }
                ));
                handlers.push(display_name_handler);
            }

            self.user.set(user, handlers);
        }

        /// Set the room of the verification, if any.
        fn set_room(&self, room: Option<&Room>) {
            let Some(room) = room else {
                // Nothing to do if there is no room.
                return;
            };

            let handler = room.own_member().connect_membership_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |own_member| {
                    if matches!(own_member.membership(), Membership::Leave | Membership::Ban) {
                        // If the user is not in the room anymore, nothing can be done with this
                        // verification.
                        imp.set_state(VerificationState::RoomLeft);

                        if let Some(handler) = imp.membership_handler.take() {
                            own_member.disconnect(handler);
                        }
                    }
                }
            ));
            self.membership_handler.replace(Some(handler));

            self.room.set(Some(room));
        }

        /// Set the state of this verification.
        pub(super) fn set_state(&self, state: VerificationState) {
            if self.state.get() == state {
                return;
            }
            let obj = self.obj();

            if state == VerificationState::Done {
                let ret = obj.emit_by_name::<bool>("done", &[]);
                if glib::Propagation::from(ret).is_stop() {
                    return;
                }
            } else if state != VerificationState::QrScan && self.qrcode_scanner.take().is_some() {
                obj.notify_qrcode_scanner();
            }

            self.state.set(state);

            obj.notify_state();

            if self.is_finished() {
                obj.notify_is_finished();
            }
        }

        /// Whether this verification is finished.
        fn is_finished(&self) -> bool {
            matches!(
                self.state.get(),
                VerificationState::Cancelled
                    | VerificationState::Dismissed
                    | VerificationState::Done
                    | VerificationState::Error
                    | VerificationState::RoomLeft
            )
        }

        /// Set the supported methods of this verification.
        fn set_supported_methods(&self, supported_methods: Vec<VerificationMethod>) {
            if *self.supported_methods.borrow() == supported_methods {
                return;
            }

            self.supported_methods.replace(supported_methods);
            self.obj().notify_supported_methods();
        }

        /// The supported methods of this verifications.
        fn supported_methods(&self) -> VerificationSupportedMethods {
            self.supported_methods.borrow().as_slice().into()
        }

        /// The display name of this verification request.
        fn display_name(&self) -> String {
            let user = self.user.obj();

            if user.is_own_user() {
                // TODO: give this request a name based on the device
                "Login Request".to_string()
            } else {
                user.display_name()
            }
        }

        /// The flow ID of this verification request.
        fn flow_id(&self) -> String {
            self.request().flow_id().to_owned()
        }

        /// Set whether this verification was viewed by the user.
        fn set_was_viewed(&self, was_viewed: bool) {
            if !was_viewed {
                // The user cannot unview the verification.
                return;
            }

            self.was_viewed.set(was_viewed);
            self.obj().notify_was_viewed();
        }

        /// Set whether this request was accepted.
        fn set_was_accepted(&self, was_accepted: bool) {
            if !was_accepted || self.was_accepted.get() {
                // The state cannot go backwards.
                return;
            }

            self.was_accepted.set(true);
            self.obj().notify_was_accepted();
        }

        /// Handle a change in the request's state.
        async fn handle_request_state(&self, state: VerificationRequestState) {
            let request = self.request();

            if !matches!(state, VerificationRequestState::Requested { .. })
                && let Some(source) = self.received_timeout_source.take()
            {
                source.remove();
            }
            if !matches!(
                state,
                VerificationRequestState::Created { .. }
                    | VerificationRequestState::Requested { .. }
            ) {
                self.set_was_accepted(true);
            }

            match state {
                VerificationRequestState::Created { .. } => {}
                VerificationRequestState::Requested { their_methods, .. } => {
                    let our_methods = load_supported_verification_methods().await;
                    let supported_methods = intersect_methods(our_methods, &their_methods);

                    if supported_methods.is_empty() {
                        self.set_state(VerificationState::NoSupportedMethods);
                    } else {
                        self.set_state(VerificationState::Requested);
                    }
                }
                VerificationRequestState::Ready {
                    their_methods,
                    our_methods,
                    ..
                } => {
                    let mut supported_methods = intersect_methods(our_methods, &their_methods);

                    // Remove the reciprocate method, it's not a flow in itself.
                    let reciprocate_idx =
                        supported_methods.iter().enumerate().find_map(|(idx, m)| {
                            (*m == VerificationMethod::ReciprocateV1).then_some(idx)
                        });
                    if let Some(idx) = reciprocate_idx {
                        supported_methods.remove(idx);
                    }

                    // Check that we can get the QR Code, to avoid exposing the method if it doesn't
                    // work.
                    let show_qr_idx = supported_methods.iter().enumerate().find_map(|(idx, m)| {
                        (*m == VerificationMethod::QrCodeShowV1).then_some(idx)
                    });
                    if let Some(idx) = show_qr_idx
                        && !self.load_qr_code().await
                    {
                        supported_methods.remove(idx);
                    }

                    if supported_methods.is_empty() {
                        // This should not happen.
                        error!(
                            "Invalid verification: no methods are supported by both sessions, cancelling…"
                        );
                        if self.obj().cancel().await.is_err() {
                            self.set_state(VerificationState::NoSupportedMethods);
                        }
                    } else {
                        self.set_supported_methods(supported_methods.clone());

                        if supported_methods.len() == 1
                            && !request.we_started()
                            && supported_methods[0] == VerificationMethod::SasV1
                        {
                            // We only go forward for SAS, because QrCodeShow is the
                            // same screen as the one to choose a method and we need
                            // to tell the user we are going to need to access the
                            // camera for QrCodeScan.
                            if self.obj().start_sas().await.is_ok() {
                                return;
                            }
                        }

                        self.set_state(VerificationState::Ready);
                    }
                }
                VerificationRequestState::Transitioned { verification } => {
                    self.set_verification(verification).await;
                }
                VerificationRequestState::Done => {
                    self.set_state(VerificationState::Done);
                }
                VerificationRequestState::Cancelled(info) => self.handle_cancelled_state(&info),
            }
        }

        /// Handle when the request was cancelled.
        fn handle_cancelled_state(&self, cancel_info: &CancelInfo) {
            debug!("Verification was cancelled: {cancel_info:?}");
            let cancel_code = cancel_info.cancel_code();

            if cancel_info.cancelled_by_us() && *cancel_code == CancelCode::User {
                // We should handle this already.
                return;
            }

            if *cancel_code == CancelCode::Accepted && !self.was_viewed.get() {
                // We can safely remove it.
                self.obj().dismiss();
                return;
            }

            self.obj().emit_by_name::<()>("cancel-info-changed", &[]);
            self.set_state(VerificationState::Cancelled);
        }

        /// Set the SDK's Verification.
        async fn set_verification(&self, verification: Verification) {
            if let Some(handle) = self.verification_changes_abort_handle.take() {
                handle.abort();
            }

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let handle = match &verification {
                Verification::SasV1(sas_verification) => {
                    let fut = sas_verification.changes().for_each(move |state| {
                        let obj_weak = obj_weak.clone();
                        async move {
                            let ctx = glib::MainContext::default();
                            ctx.spawn(async move {
                                spawn!(async move {
                                    if let Some(obj) = obj_weak.upgrade() {
                                        obj.imp().handle_sas_verification_state(state).await;
                                    }
                                });
                            });
                        }
                    });
                    spawn_tokio!(fut).abort_handle()
                }
                Verification::QrV1(qr_verification) => {
                    let fut = qr_verification.changes().for_each(move |state| {
                        let obj_weak = obj_weak.clone();
                        async move {
                            let ctx = glib::MainContext::default();
                            ctx.spawn(async move {
                                spawn!(async move {
                                    if let Some(obj) = obj_weak.upgrade() {
                                        obj.imp().handle_qr_verification_state(state);
                                    }
                                });
                            });
                        }
                    });
                    spawn_tokio!(fut).abort_handle()
                }
                _ => {
                    error!("We only support SAS and QR verification");
                    self.set_state(VerificationState::Error);
                    return;
                }
            };

            self.verification.replace(Some(verification.clone()));
            self.verification_changes_abort_handle.replace(Some(handle));

            match verification {
                Verification::SasV1(sas_verification) => {
                    self.handle_sas_verification_state(sas_verification.state())
                        .await;
                }
                Verification::QrV1(qr_verification) => {
                    self.handle_qr_verification_state(qr_verification.state());
                }
                _ => unreachable!(),
            }
        }

        /// Handle a change in the QR verification's state.
        fn handle_qr_verification_state(&self, state: QrVerificationState) {
            match state {
                QrVerificationState::Started
                | QrVerificationState::Confirmed
                | QrVerificationState::Reciprocated => {}
                QrVerificationState::Scanned => self.set_state(VerificationState::QrConfirm),
                QrVerificationState::Done { .. } => self.set_state(VerificationState::Done),
                QrVerificationState::Cancelled(info) => self.handle_cancelled_state(&info),
            }
        }

        /// The SDK's QR verification, if one was started.
        pub(super) fn qr_verification(&self) -> Option<QrVerification> {
            match self.verification.borrow().as_ref()? {
                Verification::QrV1(v) => Some(v.clone()),
                _ => None,
            }
        }

        /// Handle a change in the SAS verification's state.
        async fn handle_sas_verification_state(&self, state: SasState) {
            let Some(sas_verification) = self.sas_verification() else {
                return;
            };

            match state {
                SasState::Created { .. } | SasState::Accepted { .. } | SasState::Confirmed => {}
                SasState::Started { .. } => {
                    let handle = spawn_tokio!(async move { sas_verification.accept().await });
                    if let Err(error) = handle.await.expect("task was not aborted") {
                        error!("Could not accept SAS verification: {error}");
                        self.set_state(VerificationState::Error);
                    }
                }
                SasState::KeysExchanged { .. } => {
                    self.obj().emit_by_name::<()>("sas-data-changed", &[]);
                    self.set_state(VerificationState::SasConfirm);
                }
                SasState::Done { .. } => self.set_state(VerificationState::Done),
                SasState::Cancelled(info) => self.handle_cancelled_state(&info),
            }
        }

        /// The SDK's SAS verification, if one was started.
        pub(super) fn sas_verification(&self) -> Option<SasVerification> {
            match self.verification.borrow().as_ref()? {
                Verification::SasV1(v) => Some(v.clone()),
                _ => None,
            }
        }

        /// Try to load the QR Code.
        ///
        /// Return `true` if it was successfully loaded, `false` otherwise.
        async fn load_qr_code(&self) -> bool {
            let request = self.request().clone();

            let handle = spawn_tokio!(async move { request.generate_qr_code().await });

            let qr_verification = match handle.await.expect("task was not aborted") {
                Ok(Some(qr_verification)) => qr_verification,
                Ok(None) => {
                    error!("Could not start QR verification generation: unknown reason");
                    return false;
                }
                Err(error) => {
                    error!("Could not start QR verification generation: {error}");
                    return false;
                }
            };

            match qr_verification.to_qr_code() {
                Ok(qr_code) => {
                    self.qr_code.replace(Some(qr_code));
                    true
                }
                Err(error) => {
                    error!("Could not generate verification QR code: {error}");
                    false
                }
            }
        }
    }
}

glib::wrapper! {
    /// An identity verification request.
    pub struct IdentityVerification(ObjectSubclass<imp::IdentityVerification>);
}

impl IdentityVerification {
    /// Construct a verification for the given request.
    pub async fn new(request: VerificationRequest, user: &User, room: Option<&Room>) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("user", user)
            .property("room", room)
            .build();
        obj.imp().set_request(request).await;
        obj
    }

    /// The unique identifying key of this verification.
    pub(crate) fn key(&self) -> VerificationKey {
        VerificationKey::from_request(self.imp().request())
    }

    /// Whether this is a self-verification.
    pub(crate) fn is_self_verification(&self) -> bool {
        self.imp().request().is_self_verification()
    }

    /// Whether we started this verification.
    pub(crate) fn started_by_us(&self) -> bool {
        self.imp().request().we_started()
    }

    /// The ID of the other device that is being verified.
    pub(crate) fn other_device_id(&self) -> Option<OwnedDeviceId> {
        let request_state = self.imp().request().state();
        let other_device_data = match &request_state {
            VerificationRequestState::Requested {
                other_device_data, ..
            }
            | VerificationRequestState::Ready {
                other_device_data, ..
            } => other_device_data,
            VerificationRequestState::Transitioned { verification } => match verification {
                Verification::SasV1(sas) => sas.other_device(),
                Verification::QrV1(qr) => qr.other_device(),
                _ => None?,
            },
            VerificationRequestState::Created { .. }
            | VerificationRequestState::Done
            | VerificationRequestState::Cancelled(_) => None?,
        };

        Some(other_device_data.device_id().to_owned())
    }

    /// Information about the verification cancellation, if any.
    pub(crate) fn cancel_info(&self) -> Option<CancelInfo> {
        self.imp().request().cancel_info()
    }

    /// Cancel the verification request.
    ///
    /// This can be used to decline the request or cancel it at any time.
    pub(crate) async fn cancel(&self) -> Result<(), matrix_sdk::Error> {
        let request = self.imp().request().clone();

        if request.is_done() || request.is_passive() || request.is_cancelled() {
            return Err(matrix_sdk::Error::UnknownError(
                "Cannot cancel request that is already finished".into(),
            ));
        }

        let handle = spawn_tokio!(async move { request.cancel().await });

        match handle.await.expect("task was not aborted") {
            Ok(()) => {
                self.dismiss();
                Ok(())
            }
            Err(error) => {
                error!("Could not cancel verification request: {error}");
                Err(error)
            }
        }
    }

    /// Accept the verification request.
    pub(crate) async fn accept(&self) -> Result<(), ()> {
        let request = self.imp().request().clone();

        let VerificationRequestState::Requested { their_methods, .. } = request.state() else {
            error!("Cannot accept verification that is not in the requested state");
            return Err(());
        };
        let our_methods = load_supported_verification_methods().await;
        let methods = intersect_methods(our_methods, &their_methods);

        let handle = spawn_tokio!(async move { request.accept_with_methods(methods).await });

        match handle.await.expect("task was not aborted") {
            Ok(()) => Ok(()),
            Err(error) => {
                error!("Could not accept verification request: {error}");
                Err(())
            }
        }
    }

    /// Go back to the state to choose a verification method.
    pub(crate) fn choose_method(&self) {
        self.imp().set_state(VerificationState::Ready);
    }

    /// Whether the current SAS verification supports emoji.
    pub(crate) fn sas_supports_emoji(&self) -> bool {
        self.imp()
            .sas_verification()
            .is_some_and(|v| v.supports_emoji())
    }

    /// The list of emojis for the current SAS verification, if any.
    pub(crate) fn sas_emoji(&self) -> Option<[Emoji; 7]> {
        self.imp().sas_verification()?.emoji()
    }

    /// The list of decimals for the current SAS verification, if any.
    pub(crate) fn sas_decimals(&self) -> Option<(u16, u16, u16)> {
        self.imp().sas_verification()?.decimals()
    }

    /// The QR Code, if the `QrCodeShowV1` method is supported.
    pub(crate) fn qr_code(&self) -> Option<QrCode> {
        self.imp().qr_code.borrow().clone()
    }

    /// Whether we have the QR code.
    pub(crate) fn has_qr_code(&self) -> bool {
        self.imp().qr_code.borrow().is_some()
    }

    /// Start a QR Code scan.
    pub(crate) async fn start_qr_code_scan(&self) -> Result<(), ()> {
        let imp = self.imp();

        match QrCodeScanner::new().await {
            Some(qrcode_scanner) => {
                imp.qrcode_scanner.replace(Some(qrcode_scanner));
                self.notify_qrcode_scanner();

                imp.set_state(VerificationState::QrScan);

                Ok(())
            }
            None => Err(()),
        }
    }

    /// The QR Code was scanned.
    pub(crate) async fn qr_code_scanned(&self, data: QrVerificationData) -> Result<(), ()> {
        let imp = self.imp();
        imp.set_state(VerificationState::QrScanned);
        let request = imp.request().clone();

        let handle = spawn_tokio!(async move { request.scan_qr_code(data).await });

        match handle.await.expect("task was not aborted") {
            Ok(Some(_)) => Ok(()),
            Ok(None) => {
                error!("Could not validate scanned verification QR code: unknown reason");
                Err(())
            }
            Err(error) => {
                error!("Could not validate scanned verification QR code: {error}");
                Err(())
            }
        }
    }

    /// Confirm that the QR Code was scanned by the other party.
    pub(crate) async fn confirm_qr_code_scanned(&self) -> Result<(), ()> {
        let Some(qr_verification) = self.imp().qr_verification() else {
            error!("Cannot confirm QR Code scan without an ongoing QR verification");
            return Err(());
        };

        let handle = spawn_tokio!(async move { qr_verification.confirm().await });

        match handle.await.expect("task was not aborted") {
            Ok(()) => Ok(()),
            Err(error) => {
                error!("Could not confirm scanned verification QR code: {error}");
                Err(())
            }
        }
    }

    /// Start a SAS verification.
    pub(crate) async fn start_sas(&self) -> Result<(), ()> {
        let request = self.imp().request().clone();
        let handle = spawn_tokio!(async move { request.start_sas().await });

        match handle.await.expect("task was not aborted") {
            Ok(Some(_)) => Ok(()),
            Ok(None) => {
                error!("Could not start SAS verification: unknown reason");
                Err(())
            }
            Err(error) => {
                error!("Could not start SAS verification: {error}");
                Err(())
            }
        }
    }

    /// The SAS data does not match.
    pub(crate) async fn sas_mismatch(&self) -> Result<(), ()> {
        let Some(sas_verification) = self.imp().sas_verification() else {
            error!("Cannot send SAS mismatch without an ongoing SAS verification");
            return Err(());
        };

        let handle = spawn_tokio!(async move { sas_verification.mismatch().await });

        match handle.await.expect("task was not aborted") {
            Ok(()) => Ok(()),
            Err(error) => {
                error!("Could not send SAS verification mismatch: {error}");
                Err(())
            }
        }
    }

    /// The SAS data matches.
    pub(crate) async fn sas_match(&self) -> Result<(), ()> {
        let Some(sas_verification) = self.imp().sas_verification() else {
            error!("Cannot send SAS match without an ongoing SAS verification");
            return Err(());
        };

        let handle = spawn_tokio!(async move { sas_verification.confirm().await });

        match handle.await.expect("task was not aborted") {
            Ok(()) => Ok(()),
            Err(error) => {
                error!("Could not send SAS verification match: {error}");
                Err(())
            }
        }
    }

    /// Restart this verification with a new one to the same user.
    pub(crate) async fn restart(&self) -> Result<Self, ()> {
        let user = self.user();
        let verification_list = user.session().verification_list();

        let new_user = (!self.is_self_verification()).then_some(user);
        let new_verification = verification_list.create(new_user).await?;

        self.emit_by_name::<()>("replaced", &[&new_verification]);

        // If we restart because an unexpected error happened, try to cancel it.
        if self.cancel().await.is_err() {
            self.dismiss();
        }

        Ok(new_verification)
    }

    /// The verification can be dismissed.
    ///
    /// Also removes it from the verification list.
    pub(crate) fn dismiss(&self) {
        self.remove_from_list();
        self.emit_by_name::<()>("dismiss", &[]);
    }

    /// The verification can be removed from the verification list.
    ///
    /// You will usually want to use [`IdentityVerification::dismiss()`] because
    /// the interface listens for the signal it emits, and it calls this method
    /// internally.
    pub(crate) fn remove_from_list(&self) {
        self.emit_by_name::<()>("remove-from-list", &[]);
    }

    /// Connect to the signal emitted when the SAS data changed.
    pub fn connect_sas_data_changed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "sas-data-changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }

    /// Connect to the signal emitted when the cancel info changed.
    pub fn connect_cancel_info_changed<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "cancel-info-changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }

    /// Connect to the signal emitted when the verification has been replaced.
    ///
    /// The second parameter to the function is the new verification.
    pub fn connect_replaced<F: Fn(&Self, &Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "replaced",
            true,
            closure_local!(move |obj: Self, new_verification: Self| {
                f(&obj, &new_verification);
            }),
        )
    }

    /// Connect to the signal emitted when the verification is done, but its
    /// state does not reflect that yet.
    ///
    /// Return `glib::Propagation::Stop` in the signal handler to prevent the
    /// state from changing to `VerificationState::Done`. Can be used to replace
    /// the last screen of `IdentityVerificationView`.
    pub fn connect_done<F: Fn(&Self) -> glib::Propagation + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "done",
            true,
            closure_local!(move |obj: Self| {
                let ret = f(&obj);

                if ret.is_stop() {
                    obj.stop_signal_emission_by_name("done");
                }

                bool::from(ret)
            }),
        )
    }

    /// Connect to the signal emitted when the verification can be dismissed.
    pub fn connect_dismiss<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "dismiss",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }

    /// Connect to the signal emitted when the verification can be removed from
    /// the verification list.
    pub(super) fn connect_remove_from_list<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "remove-from-list",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}

/// Get the intersection or our methods and their methods.
fn intersect_methods(
    our_methods: Vec<VerificationMethod>,
    their_methods: &[VerificationMethod],
) -> Vec<VerificationMethod> {
    let mut supported_methods = our_methods;

    supported_methods.retain(|m| match m {
        VerificationMethod::SasV1 => their_methods.contains(&VerificationMethod::SasV1),
        VerificationMethod::QrCodeScanV1 => {
            their_methods.contains(&VerificationMethod::QrCodeShowV1)
                && their_methods.contains(&VerificationMethod::ReciprocateV1)
        }
        VerificationMethod::QrCodeShowV1 => {
            their_methods.contains(&VerificationMethod::QrCodeScanV1)
                && their_methods.contains(&VerificationMethod::ReciprocateV1)
        }
        VerificationMethod::ReciprocateV1 => {
            (their_methods.contains(&VerificationMethod::QrCodeShowV1)
                || their_methods.contains(&VerificationMethod::QrCodeScanV1))
                && their_methods.contains(&VerificationMethod::ReciprocateV1)
        }
        _ => false,
    });

    supported_methods
}
