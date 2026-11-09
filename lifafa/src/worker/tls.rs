//! TLS for the mail servers: which certificates a connection accepts, and
//! telling a certificate issued for another name from other failures.

/// Whether a host is this machine. Local mail bridges — Proton Bridge, hydroxide,
/// DavMail — terminate TLS with a certificate generated on the machine itself at
/// install time: no CA has signed it and it is issued for an address rather than
/// a name, so it fails both checks. Verification is relaxed for loopback
/// addresses only, where anyone able to intercept the connection is already
/// running code as the user, and never for a host reached over a network.
pub(super) fn is_loopback_host(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// A TLS connector for a mail server: it tolerates a local bridge’s self-signed
/// certificate and one the user accepted in GNOME Settings (#254), waives the
/// name check alone where the account asks for it (#246: a valid certificate
/// in another name, as shared hosting serves), and verifies everything else
/// normally.
pub(super) fn tls_connector(
    host: &str,
    accept_hostname_mismatch: bool,
    accept_invalid_certs: bool,
) -> async_native_tls::TlsConnector {
    let tls = async_native_tls::TlsConnector::new();
    if is_loopback_host(host) || accept_invalid_certs {
        tls.danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
    } else if accept_hostname_mismatch {
        tls.danger_accept_invalid_hostnames(true)
    } else {
        tls
    }
}

/// Whether a connection error is the certificate's name not matching the
/// host (#246), which the account's "Accept a certificate for another
/// name" switch is for. The wording is OpenSSL's, via native-tls.
pub(crate) fn is_hostname_mismatch(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    e.contains("hostname mismatch")
        || e.contains("host name mismatch")
        || e.contains("certificate name does not match")
        || e.contains("does not match the")
        || e.contains("invalid for name")
        || e.contains("notvalidforname")
}

#[cfg(test)]
mod hostname_mismatch_tests {
    use super::is_hostname_mismatch;
    use crate::worker::tls::is_loopback_host;

    #[test]
    fn names_the_name_check_only() {
        assert!(is_hostname_mismatch(
            "error:0A000086:SSL routines:tls_post_process_server_certificate:certificate verify failed: Hostname mismatch"
        ));
        assert!(is_hostname_mismatch(
            "The certificate's CN name does not match the passed value"
        ));
        assert!(!is_hostname_mismatch(
            "certificate verify failed: self-signed certificate"
        ));
        assert!(!is_hostname_mismatch(
            "connecting to imap.example.org timed out after 30 seconds"
        ));
    }

    #[test]
    fn only_this_machine_counts_as_a_local_bridge() {
        for host in [
            "127.0.0.1",
            "localhost",
            "LocalHost",
            "::1",
            "[::1]",
            "127.1.2.3",
        ] {
            assert!(is_loopback_host(host), "{host} should be loopback");
        }
        // A remote host must keep full certificate verification, however it is
        // named — including anything merely containing "localhost".
        for host in [
            "imap.gmail.com",
            "127.0.0.1.example.com",
            "localhost.evil.com",
            "10.0.1.14",
        ] {
            assert!(!is_loopback_host(host), "{host} should not be loopback");
        }
    }
}
