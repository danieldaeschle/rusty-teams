mod event;
mod host;
mod runner;

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::Stream;
use session::Session;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

pub use event::{
    EventKind, MessageEvent, PresenceUpdate, RealtimeEvent, StatusEvent, StatusKind,
    TrouterEndpoint, decode_payload,
};
pub use host::{DEFAULT_TROUTER_HOST, is_trouter_host};

use crate::error::Result;
use runner::{Runner, attach};

const STOP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct RealtimeConfig {
    pub host: Option<String>,
    pub default_host: String,
    pub discover_host: bool,
    pub ensure_interval: Duration,
    pub navigation_settle: Duration,
    pub max_reattach_backoff: Duration,
}

impl Default for RealtimeConfig {
    fn default() -> Self {
        RealtimeConfig {
            host: None,
            default_host: DEFAULT_TROUTER_HOST.to_owned(),
            discover_host: true,
            ensure_interval: Duration::from_secs(30),
            navigation_settle: Duration::from_millis(1500),
            max_reattach_backoff: Duration::from_secs(30),
        }
    }
}

pub struct Realtime {
    host: String,
    events: mpsc::UnboundedReceiver<RealtimeEvent>,
    stop: Option<oneshot::Sender<oneshot::Sender<()>>>,
    task: JoinHandle<()>,
}

impl Realtime {
    /// Works on a parked tab (same origin, localStorage readable); the CDP attachment keeps it from being parked.
    pub async fn start(session: &Session) -> Result<Realtime> {
        Self::start_with(session, RealtimeConfig::default()).await
    }

    pub async fn start_with(session: &Session, config: RealtimeConfig) -> Result<Realtime> {
        let session = session.clone();
        let host = host::resolve(&session, &config).await;
        let attached = attach(&session).await?;
        let (output, events) = mpsc::unbounded_channel();
        let (stop, stop_receiver) = oneshot::channel();
        let mut runner = Runner::new(session, config, host.clone(), output);
        if let Err(error) = runner.ensure().await {
            attached.shutdown().await;
            return Err(error.into());
        }
        let task = tokio::spawn(runner.run(attached, stop_receiver));
        Ok(Realtime {
            host,
            events,
            stop: Some(stop),
            task,
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub async fn recv(&mut self) -> Option<RealtimeEvent> {
        self.events.recv().await
    }

    /// Deletes the registration and closes the page-side socket.
    pub async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let (done, finished) = oneshot::channel();
            if stop.send(done).is_ok() {
                let _ = tokio::time::timeout(STOP_TIMEOUT, finished).await;
            }
        }
        let _ = self.task.await;
    }
}

impl Stream for Realtime {
    type Item = RealtimeEvent;

    fn poll_next(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<RealtimeEvent>> {
        self.events.poll_recv(context)
    }
}
