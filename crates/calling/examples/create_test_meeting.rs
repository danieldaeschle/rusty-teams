//! Creates a "Native client test" Meet now meeting and prints its join link (call end-to-end test).
use serde_json::{Value, json};
use session::{DEFAULT_ENDPOINT, Method, SPACES, Scope, Session};

const CREATE_URL: &str =
    "https://teams.cloud.microsoft/api/mt/emea/beta/me/calendarEvents/privateMeeting/schedulingService/create";
const SUBJECT: &str = "Native client test";
const LINK_KEYS: [&str; 4] = ["meetingUrl", "joinUrl", "meetingJoinUrl", "joinWebUrl"];

fn find_text<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    match value {
        Value::Object(map) => map.iter().find_map(|(key, inner)| {
            let direct = keys
                .iter()
                .any(|wanted| key.eq_ignore_ascii_case(wanted))
                .then(|| inner.as_str().filter(|text| !text.is_empty()))
                .flatten();
            direct.or_else(|| find_text(inner, keys))
        }),
        Value::Array(items) => items.iter().find_map(|item| find_text(item, keys)),
        _ => None,
    }
}

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let body = json!({"meetingType": "MeetNow", "isStreamEnabled": false, "subject": SUBJECT, "unhideChatThread": true});
    let created = session
        .request(Method::Post, CREATE_URL, &Scope::new(SPACES, "user_impersonation"), Some(body))
        .await
        .expect("create meeting");
    match find_text(&created.body, &LINK_KEYS) {
        Some(link) => println!("{link}"),
        None => {
            eprintln!("HTTP {} without a join link: {:#}", created.status, created.body);
            std::process::exit(1);
        }
    }
}
