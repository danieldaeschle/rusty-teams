use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

pub const GOOGLE_CLIENT_ID_ENV: &str = "TEAMS_GOOGLE_CLIENT_ID";
pub const GOOGLE_CLIENT_SECRET_ENV: &str = "TEAMS_GOOGLE_CLIENT_SECRET";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPES: &str =
    "https://www.googleapis.com/auth/calendar https://www.googleapis.com/auth/userinfo.email";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleOAuth {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthPending {
    pub authorize_url: String,
    pub redirect_uri: String,
    pub state: String,
    pub code_verifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TokenBundle {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_in: i64,
    #[serde(default)]
    pub token_type: String,
}

/// Runtime env wins; otherwise the value baked in at compile time (`option_env!`).
/// Empty strings count as missing. Used by Connect and token refresh.
pub fn client_id_from_env() -> Option<GoogleOAuth> {
    resolve_oauth(
        std::env::var(GOOGLE_CLIENT_ID_ENV).ok().as_deref(),
        std::env::var(GOOGLE_CLIENT_SECRET_ENV).ok().as_deref(),
        option_env!("TEAMS_GOOGLE_CLIENT_ID"),
        option_env!("TEAMS_GOOGLE_CLIENT_SECRET"),
    )
}

fn resolve_oauth(
    runtime_id: Option<&str>,
    runtime_secret: Option<&str>,
    compile_id: Option<&str>,
    compile_secret: Option<&str>,
) -> Option<GoogleOAuth> {
    let client_id = first_non_empty(runtime_id).or_else(|| first_non_empty(compile_id))?;
    let client_secret = first_non_empty(runtime_secret)
        .or_else(|| first_non_empty(compile_secret))
        .unwrap_or_default();
    Some(GoogleOAuth {
        client_id,
        client_secret,
    })
}

fn first_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

impl GoogleOAuth {
    pub fn begin(&self, redirect_uri: &str) -> AuthPending {
        let state = random_url_token(16);
        let code_verifier = random_url_token(32);
        let challenge = code_challenge(&code_verifier);
        let mut url = url::Url::parse(AUTH_URL).expect("auth url");
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("client_id", &self.client_id);
            query.append_pair("redirect_uri", redirect_uri);
            query.append_pair("response_type", "code");
            query.append_pair("scope", SCOPES);
            query.append_pair("access_type", "offline");
            query.append_pair("prompt", "consent");
            query.append_pair("state", &state);
            query.append_pair("code_challenge", &challenge);
            query.append_pair("code_challenge_method", "S256");
        }
        AuthPending {
            authorize_url: url.to_string(),
            redirect_uri: redirect_uri.to_owned(),
            state,
            code_verifier,
        }
    }
}

pub async fn exchange_code(
    oauth: &GoogleOAuth,
    pending: &AuthPending,
    code: &str,
) -> Result<TokenBundle> {
    let client = reqwest::Client::new();
    let mut form = vec![
        ("code", code.to_owned()),
        ("client_id", oauth.client_id.clone()),
        ("redirect_uri", pending.redirect_uri.clone()),
        ("grant_type", "authorization_code".to_owned()),
        ("code_verifier", pending.code_verifier.clone()),
    ];
    if !oauth.client_secret.is_empty() {
        form.push(("client_secret", oauth.client_secret.clone()));
    }
    let response = client
        .post(TOKEN_URL)
        .form(&form)
        .send()
        .await
        .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .json::<serde_json::Value>()
        .await
        .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(Error::Session(session::Error::api(status, TOKEN_URL, body)));
    }
    serde_json::from_value(body).map_err(|error| Error::UnexpectedAnswer(error.to_string()))
}

pub async fn refresh_access_token(oauth: &GoogleOAuth, refresh_token: &str) -> Result<TokenBundle> {
    let client = reqwest::Client::new();
    let mut form = vec![
        ("client_id", oauth.client_id.clone()),
        ("grant_type", "refresh_token".to_owned()),
        ("refresh_token", refresh_token.to_owned()),
    ];
    if !oauth.client_secret.is_empty() {
        form.push(("client_secret", oauth.client_secret.clone()));
    }
    let response = client
        .post(TOKEN_URL)
        .form(&form)
        .send()
        .await
        .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .json::<serde_json::Value>()
        .await
        .map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(Error::Session(session::Error::api(status, TOKEN_URL, body)));
    }
    let mut bundle: TokenBundle =
        serde_json::from_value(body).map_err(|error| Error::UnexpectedAnswer(error.to_string()))?;
    if bundle.refresh_token.is_empty() {
        bundle.refresh_token = refresh_token.to_owned();
    }
    Ok(bundle)
}

fn random_url_token(bytes: usize) -> String {
    let mut buffer = vec![0_u8; bytes];
    rand::rng().fill_bytes(&mut buffer);
    URL_SAFE_NO_PAD.encode(buffer)
}

fn code_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_builds_pkce_authorize_url() {
        let oauth = GoogleOAuth {
            client_id: "cid".into(),
            client_secret: String::new(),
        };
        let pending = oauth.begin("http://127.0.0.1:8765/");
        assert!(pending.authorize_url.contains("client_id=cid"));
        assert!(pending.authorize_url.contains("code_challenge_method=S256"));
        assert!(!pending.code_verifier.is_empty());
        assert_eq!(
            code_challenge("abc"),
            URL_SAFE_NO_PAD.encode(Sha256::digest(b"abc"))
        );
    }

    #[test]
    fn token_bundle_deserializes() {
        let json =
            r#"{"access_token":"a","refresh_token":"r","expires_in":3600,"token_type":"Bearer"}"#;
        let bundle: TokenBundle = serde_json::from_str(json).unwrap();
        assert_eq!(bundle.access_token, "a");
        assert_eq!(bundle.refresh_token, "r");
        assert_eq!(bundle.expires_in, 3600);
    }

    #[test]
    fn resolve_prefers_runtime_over_compile() {
        let oauth = resolve_oauth(
            Some("runtime-id"),
            Some("runtime-secret"),
            Some("compile-id"),
            Some("compile-secret"),
        )
        .unwrap();
        assert_eq!(oauth.client_id, "runtime-id");
        assert_eq!(oauth.client_secret, "runtime-secret");
    }

    #[test]
    fn resolve_falls_back_to_compile_when_runtime_empty() {
        let oauth =
            resolve_oauth(Some("  "), None, Some("compile-id"), Some("compile-secret")).unwrap();
        assert_eq!(oauth.client_id, "compile-id");
        assert_eq!(oauth.client_secret, "compile-secret");
    }

    #[test]
    fn resolve_returns_none_when_both_missing() {
        assert!(resolve_oauth(None, None, None, None).is_none());
        assert!(resolve_oauth(Some(""), None, Some("  "), None).is_none());
    }

    #[test]
    fn resolve_allows_missing_secret() {
        let oauth = resolve_oauth(Some("cid"), None, None, None).unwrap();
        assert_eq!(oauth.client_id, "cid");
        assert!(oauth.client_secret.is_empty());
    }
}
