//! Read-only: the same GETs once through the page and once from Rust with the page's token (no cookies). Prints status codes only.
use serde_json::json;
use session::{App, DEFAULT_ENDPOINT, GRAPH, IC3, Request, SPACES, Scope, Session, SessionConfig};

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let page = Session::connect(&endpoint).await.expect("browser");
    let native = Session::connect_with(
        &endpoint,
        SessionConfig {
            native_http: true,
            ..SessionConfig::default()
        },
    )
    .await
    .expect("browser");
    let client = reqwest::Client::new();
    let checks = [
        (
            "graph me",
            "https://graph.microsoft.com/v1.0/me?$select=id",
            Scope::new(GRAPH, "User.Read"),
        ),
        (
            "chatsvc conversations",
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations?view=msnp24Equivalent&pageSize=1",
            Scope::new(IC3, "Teams.AccessAsUser.All"),
        ),
        (
            "mt calendar",
            "https://teams.cloud.microsoft/api/mt/emea/v2.1/me/calendars/calendarView?startDate=2026-11-10T00:00:00Z&endDate=2026-11-11T00:00:00Z",
            Scope::new(SPACES, "user_impersonation"),
        ),
    ];
    for (label, url, scope) in checks {
        let page_status = page
            .send(Request::get(url), &scope)
            .await
            .map(|response| response.status)
            .unwrap_or_else(|error| match error {
                session::Error::Api { status, .. } => status,
                _ => 0,
            });
        let session_status = native
            .send(Request::get(url), &scope)
            .await
            .map(|response| response.status)
            .unwrap_or_else(|error| match error {
                session::Error::Api { status, .. } => status,
                _ => 0,
            });
        let raw_status = match page
            .run_with_token(App::Teams, &scope, "return token;", &json!({}), false)
            .await
        {
            Ok(serde_json::Value::String(token)) => client
                .get(url)
                .bearer_auth(token)
                .header("Origin", "https://teams.cloud.microsoft")
                .send()
                .await
                .map(|response| response.status().as_u16())
                .unwrap_or(0),
            _ => 0,
        };
        println!(
            "{label}: page {page_status}, session native {session_status}, rust without cookies {raw_status}"
        );
    }
}
