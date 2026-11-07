//! A Durable Streams server this test process starts and stops itself, for
//! the tests that need to restart or kill it.
//!
//! Plain mode (`DCP_STREAMS_URL` is `http://`): the Node reference server on a port.
//! TLS mode (`https://`): the whole stack of `scripts/streams-tls-up.sh` for
//! `DCP_STREAMS_KIND` (`node`: Node behind a TLS-terminating Caddy; `go`: the Go server
//! with Caddy's own TLS), on the CA of this run, stopped and killed as one unit.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

pub struct ServerProc {
    child: Option<Child>,
    /// TLS mode: the pid file of the stack.
    pidfile: Option<PathBuf>,
    pub port: u16,
    pub started_in: Duration,
}

pub fn server_dir() -> PathBuf {
    std::env::var("DCP_STREAMS_SERVER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/collab-spike/scripts/streams-server")
        })
}

fn scripts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/collab-spike/scripts")
}

/// A free port, found by binding to 0 and letting go.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn tls_run() -> bool {
    std::env::var("DCP_STREAMS_URL").is_ok_and(|u| u.starts_with("https://"))
}

impl ServerProc {
    /// `data_dir` `None`: in memory. In TLS mode `data_dir` is the stack's work directory and
    /// is kept across a restart.
    pub async fn start(port: u16, data_dir: Option<PathBuf>) -> Self {
        if tls_run() {
            return Self::start_tls(port, data_dir).await;
        }
        let dir = server_dir();
        assert!(
            dir.join("node_modules/@durable-streams/server").exists(),
            "the reference server is not installed in {dir:?}: run `npm ci` there (or set DCP_SKIP_STREAMS=1)"
        );
        let mut cmd = Command::new("node");
        cmd.current_dir(&dir)
            .arg("server.mjs")
            .args(["--port", &port.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        if let Some(d) = &data_dir {
            cmd.args(["--data-dir", d.to_str().unwrap()]);
        }
        let t0 = Instant::now();
        let mut child = cmd.spawn().expect("cannot run node");
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let ready = tokio::time::timeout(Duration::from_secs(30), async {
            while let Some(l) = lines.next_line().await.unwrap() {
                if l.starts_with("READY ") {
                    return true;
                }
            }
            false
        })
        .await
        .unwrap_or(false);
        assert!(ready, "the server did not print READY within 30 s");
        // Keep draining so the pipe never fills.
        tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
        ServerProc {
            child: Some(child),
            pidfile: None,
            port,
            started_in: t0.elapsed(),
        }
    }

    async fn start_tls(port: u16, data_dir: Option<PathBuf>) -> Self {
        let kind = std::env::var("DCP_STREAMS_KIND").unwrap_or_else(|_| "node".into());
        let pki = std::env::var("DCP_STREAMS_TLS_DIR")
            .expect("DCP_STREAMS_TLS_DIR (the PKI directory of this run) is not set");
        let dir = data_dir.clone().unwrap_or_else(|| {
            std::env::temp_dir().join(format!("dcp-streams-mem-{}-{port}", std::process::id()))
        });
        let script = scripts_dir().join("streams-tls-up.sh");
        let mut cmd = Command::new("bash");
        cmd.arg(&script)
            .arg(&kind)
            .env("STREAMS_TLS_PORT", port.to_string())
            .env("STREAMS_PLAIN_PORT", free_port().to_string())
            .env("STREAMS_ORIGIN_PORT", free_port().to_string())
            .env("STREAMS_TLS_DIR", pki)
            .env("STREAMS_DIR", &dir)
            .env("STREAMS_NAME", format!("restart-{}-{port}", std::process::id()))
            .env("STREAMS_KEEP_DATA", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if data_dir.is_none() {
            cmd.env("STREAMS_IN_MEMORY", "1");
        }
        let t0 = Instant::now();
        let out = cmd.output().await.expect("cannot run streams-tls-up.sh");
        assert!(
            out.status.success(),
            "streams-tls-up.sh {kind} failed: {}",
            out.status
        );
        let text = String::from_utf8_lossy(&out.stdout);
        let pidfile = text
            .lines()
            .find_map(|l| l.strip_prefix("export DCP_STREAMS_PIDFILE="))
            .expect("streams-tls-up.sh printed no DCP_STREAMS_PIDFILE")
            .to_owned();
        ServerProc {
            child: None,
            pidfile: Some(PathBuf::from(pidfile)),
            port,
            started_in: t0.elapsed(),
        }
    }

    pub fn base(&self) -> String {
        if self.pidfile.is_some() {
            format!("https://localhost:{}/v1/stream/", self.port)
        } else {
            format!("http://127.0.0.1:{}/v1/stream/", self.port)
        }
    }

    async fn down(&mut self, kill: bool) {
        if let Some(p) = self.pidfile.take() {
            let mut cmd = Command::new("bash");
            cmd.arg(scripts_dir().join("streams-tls-down.sh"));
            if kill {
                cmd.arg("-9");
            }
            let _ = cmd.arg(p).status().await;
        }
    }

    /// SIGTERM, wait for the exit.
    pub async fn stop(&mut self) {
        if self.pidfile.is_some() {
            return self.down(false).await;
        }
        if let Some(mut c) = self.child.take() {
            let pid = c.id().expect("pid");
            let _ = Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status()
                .await;
            let _ = tokio::time::timeout(Duration::from_secs(10), c.wait()).await;
        }
    }

    /// SIGKILL: no flush, no goodbye.
    pub async fn kill9(&mut self) {
        if self.pidfile.is_some() {
            return self.down(true).await;
        }
        if let Some(mut c) = self.child.take() {
            let _ = c.start_kill();
            let _ = c.wait().await;
        }
    }
}

impl Drop for ServerProc {
    /// A test that panicked must not leave a TLS stack running.
    fn drop(&mut self) {
        if let Some(p) = self.pidfile.take() {
            let _ = std::process::Command::new("bash")
                .arg(scripts_dir().join("streams-tls-down.sh"))
                .arg("-9")
                .arg(p)
                .status();
        }
    }
}
