use serde_json::{Value, json};

use crate::app::App;
use crate::error::{Error, Result};
use crate::events::{TabControl, TabEvents};
use crate::request::Outcome;
use crate::scope::Scope;
use crate::session::{FETCH_SCRIPT, NOT_GRANTED_PREFIX, Session};

const TOKEN_MARKER: &str = "if (!token) return {noToken: true, refreshError};";

impl Session {
    /// Opens a dedicated CDP connection to the app tab. Events of the tab arrive on the stream.
    pub async fn subscribe(&self, app: App) -> Result<(TabControl, TabEvents)> {
        let (control, events) = self.transport.open(app, true).await?.ok_or(Error::NoAppTab)?;
        Ok((control, events.expect("opened with events")))
    }

    /// Runs `body` inside the app tab after the token lookup of the fetch script.
    /// `body` is an async function body that sees `token` and `args`; the token never leaves the page.
    pub async fn run_with_token(
        &self,
        app: App,
        scope: &Scope,
        body: &str,
        args: &Value,
        force_refresh: bool,
    ) -> Result<Value> {
        let marker_at = FETCH_SCRIPT
            .find(TOKEN_MARKER)
            .expect("fetch.js contains the token marker");
        let payload = json!({
            "requests": [],
            "resource": scope.resource,
            "scope": scope.name,
            "marginMs": self.config.refresh_margin.as_millis() as u64,
            "concurrency": 1,
            "maxBytes": 1,
            "forceRefresh": force_refresh,
        });
        let script = format!(
            "{head}{TOKEN_MARKER}\nconst args = {args};\n{body}\n}})",
            head = &FETCH_SCRIPT[..marker_at]
        );
        let expression = format!("({script})({payload})");
        let outcome = self
            .evaluate_in(app, &expression)
            .await?
            .ok_or(Error::NoAppTab)?;
        if outcome.get("noToken").and_then(Value::as_bool) == Some(true) {
            let outcome: Outcome = serde_json::from_value(outcome)
                .map_err(|error| Error::Cdp(format!("unexpected page answer: {error}")))?;
            let reason = outcome.refresh_error.unwrap_or_default();
            if reason.starts_with(NOT_GRANTED_PREFIX) {
                return Err(Error::LoginRequired(format!("{app} app is not granted {scope}")));
            }
            let detail = if reason.is_empty() {
                String::new()
            } else {
                format!(" (token refresh failed in {app}: {reason})")
            };
            return Err(Error::NoFreshToken {
                scope: scope.to_string(),
                detail,
            });
        }
        Ok(outcome)
    }
}
