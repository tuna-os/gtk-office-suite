//! TLS and HTTP/2 spoken directly (tokio-rustls, h2) for what reqwest does not show: which ALPN
//! protocol and TLS version were negotiated, how long each handshake step took, whether a
//! session was resumed, what the server's HTTP/2 SETTINGS say, and a request cut off in the
//! middle of its body.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

/// Where a base URL points.
#[derive(Clone, Debug)]
pub struct Target {
    pub tls: bool,
    pub host: String,
    pub port: u16,
    /// The path prefix, starting and ending with `/`.
    pub prefix: String,
}

impl Target {
    pub fn parse(base: &str) -> Target {
        let (scheme, rest) = base.split_once("://").expect("a URL with a scheme");
        let (hp, path) = rest.split_once('/').unwrap_or((rest, ""));
        let tls = scheme == "https";
        let (host, port) = match hp.rsplit_once(':') {
            Some((h, p)) => (h.to_owned(), p.parse().expect("port")),
            None => (hp.to_owned(), if tls { 443 } else { 80 }),
        };
        let mut prefix = format!("/{path}");
        if !prefix.ends_with('/') {
            prefix.push('/');
        }
        Target {
            tls,
            host,
            port,
            prefix,
        }
    }

    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

pub fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A client configuration that trusts exactly the certificates of `ca_pem` and offers `alpn`.
pub fn client_config(ca_pem: &[u8], alpn: &[&str]) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    for c in CertificateDer::pem_slice_iter(ca_pem) {
        roots.add(c.expect("a PEM certificate")).expect("a usable CA");
    }
    let mut cfg = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    cfg.alpn_protocols = alpn.iter().map(|p| p.as_bytes().to_vec()).collect();
    Arc::new(cfg)
}

/// A connected TLS stream and what it took.
pub struct Conn {
    pub stream: TlsStream<TcpStream>,
    pub tcp: Duration,
    pub tls: Duration,
}

impl Conn {
    pub fn alpn(&self) -> Option<String> {
        self.stream
            .get_ref()
            .1
            .alpn_protocol()
            .map(|p| String::from_utf8_lossy(p).into_owned())
    }

    pub fn version(&self) -> String {
        format!("{:?}", self.stream.get_ref().1.protocol_version())
    }

    pub fn cipher(&self) -> String {
        format!("{:?}", self.stream.get_ref().1.negotiated_cipher_suite().map(|c| c.suite()))
    }

    /// `Full` or `Resumed`.
    pub fn kind(&self) -> String {
        format!("{:?}", self.stream.get_ref().1.handshake_kind())
    }
}

/// TCP connect to `addr`, then the TLS handshake as `server_name` (SNI and the name the
/// certificate must cover).
pub async fn connect(
    addr: SocketAddr,
    server_name: &str,
    cfg: Arc<ClientConfig>,
) -> std::io::Result<Conn> {
    let t0 = Instant::now();
    let tcp = TcpStream::connect(addr).await?;
    tcp.set_nodelay(true)?;
    let tcp_time = t0.elapsed();
    let name = ServerName::try_from(server_name.to_owned())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
    let t1 = Instant::now();
    let stream = TlsConnector::from(cfg).connect(name, tcp).await?;
    Ok(Conn {
        stream,
        tcp: tcp_time,
        tls: t1.elapsed(),
    })
}

/// Connect to the host and port of `t` (the host name is resolved by the system).
pub async fn connect_target(t: &Target, cfg: Arc<ClientConfig>) -> std::io::Result<Conn> {
    let addr = tokio::net::lookup_host(t.authority())
        .await?
        .find(|a| a.is_ipv4())
        .ok_or_else(|| std::io::Error::other("no IPv4 address"))?;
    connect(addr, &t.host, cfg).await
}

pub fn wanted_alpn() -> &'static str {
    match crate::common::wanted_version() {
        Some(dcp_streams::http::HttpVersion::Http2) => "h2",
        _ => "http/1.1",
    }
}

/// Start `PUT <prefix><rel>` with a body that is declared `declared` bytes long, send `sent`
/// bytes of it, then drop the connection without finishing: an upload cut off on the wire, over
/// whatever this run speaks (plain HTTP/1.1, TLS HTTP/1.1, TLS HTTP/2). With `finish` the body
/// is complete (`sent == declared`; HTTP/2 also ends the stream): the control that shows the
/// request itself is a valid one.
pub async fn cut_upload(base: &str, rel: &str, declared: usize, sent: usize, finish: bool) {
    let t = Target::parse(base);
    let path = format!("{}{rel}", t.prefix);
    if !t.tls {
        let mut sock = TcpStream::connect(t.authority()).await.unwrap();
        sock.write_all(request_head(&t, &path, declared).as_bytes())
            .await
            .unwrap();
        sock.write_all(&vec![7u8; sent]).await.unwrap();
        sock.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        return; // dropped: closed
    }
    let alpn = wanted_alpn();
    let cfg = client_config(&crate::common::ca_pem(), &[alpn]);
    let mut conn = connect_target(&t, cfg).await.unwrap();
    assert_eq!(conn.alpn().as_deref(), Some(alpn), "the server did not negotiate {alpn}");
    if alpn == "http/1.1" {
        conn.stream
            .write_all(request_head(&t, &path, declared).as_bytes())
            .await
            .unwrap();
        conn.stream.write_all(&vec![7u8; sent]).await.unwrap();
        conn.stream.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        return;
    }
    let (send, h2conn) = h2::client::handshake(conn.stream).await.unwrap();
    let driver = tokio::spawn(async move {
        let _ = h2conn.await;
    });
    let mut send = send.ready().await.unwrap();
    let req = http::Request::builder()
        .method("PUT")
        .uri(format!("https://{}{}", t.authority(), path))
        .header("content-type", "application/octet-stream")
        .header("stream-closed", "true")
        .header("content-length", declared.to_string())
        .body(())
        .unwrap();
    let (_resp, mut body) = send.send_request(req, false).unwrap();
    body.send_data(bytes::Bytes::from(vec![7u8; sent]), finish).unwrap();
    // Let the frames reach the wire, then vanish.
    tokio::time::sleep(Duration::from_millis(300)).await;
    driver.abort();
}

fn request_head(t: &Target, path: &str, declared: usize) -> String {
    format!(
        "PUT {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/octet-stream\r\nStream-Closed: true\r\nContent-Length: {declared}\r\n\r\n",
        t.authority()
    )
}
