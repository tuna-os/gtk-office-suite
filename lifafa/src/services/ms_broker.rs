//! Microsoft's identity broker on the session bus (#329), for tenants whose
//! Conditional Access only lets a managed device sign in. The broker
//! (`microsoft-identity-broker` from Intune, or Himmelblau) holds the
//! device's Primary Refresh Token and hands out a short-lived sign-in cookie
//! made from it. Sent to login.microsoftonline.com with the sign-in page and
//! with each token refresh, it shows Entra the device is the managed one.
//! This is what Evolution does ([MS-OAPXBC]; evolution-ews
//! `e-ms-oapxbc-util.c`), down to the requests' shape.
//!
//! Without a broker, or with no account registered in it, nothing is added
//! and the sign-in goes as it always did.

use serde_json::{json, Value};

const BUS_NAME: &str = "com.microsoft.identity.broker1";
const PATH: &str = "/com/microsoft/identity/broker1";
const INTERFACE: &str = "com.microsoft.identity.Broker1";
/// The only host the broker's cookie is good for.
const ENTRA: &str = "https://login.microsoftonline.com";
/// [MS-OAPXBC] authorization type for a PRT SSO cookie.
const PRT_SSO_COOKIE: u32 = 8;

/// A sign-in cookie from the broker: its name (`x-ms-RefreshTokenCredential`
/// in practice) and value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SsoCookie {
    pub(crate) name: String,
    pub(crate) value: String,
}

/// The broker's sign-in cookie for `user`, when `settings` signs in to
/// Entra ID and a broker on this computer has that account. `user` is the
/// address being signed in; without one, a broker holding a single
/// account uses it. Blocking: call it off the main thread.
pub(crate) fn sso_cookie(
    settings: &crate::config::OAuthSettings,
    user: Option<&str>,
) -> Option<SsoCookie> {
    if !settings.auth_url.starts_with(ENTRA) || settings.client_id.trim().is_empty() {
        return None;
    }
    match fetch(settings, user) {
        Ok(cookie) => {
            if cookie.is_some() {
                tracing::info!(target: "hylki::oauth", "identity broker: device sign-in cookie added");
            }
            cookie
        }
        Err(e) => {
            tracing::debug!(target: "hylki::oauth", "identity broker: {e}");
            None
        }
    }
}

fn fetch(
    settings: &crate::config::OAuthSettings,
    user: Option<&str>,
) -> Result<Option<SsoCookie>, String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    // One id for the pair of calls, as a broker log ties them together.
    let session = correlation_id();
    let redirect = redirect_uri(settings);
    let call = |method: &str, request: &str| -> Result<String, String> {
        conn.call_method(
            Some(BUS_NAME),
            PATH,
            Some(INTERFACE),
            method,
            &("0.0", session.as_str(), request),
        )
        .map_err(|e| format!("{method}: {e}"))?
        .body()
        .deserialize::<String>()
        .map_err(|e| format!("{method}: {e}"))
    };
    let accounts = call(
        "getAccounts",
        &accounts_request(&settings.client_id, &redirect),
    )?;
    let Some(account) = pick_account(&accounts, user) else {
        return Err(format!(
            "no broker account for {}",
            user.unwrap_or("this sign-in")
        ));
    };
    let reply = call(
        "acquirePrtSsoCookie",
        &cookie_request(&account, &settings.client_id, &redirect, &settings.auth_url),
    )?;
    Ok(parse_cookie(&reply))
}

/// The redirect the client is registered with: the account's own, else
/// the native-client page every public Entra client may use.
fn redirect_uri(settings: &crate::config::OAuthSettings) -> String {
    let own = settings.redirect_uri.trim();
    if own.is_empty() {
        format!("{ENTRA}/common/oauth2/nativeclient")
    } else {
        own.to_string()
    }
}

/// A random UUID, lower case, as the broker expects a correlation id.
fn correlation_id() -> String {
    let mut b = [0u8; 16];
    let _ = crate::rng::fill(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

fn accounts_request(client_id: &str, redirect: &str) -> String {
    json!({ "clientId": client_id, "redirectUri": redirect }).to_string()
}

/// The broker account to sign in as: the one named `user`, or the only one
/// when no name is given.
fn pick_account(reply: &str, user: Option<&str>) -> Option<Value> {
    let reply: Value = serde_json::from_str(reply).ok()?;
    let accounts = reply["accounts"].as_array()?;
    match user.map(str::trim).filter(|u| !u.is_empty()) {
        Some(user) => accounts
            .iter()
            .find(|a| {
                a["username"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(user))
            })
            .cloned(),
        None if accounts.len() == 1 => accounts.first().cloned(),
        None => None,
    }
}

fn cookie_request(account: &Value, client_id: &str, redirect: &str, sso_url: &str) -> String {
    json!({
        "account": account,
        "authParameters": {
            "account": account,
            "authority": format!("{ENTRA}/common"),
            "authorizationType": PRT_SSO_COOKIE,
            "clientId": client_id,
            "redirectUri": redirect,
            "requestedScopes": ["https://graph.microsoft.com/.default"],
            "username": account["username"],
            "ssoUrl": sso_url,
        },
        "ssoUrl": sso_url,
    })
    .to_string()
}

/// The cookie in the broker's answer: on its own, or the first of
/// `cookieItems` (brokers after 2.0.1).
fn parse_cookie(reply: &str) -> Option<SsoCookie> {
    let reply: Value = serde_json::from_str(reply).ok()?;
    let cookie = match reply.get("cookieItems") {
        Some(Value::Array(items)) => items.first()?.clone(),
        _ => reply,
    };
    let name = cookie["cookieName"].as_str().filter(|s| !s.is_empty())?;
    let value = cookie["cookieContent"].as_str().filter(|s| !s.is_empty())?;
    Some(SsoCookie {
        name: name.to_string(),
        value: value.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_account_is_picked_by_name() {
        let reply = r#"{"accounts":[{"username":"Ann@Contoso.com","homeAccountId":"1"},{"username":"bob@contoso.com"}]}"#;
        assert_eq!(
            pick_account(reply, Some("ann@contoso.com")).unwrap()["homeAccountId"],
            "1"
        );
        assert!(pick_account(reply, Some("eve@contoso.com")).is_none());
        // With no name, only a single account is a safe guess.
        assert!(pick_account(reply, None).is_none());
        let one = r#"{"accounts":[{"username":"ann@contoso.com"}]}"#;
        assert!(pick_account(one, Some(" ")).is_some());
        assert!(pick_account(r#"{"accounts":[]}"#, None).is_none());
    }

    #[test]
    fn the_cookie_request_names_the_account_twice() {
        let account = json!({"username": "ann@contoso.com"});
        let req: Value = serde_json::from_str(&cookie_request(
            &account,
            "client",
            "https://login.microsoftonline.com/common/oauth2/nativeclient",
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
        ))
        .unwrap();
        assert_eq!(req["account"], account);
        assert_eq!(req["authParameters"]["account"], account);
        assert_eq!(req["authParameters"]["authorizationType"], 8);
        assert_eq!(req["authParameters"]["username"], "ann@contoso.com");
        assert_eq!(
            req["authParameters"]["requestedScopes"][0],
            "https://graph.microsoft.com/.default"
        );
        assert_eq!(req["ssoUrl"], req["authParameters"]["ssoUrl"]);
    }

    #[test]
    fn both_cookie_shapes_parse() {
        let old = r#"{"cookieName":"x-ms-RefreshTokenCredential","cookieContent":"abc"}"#;
        let new = r#"{"cookieItems":[{"cookieName":"x-ms-RefreshTokenCredential","cookieContent":"abc"}]}"#;
        let want = SsoCookie {
            name: "x-ms-RefreshTokenCredential".into(),
            value: "abc".into(),
        };
        assert_eq!(parse_cookie(old).as_ref(), Some(&want));
        assert_eq!(parse_cookie(new).as_ref(), Some(&want));
        assert!(parse_cookie(r#"{"cookieItems":[]}"#).is_none());
        assert!(parse_cookie("not json").is_none());
    }

    #[test]
    fn correlation_ids_look_like_uuids() {
        let id = correlation_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id.matches('-').count(), 4);
        assert_eq!(&id[14..15], "4");
    }
}
