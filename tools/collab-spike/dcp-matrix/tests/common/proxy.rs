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
