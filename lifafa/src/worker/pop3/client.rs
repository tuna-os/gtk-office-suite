//! A minimal POP3 client: TLS (implicit on 995, STLS otherwise), USER/PASS,
//! UIDL, RETR, DELE, QUIT, with each command logged for the console.

use tokio::net::TcpStream;

use crate::config::AccountConfig;
use crate::worker::tls::tls_connector;

/// The POP3 conversation for the console, like the IMAP one: the command
/// (a PASS line with its secret dropped) and the server's first reply line.
fn pop3_wire(cmd: &str) {
    let shown = if cmd.starts_with("PASS ") {
        "PASS ****"
    } else {
        cmd
    };
    tracing::debug!(target: "hylki::pop3", "> {shown}");
}

fn pop3_wired(cmd: &str, reply: &[u8]) {
    let line = String::from_utf8_lossy(reply);
    let line = line.trim();
    let shown = if cmd.starts_with("PASS ") {
        "PASS"
    } else {
        cmd
    };
    if reply.starts_with(b"+OK") {
        tracing::debug!(target: "hylki::pop3", "< {} ({shown})", line.chars().take(80).collect::<String>());
    } else {
        tracing::warn!(target: "hylki::pop3", "< {line} ({shown})");
    }
}

/// A minimal async POP3 client over TLS (implicit on 995, STLS otherwise).
pub(in crate::worker) struct Pop3 {
    stream: tokio::io::BufReader<async_native_tls::TlsStream<TcpStream>>,
}

impl Pop3 {
    pub(in crate::worker) async fn connect(account: &AccountConfig) -> Result<Self, String> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let host = account.imap_host.as_str();
        let port = account.imap_port;
        let tcp = TcpStream::connect((host, port))
            .await
            .map_err(|e| e.to_string())?;
        let tls = tls_connector(host, account.tls_accept_hostname_mismatch, false);

        let stream = if port == 995 {
            tls.connect(host, tcp).await.map_err(|e| e.to_string())?
        } else {
            // STARTTLS: greet + STLS on the plaintext socket, then upgrade.
            let mut plain = BufReader::new(tcp);
            let mut line = Vec::new();
            plain
                .read_until(b'\n', &mut line)
                .await
                .map_err(|e| e.to_string())?;
            plain
                .write_all(b"STLS\r\n")
                .await
                .map_err(|e| e.to_string())?;
            plain.flush().await.map_err(|e| e.to_string())?;
            line.clear();
            plain
                .read_until(b'\n', &mut line)
                .await
                .map_err(|e| e.to_string())?;
            if !line.starts_with(b"+OK") {
                return Err("server refused STLS".to_string());
            }
            tls.connect(host, plain.into_inner())
                .await
                .map_err(|e| e.to_string())?
        };

        let mut pop = Pop3 {
            stream: BufReader::new(stream),
        };
        if port == 995 {
            pop.read_reply().await?; // greeting
        }
        Ok(pop)
    }

    async fn read_reply(&mut self) -> Result<Vec<u8>, String> {
        use tokio::io::AsyncBufReadExt;
        let mut line = Vec::new();
        self.stream
            .read_until(b'\n', &mut line)
            .await
            .map_err(|e| e.to_string())?;
        if line.is_empty() {
            return Err("connection closed".to_string());
        }
        Ok(line)
    }

    async fn send(&mut self, cmd: &str) -> Result<(), String> {
        use tokio::io::AsyncWriteExt;
        self.stream
            .write_all(cmd.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        self.stream
            .write_all(b"\r\n")
            .await
            .map_err(|e| e.to_string())?;
        self.stream.flush().await.map_err(|e| e.to_string())
    }

    /// Single-line command: returns Err with the server text on `-ERR`.
    pub(in crate::worker) async fn command(&mut self, cmd: &str) -> Result<(), String> {
        pop3_wire(cmd);
        self.send(cmd).await?;
        let reply = self.read_reply().await?;
        pop3_wired(cmd, &reply);
        if reply.starts_with(b"+OK") {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&reply).trim().to_string())
        }
    }

    /// Multi-line command: returns the dot-unstuffed body bytes after `+OK`.
    async fn multiline(&mut self, cmd: &str) -> Result<Vec<u8>, String> {
        pop3_wire(cmd);
        self.send(cmd).await?;
        let first = self.read_reply().await?;
        pop3_wired(cmd, &first);
        if !first.starts_with(b"+OK") {
            return Err(String::from_utf8_lossy(&first).trim().to_string());
        }
        let mut out = Vec::new();
        loop {
            let line = self.read_reply().await?;
            let Some(content) = unstuffed(&line) else {
                break;
            };
            out.extend_from_slice(content);
            out.extend_from_slice(b"\r\n");
        }
        Ok(out)
    }

    pub(in crate::worker) async fn login(&mut self, user: &str, pass: &str) -> Result<(), String> {
        self.command(&format!("USER {user}")).await?;
        self.command(&format!("PASS {pass}")).await
    }

    /// Returns (message number, server UID) pairs.
    pub(in crate::worker) async fn uidl(&mut self) -> Result<Vec<(u32, String)>, String> {
        let body = self.multiline("UIDL").await?;
        Ok(parse_uidl(&body))
    }

    pub(in crate::worker) async fn retr(&mut self, num: u32) -> Result<Vec<u8>, String> {
        self.multiline(&format!("RETR {num}")).await
    }

    pub(in crate::worker) async fn quit(&mut self) {
        let _ = self.command("QUIT").await;
    }
}

fn strip_crlf(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    while end > 0 && (line[end - 1] == b'\n' || line[end - 1] == b'\r') {
        end -= 1;
    }
    &line[..end]
}

/// The (message number, server UID) pairs of a UIDL listing; a line that
/// does not start with a number and a UID is skipped.
fn parse_uidl(body: &[u8]) -> Vec<(u32, String)> {
    let text = String::from_utf8_lossy(body);
    let mut out = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(n), Some(uid)) = (parts.next(), parts.next()) {
            if let Ok(num) = n.parse::<u32>() {
                out.push((num, uid.to_string()));
            }
        }
    }
    out
}

/// One line of a multi-line reply without its line end and with the
/// dot-stuffing undone (a leading `.` is doubled on the wire); `None` for
/// the lone `.` that ends the reply.
fn unstuffed(line: &[u8]) -> Option<&[u8]> {
    let trimmed = strip_crlf(line);
    if trimmed == b"." {
        return None;
    }
    Some(if trimmed.starts_with(b"..") {
        &trimmed[1..]
    } else {
        trimmed
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uidl_lines_pair_a_number_with_a_uid() {
        assert_eq!(
            parse_uidl(b"1 abc\r\n2 def\r\nx y\r\n3\r\n 4 ghi extra\r\n"),
            [
                (1, "abc".to_string()),
                (2, "def".to_string()),
                (4, "ghi".to_string())
            ]
        );
        assert!(parse_uidl(b"").is_empty());
    }

    #[test]
    fn a_multiline_reply_loses_its_dot_stuffing_and_ends_at_a_lone_dot() {
        assert_eq!(unstuffed(b".\r\n"), None);
        assert_eq!(unstuffed(b".\n"), None);
        assert_eq!(unstuffed(b"..x\r\n"), Some(&b".x"[..]));
        assert_eq!(unstuffed(b"...\r\n"), Some(&b".."[..]));
        assert_eq!(unstuffed(b".x\r\n"), Some(&b".x"[..]));
        assert_eq!(unstuffed(b"\r\n"), Some(&b""[..]));
        assert_eq!(unstuffed(b"text"), Some(&b"text"[..]));
    }

    #[test]
    fn strip_crlf_drops_every_trailing_line_end() {
        assert_eq!(strip_crlf(b"+OK\r\n"), b"+OK");
        assert_eq!(strip_crlf(b"a\n\r\n"), b"a");
        assert_eq!(strip_crlf(b"\r\n"), b"");
        assert_eq!(strip_crlf(b"a\rb"), b"a\rb");
    }
}
