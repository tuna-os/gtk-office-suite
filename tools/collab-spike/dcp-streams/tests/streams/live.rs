//! Readers that follow a stream live, by long poll (what the binding does) or by SSE (what the
//! protocol also offers and the binding does not use), for the transport tests.

use std::time::Instant;

use dcp_streams::Http;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live {
    LongPoll,
    Sse,
}

#[derive(Debug)]
pub enum EvKind {
    /// The reader is about to wait (long poll) or has received the SSE response head (SSE).
    Connected,
    /// `n` messages arrived.
    Data(usize),
    /// The response ended (SSE: the server or a middlebox closed it) and the reader reconnects.
    Reconnect(String),
    /// The reader gave up.
    Failed(String),
}

#[derive(Debug)]
pub struct Ev {
    pub idx: usize,
    pub at: Instant,
    pub kind: EvKind,
}

/// Follow `path` from `offset` for ever, reporting to `tx`, until aborted or a hard failure.
/// A long poll that fails is retried by `Http` itself; an SSE response that ends is reopened
/// from the last offset the server reported.
pub fn follow(
    http: Http,
    path: String,
    offset: String,
    live: Live,
    idx: usize,
    tx: UnboundedSender<Ev>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut off = offset;
        let ev = |kind| Ev { idx, at: Instant::now(), kind };
        let _ = tx.send(ev(EvKind::Connected));
        loop {
            match live {
                Live::LongPoll => match http.long_poll(&path, &off, true).await {
                    Ok(r) => {
                        let at = Instant::now();
                        off = r.next;
                        if !r.items.is_empty() {
                            let _ = tx.send(Ev { idx, at, kind: EvKind::Data(r.items.len()) });
                        }
                    }
                    // A transport error (the connection died under the poll) is not the end of the
                    // reader: it resumes from the offset it holds. Anything else is.
                    Err(e) if matches!(e, dcp_streams::http::HttpError::Transport(_)) => {
                        let _ = tx.send(ev(EvKind::Reconnect(e.to_string())));
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                    Err(e) => {
                        let _ = tx.send(ev(EvKind::Failed(e.to_string())));
                        return;
                    }
                },
                Live::Sse => {
                    let resp = http
                        .client()
                        .get(http.url(&path))
                        .query(&[("offset", off.as_str()), ("live", "sse")])
                        .header("accept", "text/event-stream")
                        .send()
                        .await;
                    let mut resp = match resp {
                        Ok(r) if r.status() == 200 => r,
                        Ok(r) => {
                            let s = r.status();
                            let body = r.text().await.unwrap_or_default();
                            let _ = tx.send(ev(EvKind::Failed(format!("SSE answered {s}: {body}"))));
                            return;
                        }
                        Err(e) => {
                            let _ = tx.send(ev(EvKind::Reconnect(format!("send: {e}"))));
                            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            continue;
                        }
                    };
                    let _ = tx.send(ev(EvKind::Connected));
                    let mut buf = String::new();
                    let why = loop {
                        match resp.chunk().await {
                            Ok(Some(c)) => {
                                let at = Instant::now();
                                buf.push_str(&String::from_utf8_lossy(&c));
                                while let Some(i) = buf.find("\n\n") {
                                    let block: String = buf.drain(..i + 2).collect();
                                    let (mut name, mut data) = (String::new(), String::new());
                                    for line in block.lines() {
                                        if let Some(v) = line.strip_prefix("event:") {
                                            name = v.trim().to_owned();
                                        } else if let Some(v) = line.strip_prefix("data:") {
                                            data.push_str(v);
                                        }
                                    }
                                    let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
                                    match name.as_str() {
                                        "data" => {
                                            let n = v.as_array().map_or(1, |a| a.len());
                                            let _ = tx.send(Ev { idx, at, kind: EvKind::Data(n) });
                                        }
                                        "control" => {
                                            if let Some(o) = v.get("streamNextOffset").and_then(|o| o.as_str()) {
                                                off = o.to_owned();
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Ok(None) => break "response ended".to_owned(),
                            Err(e) => break format!("{e}"),
                        }
                    };
                    let _ = tx.send(ev(EvKind::Reconnect(why)));
                }
            }
        }
    })
}
