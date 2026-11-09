use std::time::{Duration, SystemTime};

use gettextrs::gettext;
use matrix_sdk::{ClientBuildError, Error, HttpError};
use ruma::api::error::{ErrorBody, ErrorKind, RetryAfter, StandardErrorBody};

use crate::ngettext_f;

pub trait UserFacingError {
    fn to_user_facing(&self) -> String;
}

impl UserFacingError for HttpError {
    fn to_user_facing(&self) -> String {
        if let HttpError::Reqwest(error) = self {
            // TODO: Add more information based on the error
            if error.is_timeout() {
                gettext("Connection timed out.")
            } else {
                gettext("Could not connect to the homeserver.")
            }
        } else if let Some(ErrorBody::Standard(StandardErrorBody { kind, message, .. })) =
            self.as_client_api_error().map(|error| &error.body)
        {
            match kind {
                ErrorKind::Forbidden => gettext("Invalid credentials."),
                ErrorKind::UserDeactivated => gettext("Account deactivated."),
                ErrorKind::LimitExceeded(limit_exceeded) => {
                    if let Some(retry_after) = &limit_exceeded.retry_after {
                        let duration = match retry_after {
                            RetryAfter::Delay(duration) => *duration,
                            RetryAfter::DateTime(until) => until
                                .duration_since(SystemTime::now())
                                // An error means that the date provided is in the past, which
                                // doesn't make sense. Let's not panic anyway and default to 1
                                // second.
                                .unwrap_or_else(|_| Duration::from_secs(1)),
                        };
                        let secs = duration.as_secs() as u32;
                        ngettext_f(
                            // Translators: Do NOT translate the content between '{' and '}',
                            // this is a variable name.
                            "Rate limit exceeded, retry in 1 second.",
                            "Rate limit exceeded, retry in {n} seconds.",
                            secs,
                            &[("n", &secs.to_string())],
                        )
                    } else {
                        gettext("Rate limit exceeded, try again later.")
                    }
                }
                _ => {
                    // TODO: The server may not give us pretty enough error message. We should
                    // add our own error message.
                    message.clone()
                }
            }
        } else {
            gettext("Unexpected connection error.")
        }
    }
}

impl UserFacingError for Error {
    fn to_user_facing(&self) -> String {
        match self {
            Error::DecryptorError(_) => gettext("Could not decrypt the event."),
            Error::Http(http_error) => http_error.to_user_facing(),
            _ => gettext("Unexpected error."),
        }
    }
}

impl UserFacingError for ClientBuildError {
    fn to_user_facing(&self) -> String {
        match self {
            ClientBuildError::Url(_) => gettext("Invalid URL."),
            ClientBuildError::AutoDiscovery(_) => gettext("Could not discover homeserver."),
            ClientBuildError::Http(err) => err.to_user_facing(),
            ClientBuildError::SqliteStore(_) => gettext("Could not open the store."),
            _ => gettext("Unexpected error."),
        }
    }
}
