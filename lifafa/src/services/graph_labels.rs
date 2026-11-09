//! The organization's sensitivity labels over Microsoft Graph (#411), to
//! name a message's label when the message leaves the name out: Purview
//! writes only the label's id into `msip_labels` unless the tenant asks for
//! more, and the reader's pill then says only "Labeled".
//!
//! The list is the labels published to the user, from Graph's beta
//! `/me/security/informationProtection/sensitivityLabels` (there is no v1.0
//! one), read once for each account with the switch on and again after an
//! hour. A label from another organization is not on it and stays
//! unnamed.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::config::{AccountConfig, Protocol};
use crate::mail::sensitivity::SensitivityLabel;
use crate::services::graph_api::bearer;

const URL: &str =
    "https://graph.microsoft.com/beta/me/security/informationProtection/sensitivityLabels";

/// How long a list is used. Labels change when an administrator edits the
/// policy, which is rare.
const KEEP: Duration = Duration::from_secs(60 * 60);

/// How long after a failed read the next one is tried: messages open one
/// after another, and each would otherwise ask again.
const RETRY: Duration = Duration::from_secs(5 * 60);

const TIMEOUT: Duration = Duration::from_secs(10);

/// A tenant with more labels than this has more than anyone reads.
const MAX_PAGES: usize = 5;

/// What the organization says about one of its labels.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TenantLabel {
    name: String,
    color: String,
    tooltip: String,
}

/// The accounts whose list is read: Microsoft 365 over Graph with the
/// switch on, signed in within Hylki. A token from GNOME Online Accounts
/// carries only the scopes GNOME asked for.
fn usable(account: &AccountConfig) -> bool {
    account.enabled
        && account.protocol == Protocol::Graph
        && account.graph_label_names
        && account.oauth
        && account.goa_id.is_none()
}

#[derive(Default)]
struct State {
    /// By account address: when read, and the labels by lower-case id.
    lists: HashMap<String, (Instant, HashMap<String, TenantLabel>)>,
    /// When a read last failed, by account.
    failed: HashMap<String, Instant>,
    /// Accounts the tenant refused the list, not asked again this session.
    refused: HashSet<String>,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}

/// `label` with its name, color and the organization's description filled
/// in from the account's list, when it is a Purview label on that list.
/// Blocking: the first label of a session reads the list.
pub(crate) fn fill(account: &AccountConfig, label: &mut SensitivityLabel) {
    if label.id.is_empty() || !usable(account) {
        return;
    }
    let Some(found) = lookup(account, &label.id.to_ascii_lowercase()) else {
        return;
    };
    if label.name.is_empty() {
        label.name = found.name;
    }
    label.color = found.color;
    label.tooltip = found.tooltip;
}

fn lookup(account: &AccountConfig, id: &str) -> Option<TenantLabel> {
    let email = account.email.as_str();
    let now = Instant::now();
    {
        let st = state().lock().ok()?;
        if let Some((at, labels)) = st.lists.get(email) {
            if now.duration_since(*at) < KEEP {
                return labels.get(id).cloned();
            }
        }
        if st.refused.contains(email)
            || st
                .failed
                .get(email)
                .is_some_and(|at| now.duration_since(*at) < RETRY)
        {
            return None;
        }
    }
    let read = read_list(account);
    let mut st = state().lock().ok()?;
    match read {
        Ok(labels) => {
            tracing::debug!(target: "hylki::graph", "< OK sensitivity labels ({} for {email})", labels.len());
            let found = labels.get(id).cloned();
            st.lists.insert(email.to_string(), (Instant::now(), labels));
            st.failed.remove(email);
            found
        }
        Err(Failure::Refused(why)) => {
            tracing::warn!(target: "hylki::graph", "sensitivity labels refused for {email} ({why}); not asked again this session");
            st.refused.insert(email.to_string());
            None
        }
        Err(Failure::Failed(why)) => {
            tracing::warn!(target: "hylki::graph", "sensitivity labels for {email} failed: {why}");
            st.failed.insert(email.to_string(), Instant::now());
            None
        }
    }
}

#[derive(Debug)]
enum Failure {
    /// The tenant does not let this account read its labels, or the
    /// sign-in lacks the scope: the next read would get the same.
    Refused(String),
    /// The network, a timeout, a server error, a lapsed token.
    Failed(String),
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| ureq::AgentBuilder::new().timeout(TIMEOUT).build())
}

fn get(token: &str, url: &str) -> Result<serde_json::Value, (Option<u16>, String)> {
    tracing::debug!(target: "hylki::graph", "> GET {}", url.strip_prefix("https://graph.microsoft.com").unwrap_or(url));
    match agent().get(url).set("Authorization", &bearer(token)).call() {
        Ok(resp) => resp.into_json().map_err(|e| (None, e.to_string())),
        Err(ureq::Error::Status(code, resp)) => {
            let v = resp.into_json::<serde_json::Value>().unwrap_or_default();
            let what = v["error"]["code"].as_str().unwrap_or_default();
            Err((
                Some(code),
                format!("HTTP {code} {what}").trim_end().to_string(),
            ))
        }
        Err(e) => Err((None, e.to_string())),
    }
}

fn read_list(account: &AccountConfig) -> Result<HashMap<String, TenantLabel>, Failure> {
    let mut token = crate::worker::oauth_token_blocking(account)
        .ok_or_else(|| Failure::Failed("no token".into()))?;
    let mut labels = HashMap::new();
    let mut url = URL.to_string();
    let mut renewed = false;
    for _ in 0..MAX_PAGES {
        let page = match get(&token, &url) {
            Err((Some(401), _)) if !renewed => {
                renewed = true;
                crate::worker::forget_access_token(account);
                token = crate::worker::oauth_token_blocking(account)
                    .ok_or_else(|| Failure::Failed("no token".into()))?;
                continue;
            }
            Err((Some(400 | 403 | 404), why)) => return Err(Failure::Refused(why)),
            Err((_, why)) => return Err(Failure::Failed(why)),
            Ok(page) => page,
        };
        labels.extend(parse(&page));
        match page["@odata.nextLink"].as_str() {
            Some(next) if next.starts_with("https://graph.microsoft.com/") => {
                url = next.to_string()
            }
            _ => break,
        }
    }
    Ok(labels)
}

/// A label's name: `displayName` in the resource's description, `name` in
/// what the endpoint has been seen to send.
fn name_of(v: &serde_json::Value) -> String {
    [&v["displayName"], &v["name"]]
        .into_iter()
        .filter_map(|n| n.as_str())
        .map(str::trim)
        .find(|n| !n.is_empty())
        .unwrap_or_default()
        .to_string()
}

/// The labels of one page, by lower-case id. A sublabel is named after its
/// parent too, "Confidential / Finance", the way the header writes it.
fn parse(page: &serde_json::Value) -> Vec<(String, TenantLabel)> {
    let mut out = Vec::new();
    for v in page["value"].as_array().into_iter().flatten() {
        let Some(id) = v["id"].as_str().filter(|id| !id.is_empty()) else {
            continue;
        };
        let name = name_of(v);
        if name.is_empty() {
            continue;
        }
        let parent = name_of(&v["parent"]);
        let name = if parent.is_empty() {
            name
        } else {
            format!("{parent} / {name}")
        };
        let tooltip = [&v["tooltip"], &v["description"]]
            .into_iter()
            .filter_map(|t| t.as_str())
            .map(str::trim)
            .find(|t| !t.is_empty())
            .unwrap_or_default()
            .to_string();
        out.push((
            id.to_ascii_lowercase(),
            TenantLabel {
                name,
                color: color(v["color"].as_str().unwrap_or_default()),
                tooltip,
            },
        ));
    }
    out
}

/// `#rrggbb`, or empty: the value goes into the reader's markup.
fn color(raw: &str) -> String {
    let raw = raw.trim();
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        format!("#{}", hex.to_ascii_lowercase())
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_named_with_their_parent() {
        let page: serde_json::Value = serde_json::from_str(
            r##"{
              "value": [
                {
                  "id": "0D39DC11-75FF-4309-8B32-FF94F0E41607",
                  "name": "Any User (No Protection)",
                  "description": "",
                  "color": "",
                  "tooltip": "Data labeled for Any User will not be protected.",
                  "parent": {"id": "566663c7-4d8d-4b8f-b280-784a31971dbe", "name": "Highly Confidential", "parent": null}
                },
                {"id": "566663c7-4d8d-4b8f-b280-784a31971dbe", "displayName": "Highly Confidential", "color": "#A4262C", "parent": null},
                {"id": "c0ffee00-0000-0000-0000-000000000000", "name": "", "color": "#00ff00"},
                {"id": "c0ffee00-0000-0000-0000-000000000001", "name": "Public", "color": "red;background:url(x)", "description": "Anyone may read it."}
              ]
            }"##,
        )
        .unwrap();
        let got: HashMap<String, TenantLabel> = parse(&page).into_iter().collect();
        assert_eq!(got.len(), 3, "a label without a name is left out");
        let sub = &got["0d39dc11-75ff-4309-8b32-ff94f0e41607"];
        assert_eq!(sub.name, "Highly Confidential / Any User (No Protection)");
        assert_eq!(
            sub.tooltip,
            "Data labeled for Any User will not be protected."
        );
        assert_eq!(got["566663c7-4d8d-4b8f-b280-784a31971dbe"].color, "#a4262c");
        let public = &got["c0ffee00-0000-0000-0000-000000000001"];
        assert_eq!(public.color, "", "only a color goes into the markup");
        assert_eq!(public.tooltip, "Anyone may read it.");
        assert!(parse(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn only_accounts_signed_in_here_are_asked() {
        let acc = AccountConfig {
            protocol: Protocol::Graph,
            oauth: true,
            graph_label_names: true,
            ..crate::config::blank_account()
        };
        assert!(usable(&acc));
        assert!(!usable(&AccountConfig {
            goa_id: Some("account_1".into()),
            ..acc.clone()
        }));
        assert!(!usable(&AccountConfig {
            graph_label_names: false,
            ..acc.clone()
        }));
        assert!(!usable(&AccountConfig {
            protocol: Protocol::Imap,
            ..acc.clone()
        }));
        // An account that may not ask leaves the label as the header has it.
        let mut label = SensitivityLabel {
            id: "x".into(),
            ..Default::default()
        };
        fill(
            &AccountConfig {
                enabled: false,
                ..acc
            },
            &mut label,
        );
        assert_eq!(
            label,
            SensitivityLabel {
                id: "x".into(),
                ..Default::default()
            }
        );
    }
}
