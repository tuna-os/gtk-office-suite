//! The organization's directory over Microsoft Graph (#410), for the
//! composer's recipient suggestions: what LDAP directories are through EDS
//! (`directory`), for a Microsoft 365 account signed in within Hylki whose
//! "Search the organization directory" switch is on.
//!
//! Two lists are asked. `/me/people` is the people the user works with,
//! ranked by Microsoft, and answers with only `People.Read`. `/users` is
//! everyone in the tenant, asked only when the first comes up short; some
//! tenants keep it from their users, which costs nothing but that list.
//! Nothing is copied down: answers are kept for a few minutes so a name
//! typed again does not ask again.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::config::{AccountConfig, Protocol};
use crate::contacts::Suggestion;
use crate::services::graph_api::{bearer, GRAPH_BASE};

/// As many as the composer's list shows, and the point below which the
/// tenant's whole list is asked as well.
const TOP: usize = 10;

/// How long an answer is reused. A directory changes slowly; the composer
/// asks on every pause in the typing.
const KEEP: Duration = Duration::from_secs(5 * 60);

/// A search answers while the user types, so a slow tenant loses the
/// suggestion rather than holding up the list.
const TIMEOUT: Duration = Duration::from_secs(4);

/// When Microsoft asks for a pause without saying how long.
const DEFAULT_RETRY_AFTER: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Endpoint {
    People,
    Users,
}

impl Endpoint {
    fn name(self) -> &'static str {
        match self {
            Endpoint::People => "/me/people",
            Endpoint::Users => "/users",
        }
    }
}

/// The accounts a search asks: Microsoft 365 over Graph with the switch on,
/// signed in within Hylki. A token from GNOME Online Accounts carries only
/// the scopes GNOME asked for, which do not include the directory.
pub(crate) fn usable(account: &AccountConfig) -> bool {
    account.enabled
        && account.protocol == Protocol::Graph
        && account.graph_directory
        && account.oauth
        && account.goa_id.is_none()
}

/// The configured accounts a search would ask (reads the accounts file).
pub(crate) fn accounts() -> Vec<AccountConfig> {
    crate::config::load()
        .unwrap_or_default()
        .into_iter()
        .filter(usable)
        .collect()
}

/// A `$search` term: Graph's quoted string, with its quote and backslash
/// escaped, so a typed `"` cannot end the term early.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn people_url(typed: &str) -> String {
    format!(
        "{GRAPH_BASE}/me/people?$search={}&$top={TOP}&$select=displayName,scoredEmailAddresses",
        crate::encoding::percent::encode(&quoted(typed))
    )
}

/// The tenant's users whose name or address starts with a word of `typed`.
/// `$search` on directory objects needs `ConsistencyLevel: eventual`.
fn users_url(typed: &str) -> String {
    let term = format!(
        "{} OR {}",
        quoted(&format!("displayName:{typed}")),
        quoted(&format!("mail:{typed}"))
    );
    format!(
        "{GRAPH_BASE}/users?$search={}&$select=displayName,mail&$top={TOP}",
        crate::encoding::percent::encode(&term)
    )
}

fn suggestion(name: &str, address: &str) -> Option<Suggestion> {
    let address = address.trim();
    if !address.contains('@') || address.contains(['\r', '\n', ' ']) {
        return None;
    }
    let name = name.trim();
    Some(Suggestion {
        name: if name.is_empty() {
            address.to_string()
        } else {
            name.to_string()
        },
        email: address.to_string(),
        from_contacts: true,
        from_directory: true,
        ..Default::default()
    })
}

/// `/me/people`: a person may have several addresses, each its own
/// suggestion.
fn parse_people(json: &serde_json::Value) -> Vec<Suggestion> {
    let mut out = Vec::new();
    for person in json["value"].as_array().into_iter().flatten() {
        let name = person["displayName"].as_str().unwrap_or_default();
        for scored in person["scoredEmailAddresses"]
            .as_array()
            .into_iter()
            .flatten()
        {
            out.extend(suggestion(
                name,
                scored["address"].as_str().unwrap_or_default(),
            ));
        }
    }
    out
}

/// `/users`: a user without `mail` has no mailbox, and their sign-in name
/// would only bounce.
fn parse_users(json: &serde_json::Value) -> Vec<Suggestion> {
    json["value"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|u| {
            suggestion(
                u["displayName"].as_str().unwrap_or_default(),
                u["mail"].as_str().unwrap_or_default(),
            )
        })
        .collect()
}

/// `more` added to `into`, one suggestion per address: the first keeps its
/// place, and gets a name from a later one when it had none.
fn merge(into: &mut Vec<Suggestion>, more: Vec<Suggestion>) {
    for s in more {
        let key = s.email.to_lowercase();
        match into.iter_mut().find(|t| t.email.to_lowercase() == key) {
            Some(t) => {
                if t.name == t.email && s.name != s.email {
                    t.name = s.name;
                }
            }
            None => into.push(s),
        }
    }
}

/// Whether an entry found for a shorter query still answers `query`: each
/// word typed is in its name or address.
fn still_matches(s: &Suggestion, query: &str) -> bool {
    let name = s.name.to_lowercase();
    let email = s.email.to_lowercase();
    query
        .split_whitespace()
        .all(|w| name.contains(w) || email.contains(w))
}

/// One account's answer to a query.
#[derive(Clone)]
struct Answer {
    at: Instant,
    found: Vec<Suggestion>,
    /// Everything the directory has for the query, not a first page: a
    /// longer query can be answered from it.
    complete: bool,
}

#[derive(Default)]
struct State {
    /// By (account, lower-case query).
    answers: HashMap<(String, String), Answer>,
    /// Lists a tenant refused this account, not asked again this session.
    refused: HashSet<(String, Endpoint)>,
    /// Accounts Microsoft asked to wait, until when.
    paused: HashMap<String, Instant>,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}

impl State {
    /// A kept answer for `query`, or one for a shorter query that held
    /// everything, narrowed to it.
    fn recall(&mut self, account: &str, query: &str, now: Instant) -> Option<Vec<Suggestion>> {
        self.answers.retain(|_, a| now.duration_since(a.at) < KEEP);
        if let Some(a) = self.answers.get(&(account.to_string(), query.to_string())) {
            return Some(a.found.clone());
        }
        self.answers
            .iter()
            .filter(|((acc, q), a)| acc == account && a.complete && query.starts_with(q.as_str()))
            .max_by_key(|((_, q), _)| q.len())
            .map(|(_, a)| {
                a.found
                    .iter()
                    .filter(|s| still_matches(s, query))
                    .cloned()
                    .collect()
            })
    }

    fn keep(&mut self, account: &str, query: &str, answer: Answer) {
        self.answers
            .insert((account.to_string(), query.to_string()), answer);
    }
}

/// Why a request had no answer.
#[derive(Debug)]
enum Failure {
    /// The token was turned down: renew it and try again, once.
    Unauthorized,
    /// The tenant does not let this account read this list, or there is
    /// no such list for it: not asked again this session.
    Refused(String),
    /// Too many requests: wait this many seconds.
    Throttled(u64),
    /// The network, a timeout, a server error: try again next time.
    Failed(String),
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| ureq::AgentBuilder::new().timeout(TIMEOUT).build())
}

fn failure(e: ureq::Error) -> Failure {
    match e {
        ureq::Error::Status(401, _) => Failure::Unauthorized,
        ureq::Error::Status(429, resp) => Failure::Throttled(
            resp.header("Retry-After")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(DEFAULT_RETRY_AFTER),
        ),
        ureq::Error::Status(code, resp) => {
            status_failure(code, &resp.into_string().unwrap_or_default())
        }
        other => Failure::Failed(other.to_string()),
    }
}

fn status_failure(code: u16, body: &str) -> Failure {
    let v = serde_json::from_str::<serde_json::Value>(body).unwrap_or_default();
    let what = v["error"]["code"]
        .as_str()
        .or(v["error"]["message"].as_str())
        .unwrap_or_default();
    let why = format!("HTTP {code} {what}").trim_end().to_string();
    // Authorization_RequestDenied, ErrorAccessDenied: the tenant's answer,
    // which the next query will get as well. A 400 that names its error
    // may be this query's alone; a bare one is what a personal Microsoft
    // account, which has no organization, gets for every query.
    if matches!(code, 403 | 404) || (code == 400 && what.is_empty()) {
        Failure::Refused(why)
    } else {
        Failure::Failed(why)
    }
}

fn get(token: &str, endpoint: Endpoint, typed: &str) -> Result<serde_json::Value, Failure> {
    let (url, consistency) = match endpoint {
        Endpoint::People => (people_url(typed), None),
        Endpoint::Users => (users_url(typed), Some("eventual")),
    };
    // The query is a person's name, so the log names only the list asked.
    tracing::debug!(target: "hylki::graph", "> GET {} (directory search)", endpoint.name());
    let mut req = agent().get(&url).set("Authorization", &bearer(token));
    if let Some(level) = consistency {
        req = req.set("ConsistencyLevel", level);
    }
    req.call()
        .map_err(failure)
        .and_then(|resp| resp.into_json().map_err(|e| Failure::Failed(e.to_string())))
}

/// The people whose name or address starts with what was typed, from the
/// directory of each of `accounts` that may be asked. Blocking: call it off
/// the main thread. A failure is logged and costs only the suggestions.
pub(crate) fn search(typed: &str, accounts: &[AccountConfig]) -> Vec<Suggestion> {
    let typed = typed.trim();
    if typed.chars().count() < crate::contacts::directory::MIN_QUERY || typed.len() > 128 {
        return Vec::new();
    }
    let query = typed.to_lowercase();
    let mut out = Vec::new();
    for account in accounts.iter().filter(|a| usable(a)) {
        merge(&mut out, search_account(account, typed, &query));
    }
    out
}

fn search_account(account: &AccountConfig, typed: &str, query: &str) -> Vec<Suggestion> {
    let email = account.email.as_str();
    let now = Instant::now();
    {
        let Ok(mut st) = state().lock() else {
            return Vec::new();
        };
        if let Some(found) = st.recall(email, query, now) {
            return found;
        }
        if st.paused.get(email).is_some_and(|until| now < *until) {
            return Vec::new();
        }
    }
    let Some(mut token) = crate::worker::oauth_token_blocking(account) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut complete = true;
    // Only an answer that will hold is kept: a timeout now says nothing
    // about the next try.
    let mut keep = true;
    for endpoint in [Endpoint::People, Endpoint::Users] {
        if endpoint == Endpoint::Users && found.len() >= TOP {
            complete = false;
            break;
        }
        if state()
            .lock()
            .is_ok_and(|st| st.refused.contains(&(email.to_string(), endpoint)))
        {
            complete = false;
            continue;
        }
        let mut answer = get(&token, endpoint, typed);
        if matches!(answer, Err(Failure::Unauthorized)) {
            crate::worker::forget_access_token(account);
            match crate::worker::oauth_token_blocking(account) {
                Some(fresh) => {
                    token = fresh;
                    answer = get(&token, endpoint, typed);
                }
                None => return found,
            }
        }
        match answer {
            Ok(json) => {
                let page = match endpoint {
                    Endpoint::People => parse_people(&json),
                    Endpoint::Users => parse_users(&json),
                };
                // A full page may have more behind it; counted in entries,
                // since a person can bring several addresses or none.
                if json["value"].as_array().map_or(0, Vec::len) >= TOP {
                    complete = false;
                }
                tracing::debug!(target: "hylki::graph", "< OK {} ({} found)", endpoint.name(), page.len());
                merge(&mut found, page);
            }
            Err(Failure::Refused(why)) => {
                tracing::warn!(
                    target: "hylki::graph",
                    "directory search: {} refused for {email} ({why}); not asked again this session",
                    endpoint.name()
                );
                if let Ok(mut st) = state().lock() {
                    st.refused.insert((email.to_string(), endpoint));
                }
                complete = false;
            }
            Err(Failure::Throttled(secs)) => {
                tracing::warn!(target: "hylki::graph", "directory search: throttled for {email}, waiting {secs} s");
                if let Ok(mut st) = state().lock() {
                    st.paused.insert(
                        email.to_string(),
                        Instant::now() + Duration::from_secs(secs),
                    );
                }
                complete = false;
                keep = false;
                break;
            }
            Err(Failure::Unauthorized) => {
                tracing::warn!(target: "hylki::graph", "directory search: {} turned the token down for {email}", endpoint.name());
                complete = false;
                keep = false;
            }
            Err(Failure::Failed(why)) => {
                tracing::warn!(target: "hylki::graph", "directory search: {} failed for {email}: {why}", endpoint.name());
                complete = false;
                keep = false;
            }
        }
    }
    if keep {
        if let Ok(mut st) = state().lock() {
            st.keep(
                email,
                query,
                Answer {
                    at: Instant::now(),
                    found: found.clone(),
                    complete,
                },
            );
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, email: &str) -> Suggestion {
        Suggestion {
            name: name.into(),
            email: email.into(),
            from_contacts: true,
            from_directory: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_typed_quote_cannot_end_the_search_term() {
        assert_eq!(quoted(r#"ann"e\x"#), r#""ann\"e\\x""#);
        let url = people_url("Ann \"Lee\"");
        assert_eq!(
            url,
            "https://graph.microsoft.com/v1.0/me/people?$search=%22Ann%20%5C%22Lee%5C%22%22\
             &$top=10&$select=displayName,scoredEmailAddresses"
        );
        let url = users_url("o'br&en");
        assert!(
            url.starts_with("https://graph.microsoft.com/v1.0/users?$search="),
            "{url}"
        );
        let term = url
            .split("$search=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();
        assert_eq!(
            crate::encoding::percent::decode(term, false),
            r#""displayName:o'br&en" OR "mail:o'br&en""#
        );
        assert!(url.ends_with("&$select=displayName,mail&$top=10"), "{url}");
    }

    #[test]
    fn people_answers_give_every_address() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
              "@odata.context": "https://graph.microsoft.com/v1.0/$metadata#users('me')/people",
              "value": [
                {
                  "id": "33b43a5b-87d6-41ec-91f8-a2610a3f3d40",
                  "displayName": "Lynne Robbins",
                  "scoredEmailAddresses": [
                    {"address": "LynneR@contoso.onmicrosoft.com", "relevanceScore": 8.0, "selectionLikelihood": "notSpecified"},
                    {"address": "lynne.robbins@contoso.com", "relevanceScore": 2.0, "selectionLikelihood": "notSpecified"}
                  ],
                  "personType": {"class": "Person", "subclass": "OrganizationUser"}
                },
                {
                  "id": "e3d0513b-449e-4198-ba6f-bd97ae7cae85",
                  "displayName": "Room 101",
                  "scoredEmailAddresses": [],
                  "personType": {"class": "Person", "subclass": "OrganizationUser"}
                },
                {
                  "id": "a1b2",
                  "displayName": "",
                  "scoredEmailAddresses": [{"address": "sales@contoso.com", "relevanceScore": 1.0}],
                  "personType": {"class": "Group", "subclass": "UnifiedGroup"}
                }
              ]
            }"#,
        )
        .unwrap();
        let found = parse_people(&json);
        let got: Vec<(&str, &str)> = found
            .iter()
            .map(|s| (s.name.as_str(), s.email.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("Lynne Robbins", "LynneR@contoso.onmicrosoft.com"),
                ("Lynne Robbins", "lynne.robbins@contoso.com"),
                ("sales@contoso.com", "sales@contoso.com"),
            ]
        );
        assert!(found.iter().all(|s| s.from_contacts && !s.own));
        assert!(parse_people(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn users_without_a_mailbox_are_left_out() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
              "@odata.context": "https://graph.microsoft.com/v1.0/$metadata#users(displayName,mail)",
              "@odata.count": 3,
              "value": [
                {"displayName": "Adele Vance", "mail": "AdeleV@contoso.com"},
                {"displayName": "Sync Service", "mail": null},
                {"displayName": "Alex Wilber", "mail": "AlexW@contoso.com"}
              ]
            }"#,
        )
        .unwrap();
        let got: Vec<String> = parse_users(&json).into_iter().map(|s| s.email).collect();
        assert_eq!(got, ["AdeleV@contoso.com", "AlexW@contoso.com"]);
    }

    #[test]
    fn one_suggestion_per_address() {
        let mut found = vec![
            s("adelev@contoso.com", "adelev@contoso.com"),
            s("Alex Wilber", "AlexW@contoso.com"),
        ];
        merge(
            &mut found,
            vec![
                s("Adele Vance", "AdeleV@Contoso.com"),
                s("Someone Else", "alexw@contoso.com"),
                s("Diego", "diegos@contoso.com"),
            ],
        );
        let got: Vec<(&str, &str)> = found
            .iter()
            .map(|s| (s.name.as_str(), s.email.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("Adele Vance", "adelev@contoso.com"),
                ("Alex Wilber", "AlexW@contoso.com"),
                ("Diego", "diegos@contoso.com"),
            ]
        );
    }

    #[test]
    fn a_whole_answer_serves_longer_queries() {
        let mut st = State::default();
        let t0 = Instant::now();
        let found = vec![
            s("Adele Vance", "adelev@contoso.com"),
            s("Alex Wilber", "alexw@contoso.com"),
        ];
        st.keep(
            "me@contoso.com",
            "ade",
            Answer {
                at: t0,
                found: found.clone(),
                complete: true,
            },
        );
        assert_eq!(
            st.recall("me@contoso.com", "ade", t0).map(|f| f.len()),
            Some(2)
        );
        let narrowed = st.recall("me@contoso.com", "adele v", t0).unwrap();
        assert_eq!(narrowed.len(), 1);
        assert_eq!(narrowed[0].email, "adelev@contoso.com");
        assert!(
            st.recall("other@contoso.com", "adele", t0).is_none(),
            "answers are the account's own"
        );
        // A first page says nothing about what a longer query would find.
        st.keep(
            "me@contoso.com",
            "ale",
            Answer {
                at: t0,
                found,
                complete: false,
            },
        );
        assert!(st.recall("me@contoso.com", "alex", t0).is_none());
        // And nothing is kept past its time.
        assert!(st
            .recall("me@contoso.com", "ade", t0 + KEEP + Duration::from_secs(1))
            .is_none());
        assert!(st.answers.is_empty());
    }

    #[test]
    fn a_list_the_account_cannot_have_is_not_asked_again() {
        assert!(matches!(status_failure(400, ""), Failure::Refused(_)));
        assert!(matches!(
            status_failure(403, r#"{"error":{"code":"Authorization_RequestDenied"}}"#),
            Failure::Refused(_)
        ));
        assert!(matches!(
            status_failure(400, r#"{"error":{"code":"Request_BadRequest"}}"#),
            Failure::Failed(_)
        ));
        assert!(matches!(status_failure(503, ""), Failure::Failed(_)));
    }

    #[test]
    fn only_accounts_signed_in_here_are_asked() {
        let acc = AccountConfig {
            protocol: Protocol::Graph,
            oauth: true,
            graph_directory: true,
            ..crate::config::blank_account()
        };
        assert!(usable(&acc));
        assert!(!usable(&AccountConfig {
            goa_id: Some("account_1".into()),
            ..acc.clone()
        }));
        assert!(!usable(&AccountConfig {
            graph_directory: false,
            ..acc.clone()
        }));
        assert!(!usable(&AccountConfig {
            protocol: Protocol::Imap,
            ..acc.clone()
        }));
        assert!(!usable(&AccountConfig {
            enabled: false,
            ..acc
        }));
        assert!(search("ad", &[]).is_empty());
    }
}
