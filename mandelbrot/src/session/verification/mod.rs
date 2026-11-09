use gtk::{glib, prelude::*};
use matrix_sdk::encryption::verification::VerificationRequest;
use ruma::{OwnedUserId, UserId, events::key::verification::VerificationMethod};

mod identity_verification;
mod verification_list;

pub(crate) use self::{
    identity_verification::{
        IdentityVerification, VerificationState, VerificationSupportedMethods,
    },
    verification_list::VerificationList,
};
use crate::{components::Camera, prelude::*};

/// A unique key to identify an identity verification.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) struct VerificationKey {
    /// The ID of the user being verified.
    pub(crate) user_id: OwnedUserId,
    /// The ID of the verification.
    pub(crate) flow_id: String,
}

impl VerificationKey {
    /// Create a new `VerificationKey` with the given user ID and flow ID.
    pub(crate) fn new(user_id: OwnedUserId, flow_id: String) -> Self {
        Self { user_id, flow_id }
    }

    /// Create a new `VerificationKey` from the given [`VerificationRequest`].
    pub(crate) fn from_request(request: &VerificationRequest) -> Self {
        Self::new(
            request.other_user_id().to_owned(),
            request.flow_id().to_owned(),
        )
    }
}

impl StaticVariantType for VerificationKey {
    fn static_variant_type() -> std::borrow::Cow<'static, glib::VariantTy> {
        <(String, String)>::static_variant_type()
    }
}

impl ToVariant for VerificationKey {
    fn to_variant(&self) -> glib::Variant {
        (self.user_id.as_str(), self.flow_id.as_str()).to_variant()
    }
}

impl FromVariant for VerificationKey {
    fn from_variant(variant: &glib::Variant) -> Option<Self> {
        let (user_id_str, flow_id) = variant.get::<(String, String)>()?;
        let user_id = UserId::parse(user_id_str).ok()?;
        Some(Self { user_id, flow_id })
    }
}

/// Load the supported verification methods on this system.
async fn load_supported_verification_methods() -> Vec<VerificationMethod> {
    let mut methods = vec![
        VerificationMethod::SasV1,
        VerificationMethod::QrCodeShowV1,
        VerificationMethod::ReciprocateV1,
    ];

    let has_cameras = Camera::has_cameras().await;

    if has_cameras {
        methods.push(VerificationMethod::QrCodeScanV1);
    }

    methods
}
