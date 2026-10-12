use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::middleware::from_fn_with_state;
use axum::routing::get;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::auth::{AuthState, MAX_BODY_BYTES, healthz, require_auth};
use crate::hub::AgentHub;
use crate::source::TeamsSource;
use crate::tools::TeamsTools;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("port {port} is already in use, choose another port in rusty-teams Settings > MCP")]
    PortInUse { port: u16 },
    #[error("cannot start the MCP server on port {port}: {source}")]
    Bind { port: u16, source: std::io::Error },
}

pub struct ServerHandle {
    pub addr: SocketAddr,
    shutdown: CancellationToken,
    task: JoinHandle<()>,
}

impl ServerHandle {
    pub async fn shutdown(self) {
        self.shutdown.cancel();
        let _ = self.task.await;
    }
}

pub async fn start(
    hub: Arc<AgentHub>,
    source: Arc<dyn TeamsSource>,
    port: u16,
    token: String,
) -> Result<ServerHandle, ServerError> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::AddrInUse => ServerError::PortInUse { port },
            _ => ServerError::Bind { port, source },
        })?;
    let addr = listener
        .local_addr()
        .map_err(|source| ServerError::Bind { port, source })?;
    let auth = AuthState {
        port: addr.port(),
        token,
    };
    let shutdown = CancellationToken::new();
    let mut config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(auth.allowed_hosts())
        .with_allowed_origins(auth.allowed_origins());
    config.cancellation_token = shutdown.child_token();
    config.max_request_body_bytes = MAX_BODY_BYTES;
    let service: StreamableHttpService<TeamsTools, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(TeamsTools::new(hub.clone(), source.clone())),
            Default::default(),
            config,
        );
    let router = Router::new()
        .nest_service("/mcp", service)
        .layer(from_fn_with_state(auth, require_auth))
        .route("/healthz", get(healthz));
    let graceful = shutdown.clone();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move { graceful.cancelled_owned().await })
            .await;
    });
    Ok(ServerHandle {
        addr,
        shutdown,
        task,
    })
}
