use std::fmt;

use libwebrtc::peer_connection_factory::IceServer;
use serde::Deserialize;
use session::{IC3, Request, Scope, Session};

use crate::error::{Error, Result};

pub const TOKENS_URL: &str = "https://teams.microsoft.com/trap/tokens";
pub const DEFAULT_RELAY_HOST: &str = "gateway-eu.az.relay.teams.cloud.microsoft";

pub fn ic3_scope() -> Scope {
    Scope::new(IC3, "Teams.AccessAsUser.All")
}

#[derive(Clone, Deserialize)]
pub struct RelayCredentials {
    #[serde(default)]
    pub realm: String,
    pub username: String,
    pub password: String,
}

impl fmt::Debug for RelayCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelayCredentials")
            .field("realm", &self.realm)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct TokensAnswer {
    tokens: Vec<RelayCredentials>,
    #[serde(default)]
    expires: u64,
}

#[derive(Debug, Clone)]
pub struct RelayGrant {
    pub credentials: RelayCredentials,
    pub expires_in_seconds: u64,
}

impl RelayGrant {
    pub fn ice_server(&self, relay_host: &str) -> IceServer {
        IceServer {
            urls: vec![
                format!("turn:{relay_host}:3478?transport=udp"),
                format!("turn:{relay_host}:443?transport=tcp"),
                format!("turns:{relay_host}:443"),
            ],
            username: self.credentials.username.clone(),
            password: self.credentials.password.clone(),
        }
    }
}

pub async fn fetch_relay_grant(session: &Session) -> Result<RelayGrant> {
    let mut request = Request::get(TOKENS_URL);
    request.headers = vec![
        ("api-version".into(), "2".into()),
        ("X-MS-Migration".into(), "True".into()),
        ("Accept".into(), "application/json, text/javascript".into()),
    ];
    let response = session.send(request, &ic3_scope()).await?;
    let answer: TokensAnswer = serde_json::from_value(response.body)
        .map_err(|error| Error::Signaling(format!("trap/tokens answer: {error}")))?;
    let credentials = answer
        .tokens
        .into_iter()
        .next()
        .ok_or_else(|| Error::Signaling("trap/tokens returned no relay token".into()))?;
    Ok(RelayGrant {
        credentials,
        expires_in_seconds: answer.expires,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_secrets() {
        let credentials = RelayCredentials {
            realm: "rtcmedia".into(),
            username: "user-secret".into(),
            password: "pass-secret".into(),
        };
        let text = format!("{credentials:?}");
        assert!(text.contains("rtcmedia") && !text.contains("secret"));
    }

    #[test]
    fn ice_server_lists_udp_tcp_and_tls() {
        let grant = RelayGrant {
            credentials: RelayCredentials { realm: String::new(), username: "u".into(), password: "p".into() },
            expires_in_seconds: 604800,
        };
        let server = grant.ice_server(DEFAULT_RELAY_HOST);
        assert_eq!(server.urls.len(), 3);
        assert!(server.urls[2].starts_with("turns:"));
    }
}
