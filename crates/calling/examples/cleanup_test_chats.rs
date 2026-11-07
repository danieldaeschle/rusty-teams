use serde_json::{Value, json};
use session::{DEFAULT_ENDPOINT, Method, Scope, Session};

const SUBJECT: &str = "Native client test";
const GRAPH: &str = "https://graph.microsoft.com/v1.0";

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let scope = Scope::graph("Chat.ReadWrite");
    let me = session
        .request(Method::Get, &format!("{GRAPH}/me?$select=id"), &scope, None)
        .await
        .expect("me");
    let organization = session
        .request(Method::Get, &format!("{GRAPH}/organization?$select=id"), &scope, None)
        .await
        .expect("organization");
    let user = json!({
        "id": me.body.get("id").and_then(Value::as_str).unwrap_or_default(),
        "tenantId": organization.body.pointer("/value/0/id").and_then(Value::as_str).unwrap_or_default(),
    });
    let mut next = Some(format!(
        "{GRAPH}/me/chats?$select=id,topic&$filter=topic eq '{SUBJECT}'&$top=50"
    ));
    let mut hidden = 0;
    while let Some(url) = next.take() {
        let page = session.request(Method::Get, &url, &scope, None).await.expect("list");
        let chats = page.body.get("value").and_then(Value::as_array).cloned().unwrap_or_default();
        for chat in chats {
            if chat.get("topic").and_then(Value::as_str) != Some(SUBJECT) {
                continue;
            }
            let id = chat.get("id").and_then(Value::as_str).unwrap_or_default();
            let hide_url = format!("{GRAPH}/chats/{id}/hideForUser");
            let answer = session
                .request(Method::Post, &hide_url, &scope, Some(json!({ "user": user })))
                .await
                .expect("hide");
            println!("# hide test meeting chat: HTTP {}", answer.status);
            hidden += 1;
        }
        next = page.body.get("@odata.nextLink").and_then(Value::as_str).map(str::to_owned);
    }
    println!("# hid {hidden} test meeting chat(s)");
}
