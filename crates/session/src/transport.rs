use std::future::Future;
use std::pin::Pin;

use serde_json::json;

use crate::app::{App, find_app_tab, has_login_tab};
use crate::error::Result;
use crate::events::{TabControl, TabEvents, open};
use crate::http::list_targets;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type OpenedTab = (TabControl, Option<TabEvents>);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Diagnosis {
    pub login_pending: bool,
    pub app_tab_open: bool,
}

/// Where the signed-in Teams and Outlook web apps live: a debug port or an embedded webview.
pub trait Transport: Send + Sync {
    fn check(&self) -> BoxFuture<'_, Result<()>>;

    /// `None` when the app has no tab. Without `with_events` the tab's protocol events are dropped.
    fn open(&self, app: App, with_events: bool) -> BoxFuture<'_, Result<Option<OpenedTab>>>;

    /// Reloads the app so it fetches tokens again.
    fn wake(&self, app: App) -> BoxFuture<'_, Result<()>>;

    fn diagnose(&self) -> BoxFuture<'_, Result<Diagnosis>>;
}

pub struct CdpTransport {
    endpoint: String,
}

impl CdpTransport {
    pub fn new(endpoint: &str) -> Self {
        CdpTransport {
            endpoint: endpoint.trim_end_matches('/').to_owned(),
        }
    }
}

impl Transport for CdpTransport {
    fn check(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { list_targets(&self.endpoint).await.map(drop) })
    }

    fn open(&self, app: App, with_events: bool) -> BoxFuture<'_, Result<Option<OpenedTab>>> {
        Box::pin(async move {
            let targets = list_targets(&self.endpoint).await?;
            let Some(websocket_url) = find_app_tab(&targets, app).and_then(|target| target.websocket_url.clone()) else {
                return Ok(None);
            };
            open(&websocket_url, with_events).await.map(Some)
        })
    }

    fn wake(&self, app: App) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let targets = list_targets(&self.endpoint).await?;
            let Some(target) = find_app_tab(&targets, app) else {
                return Ok(());
            };
            let parked = app.is_park_url(&target.url);
            let (control, _) = open(target.websocket_url.as_deref().unwrap_or_default(), false).await?;
            if parked {
                control.call("Page.navigate", json!({"url": app.start_url()})).await?;
            } else {
                control.call("Page.reload", json!({"ignoreCache": false})).await?;
            }
            Ok(())
        })
    }

    fn diagnose(&self) -> BoxFuture<'_, Result<Diagnosis>> {
        Box::pin(async move {
            let targets = list_targets(&self.endpoint).await?;
            Ok(Diagnosis {
                login_pending: has_login_tab(&targets),
                app_tab_open: App::ALL.iter().any(|&app| find_app_tab(&targets, app).is_some()),
            })
        })
    }
}
