//! A TCP forwarder that can be cut and restored, to simulate the network going
//! away between a client and Synapse without touching the container.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

pub struct Proxy {
    pub url: String,
    up: Arc<AtomicBool>,
    cut: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
}

impl Proxy {
    /// `target` is `host:port` of the real homeserver.
    pub async fn start(target: &str) -> Proxy {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind proxy");
        let addr: SocketAddr = listener.local_addr().unwrap();
        let up = Arc::new(AtomicBool::new(true));
        let cut = Arc::new(Notify::new());
        let target = target.to_owned();
        let (up2, cut2) = (up.clone(), cut.clone());
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut inbound, _)) = listener.accept().await else { return };
                if !up2.load(Ordering::SeqCst) {
                    drop(inbound);
                    continue;
                }
                let target = target.clone();
                let cut = cut2.clone();
                let up = up2.clone();
                tokio::spawn(async move {
                    let Ok(mut outbound) = TcpStream::connect(&target).await else { return };
                    let cut_fut = async {
                        loop {
                            cut.notified().await;
                            if !up.load(Ordering::SeqCst) {
                                return;
                            }
                        }
                    };
                    tokio::select! {
                        _ = copy_bidirectional(&mut inbound, &mut outbound) => {}
                        _ = cut_fut => {}
                    }
                });
            }
        });
        Proxy { url: format!("http://{addr}"), up, cut, task }
    }

    /// Drop every open connection and refuse new ones.
    pub fn cut(&self) {
        self.up.store(false, Ordering::SeqCst);
        self.cut.notify_waiters();
    }

    pub fn restore(&self) {
        self.up.store(true, Ordering::SeqCst);
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

// ---------------------------------------------------------------------------
// An HTTP-aware forwarder that can cut the next media upload short.
// ---------------------------------------------------------------------------

use std::sync::atomic::AtomicUsize;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Forwards each HTTP/1.1 request on its own upstream connection with
/// `Connection: close`, so it never has to parse a response. When armed, the
/// next media upload's body is cut to half length (and `Content-Length`
/// rewritten), so Synapse stores a valid-looking but truncated blob and answers
/// 200. That is the failure "a truncated upload is never published" is about.
pub struct FaultProxy {
    pub url: String,
    truncate_next: Arc<AtomicBool>,
    pub truncated: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl FaultProxy {
    pub async fn start(target: &str) -> FaultProxy {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind fault proxy");
        let addr = listener.local_addr().unwrap();
        let truncate_next = Arc::new(AtomicBool::new(false));
        let truncated = Arc::new(AtomicUsize::new(0));
        let (t2, c2, target) = (truncate_next.clone(), truncated.clone(), target.to_owned());
        let task = tokio::spawn(async move {
            loop {
                let Ok((client, _)) = listener.accept().await else { return };
                let (t, c, target) = (t2.clone(), c2.clone(), target.clone());
                tokio::spawn(async move {
                    let _ = serve_one(client, &target, &t, &c).await;
                });
            }
        });
        FaultProxy { url: format!("http://{addr}"), truncate_next, truncated, task }
    }

    pub fn arm_truncate(&self) {
        self.truncate_next.store(true, Ordering::SeqCst);
    }
}

impl Drop for FaultProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

async fn serve_one(mut client: TcpStream, target: &str, truncate_next: &AtomicBool, truncated: &AtomicUsize) -> std::io::Result<()> {
    // Request head.
    let mut buf = Vec::new();
    let head_end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        let mut chunk = [0u8; 8192];
        let n = client.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("").to_owned();
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut content_length = 0usize;
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            let (k, v) = (k.trim().to_owned(), v.trim().to_owned());
            match k.to_ascii_lowercase().as_str() {
                "content-length" => content_length = v.parse().unwrap_or(0),
                "connection" | "proxy-connection" | "keep-alive" => {}
                _ => headers.push((k, v)),
            }
        }
    }
    // Body.
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < content_length {
        let mut chunk = vec![0u8; 64 * 1024];
        let n = client.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);

    let is_upload = (request_line.starts_with("POST ") || request_line.starts_with("PUT ")) && request_line.contains("/media/") && request_line.contains("upload");
    if is_upload && truncate_next.swap(false, Ordering::SeqCst) {
        body.truncate(body.len() / 2);
        truncated.fetch_add(1, Ordering::SeqCst);
    }

    let mut out = Vec::with_capacity(body.len() + 512);
    out.extend_from_slice(request_line.as_bytes());
    out.extend_from_slice(b"\r\n");
    for (k, v) in &headers {
        out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
    }
    if content_length > 0 || request_line.starts_with("POST ") || request_line.starts_with("PUT ") {
        out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    out.extend_from_slice(&body);

    let mut upstream = TcpStream::connect(target).await?;
    upstream.write_all(&out).await?;
    tokio::io::copy(&mut upstream, &mut client).await?;
    client.shutdown().await?;
    Ok(())
}

impl dcp_conformance::Faults for FaultProxy {
    fn truncate_next_upload(&self) {
        self.arm_truncate();
    }
}
