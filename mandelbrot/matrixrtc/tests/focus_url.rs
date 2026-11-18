// SPDX-License-Identifier: GPL-3.0-or-later

//! The URLs of the media servers of a call are checked before anything is
//! sent to them: a call membership, which any room member can write, names
//! the JWT service, and the JWT service names the SFU.

use mandelbrot_matrixrtc::{FocusUrlError, check_service_url, check_sfu_url};

#[test]
fn service_url_over_https_is_accepted() {
    for url in [
        "https://livekit-jwt.call.matrix.org",
        "https://matrix.example.org/livekit/jwt/",
        "https://sfu.example.org:8443/jwt",
        "HTTPS://SFU.EXAMPLE.ORG",
    ] {
        assert!(check_service_url(url, false).is_ok(), "{url}");
        assert!(check_service_url(url, true).is_ok(), "{url}");
    }
}

#[test]
fn service_url_over_plain_http_is_refused() {
    for url in [
        "http://192.168.1.1/apply.cgi",
        "http://127.0.0.1:6080",
        "http://livekit-jwt.example.org",
    ] {
        assert_eq!(
            check_service_url(url, false),
            Err(FocusUrlError::Scheme("http".to_owned())),
            "{url}"
        );
    }
}

#[test]
fn service_url_over_plain_http_is_accepted_when_allowed() {
    assert!(check_service_url("http://127.0.0.1:6080", true).is_ok());
}

#[test]
fn service_url_with_another_scheme_is_refused() {
    for (url, scheme) in [
        ("file:///etc/passwd", "file"),
        ("ftp://example.org/", "ftp"),
        ("wss://sfu.example.org", "wss"),
        ("unix:/run/user/1000/bus", "unix"),
    ] {
        for allow_plain in [false, true] {
            assert_eq!(
                check_service_url(url, allow_plain),
                Err(FocusUrlError::Scheme(scheme.to_owned())),
                "{url}"
            );
        }
    }
}

#[test]
fn service_url_with_a_query_or_a_fragment_is_refused() {
    // The endpoint is appended to the URL: after a `?` or a `#` it would not
    // be part of the request path any more.
    for url in [
        "https://example.org/apply.cgi?reboot=1&x=",
        "https://example.org/jwt?",
        "https://example.org/jwt#",
        "https://example.org/apply.cgi#/sfu/get",
    ] {
        assert_eq!(
            check_service_url(url, false),
            Err(FocusUrlError::QueryOrFragment),
            "{url}"
        );
    }

    // Allowing plain HTTP does not allow a query.
    assert_eq!(
        check_service_url("http://192.168.1.1/apply.cgi?reboot=1&x=", true),
        Err(FocusUrlError::QueryOrFragment)
    );
}

#[test]
fn service_url_with_credentials_is_refused() {
    for url in [
        "https://user@example.org/jwt",
        "https://user:secret@example.org/jwt",
        "https://:secret@example.org/jwt",
    ] {
        assert_eq!(
            check_service_url(url, false),
            Err(FocusUrlError::Credentials),
            "{url}"
        );
    }
}

#[test]
fn service_url_that_is_not_a_url_is_refused() {
    for url in [
        "",
        "livekit-jwt.example.org",
        "//example.org/jwt",
        "https://",
    ] {
        assert_eq!(
            check_service_url(url, false),
            Err(FocusUrlError::Invalid),
            "{url}"
        );
    }
}

#[test]
fn sfu_url_over_tls_is_accepted() {
    for url in [
        "wss://sfu.example.org",
        "wss://sfu.example.org:7881/livekit",
        "https://sfu.example.org",
    ] {
        assert!(check_sfu_url(url, false).is_ok(), "{url}");
        assert!(check_sfu_url(url, true).is_ok(), "{url}");
    }
}

#[test]
fn sfu_url_without_tls_is_refused_unless_allowed() {
    for (url, scheme) in [
        ("ws://127.0.0.1:7880", "ws"),
        ("http://127.0.0.1:7880", "http"),
    ] {
        assert_eq!(
            check_sfu_url(url, false),
            Err(FocusUrlError::Scheme(scheme.to_owned())),
            "{url}"
        );
        assert!(check_sfu_url(url, true).is_ok(), "{url}");
    }
}

#[test]
fn sfu_url_that_is_not_a_websocket_or_http_url_is_refused() {
    for allow_plain in [false, true] {
        assert_eq!(
            check_sfu_url("file:///etc/passwd", allow_plain),
            Err(FocusUrlError::Scheme("file".to_owned()))
        );
        assert_eq!(
            check_sfu_url("wss://user:secret@sfu.example.org", allow_plain),
            Err(FocusUrlError::Credentials)
        );
        assert_eq!(
            check_sfu_url("sfu.example.org", allow_plain),
            Err(FocusUrlError::Invalid)
        );
    }
}
