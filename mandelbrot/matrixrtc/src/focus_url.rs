// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks on the URLs of the media servers of a call.
//!
//! The URL of the JWT service comes from a call membership, which any room
//! member can write, and the URL of the SFU comes from that service. A web
//! client has a browser around it that refuses a plain HTTP request from a
//! secure page and asks a server on another origin before posting JSON to
//! it. A native client has neither, so these checks run before anything is
//! sent to either URL.

use url::Url;

/// Why the URL of a call server was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FocusUrlError {
    /// The URL could not be parsed.
    #[error("it is not a valid URL")]
    Invalid,

    /// The URL has a scheme that is not allowed.
    #[error("its scheme `{0}` is not allowed")]
    Scheme(String),

    /// The URL carries a user name or a password.
    #[error("it carries a user name or a password")]
    Credentials,

    /// The URL carries a query or a fragment.
    #[error("it carries a query or a fragment")]
    QueryOrFragment,
}

/// Check the `livekit_service_url` of a focus, before the OpenID token is
/// sent to it.
///
/// The URL must be `https`, with no user name, password, query or fragment:
/// the request then goes to the path of the URL followed by the endpoint,
/// and nowhere else. With a query, the endpoint appended to the URL would
/// land in the query, and the request path would be whatever the URL says.
///
/// `allow_plain` also accepts `http`. Pass it only when the homeserver of
/// the account is itself reached over plain HTTP, as in a loopback test
/// stack.
pub fn check_service_url(service_url: &str, allow_plain: bool) -> Result<(), FocusUrlError> {
    let url = Url::parse(service_url).map_err(|_| FocusUrlError::Invalid)?;
    check_scheme_and_credentials(&url, &["https"], &["http"], allow_plain)?;

    if url.query().is_some() || url.fragment().is_some() {
        return Err(FocusUrlError::QueryOrFragment);
    }

    Ok(())
}

/// Check the SFU URL returned by a JWT service, before connecting to it.
///
/// The URL must be `wss` or `https`, with no user name or password.
///
/// `allow_plain` also accepts `ws` and `http`, see [`check_service_url`].
pub fn check_sfu_url(sfu_url: &str, allow_plain: bool) -> Result<(), FocusUrlError> {
    let url = Url::parse(sfu_url).map_err(|_| FocusUrlError::Invalid)?;
    check_scheme_and_credentials(&url, &["wss", "https"], &["ws", "http"], allow_plain)
}

/// Check that the scheme of the URL is one of `secure`, or one of `plain` if
/// `allow_plain` is set, and that the URL carries no user name or password.
fn check_scheme_and_credentials(
    url: &Url,
    secure: &[&str],
    plain: &[&str],
    allow_plain: bool,
) -> Result<(), FocusUrlError> {
    let scheme = url.scheme();
    if !(secure.contains(&scheme) || (allow_plain && plain.contains(&scheme))) {
        return Err(FocusUrlError::Scheme(scheme.to_owned()));
    }

    if !url.username().is_empty() || url.password().is_some() {
        return Err(FocusUrlError::Credentials);
    }

    Ok(())
}
