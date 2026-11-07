//! What TLS and HTTP/2 must do and must refuse (RFC-0004, TLS spike). Run against a server
//! behind TLS (`scripts/streams-tls-up.sh`): `DCP_STREAMS_URL` is `https://`, `DCP_STREAMS_CA` the
//! CA of the run, `DCP_STREAMS_HTTP` the HTTP version under test (`1.1`, `2` or `auto`). A test
//! that needs one of these FAILS without it unless `DCP_SKIP_STREAMS_TLS=1`, which prints that it
//! skipped.
//!
//! Every refusal has a control next to it that succeeds on the same server, so a refusal cannot
//! be a dead server, a wrong port or a malformed request.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dcp_streams::crypto;
use dcp_streams::http::{HttpError, HttpVersion, Transport, JSON};
use dcp_streams::Http;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::common::*;
use crate::rawtls::*;
use crate::require_tls;

fn stream_path() -> String {
    format!("dcp/tls-{}/s", hex::encode(crypto::random::<6>()))
}

fn err_text(e: &HttpError) -> String {
    format!("{e}")
}

fn tls_transport(ca: Vec<u8>, version: Option<HttpVersion>) -> Transport {
    Transport {
        ca_pem: Some(ca),
        version,
        require_tls: true,
        ..Transport::default()
    }
}

/// The same server, by the host and port of `base`, under another URL authority.
fn with_authority(base: &str, host: &str) -> String {
    let t = Target::parse(base);
    format!("https://{host}:{}{}", t.port, t.prefix)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_negotiated_version_is_the_one_under_test() {
    let base = require_tls!();
    let want = wanted_alpn();
    let t = Target::parse(&base);

    // The handshake itself, with rustls directly: ALPN, TLS version, cipher.
    let c = connect_target(&t, client_config(&ca_pem(), &[want]))
        .await
        .expect("a TLS handshake against the test CA");
    println!(
        "TLS-NEGOTIATED kind={} alpn={:?} version={} cipher={} tcp_connect={:?} tls_handshake={:?}",
        std::env::var("DCP_STREAMS_KIND").unwrap_or_default(),
        c.alpn(),
        c.version(),
        c.cipher(),
        c.tcp,
        c.tls
    );
    assert_eq!(c.alpn().as_deref(), Some(want), "the server negotiated another protocol");
    assert_eq!(
        c.stream.get_ref().1.protocol_version(),
        Some(rustls::ProtocolVersion::TLSv1_3),
        "not TLS 1.3"
    );
    drop(c);

    // The binding's own client: every response was of the version under test.
    let http = http(&base);
    let path = stream_path();
    http.create(&path, JSON, None, false).await.unwrap();
    http.head(&path).await.unwrap();
    let s = http.snapshot();
    match want {
        "h2" => assert!(s.requests == 2 && s.responses_h2 == 2 && s.responses_h1 == 0, "{s:?}"),
        _ => assert!(s.requests == 2 && s.responses_h1 == 2 && s.responses_h2 == 0, "{s:?}"),
    }

    // Controls: the counters can see the other version too. This server offers both, so a client
    // that asks for the other one gets it (the check above is not "always passes").
    let other = if want == "h2" { HttpVersion::Http1 } else { HttpVersion::Http2 };
    let o = Http::with_transport(&base, tls_transport(ca_pem(), Some(other)));
    o.head(&path).await.unwrap();
    let s = o.snapshot();
    if want == "h2" {
        assert_eq!((s.responses_h1, s.responses_h2), (1, 0), "{s:?}");
    } else {
        assert_eq!((s.responses_h1, s.responses_h2), (0, 1), "{s:?}");
    }

    // A client that does not say which version it needs takes whatever ALPN gives it.
    let auto = Http::with_transport(&base, tls_transport(ca_pem(), None));
    auto.head(&path).await.unwrap();
    let s = auto.snapshot();
    println!("AUTO-NEGOTIATED h1={} h2={}", s.responses_h1, s.responses_h2);
    assert_eq!(s.responses_h2, 1, "a client offering h2 and http/1.1 to a server that has both gets h2");

    // What the server says about where the stream is: logged, never used.
    let resp = http
        .client()
        .put(http.url(&stream_path()))
        .header("content-type", JSON)
        .send()
        .await
        .unwrap();
    println!(
        "LOCATION-HEADER status={} version={:?} location={:?}",
        resp.status(),
        resp.version(),
        resp.headers().get("location").map(|v| v.to_str().unwrap_or("?").to_owned())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_that_does_not_trust_the_servers_ca_fails_closed() {
    let base = require_tls!();
    let Some(other_ca) = tls_env(module_path!(), "DCP_STREAMS_OTHER_CA") else {
        return;
    };
    let other_ca = std::fs::read(other_ca).unwrap();
    let version = wanted_version();
    let path = stream_path();

    // Control: the right CA, same URL, same version: works.
    let good = http(&base);
    good.create(&path, JSON, None, false).await.expect("the control must work");

    // The wrong CA: the chain does not end at a trusted root.
    let wrong = Http::with_transport(&base, tls_transport(other_ca.clone(), version));
    let e = wrong.head(&path).await.expect_err("a server signed by an unknown CA was trusted");
    println!("WRONG-CA error: {}", err_text(&e));
    assert!(matches!(e, HttpError::Transport(_)), "{e:?}");
    assert!(err_text(&e).contains("UnknownIssuer"), "not a certificate verdict: {e}");
    let s = wrong.snapshot();
    assert_eq!(s.responses_h1 + s.responses_h2, 0, "a response arrived over an unverified connection");
    assert!(s.transport_errors >= 1);

    // No CA at all: there are no built-in roots, so no https server is trusted.
    let none = Http::with_transport(&base, Transport { version, require_tls: true, ..Transport::default() });
    let e = none.head(&path).await.expect_err("trusted with no CA configured");
    println!("NO-CA error: {}", err_text(&e));
    assert!(err_text(&e).contains("UnknownIssuer"), "{e}");

    // The same verdict from rustls directly, without reqwest in the way.
    let t = Target::parse(&base);
    let raw = connect_target(&t, client_config(&other_ca, &[wanted_alpn()]))
        .await
        .err()
        .expect("rustls accepted a chain from an unknown CA");
    println!("WRONG-CA rustls: {raw}");
    assert!(raw.to_string().contains("UnknownIssuer"), "{raw}");
    // ...and the same direct handshake with the right CA works.
    connect_target(&t, client_config(&ca_pem(), &[wanted_alpn()])).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_certificate_for_another_host_name_is_refused() {
    let base = require_tls!();
    let version = wanted_version();
    let t = Target::parse(&base);
    let path = stream_path();
    let addr: SocketAddr = format!("127.0.0.1:{}", t.port).parse().unwrap();

    // Control: the name the certificate covers works, by DNS and with an explicit resolve.
    let good = http(&base);
    good.create(&path, JSON, None, false).await.expect("the control must work");
    let mut tr = tls_transport(ca_pem(), version);
    tr.resolve = vec![("localhost".into(), addr)];
    Http::with_transport(&with_authority(&base, "localhost"), tr)
        .head(&path)
        .await
        .expect("the same connection by `localhost`, resolved by hand, must work");

    // The certificate has SAN `DNS:localhost` only: an IP address is not covered...
    let by_ip = Http::with_transport(&with_authority(&base, "127.0.0.1"), tls_transport(ca_pem(), version));
    let e = by_ip.head(&path).await.expect_err("a certificate without an IP SAN was accepted for an IP");
    println!("IP-MISMATCH error: {}", err_text(&e));
    assert!(err_text(&e).contains("not valid for name"), "{e}");

    // ...nor is another host name that resolves to the very same server.
    let mut tr = tls_transport(ca_pem(), version);
    tr.resolve = vec![("streams.invalid".into(), addr)];
    let by_name = Http::with_transport(&with_authority(&base, "streams.invalid"), tr);
    let e = by_name.head(&path).await.expect_err("a certificate for localhost was accepted for streams.invalid");
    println!("NAME-MISMATCH error: {}", err_text(&e));
    assert!(err_text(&e).contains("not valid for name"), "{e}");
    let s = by_name.snapshot();
    assert_eq!(s.responses_h1 + s.responses_h2, 0);

    // rustls directly: SNI is the name that is checked.
    let raw = connect(addr, "streams.invalid", client_config(&ca_pem(), &[wanted_alpn()]))
        .await
        .err()
        .expect("rustls accepted a certificate for the wrong name");
    assert!(raw.to_string().contains("not valid for name"), "{raw}");
    connect(addr, "localhost", client_config(&ca_pem(), &[wanted_alpn()])).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_certificate_is_refused() {
    let _ = require_tls!();
    let Some(expired) = tls_env(module_path!(), "DCP_STREAMS_EXPIRED_URL") else {
        return;
    };
    let version = wanted_version();
    // Control: the trust anchor and host name are right (`expired.pem` is signed by the run's CA
    // for `localhost`); only the dates are wrong, and the verdict says so.
    let c = Http::with_transport(&expired, tls_transport(ca_pem(), version));
    let e = c.head("dcp/x").await.expect_err("an expired certificate was accepted");
    println!("EXPIRED error: {}", err_text(&e));
    assert!(err_text(&e).contains("certificate expired"), "{e}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_that_requires_h2_does_not_fall_back_to_http_1_1() {
    let _ = require_tls!();
    let Some(h1only) = tls_env(module_path!(), "DCP_STREAMS_H1ONLY_URL") else {
        return;
    };
    let path = stream_path();
    let t = Target::parse(&h1only);

    // The server offers `http/1.1` only. A client that requires HTTP/1.1 works...
    let h1 = Http::with_transport(&h1only, tls_transport(ca_pem(), Some(HttpVersion::Http1)));
    h1.create(&path, JSON, None, false).await.expect("control: HTTP/1.1 must work");
    assert_eq!(h1.snapshot().responses_h1, 1);

    // ...so does one that does not care, and it silently runs on HTTP/1.1: a client without
    // a requirement cannot tell it was downgraded.
    let auto = Http::with_transport(&h1only, tls_transport(ca_pem(), None));
    auto.head(&path).await.unwrap();
    let s = auto.snapshot();
    assert_eq!((s.responses_h1, s.responses_h2), (1, 0));

    // A client that requires HTTP/2 fails at the handshake: ALPN has no common protocol.
    let h2 = Http::with_transport(&h1only, tls_transport(ca_pem(), Some(HttpVersion::Http2)));
    let e = h2.head(&path).await.expect_err("h2 required, h1-only server answered");
    println!("REQUIRE-H2 on h1-only server: {}", err_text(&e));
    assert!(matches!(e, HttpError::Transport(_)), "{e:?}");
    let s = h2.snapshot();
    assert_eq!(s.responses_h1 + s.responses_h2, 0, "an HTTP/1.1 response was accepted: {s:?}");

    // The handshake alone says why.
    let raw = connect_target(&t, client_config(&ca_pem(), &["h2"]))
        .await
        .err()
        .expect("a handshake offering only h2 succeeded against an h1-only server");
    println!("REQUIRE-H2 rustls: {raw}");
    assert!(raw.to_string().contains("NoApplicationProtocol"), "{raw}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plain_http_to_the_tls_port_and_a_client_that_requires_tls() {
    let base = require_tls!();
    let t = Target::parse(&base);
    let path = stream_path();
    http(&base).create(&path, JSON, None, false).await.expect("control");

    // A plaintext request to the TLS port is not answered with the stream.
    let plain_base = format!("http://localhost:{}{}", t.port, t.prefix);
    let r = Http::new(&plain_base).head(&path).await;
    println!("PLAIN-TO-TLS-PORT: {r:?}");
    assert!(r.is_err(), "a plaintext request to the TLS port was served: {r:?}");

    // A client that requires TLS does not even try a plain URL.
    let strict = Http::with_transport(
        &plain_base,
        Transport { require_tls: true, ..Transport::default() },
    );
    let r = strict.head(&path).await;
    assert!(matches!(r, Err(HttpError::Protocol(_))), "{r:?}");
    assert_eq!(strict.snapshot().requests, 0, "a request was sent to an http:// URL");
}

/// A server that answers every request with a redirect from `https://` to a plain `http://` URL,
/// and a plain listener that counts who arrives. Neither is a Durable Streams server.
async fn redirect_rig() -> (String, Arc<AtomicU64>, u16) {
    let plain = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let plain_port = plain.local_addr().unwrap().port();
    let arrived = Arc::new(AtomicU64::new(0));
    let n = arrived.clone();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = plain.accept().await {
            n.fetch_add(1, Ordering::Relaxed);
            tokio::spawn(async move {
                let mut b = [0u8; 4096];
                let _ = s.read(&mut b).await;
                let _ = s
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
            });
        }
    });

    let dir = std::path::PathBuf::from(
        std::env::var("DCP_STREAMS_TLS_DIR").expect("DCP_STREAMS_TLS_DIR is not set"),
    );
    let certs: Vec<CertificateDer<'static>> =
        CertificateDer::pem_slice_iter(&std::fs::read(dir.join("server.pem")).unwrap())
            .collect::<Result<_, _>>()
            .unwrap();
    let key = PrivateKeyDer::from_pem_slice(&std::fs::read(dir.join("server.key")).unwrap()).unwrap();
    let mut cfg = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    let acceptor = TlsAcceptor::from(Arc::new(cfg));
    let tls = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tls_port = tls.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((s, _)) = tls.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut s) = acceptor.accept(s).await else { return };
                let mut b = [0u8; 4096];
                let _ = s.read(&mut b).await;
                let resp = format!(
                    "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:{plain_port}/v1/stream/x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = s.write_all(resp.as_bytes()).await;
                let _ = s.shutdown().await;
            });
        }
    });
    (format!("https://localhost:{tls_port}/v1/stream/"), arrived, plain_port)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_redirect_from_https_to_http_is_not_followed() {
    let _ = require_tls!();
    let (base, arrived, plain_port) = redirect_rig().await;

    // The binding's client: the 307 is an error and nothing reaches the plain listener.
    let http = Http::with_transport(&base, tls_transport(ca_pem(), Some(HttpVersion::Http1)));
    let r = http.head("x").await;
    println!("REDIRECT: {r:?}");
    assert!(matches!(r, Err(HttpError::Status(307, _))), "{r:?}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(arrived.load(Ordering::Relaxed), 0, "the client went to the plain URL");

    // Control: a stock reqwest client (default redirect policy, https not required) given the
    // same CA does follow it, so the rig does redirect and the counter does count.
    let stock = reqwest::Client::builder()
        .use_rustls_tls()
        .tls_built_in_root_certs(false)
        .add_root_certificate(reqwest::Certificate::from_pem(&ca_pem()).unwrap())
        .http1_only()
        .build()
        .unwrap();
    let resp = stock.get(format!("{base}x")).send().await.unwrap();
    assert_eq!(resp.status(), 200, "the stock client was not redirected to port {plain_port}");
    assert_eq!(arrived.load(Ordering::Relaxed), 1, "the stock client did not follow the redirect");
}
