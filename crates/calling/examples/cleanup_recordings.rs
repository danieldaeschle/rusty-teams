use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use session::{DEFAULT_ENDPOINT, Method, Scope, Session};

const LIST_URL: &str = "https://graph.microsoft.com/v1.0/me/drive/root:/Videos/Recordings:/children?$select=id,name,createdDateTime&$top=50";
const SUBJECT: &str = "Native client test";

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let scope = Scope::graph("Files.ReadWrite.All");
    let listed = session.request(Method::Get, LIST_URL, &scope, None).await.expect("list");
    println!("# list: HTTP {}", listed.status);
    let cutoff = Utc::now() - Duration::minutes(60);
    let items = listed.body.get("value").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut deleted = 0;
    for item in items {
        let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
        let created = item
            .get("createdDateTime")
            .and_then(Value::as_str)
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|time| time.with_timezone(&Utc));
        let recent = created.is_some_and(|time| time > cutoff);
        if recent {
            println!("# recent recording, starts with test subject: {}", name.starts_with(SUBJECT));
        }
        if recent && name.starts_with(SUBJECT) {
            let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
            let url = format!("https://graph.microsoft.com/v1.0/me/drive/items/{id}");
            let answer = session.request(Method::Delete, &url, &scope, None).await.expect("delete");
            println!("# delete recent test recording: HTTP {}", answer.status);
            deleted += 1;
        }
    }
    println!("# deleted {deleted} test recording(s)");
}
