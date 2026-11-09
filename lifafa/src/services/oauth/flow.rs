//! The interactive sign-in: the authorization request with PKCE, the
//! browser, and the loopback listener that catches the redirect.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use crate::config::OAuthSettings;

use super::tokens::{exchange_code, FlowResult};
use super::DROPBOX_REDIRECT_PORT;
use crate::encoding::base64_encode;

/// The Hylki app icon, embedded so the success page needs no external resources.
const ICON_PNG: &[u8] =
    include_bytes!("../../../data/icons/hicolor/256x256/apps/co.hyprlab.Hylki.png");

/// Branded sign-in success page. `__ICON__` is replaced with the app icon (as a
/// data URI) at runtime by [`success_page`]. Self-contained (inline CSS/SVG,
/// system fonts) so it renders offline and adapts to light/dark.
const SUCCESS_TEMPLATE: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Hylki — Signed in</title>
<style>
  :root { color-scheme: light dark; --bg1:#0b1220; --bg2:#0e1526; --glow:rgba(53,132,228,.28);
          --card:rgba(255,255,255,.045); --stroke:rgba(255,255,255,.09); --shadow:rgba(0,0,0,.5);
          --fg:#eef2f8; --muted:#9fb0cc; --faint:#7688a6; }
  @media (prefers-color-scheme: light) {
    :root { --bg1:#f4f7fc; --bg2:#e9eef7; --glow:rgba(53,132,228,.16);
            --card:rgba(255,255,255,.82); --stroke:rgba(24,38,64,.08); --shadow:rgba(40,66,110,.16);
            --fg:#182338; --muted:#546482; --faint:#7a89a6; }
  }
  * { box-sizing:border-box; margin:0; padding:0; }
  html,body { height:100%; }
  body {
    display:grid; place-items:center; padding:24px;
    font-family:"Cantarell","Inter",-apple-system,system-ui,"Segoe UI",sans-serif;
    color:var(--fg);
    background:
      radial-gradient(1000px 560px at 50% -12%, var(--glow), transparent 70%),
      linear-gradient(155deg, var(--bg1), var(--bg2));
  }
  .card {
    width:min(440px, 92vw); text-align:center;
    padding:44px 38px 34px; border-radius:24px;
    background:var(--card); border:1px solid var(--stroke);
    box-shadow:0 30px 80px var(--shadow);
    -webkit-backdrop-filter:blur(22px); backdrop-filter:blur(22px);
    animation:rise .5s cubic-bezier(.2,.8,.2,1) both;
  }
  @keyframes rise { from { opacity:0; transform:translateY(14px) scale(.98); } }
  .hero { position:relative; width:92px; margin:0 auto 22px;
          animation:pop .5s .12s cubic-bezier(.2,1.4,.4,1) both; }
  .hero img { width:92px; height:92px; display:block; }
  @keyframes pop { from { transform:scale(.4); opacity:0; } }
  .check { position:absolute; right:-5px; bottom:-5px; width:33px; height:33px; border-radius:50%;
           display:grid; place-items:center;
           background:linear-gradient(135deg,#34c759,#2ba24b);
           box-shadow:0 6px 16px rgba(40,167,69,.45), 0 0 0 4px var(--card); }
  .check svg { width:17px; height:17px; }
  .check svg path { stroke-dasharray:30; stroke-dashoffset:30; animation:draw .45s .45s ease forwards; }
  @keyframes draw { to { stroke-dashoffset:0; } }
  .brand { font-size:19px; font-weight:800; letter-spacing:-.005em;
           color:#4f9bff; margin-bottom:6px; }
  h1 { font-size:25px; font-weight:800; letter-spacing:-.01em; margin-bottom:10px; }
  p  { color:var(--muted); font-size:15px; line-height:1.6; }
  .hint { margin-top:26px; font-size:12.5px; color:var(--faint); }
</style>
</head>
<body>
  <main class="card">
    <div class="hero">
      <img src="data:image/png;base64,__ICON__" alt="Hylki">
      <span class="check">
        <svg viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="3.2"
             stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.2 4.3L19 6.8"/></svg>
      </span>
    </div>
    <div class="brand">Hylki</div>
    <h1>You&rsquo;re signed in</h1>
    <p>Your account is connected. You can close this tab and head back to Hylki.</p>
    <div class="hint">It&rsquo;s safe to close this window.</div>
  </main>
</body>
</html>"##;

/// Build the success page with the app icon embedded as a data URI.
fn success_page() -> String {
    SUCCESS_TEMPLATE.replace("__ICON__", &base64_encode(ICON_PNG))
}

/// Open a URI in the user's default handler (a browser, for http/https).
///
/// Uses the XDG **OpenURI portal** over D-Bus so it works inside a Flatpak
/// sandbox as well as natively, and is safe to call from any thread (the OAuth
/// flow opens the browser from a worker thread). Falls back to `xdg-open` if the
/// portal isn't available.
pub(crate) fn open_uri(uri: &str) {
    if open_uri_portal(uri).is_err() {
        let _ = std::process::Command::new("xdg-open").arg(uri).spawn();
    }
}

fn open_uri_portal(uri: &str) -> Result<(), Box<dyn std::error::Error>> {
    let conn = zbus::blocking::Connection::session()?;
    // OpenURI(parent_window: s, uri: s, options: a{sv}) — fire and forget.
    let options: std::collections::HashMap<&str, zbus::zvariant::Value> =
        std::collections::HashMap::new();
    conn.call_method(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.portal.OpenURI"),
        "OpenURI",
        &("", uri, options),
    )?;
    Ok(())
}

/// Run the interactive authorization-code + PKCE flow (blocking — call off the
/// UI thread). Opens the browser and waits for the loopback redirect.
pub(crate) fn run_flow(settings: &OAuthSettings) -> Result<FlowResult, String> {
    run_flow_as(settings, None)
}

/// What a Microsoft sign-in adds to its request. With the address of an
/// account already set up (#410), Entra goes straight to that account and
/// asks only for what it has not approved; otherwise the account picker.
/// The picker, not a forced consent screen: in a tenant where users may not
/// consent, `prompt=consent` asks an admin every time even after the app was
/// approved.
pub(crate) fn microsoft_prompt(login_hint: Option<&str>) -> String {
    match login_hint.map(str::trim).filter(|h| !h.is_empty()) {
        Some(hint) => format!("&login_hint={}", crate::encoding::percent::encode(hint)),
        None => "&prompt=select_account".into(),
    }
}

/// [`run_flow`] for the account `login_hint` names, where the provider
/// takes a hint (Microsoft).
pub(crate) fn run_flow_as(
    settings: &OAuthSettings,
    login_hint: Option<&str>,
) -> Result<FlowResult, String> {
    // Dropbox matches the redirect URI exactly, port and all, so its
    // listener sits on a fixed port the app registers; the others accept
    // any loopback port.
    let dropbox = settings.token_url.contains("dropboxapi.com");
    let listener = if dropbox {
        bind_fixed_port(DROPBOX_REDIRECT_PORT)?
    } else {
        TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?
    };
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    // Microsoft (Entra) only ignores the port when matching *localhost* loopback
    // redirects — a random-port 127.0.0.1 URI would need the exact port registered,
    // which we can't do. Google (and others) accept the 127.0.0.1 literal. The
    // listener is on 127.0.0.1 either way; browsers resolve localhost to it.
    let microsoft = settings.token_url.contains("microsoftonline");
    let host = if microsoft || dropbox {
        "localhost"
    } else {
        "127.0.0.1"
    };
    // Microsoft compares the redirect with the registered `http://localhost`
    // to the letter, the port aside, so it gets no trailing slash.
    let redirect = if microsoft {
        format!("http://{host}:{port}")
    } else {
        format!("http://{host}:{port}/")
    };
    // What asks for a refresh token: Dropbox has its own parameter for it,
    // and Microsoft's `offline_access` scope is the request.
    let offline = if dropbox {
        "&token_access_type=offline".to_string()
    } else if microsoft {
        microsoft_prompt(login_hint)
    } else {
        "&access_type=offline&prompt=consent".to_string()
    };

    let request = Authorization::new(settings, &redirect, &offline)?;

    // Open the system browser (via the OpenURI portal, so it works in a Flatpak).
    open_uri(&request.url);

    // Wait for the redirect (with a timeout so a cancelled sign-in doesn't hang).
    let code = wait_for_code(&listener, &request.state).map_err(|e| {
        if microsoft {
            explain_microsoft_refusal(settings, e)
        } else {
            e
        }
    })?;
    exchange_code(settings, &request, &code)
}

/// One authorization request: the address to open, and what checks and
/// redeems its answer.
pub(crate) struct Authorization {
    pub(crate) url: String,
    pub(super) redirect: String,
    pub(super) state: String,
    pub(super) verifier: String,
}

impl Authorization {
    /// `extra` is appended to the query as it is ("&prompt=…").
    pub(crate) fn new(
        settings: &OAuthSettings,
        redirect: &str,
        extra: &str,
    ) -> Result<Self, String> {
        // PKCE S256 (RFC 7636 §4.2). With `plain` the challenge *is* the verifier, so
        // anyone who gets to read the authorization URL — browser history, an
        // extension, another local process — can redeem a stolen code, which is the
        // one thing PKCE exists to prevent. RFC 8252 requires S256 of any client that
        // can compute SHA-256.
        let entropy = |e| format!("no secure randomness available: {e}");
        let verifier = crate::rng::token(64).map_err(entropy)?;
        let state = crate::rng::token(24).map_err(entropy)?;
        let challenge = pkce_challenge(&verifier);
        let url = format!(
            "{base}?response_type=code&client_id={cid}&redirect_uri={redir}&scope={scope}\
             &code_challenge={chal}&code_challenge_method=S256&state={st}{extra}",
            base = settings.auth_url,
            cid = crate::encoding::percent::encode(&settings.client_id),
            redir = crate::encoding::percent::encode(redirect),
            scope = crate::encoding::percent::encode(&settings.scopes),
            chal = crate::encoding::percent::encode(&challenge),
            st = crate::encoding::percent::encode(&state),
        );
        Ok(Self {
            url,
            redirect: redirect.to_string(),
            state,
            verifier,
        })
    }

    /// For a sign-in in Hylki's own window (#329): the code a navigation
    /// to `uri` carries, `None` while it is not the redirect, or the
    /// provider's refusal.
    pub(crate) fn answer(
        &self,
        uri: &str,
        settings: &OAuthSettings,
    ) -> Option<Result<String, String>> {
        let rest = uri.strip_prefix(self.redirect.as_str())?;
        if !(rest.is_empty() || rest.starts_with('?') || rest.starts_with('#')) {
            return None;
        }
        let line = format!("GET /{rest} HTTP/1.1");
        let (code, state) = parse_redirect(&line);
        Some(match code {
            Some(code) if state.as_deref() == Some(self.state.as_str()) => Ok(code),
            Some(_) => Err("sign-in state mismatch (possible CSRF)".into()),
            None => Err(match redirect_error(&line) {
                Some((error, description)) => {
                    let first = description.lines().next().unwrap_or("").trim().to_string();
                    explain_microsoft_refusal(
                        settings,
                        if first.is_empty() { error } else { first },
                    )
                }
                None => "no authorization code returned".into(),
            }),
        })
    }
}

/// Listen on a fixed loopback port. An earlier sign-in of ours still
/// waiting there (the wait lasts five minutes, and its dialog may be long
/// gone) is told to stop with a `cancel` request, and the bind is tried
/// again.
fn bind_fixed_port(port: u16) -> Result<TcpListener, String> {
    let mut last = None;
    for _ in 0..8 {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => return Ok(l),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
                if let Ok(mut s) =
                    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2))
                {
                    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                    let _ = s.write_all(
                        b"GET /?cancel=1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                    );
                    let mut sink = [0u8; 512];
                    let _ = s.read(&mut sink);
                }
                last = Some(e);
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => {
                return Err(format!(
                    "could not listen on localhost port {port} for the sign-in redirect: {e}"
                ))
            }
        }
    }
    Err(format!(
        "could not listen on localhost port {port} for the sign-in redirect: {}. Another program is using it.",
        last.map(|e| e.to_string()).unwrap_or_default()
    ))
}

/// Accept the browser redirect and return the authorization code, validating the
/// anti-CSRF state. Times out after 5 minutes.
fn wait_for_code(listener: &TcpListener, expected_state: &str) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false).ok();
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
                let req = String::from_utf8_lossy(&buf[..n]);
                let line = req.lines().next().unwrap_or("");
                // A newer sign-in of ours taking the port over.
                if line.starts_with("GET /?cancel=1 ") {
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    return Err("this sign-in was replaced by a newer one".into());
                }
                let (code, state) = parse_redirect(line);
                // The provider came back with an error instead of a code: the
                // user declined, or the account's organisation does not let
                // them approve the app (#329). Say so, in the browser too.
                if code.is_none() {
                    if let Some((error, description)) = redirect_error(line) {
                        let body = "<!doctype html><meta charset=utf-8><title>Hylki</title>\
                                    <p style=\"font:16px system-ui;margin:3em\">The sign-in did not finish. \
                                    You can close this tab and return to Hylki.</p>";
                        let _ = stream.write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                                 Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            )
                            .as_bytes(),
                        );
                        let first = description.lines().next().unwrap_or("").trim().to_string();
                        return Err(if first.is_empty() { error } else { first });
                    }
                }

                let body = success_page();
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                );

                if state.as_deref() != Some(expected_state) {
                    return Err("sign-in state mismatch (possible CSRF)".into());
                }
                return code.ok_or_else(|| "no authorization code returned".to_string());
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Err("sign-in timed out".into());
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// `error` and `error_description` from a redirect that carries no code.
fn redirect_error(request_line: &str) -> Option<(String, String)> {
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut error = None;
    let mut description = String::new();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            match k {
                "error" => error = Some(crate::encoding::percent::decode(v, true)),
                "error_description" => description = crate::encoding::percent::decode(v, true),
                _ => {}
            }
        }
    }
    error.map(|e| (e, description))
}

/// A Microsoft sign-in that failed for want of consent, with where an
/// organisation's administrator approves Hylki for all its users. Users of
/// work and school accounts elsewhere cannot approve an app from a publisher
/// Microsoft has not verified themselves (#329).
fn explain_microsoft_refusal(settings: &OAuthSettings, reason: String) -> String {
    let consent = [
        "AADSTS65001",
        "AADSTS65004",
        "AADSTS90094",
        "AADSTS90095",
        "consent_required",
        "access_denied",
    ]
    .iter()
    .any(|c| reason.contains(c));
    if !consent {
        return reason;
    }
    let link = format!(
        "https://login.microsoftonline.com/organizations/adminconsent?client_id={}",
        settings.client_id
    );
    crate::i18n::i18n_f(
        "{reason} If your organization manages your account, an administrator can approve Hylki for everyone at {link}",
        &[("reason", &reason), ("link", &link)],
    )
}

/// Pull `code` and `state` out of an HTTP request line ("GET /?code=…&state=… HTTP/1.1").
fn parse_redirect(request_line: &str) -> (Option<String>, Option<String>) {
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut code = None;
    let mut state = None;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            let value = crate::encoding::percent::decode(v, true);
            match k {
                "code" => code = Some(value),
                "state" => state = Some(value),
                _ => {}
            }
        }
    }
    (code, state)
}

/// The PKCE S256 code challenge for a verifier: base64url(SHA-256(verifier)),
/// without padding (RFC 7636 §4.2).
fn pkce_challenge(verifier: &str) -> String {
    use sha2::Digest;
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        sha2::Sha256::digest(verifier.as_bytes()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_redirect_line_yields_code_and_state() {
        assert_eq!(
            parse_redirect("GET /?code=a%2Fb&state=s+1&x=y HTTP/1.1"),
            (Some("a/b".to_string()), Some("s 1".to_string()))
        );
        assert_eq!(
            parse_redirect("GET /cb?code=1&code=2 HTTP/1.1")
                .0
                .as_deref(),
            Some("2"),
            "the last one wins"
        );
        assert_eq!(parse_redirect("GET / HTTP/1.1"), (None, None));
        assert_eq!(parse_redirect(""), (None, None));
        assert_eq!(parse_redirect("GET /?code HTTP/1.1"), (None, None));
    }

    #[test]
    fn a_navigation_is_answered_only_at_the_redirect() {
        let request = Authorization {
            url: String::new(),
            redirect: "http://localhost/cb".into(),
            state: "st".into(),
            verifier: "v".into(),
        };
        let settings = OAuthSettings {
            client_id: "abc".into(),
            ..Default::default()
        };
        let answer = |uri: &str| request.answer(uri, &settings);
        assert_eq!(answer("https://elsewhere/"), None);
        assert_eq!(answer("http://localhost/cbx"), None);
        assert_eq!(
            answer("http://localhost/cb?code=c1&state=st"),
            Some(Ok("c1".into()))
        );
        assert_eq!(
            answer("http://localhost/cb?code=c1&state=zz"),
            Some(Err("sign-in state mismatch (possible CSRF)".into()))
        );
        let none = Some(Err("no authorization code returned".to_string()));
        assert_eq!(answer("http://localhost/cb"), none);
        // A code in the fragment is not read.
        assert_eq!(answer("http://localhost/cb#code=c1&state=st"), none);
        assert_eq!(
            answer("http://localhost/cb?error=access_denied&error_description=Line+one%0Aline+two"),
            Some(Err("Line one".into()))
        );
        let consent = answer("http://localhost/cb?error=consent_required")
            .unwrap()
            .unwrap_err();
        assert!(
            consent.starts_with("consent_required If your organization")
                && consent.contains("adminconsent?client_id=abc"),
            "{consent}"
        );
    }

    #[test]
    fn a_refused_redirect_says_why() {
        let line = "GET /?error=access_denied&error_description=AADSTS65004%3a+User+declined+to+consent.%0d%0aTrace+ID%3a+x&state=s HTTP/1.1";
        let (error, description) = redirect_error(line).expect("an error");
        assert_eq!(error, "access_denied");
        assert!(description.starts_with("AADSTS65004: User declined to consent."));
        assert_eq!(parse_redirect(line).0, None);
        let settings = OAuthSettings {
            client_id: "abc".into(),
            ..Default::default()
        };
        let told =
            explain_microsoft_refusal(&settings, "AADSTS65004: User declined to consent.".into());
        assert!(told.contains("adminconsent?client_id=abc"), "{told}");
        assert_eq!(
            explain_microsoft_refusal(&settings, "sign-in timed out".into()),
            "sign-in timed out"
        );
    }

    #[test]
    fn a_stale_waiter_gives_up_the_fixed_port() {
        // Something of ours on the port, waiting like wait_for_code does:
        // one request ends it.
        let old = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = old.local_addr().unwrap().port();
        let waiter = std::thread::spawn(move || {
            let (mut s, _) = old.accept().unwrap();
            let mut buf = [0u8; 256];
            let n = s.read(&mut buf).unwrap();
            let line = String::from_utf8_lossy(&buf[..n])
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            drop(old);
            line
        });
        let fresh = bind_fixed_port(port).expect("takes the port over");
        assert_eq!(fresh.local_addr().unwrap().port(), port);
        assert!(waiter.join().unwrap().starts_with("GET /?cancel=1 "));
    }

    #[test]
    fn the_pkce_challenge_matches_the_rfc_7636_vector() {
        // RFC 7636 appendix B. With `plain` the challenge *was* the verifier, so
        // reading the authorization URL was enough to redeem a stolen code.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = pkce_challenge(verifier);
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_ne!(challenge, verifier);
        // base64url without padding: no `+`, `/` or `=` to percent-encode.
        assert!(!challenge.contains(['+', '/', '=']), "{challenge}");
    }
}
