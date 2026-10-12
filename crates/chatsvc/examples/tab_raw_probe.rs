use serde_json::Value;
use session::{DEFAULT_ENDPOINT, Request, Scope, Session};

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let needle = std::env::args().nth(1).unwrap_or_else(|| "Lunch".to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let url = "https://teams.cloud.microsoft/api/csa/emea/api/v1/teams/users/me?isPrefetch=false&enableMembershipSummary=true";
    let scope = Scope::new("https://chatsvcagg.teams.microsoft.com", "user_impersonation");
    let answer = session.batch(&[Request::get(url)], &scope).await.expect("csa").remove(0);
    println!("# status {}", answer.status);
    for team in answer.body.get("teams").and_then(Value::as_array).into_iter().flatten() {
        for channel in team.get("channels").and_then(Value::as_array).into_iter().flatten() {
            let name = channel.get("displayName").and_then(Value::as_str).unwrap_or_default();
            if !name.contains(&needle) {
                continue;
            }
            println!("channel {name}");
            for tab in channel.get("tabs").and_then(Value::as_array).into_iter().flatten() {
                let mut shown = tab.clone();
                if let Some(object) = shown.as_object_mut() {
                    object.retain(|key, _| key != "replyChainId");
                }
                println!("  {}", serde_json::to_string(&shown).unwrap_or_default().chars().take(1500).collect::<String>());
            }
        }
    }
}
