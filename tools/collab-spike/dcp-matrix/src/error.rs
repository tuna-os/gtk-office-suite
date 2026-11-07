//! Mapping of matrix-sdk errors onto the binding-neutral `StoreError`, and the
//! classification the send queue retries on.

use std::time::Duration;

use dcp_core::StoreError;
use matrix_sdk::ruma::api::error::{ErrorKind, RetryAfter};

pub(crate) enum Class {
    /// HTTP 429 / `M_LIMIT_EXCEEDED`, with the server's suggested wait if any.
    RateLimited(Option<Duration>),
    Forbidden,
    NotFound,
    TooLarge,
    /// The request never got an answer (connection refused, reset, timeout).
    Network,
    Other,
}

pub(crate) fn classify(e: &matrix_sdk::Error) -> Class {
    if let Some(api) = e.as_client_api_error() {
        if api.status_code.as_u16() == 429 {
            let wait = match api.error_kind() {
                Some(ErrorKind::LimitExceeded(d)) => match d.retry_after {
                    Some(RetryAfter::Delay(d)) => Some(d),
                    _ => None,
                },
                _ => None,
            };
            return Class::RateLimited(wait);
        }
        return match api.status_code.as_u16() {
            403 => Class::Forbidden,
            404 => Class::NotFound,
            413 => Class::TooLarge,
            _ => Class::Other,
        };
    }
    if let matrix_sdk::Error::Http(h) = e {
        if matches!(h.as_ref(), matrix_sdk::HttpError::Reqwest(_)) {
            return Class::Network;
        }
    }
    Class::Other
}

pub(crate) fn to_store(e: matrix_sdk::Error, ctx: &str) -> StoreError {
    match classify(&e) {
        Class::RateLimited(w) => StoreError::RateLimited { retry_after_ms: w.map(|d| d.as_millis() as u64) },
        Class::Forbidden => StoreError::Forbidden(format!("{ctx}: {e}")),
        Class::NotFound => StoreError::NotFound(format!("{ctx}: {e}")),
        Class::TooLarge => StoreError::Invalid(format!("{ctx}: event too large (M_TOO_LARGE): {e}")),
        Class::Network | Class::Other => StoreError::Transport(format!("{ctx}: {e}")),
    }
}
