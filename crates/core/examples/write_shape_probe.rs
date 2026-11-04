use serde_json::Value;
use session::{DEFAULT_ENDPOINT, IC3, Request, Scope, Session};

const NOTES_URL: &str = "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations/48%3Anotes/messages?view=msnp24Equivalent&pageSize=200";

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let scope = Scope::new(IC3, "Teams.AccessAsUser.All");
    let answer = session
        .batch(&[Request::get(NOTES_URL)], &scope)
        .await
        .expect("chatsvc")
        .remove(0);
    println!("# GET status {}", answer.status);
    for message in answer.body["messages"].as_array().into_iter().flatten() {
        let messagetype = message["messagetype"].as_str().unwrap_or("");
        if !messagetype.starts_with("RichText") && messagetype != "Text" {
            continue;
        }
        let properties = message["properties"].as_object();
        let keys: Vec<String> = properties
            .into_iter()
            .flatten()
            .map(|(key, value)| format!("{key}={}", shape(key, value)))
            .collect();
        println!(
            "- type={messagetype} contenttype={} clientmessageid_len={} ams={} imdisplayname={} props=[{}]",
            message["contenttype"],
            message["clientmessageid"]
                .as_str()
                .map(str::len)
                .unwrap_or(0),
            message["amsreferences"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0),
            message["imdisplayname"].is_string(),
            keys.join(", ")
        );
        println!(
            "  skeleton: {}",
            skeleton(message["content"].as_str().unwrap_or(""))
        );
    }
}

fn shape(key: &str, value: &Value) -> String {
    match value {
        Value::String(text) if key == "formatVariant" || key == "importance" => format!("{text:?}"),
        Value::String(text) if text.starts_with('[') => {
            let parsed: Value = serde_json::from_str(text).unwrap_or(Value::Null);
            let entries = parsed.as_array().cloned().unwrap_or_default();
            let fields: Vec<String> = entries
                .first()
                .and_then(Value::as_object)
                .map(|object| {
                    object
                        .iter()
                        .map(|(field, field_value)| match field.as_str() {
                            "@type" | "mentionType" | "fileType" | "type" => {
                                format!("{field}:{field_value}")
                            }
                            "itemid" => format!(
                                "itemid:{}",
                                if field_value.is_string() {
                                    "str"
                                } else {
                                    "num"
                                }
                            ),
                            _ => field.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            format!("str[{}]{{{}}}", entries.len(), fields.join(" "))
        }
        Value::String(text) => format!("str({})", text.len()),
        Value::Null => "null".into(),
        other => format!("{}", other.to_string().len()),
    }
}

fn skeleton(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        let tag = &rest[start + 1..start + end];
        let name: String = tag.split_whitespace().next().unwrap_or("").to_owned();
        let itemtype = tag
            .split("itemtype=\"")
            .nth(1)
            .and_then(|value| value.split('"').next())
            .map(|value| format!(" itemtype={value}"))
            .unwrap_or_default();
        let attributes: Vec<&str> = tag
            .split_whitespace()
            .skip(1)
            .filter_map(|attribute| attribute.split('=').next())
            .filter(|attribute| *attribute != "itemtype")
            .collect();
        out.push_str(&format!("<{name}{itemtype} [{}]>", attributes.join(",")));
        rest = &rest[start + end + 1..];
    }
    out
}
