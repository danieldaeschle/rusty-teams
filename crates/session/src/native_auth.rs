use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use serde::Deserialize;
use tempfile::NamedTempFile;
use tokio::sync::Mutex;
use zeroize::{Zeroize, Zeroizing};

use crate::app::App;
use crate::error::{Error, Result};
use crate::scope::{IC3, Scope};
use crate::transport::{BoxFuture, Diagnosis, OpenedTab, Transport};

/// Public client used by Teams Web. It is an identifier, not a secret.
pub const TEAMS_WEB_CLIENT_ID: &str = "5e3ce6c0-2b1f-4285-8d4b-75ee78787346";

const TEAMS_ORIGIN: &str = "https://teams.cloud.microsoft";
const MICROSOFT_AUTHORITY: &str = "https://login.microsoftonline.com";
const USER_AGENT: &str = concat!("Rusty-Teams/", env!("CARGO_PKG_VERSION"));
const REQUEST_TIMEOUT: Duration = Duration::from_secs(25);
const ACCESS_MARGIN: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TokenKey {
    tenant: String,
    resource: String,
}

impl TokenKey {
    fn new(scope: &Scope, tenant: Option<&str>) -> Self {
        TokenKey {
            tenant: tenant.unwrap_or_default().to_ascii_lowercase(),
            resource: scope.resource.trim_end_matches('/').to_ascii_lowercase(),
        }
    }
}

struct HeldAccess {
    token: Zeroizing<String>,
    scopes: Vec<String>,
    usable_until: Instant,
}

struct AuthState {
    client: reqwest::Client,
    refresh_token: Option<Zeroizing<String>>,
    access: HashMap<TokenKey, HeldAccess>,
}

impl AuthState {
    fn load(path: &Path) -> Result<Self> {
        let refresh_token = load_refresh_token(path)?;
        crate::native::ensure_crypto_provider();
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .map_err(|_| Error::NativeAuth("could not create the OAuth HTTP client".into()))?;
        Ok(AuthState {
            client,
            refresh_token,
            access: HashMap::new(),
        })
    }

    fn install_grant(
        &mut self,
        path: &Path,
        key: TokenKey,
        requested: &Scope,
        grant: TokenGrant,
        signed_in: Option<Zeroizing<String>>,
    ) -> Result<String> {
        if !grants(&grant.scopes, requested) {
            return Err(Error::NativeAuth(format!(
                "the Teams web application is not granted {}",
                requested.name
            )));
        }
        if let Some(refresh_token) = grant.refresh_token.or(signed_in) {
            store_refresh_token(path, refresh_token.as_str())?;
            self.refresh_token = Some(refresh_token);
        }
        let token = grant.access_token.to_string();
        let usable_for = grant.expires_in.saturating_sub(ACCESS_MARGIN);
        let now = Instant::now();
        self.access.insert(
            key,
            HeldAccess {
                token: grant.access_token,
                scopes: grant.scopes,
                usable_until: now.checked_add(usable_for).unwrap_or(now),
            },
        );
        Ok(token)
    }
}

type InteractiveSignIn = dyn Fn() -> BoxFuture<'static, Result<String>> + Send + Sync;

/// Browserless transport shared by every session. The host opens a sign-in window on demand.
pub struct NativeAuth {
    refresh_token_file: Option<PathBuf>,
    authority: String,
    sign_in: Box<InteractiveSignIn>,
    state: Mutex<Option<AuthState>>,
}

impl NativeAuth {
    pub fn teams(
        refresh_token_file: Option<PathBuf>,
        sign_in: impl Fn() -> BoxFuture<'static, Result<String>> + Send + Sync + 'static,
    ) -> Self {
        NativeAuth {
            refresh_token_file,
            authority: MICROSOFT_AUTHORITY.to_owned(),
            sign_in: Box::new(sign_in),
            state: Mutex::new(None),
        }
    }

    async fn token(
        &self,
        scope: &Scope,
        tenant: Option<&str>,
        force_refresh: bool,
    ) -> Result<String> {
        if scope.nested_app.is_some() {
            return Err(Error::NativePageRequired);
        }
        let path = self.refresh_token_file.as_deref().ok_or_else(|| {
            Error::NativeAuth("could not locate the native credential directory".into())
        })?;
        let key = TokenKey::new(scope, tenant);
        // Serialize rotation and interactive renewal across home, guest and realtime sessions.
        let mut state = self.state.lock().await;
        let state = match state.as_mut() {
            Some(state) => state,
            None => state.insert(AuthState::load(path)?),
        };
        if force_refresh {
            state.access.remove(&key);
        }
        if let Some(held) = state.access.get(&key)
            && Instant::now() < held.usable_until
            && grants(&held.scopes, scope)
        {
            return Ok(held.token.to_string());
        }

        if let Some(credential) = &state.refresh_token {
            match self.refresh(&state.client, scope, tenant, credential).await {
                Ok(grant) => return state.install_grant(path, key, scope, grant, None),
                Err(Error::LoginRequired(_)) => {
                    state.refresh_token = None;
                    state.access.clear();
                }
                Err(error) => return Err(error),
            }
        }

        let credential = Zeroizing::new((self.sign_in)().await?);
        if credential.is_empty() {
            return Err(Error::LoginRequired(
                "the browser did not return a reusable Teams credential".into(),
            ));
        }
        let grant = self
            .refresh(&state.client, scope, tenant, &credential)
            .await?;
        state.install_grant(path, key, scope, grant, Some(credential))
    }

    async fn refresh(
        &self,
        client: &reqwest::Client,
        scope: &Scope,
        tenant: Option<&str>,
        refresh_token: &str,
    ) -> Result<TokenGrant> {
        let requested = format!("{}/.default", scope.resource.trim_end_matches('/'));
        let refresh_scope = format!("{requested} openid profile offline_access");
        let form = [
            ("client_id", TEAMS_WEB_CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", refresh_scope.as_str()),
        ];
        let authority = &self.authority;
        let tenant = tenant.unwrap_or("common");
        let response = client
            .post(format!("{authority}/{tenant}/oauth2/v2.0/token"))
            .header("Origin", TEAMS_ORIGIN)
            .form(&form)
            .send()
            .await
            .map_err(|_| Error::NativeAuth("OAuth refresh request failed".into()))?;
        token_grant(response, "OAuth refresh").await
    }
}

impl Transport for NativeAuth {
    fn check(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.token(&Scope::new(IC3, "Teams.AccessAsUser.All"), None, false)
                .await
                .map(drop)
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

    fn native_token<'a>(
        &'a self,
        scope: &'a Scope,
        tenant: Option<&'a str>,
        force_refresh: bool,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(self.token(scope, tenant, force_refresh))
    }

    fn supports_tabs(&self) -> bool {
        false
    }
}

struct TokenGrant {
    access_token: Zeroizing<String>,
    refresh_token: Option<Zeroizing<String>>,
    scopes: Vec<String>,
    expires_in: Duration,
}

#[derive(Deserialize)]
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
        .filter(|token| !token.is_empty())
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
        refresh_token: answer
            .refresh_token
            .take()
            .filter(|token| !token.is_empty())
            .map(Zeroizing::new),
        scopes,
        expires_in: Duration::from_secs(answer.expires_in.unwrap_or_default()),
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
    match fs::read_to_string(path) {
        Ok(token) if token.is_empty() => Ok(None),
        Ok(token) => Ok(Some(Zeroizing::new(token))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::NativeAuth(format!(
            "could not read the saved native credential: {error}"
        ))),
    }
}

fn store_refresh_token(path: &Path, token: &str) -> Result<()> {
    let save_error =
        |error| Error::NativeAuth(format!("could not save the native credential: {error}"));
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(save_error)?;
    // Unix temporary files are private (0600) and live on the same filesystem as the destination.
    let mut file = NamedTempFile::new_in(parent).map_err(save_error)?;
    #[cfg(unix)]
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(save_error)?;
    file.write_all(token.as_bytes()).map_err(save_error)?;
    file.as_file().sync_all().map_err(save_error)?;
    file.persist(path)
        .map_err(|error| save_error(error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::{Value, json};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    use super::*;
    use crate::{Session, SessionConfig};

    const TEAMS_SCOPE: &str = "https://ic3.teams.office.com/Teams.AccessAsUser.All";

    fn grant(token: &str, refresh_token: Option<&str>, expires_in: u64) -> Value {
        json!({
            "access_token": token,
            "refresh_token": refresh_token,
            "scope": TEAMS_SCOPE,
            "expires_in": expires_in,
        })
    }

    async fn oauth_server(
        replies: Vec<(&'static str, &'static str, &'static str, Value)>,
    ) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let authority = format!("http://{}", listener.local_addr().unwrap());
        let worker = tokio::spawn(async move {
            for (tenant, credential, status, body) in replies {
                let (socket, _) = listener.accept().await.unwrap();
                let mut socket = BufReader::new(socket);
                let mut line = String::new();
                socket.read_line(&mut line).await.unwrap();
                assert_eq!(
                    line,
                    format!("POST /{tenant}/oauth2/v2.0/token HTTP/1.1\r\n")
                );
                let mut content_length = None;
                loop {
                    line.clear();
                    assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':')
                        && name.eq_ignore_ascii_case("content-length")
                    {
                        content_length = Some(value.trim().parse().unwrap());
                    }
                }
                let mut form = vec![0; content_length.unwrap()];
                socket.read_exact(&mut form).await.unwrap();
                assert!(
                    url::form_urlencoded::parse(&form)
                        .any(|(key, value)| key == "refresh_token" && value == credential)
                );
                let body = body.to_string();
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (authority, worker)
    }

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

    #[tokio::test]
    async fn existing_sessions_share_a_single_interactive_renewal() {
        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join("native-token");
        store_refresh_token(&token_file, "saved-refresh").unwrap();
        let (authority, server) = oauth_server(vec![
            (
                "common",
                "saved-refresh",
                "200 OK",
                grant("home-before", None, 3600),
            ),
            (
                "guest",
                "saved-refresh",
                "200 OK",
                grant("guest-before", None, 3600),
            ),
            (
                "common",
                "saved-refresh",
                "400 Bad Request",
                json!({"error": "invalid_grant"}),
            ),
            (
                "common",
                "interactive-refresh",
                "200 OK",
                grant("home-after", Some("rotated-refresh"), 3600),
            ),
            (
                "guest",
                "rotated-refresh",
                "200 OK",
                grant("guest-after", None, 3600),
            ),
        ])
        .await;
        let sign_ins = Arc::new(AtomicUsize::new(0));
        let mut auth = NativeAuth::teams(Some(token_file.clone()), {
            let sign_ins = sign_ins.clone();
            move || {
                sign_ins.fetch_add(1, Ordering::Relaxed);
                Box::pin(async { Ok("interactive-refresh".to_owned()) })
            }
        });
        auth.authority = authority;
        let session = Session::with_transport(Arc::new(auth), SessionConfig::default())
            .await
            .unwrap();
        let scope = Scope::new(IC3, "Teams.AccessAsUser.All");
        let guest = session.in_tenant("GUEST");
        assert_eq!(
            guest.access_token(App::Teams, &scope, false).await.unwrap(),
            "guest-before"
        );
        assert_eq!(sign_ins.load(Ordering::Relaxed), 0);

        let (home_token, guest_token) = tokio::join!(
            session.access_token(App::Teams, &scope, true),
            guest.access_token(App::Teams, &scope, false),
        );
        assert_eq!(home_token.unwrap(), "home-after");
        assert_eq!(guest_token.unwrap(), "guest-after");
        assert_eq!(sign_ins.load(Ordering::Relaxed), 1);
        assert_eq!(fs::read_to_string(&token_file).unwrap(), "rotated-refresh");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn opaque_access_token_expires_according_to_the_grant() {
        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join("native-token");
        let (authority, server) = oauth_server(vec![
            (
                "common",
                "first-refresh",
                "200 OK",
                grant("short-lived", None, 1),
            ),
            (
                "common",
                "first-refresh",
                "200 OK",
                grant("opaque-access", None, 3600),
            ),
        ])
        .await;
        let mut auth = NativeAuth::teams(Some(token_file.clone()), || {
            Box::pin(async { Ok("first-refresh".to_owned()) })
        });
        auth.authority = authority;
        let session = Session::with_transport(Arc::new(auth), SessionConfig::default())
            .await
            .unwrap();
        let http = crate::native::NativeHttp::new(Duration::from_secs(1), 1024).unwrap();
        let scope = Scope::new(IC3, "Teams.AccessAsUser.All");
        for _ in 0..2 {
            let credential = http.token(&session, &scope, false).await.unwrap().unwrap();
            assert_eq!(credential, (App::Teams, "opaque-access".to_owned()));
        }
        assert_eq!(fs::read_to_string(&token_file).unwrap(), "first-refresh");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejected_sign_in_does_not_overwrite_the_saved_credential() {
        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join("native-token");
        store_refresh_token(&token_file, "saved-refresh").unwrap();
        let (authority, server) = oauth_server(vec![
            (
                "common",
                "saved-refresh",
                "400 Bad Request",
                json!({"error": "invalid_grant"}),
            ),
            (
                "common",
                "unusable-refresh",
                "400 Bad Request",
                json!({"error": "invalid_grant"}),
            ),
        ])
        .await;
        let mut auth = NativeAuth::teams(Some(token_file.clone()), || {
            Box::pin(async { Ok("unusable-refresh".to_owned()) })
        });
        auth.authority = authority;
        let result = Session::with_transport(Arc::new(auth), SessionConfig::default()).await;
        assert!(matches!(result, Err(Error::LoginRequired(_))));
        assert_eq!(fs::read_to_string(&token_file).unwrap(), "saved-refresh");
        server.await.unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rotation_replaces_the_private_file_without_truncating_the_old_inode() {
        use std::io::Read as _;
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join("native-token");
        store_refresh_token(&token_file, "old-refresh").unwrap();
        let mut old_file = fs::File::open(&token_file).unwrap();
        store_refresh_token(&token_file, "new-refresh").unwrap();
        let mut old_token = String::new();
        old_file.read_to_string(&mut old_token).unwrap();
        assert_eq!(old_token, "old-refresh");
        assert_eq!(fs::read_to_string(&token_file).unwrap(), "new-refresh");
        let mode = fs::metadata(token_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
