use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

use serde::Deserialize;
use tokio::sync::Mutex;
use zeroize::{Zeroize, Zeroizing};

use crate::app::App;
use crate::error::{Error, Result};
use crate::scope::{IC3, Scope};
use crate::transport::{BoxFuture, Diagnosis, OpenedTab, Transport};

/// Public client used by Teams Web. It is an identifier, not a secret.
pub const TEAMS_WEB_CLIENT_ID: &str = "5e3ce6c0-2b1f-4285-8d4b-75ee78787346";

const TEAMS_ORIGIN: &str = "https://teams.cloud.microsoft";
const USER_AGENT: &str = concat!("Rusty-Teams/", env!("CARGO_PKG_VERSION"));
const REQUEST_TIMEOUT: Duration = Duration::from_secs(25);
const ACCESS_MARGIN: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct NativeAuthConfig {
    pub refresh_token_file: PathBuf,
    pub client_id: String,
    pub authority: String,
}

impl NativeAuthConfig {
    pub fn teams(refresh_token_file: impl Into<PathBuf>) -> Self {
        NativeAuthConfig {
            refresh_token_file: refresh_token_file.into(),
            client_id: TEAMS_WEB_CLIENT_ID.to_owned(),
            authority: "common".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TokenKey {
    tenant: String,
    resource: String,
    nested_client: Option<&'static str>,
}

impl TokenKey {
    fn new(scope: &Scope, tenant: Option<&str>) -> Self {
        TokenKey {
            tenant: tenant.unwrap_or_default().to_ascii_lowercase(),
            resource: scope.resource.trim_end_matches('/').to_ascii_lowercase(),
            nested_client: scope.nested_app.map(|app| app.client_id),
        }
    }
}

struct HeldAccess {
    token: Zeroizing<String>,
    scopes: Vec<String>,
    usable_until: Instant,
}

struct AuthState {
    refresh_token: Zeroizing<String>,
    access: HashMap<TokenKey, HeldAccess>,
}

/// Native OAuth provider. Access tokens stay in memory; the refresh credential can be persisted.
pub struct NativeAuth {
    config: NativeAuthConfig,
    client: reqwest::Client,
    state: Mutex<AuthState>,
}

impl NativeAuth {
    pub fn new(config: NativeAuthConfig) -> Result<Self> {
        let refresh_token = load_refresh_token(&config.refresh_token_file)?.ok_or_else(|| {
            Error::LoginRequired("no saved native sign-in credential was found".into())
        })?;
        Self::build(config, refresh_token)
    }

    /// Imports the credential produced by the normal browser sign-in and hands off to native OAuth.
    pub fn from_refresh_token(config: NativeAuthConfig, refresh_token: String) -> Result<Self> {
        if refresh_token.is_empty() {
            return Err(Error::LoginRequired(
                "the browser did not return a reusable Teams credential".into(),
            ));
        }
        let refresh_token = Zeroizing::new(refresh_token);
        store_refresh_token(&config.refresh_token_file, refresh_token.as_str())?;
        Self::build(config, refresh_token)
    }

    fn build(config: NativeAuthConfig, refresh_token: Zeroizing<String>) -> Result<Self> {
        crate::native::ensure_crypto_provider();
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .map_err(|_| Error::NativeAuth("could not create the OAuth HTTP client".into()))?;
        Ok(NativeAuth {
            config,
            client,
            state: Mutex::new(AuthState {
                refresh_token,
                access: HashMap::new(),
            }),
        })
    }

    /// Proves that the saved SSO session can obtain a real Teams token.
    pub async fn check(&self) -> Result<()> {
        self.token(&Scope::new(IC3, "Teams.AccessAsUser.All"), None, false)
            .await
            .map(drop)
    }

    pub async fn token(
        &self,
        scope: &Scope,
        tenant: Option<&str>,
        force_refresh: bool,
    ) -> Result<String> {
        let key = TokenKey::new(scope, tenant);
        let mut state = self.state.lock().await;
        if !force_refresh
            && let Some(held) = state.access.get(&key)
            && Instant::now() < held.usable_until
            && grants(&held.scopes, scope)
        {
            return Ok(held.token.to_string());
        }

        if scope.nested_app.is_some() {
            self.nested_token(&mut state, key, scope, tenant).await
        } else {
            self.regular_token(&mut state, key, scope, tenant).await
        }
    }

    async fn regular_token(
        &self,
        state: &mut AuthState,
        key: TokenKey,
        scope: &Scope,
        tenant: Option<&str>,
    ) -> Result<String> {
        let refresh_token = Zeroizing::new(state.refresh_token.to_string());
        let grant = self.refresh(scope, tenant, &refresh_token).await?;
        self.install_grant(state, key, scope, grant, true)
    }

    async fn nested_token(
        &self,
        state: &mut AuthState,
        key: TokenKey,
        scope: &Scope,
        tenant: Option<&str>,
    ) -> Result<String> {
        let first = Zeroizing::new(state.refresh_token.to_string());
        if let Ok(grant) = self.refresh(scope, tenant, &first).await {
            return self.install_grant(state, key.clone(), scope, grant, false);
        }

        // Refresh the Teams broker credential once, then retry the nested-app exchange.
        let base_scope = Scope::new(IC3, "Teams.AccessAsUser.All");
        let base_key = TokenKey::new(&base_scope, None);
        let grant = self.refresh(&base_scope, None, &first).await?;
        let _ = self.install_grant(state, base_key, &base_scope, grant, true)?;
        let retry = Zeroizing::new(state.refresh_token.to_string());
        let grant = self.refresh(scope, tenant, &retry).await?;
        self.install_grant(state, key, scope, grant, false)
    }

    fn install_grant(
        &self,
        state: &mut AuthState,
        key: TokenKey,
        requested: &Scope,
        mut grant: TokenGrant,
        rotate_refresh: bool,
    ) -> Result<String> {
        if !grants(&grant.scopes, requested) {
            return Err(Error::LoginRequired(format!(
                "the Teams web application is not granted {}",
                requested.name
            )));
        }
        if rotate_refresh && let Some(refresh_token) = grant.refresh_token.take() {
            store_refresh_token(&self.config.refresh_token_file, refresh_token.as_str())?;
            state.refresh_token = refresh_token;
        }
        let token = grant.access_token.to_string();
        let scopes = grant.scopes;
        let usable_for = grant
            .expires_in
            .checked_sub(ACCESS_MARGIN)
            .unwrap_or(Duration::ZERO);
        state.access.insert(
            key,
            HeldAccess {
                token: grant.access_token,
                scopes,
                usable_until: Instant::now() + usable_for,
            },
        );
        Ok(token)
    }

    async fn refresh(
        &self,
        scope: &Scope,
        tenant: Option<&str>,
        refresh_token: &str,
    ) -> Result<TokenGrant> {
        let requested = format!("{}/.default", scope.resource.trim_end_matches('/'));
        let refresh_scope = format!("{requested} openid profile offline_access");
        let mut form = vec![
            ("client_id", self.config.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", refresh_scope.as_str()),
        ];
        if let Some(nested) = scope.nested_app {
            form[0].1 = nested.client_id;
            form.extend([
                ("brk_client_id", self.config.client_id.as_str()),
                ("redirect_uri", nested.redirect_uri),
                ("brk_redirect_uri", nested.broker_redirect_uri),
            ]);
        }
        let response = self
            .client
            .post(self.token_url(tenant))
            .header("Origin", TEAMS_ORIGIN)
            .form(&form)
            .send()
            .await
            .map_err(|_| Error::NativeAuth("OAuth refresh request failed".into()))?;
        token_grant(response, "OAuth refresh").await
    }

    fn token_url(&self, tenant: Option<&str>) -> String {
        format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            tenant.unwrap_or(&self.config.authority)
        )
    }
}

struct TokenGrant {
    access_token: Zeroizing<String>,
    refresh_token: Option<Zeroizing<String>>,
    scopes: Vec<String>,
    expires_in: Duration,
}

#[derive(Deserialize, Default)]
struct TokenAnswer {
    access_token: Option<String>,
    refresh_token: Option<String>,
    scope: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
}

impl Drop for TokenAnswer {
    fn drop(&mut self) {
        if let Some(value) = self.access_token.as_mut() {
            value.zeroize();
        }
        if let Some(value) = self.refresh_token.as_mut() {
            value.zeroize();
        }
    }
}

async fn token_grant(response: reqwest::Response, stage: &str) -> Result<TokenGrant> {
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|_| Error::NativeAuth(format!("{stage} returned an unreadable response")))?;
    let body = Zeroizing::new(body.to_vec());
    let mut answer: TokenAnswer = serde_json::from_slice(&body)
        .map_err(|_| Error::NativeAuth(format!("{stage} returned an invalid response")))?;
    if !status.is_success() || answer.error.is_some() {
        let code = answer.error.as_deref().unwrap_or("unknown_error");
        return Err(oauth_error(stage, status.as_u16(), code));
    }
    let access_token = answer
        .access_token
        .take()
        .ok_or_else(|| Error::NativeAuth(format!("{stage} returned no access token")))?;
    let scopes = answer
        .scope
        .as_deref()
        .unwrap_or_default()
        .split_ascii_whitespace()
        .map(|value| value.to_ascii_lowercase())
        .collect();
    Ok(TokenGrant {
        access_token: Zeroizing::new(access_token),
        refresh_token: answer.refresh_token.take().map(Zeroizing::new),
        scopes,
        expires_in: Duration::from_secs(answer.expires_in.unwrap_or(3600)),
    })
}

fn requested_scope(scope: &Scope) -> String {
    format!(
        "{}/{}",
        scope.resource.trim_end_matches('/'),
        scope.name.trim_start_matches('/')
    )
}

fn grants(granted: &[String], requested: &Scope) -> bool {
    let base = requested
        .resource
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let full = requested_scope(requested).to_ascii_lowercase();
    let short = requested.name.to_ascii_lowercase();
    granted.iter().any(|value| {
        let normalized = value.trim_end_matches('/');
        if requested.name == ".default" {
            normalized.starts_with(&format!("{base}/"))
        } else {
            covers(normalized, &full)
                || (base == "https://graph.microsoft.com" && covers(normalized, &short))
        }
    })
}

fn covers(granted: &str, requested: &str) -> bool {
    granted == requested || granted == format!("{requested}.all")
}

fn oauth_error(stage: &str, status: u16, code: &str) -> Error {
    match code {
        "interaction_required"
        | "login_required"
        | "consent_required"
        | "account_selection_required"
        | "invalid_grant" => Error::LoginRequired(format!(
            "the saved Microsoft session must be renewed ({code})"
        )),
        _ => Error::NativeAuth(format!("{stage} was refused with HTTP {status} ({code})")),
    }
}

fn load_refresh_token(path: &Path) -> Result<Option<Zeroizing<String>>> {
    let token = match std::fs::read_to_string(path) {
        Ok(token) => Zeroizing::new(token),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::NativeAuth(format!(
                "could not read the saved native credential: {error}"
            )));
        }
    };
    if token.is_empty() {
        return Err(Error::LoginRequired(
            "the saved native credential is empty and must be renewed".into(),
        ));
    }
    Ok(Some(token))
}

fn store_refresh_token(path: &Path, token: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            Error::NativeAuth(format!(
                "could not create the native credential directory: {error}"
            ))
        })?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path).map_err(|error| {
        Error::NativeAuth(format!(
            "could not open the native credential file: {error}"
        ))
    })?;
    #[cfg(unix)]
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|error| {
            Error::NativeAuth(format!(
                "could not protect the native credential file: {error}"
            ))
        })?;
    file.write_all(token.as_bytes()).map_err(|error| {
        Error::NativeAuth(format!(
            "could not save the native credential: {error}"
        ))
    })?;
    file.sync_all().map_err(|error| {
        Error::NativeAuth(format!(
            "could not flush the native credential file: {error}"
        ))
    })
}

/// A strict no-browser transport. It never opens, wakes, or supervises a web page.
pub struct NativeTransport {
    auth: Option<Arc<NativeAuth>>,
    startup_error: Option<String>,
    startup_login_required: bool,
}

impl NativeTransport {
    pub fn teams(refresh_token_file: impl Into<PathBuf>) -> Self {
        Self::new(NativeAuthConfig::teams(refresh_token_file))
    }

    pub fn new(config: NativeAuthConfig) -> Self {
        match NativeAuth::new(config) {
            Ok(auth) => NativeTransport {
                auth: Some(Arc::new(auth)),
                startup_error: None,
                startup_login_required: false,
            },
            Err(error) => {
                let startup_login_required = matches!(error, Error::LoginRequired(_));
                NativeTransport {
                    auth: None,
                    startup_error: Some(error.to_string()),
                    startup_login_required,
                }
            }
        }
    }

    pub fn failed(reason: impl Into<String>) -> Self {
        NativeTransport {
            auth: None,
            startup_error: Some(reason.into()),
            startup_login_required: false,
        }
    }
}

impl Transport for NativeTransport {
    fn check(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            match &self.auth {
                Some(auth) => auth.check().await,
                None => {
                    let reason = self
                        .startup_error
                        .clone()
                        .unwrap_or_else(|| "native authentication is unavailable".into());
                    if self.startup_login_required {
                        Err(Error::LoginRequired(reason))
                    } else {
                        Err(Error::NativeAuth(reason))
                    }
                }
            }
        })
    }

    fn open(&self, _app: App, _with_events: bool) -> BoxFuture<'_, Result<Option<OpenedTab>>> {
        Box::pin(async { Ok(None) })
    }

    fn wake(&self, _app: App) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn diagnose(&self) -> BoxFuture<'_, Result<Diagnosis>> {
        Box::pin(async { Ok(Diagnosis::default()) })
    }

    fn native_auth(&self) -> Option<Arc<NativeAuth>> {
        self.auth.clone()
    }

    fn supports_tabs(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_scope_names_like_msal() {
        assert_eq!(
            requested_scope(&Scope::new(IC3, "Teams.AccessAsUser.All")),
            "https://ic3.teams.office.com/Teams.AccessAsUser.All"
        );
        assert!(grants(
            &["https://ic3.teams.office.com/teams.accessasuser.all".into()],
            &Scope::new(IC3, "Teams.AccessAsUser.All")
        ));
        assert!(grants(&["user.read".into()], &Scope::graph("User.Read")));
    }

    #[test]
    fn saved_refresh_token_builds_native_auth() {
        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join("native-token");
        store_refresh_token(&token_file, "dummy-refresh-token").unwrap();
        let config = NativeAuthConfig::teams(&token_file);

        let auth = NativeAuth::new(config).unwrap();
        let state = auth.state.try_lock().unwrap();
        assert_eq!(state.refresh_token.as_str(), "dummy-refresh-token");
    }

    #[test]
    fn missing_refresh_token_requests_the_existing_login_flow() {
        let directory = tempfile::tempdir().unwrap();
        let config = NativeAuthConfig::teams(directory.path().join("missing-token"));

        assert!(matches!(
            NativeAuth::new(config),
            Err(Error::LoginRequired(_))
        ));
    }

    #[test]
    fn rotating_grant_replaces_the_saved_refresh_token() {
        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join("native-token");
        store_refresh_token(&token_file, "old-refresh-token").unwrap();
        let config = NativeAuthConfig::teams(&token_file);
        let auth = NativeAuth::new(config).unwrap();
        let scope = Scope::new(IC3, "Teams.AccessAsUser.All");
        let grant = TokenGrant {
            access_token: Zeroizing::new("dummy-access-token".to_owned()),
            refresh_token: Some(Zeroizing::new("new-refresh-token".to_owned())),
            scopes: vec![requested_scope(&scope).to_ascii_lowercase()],
            expires_in: Duration::from_secs(3600),
        };

        let mut state = auth.state.try_lock().unwrap();
        auth.install_grant(
            &mut state,
            TokenKey::new(&scope, None),
            &scope,
            grant,
            true,
        )
        .unwrap();
        drop(state);

        let saved = load_refresh_token(&token_file).unwrap().unwrap();
        assert_eq!(saved.as_str(), "new-refresh-token");
    }

    #[test]
    fn browser_credential_is_imported_and_saved() {
        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join("native-token");
        let config = NativeAuthConfig::teams(&token_file);

        let auth = NativeAuth::from_refresh_token(config, "browser-refresh-token".into()).unwrap();
        let state = auth.state.try_lock().unwrap();
        assert_eq!(state.refresh_token.as_str(), "browser-refresh-token");
        drop(state);
        let saved = load_refresh_token(&token_file).unwrap().unwrap();
        assert_eq!(saved.as_str(), "browser-refresh-token");
    }

    #[cfg(unix)]
    #[test]
    fn saved_refresh_token_is_private() {
        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join("native-token");
        store_refresh_token(&token_file, "dummy-refresh-token").unwrap();

        let mode = std::fs::metadata(token_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
