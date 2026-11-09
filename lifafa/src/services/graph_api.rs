//! Microsoft Graph: what every client of it sends the same way, the
//! address and the bearer token. The clients (the mail backend in
//! `worker::graph`, OneDrive in `cloud::onedrive`, `graph_labels`,
//! `graph_directory` and the sign-in's `oauth::microsoft_whoami`) keep their
//! own agents, timeouts, throttling and error wording, which differ on
//! purpose: each says what went wrong in its own setting.

/// The v1.0 endpoint. The sensitivity labels are read from the beta one
/// (`graph_labels`), which no other client uses.
pub(crate) const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";

/// The `Authorization` header's value for an access token.
pub(crate) fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The address and header the five clients each spelt out before.
    #[test]
    fn every_client_asks_the_same_endpoint_with_a_bearer_token() {
        assert_eq!(GRAPH_BASE, "https://graph.microsoft.com/v1.0");
        assert_eq!(bearer("eyJ0.abc"), "Bearer eyJ0.abc");
        assert_eq!(
            format!("{GRAPH_BASE}/me?$select=displayName,mail,userPrincipalName"),
            "https://graph.microsoft.com/v1.0/me?$select=displayName,mail,userPrincipalName"
        );
    }
}
