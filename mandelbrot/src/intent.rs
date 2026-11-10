use std::borrow::Cow;

use gtk::{glib, prelude::*};

use crate::{session::VerificationKey, utils::matrix::MatrixIdUri};

/// Intents that can be handled by a session.
///
/// It cannot be cloned intentionally, so it is handled only once.
#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum SessionIntent {
    /// Show the target of a Matrix ID URI.
    ShowMatrixId(MatrixIdUri),
    /// Show an ongoing identity verification.
    ShowIdentityVerification(VerificationKey),
    /// Show the call view of a room.
    ShowRoomCall(MatrixIdUri),
}

impl SessionIntent {
    /// The application action name for the [`SessionIntent::ShowMatrixId`]
    /// variant.
    pub(crate) const SHOW_MATRIX_ID_APP_ACTION_NAME: &str = "app.show-matrix-id";

    /// The action name without the `app.` prefix for the
    /// [`SessionIntent::ShowMatrixId`] variant.
    pub(crate) const SHOW_MATRIX_ID_ACTION_NAME: &str =
        Self::SHOW_MATRIX_ID_APP_ACTION_NAME.split_at(4).1;

    /// The application action name for the
    /// [`SessionIntent::ShowIdentityVerification`] variant.
    pub(crate) const SHOW_IDENTITY_VERIFICATION_APP_ACTION_NAME: &str =
        "app.show-identity-verification";

    /// The action name without the `app.` prefix for the
    /// [`SessionIntent::ShowIdentityVerification`] variant.
    pub(crate) const SHOW_IDENTITY_VERIFICATION_ACTION_NAME: &str =
        Self::SHOW_IDENTITY_VERIFICATION_APP_ACTION_NAME
            .split_at(4)
            .1;

    /// The application action name for the [`SessionIntent::ShowRoomCall`]
    /// variant.
    pub(crate) const SHOW_ROOM_CALL_APP_ACTION_NAME: &str = "app.show-room-call";

    /// The action name without the `app.` prefix for the
    /// [`SessionIntent::ShowRoomCall`] variant.
    pub(crate) const SHOW_ROOM_CALL_ACTION_NAME: &str =
        Self::SHOW_ROOM_CALL_APP_ACTION_NAME.split_at(4).1;

    /// Get the application action name for this session intent type.
    pub(crate) fn app_action_name(&self) -> &'static str {
        match self {
            SessionIntent::ShowMatrixId(_) => Self::SHOW_MATRIX_ID_APP_ACTION_NAME,
            SessionIntent::ShowIdentityVerification(_) => {
                Self::SHOW_IDENTITY_VERIFICATION_APP_ACTION_NAME
            }
            SessionIntent::ShowRoomCall(_) => Self::SHOW_ROOM_CALL_APP_ACTION_NAME,
        }
    }

    /// Convert the given `GVariant` to a [`SessionIntent::ShowMatrixId`] and
    /// session ID, given the intent type.
    ///
    /// Returns a  `(session_id, intent)` tuple on success. Returns `None` if
    /// the `GVariant` could not be parsed successfully.
    pub(crate) fn show_matrix_id_from_variant(variant: &glib::Variant) -> Option<(String, Self)> {
        let SessionIntentActionParameter {
            session_id,
            payload,
        } = variant.get()?;

        Some((session_id, Self::ShowMatrixId(payload.get()?)))
    }

    /// Convert the given `GVariant` to a
    /// [`SessionIntent::ShowIdentityVerification`] and session ID, given the
    /// intent type.
    ///
    /// Returns a  `(session_id, intent)` tuple on success. Returns `None` if
    /// the `GVariant` could not be parsed successfully.
    pub(crate) fn show_identity_verification_from_variant(
        variant: &glib::Variant,
    ) -> Option<(String, Self)> {
        let SessionIntentActionParameter {
            session_id,
            payload,
        } = variant.get()?;

        Some((session_id, Self::ShowIdentityVerification(payload.get()?)))
    }

    /// Convert the given `GVariant` to a [`SessionIntent::ShowRoomCall`] and
    /// session ID, given the intent type.
    ///
    /// Returns a  `(session_id, intent)` tuple on success. Returns `None` if
    /// the `GVariant` could not be parsed successfully.
    pub(crate) fn show_room_call_from_variant(variant: &glib::Variant) -> Option<(String, Self)> {
        let SessionIntentActionParameter {
            session_id,
            payload,
        } = variant.get()?;

        Some((session_id, Self::ShowRoomCall(payload.get()?)))
    }

    /// Convert this intent to a `GVariant` with the given session ID.
    pub(crate) fn to_variant_with_session_id(&self, session_id: String) -> glib::Variant {
        let payload = match self {
            Self::ShowMatrixId(uri) | Self::ShowRoomCall(uri) => uri.to_variant(),
            Self::ShowIdentityVerification(key) => key.to_variant(),
        };

        SessionIntentActionParameter {
            session_id,
            payload,
        }
        .to_variant()
    }
}

impl StaticVariantType for SessionIntent {
    fn static_variant_type() -> Cow<'static, glib::VariantTy> {
        SessionIntentActionParameter::static_variant_type()
    }
}

impl From<MatrixIdUri> for SessionIntent {
    fn from(value: MatrixIdUri) -> Self {
        Self::ShowMatrixId(value)
    }
}

impl From<VerificationKey> for SessionIntent {
    fn from(value: VerificationKey) -> Self {
        Self::ShowIdentityVerification(value)
    }
}

/// The payload of a [`SessionIntent`], when converted to a `GVariant` for an
/// app action.
#[derive(Debug, Clone, glib::Variant)]
struct SessionIntentActionParameter {
    /// The ID of the session that should handle the intent.
    session_id: String,
    /// The payload of the intent.
    payload: glib::Variant,
}
