use std::time::Duration;

use serde_json::{Value, json};

use crate::app::App;
use crate::error::{Error, Result};
use crate::events::{TabControl, TabEvents};
use crate::request::Outcome;
use crate::scope::Scope;
use crate::session::{FETCH_SCRIPT, NOT_GRANTED_PREFIX, Session};

const LOADING_DOCUMENT_DENIAL: &str = "Access is denied for this document";
const LOADING_DOCUMENT_WAIT: Duration = Duration::from_secs(1);
const TOKEN_MARKER: &str = "if (!token) return {noToken: true, refreshError, page};";
const ACQUIRE_CALL: &str = "const {token, refreshError} = await acquire(resource, scope);";

/// A JS function expression: `(factory)({marginMs, forceRefresh})` resolves to `acquire(resource, scope)`, which answers `{token, refreshError}`.
pub fn token_acquirer_script() -> String {
    let call_at = FETCH_SCRIPT
        .find(ACQUIRE_CALL)
        .expect("fetch.js contains the acquire call");
    format!("{}return acquire;}})", &FETCH_SCRIPT[..call_at])
}

impl Session {
    /// Opens a dedicated CDP connection to the app tab. Events of the tab arrive on the stream.
    pub async fn subscribe(&self, app: App) -> Result<(TabControl, TabEvents)> {
        let (control, events) = self
            .transport
            .open(app, true)
            .await?
            .ok_or(Error::NoAppTab)?;
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
        // Right after launch the app tab can still hold a document without storage access; it settles within a second.
        let outcome = match self.evaluate_in(app, &expression).await {
            Err(Error::Cdp(text)) if text.contains(LOADING_DOCUMENT_DENIAL) => {
                tokio::time::sleep(LOADING_DOCUMENT_WAIT).await;
                self.evaluate_in(app, &expression).await
            }
            outcome => outcome,
        }?
        .ok_or(Error::NoAppTab)?;
        if outcome.get("noToken").and_then(Value::as_bool) == Some(true) {
            let outcome: Outcome = serde_json::from_value(outcome)
                .map_err(|error| Error::Cdp(format!("unexpected page answer: {error}")))?;
            let reason = outcome.refresh_error.unwrap_or_default();
            if reason.starts_with(NOT_GRANTED_PREFIX) {
                return Err(Error::LoginRequired(format!(
                    "{app} app is not granted {scope}"
                )));
            }
            let mut detail = if reason.is_empty() {
                String::new()
            } else {
                format!(" (token refresh failed in {app}: {reason})")
            };
            if let Some(page) = outcome.page {
                detail.push_str(&format!(" [{app}: {page}]"));
            }
            return Err(Error::NoFreshToken {
                scope: scope.to_string(),
                detail,
            });
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquirer_script_returns_the_acquire_function_before_any_request_runs() {
        let script = token_acquirer_script();
        assert!(script.starts_with("(async ({"));
        assert!(script.ends_with("return acquire;})"));
        assert!(!script.contains("const run = "));
    }
}
