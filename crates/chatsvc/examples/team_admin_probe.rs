use serde_json::{Value, json};
use session::{DEFAULT_ENDPOINT, Method, Request, SPACES, Scope, Session};

const CSA_RESOURCE: &str = "https://chatsvcagg.teams.microsoft.com";
const MIDDLE_TIER: &str = "https://teams.cloud.microsoft/api/mt/emea/beta";
const CSA: &str = "https://teams.cloud.microsoft/api/csa/emea/api/v1";
const PROVISIONING: &str = "https://teams.cloud.microsoft/fabric/emea/templates/api";
const PROVISIONING_RESOURCE: &str = "https://templates.teams.microsoft.com";
const GRAPH: &str = "https://graph.microsoft.com";

#[tokio::main]
async fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    if let ["lib-create", team_thread_id, group_id, name, privacy, object_id, tenant_id] =
        arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice()
    {
        let channel = chatsvc::NewChannel {
            team_thread_id,
            group_id,
            name,
            description: "",
            privacy: if *privacy == "private" {
                chatsvc::ChannelPrivacy::Private
            } else {
                chatsvc::ChannelPrivacy::Standard
            },
        };
        let mailbox = chatsvc::Mailbox { object_id, tenant_id };
        let outcome = chatsvc::TeamAdmin::new(&session)
            .create_channel(&channel, mailbox)
            .await;
        println!("{outcome:?}");
        return;
    }
    let (method, path, body) = match arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["create-team", name] => (
            Method::Post,
            "teams/create".to_owned(),
            Some(json!({"displayName": name, "description": "", "accessType": "Private"})),
        ),
        ["create-channel", team_thread_id, group_id, name, channel_type] => (
            Method::Post,
            format!("teams/{team_thread_id}/channels"),
            Some(json!({"displayName": name, "description": "", "groupId": group_id, "channelType": channel_type})),
        ),
        ["delete-channel", team_thread_id, channel_id] => (
            Method::Delete,
            format!("teams/{team_thread_id}/channels/{channel_id}"),
            Some(json!({})),
        ),
        ["delete-team", group_id] => (Method::Delete, format!("teams/{group_id}/delete"), None),
        ["get", path] => (Method::Get, (*path).to_owned(), None),
        ["csa-get", path] => (Method::Get, format!("csa:{path}"), None),
        ["graph-get", path] => (Method::Get, format!("graph:{path}"), None),
        ["create-private", group_id, team_thread_id, name] => (
            Method::Post,
            format!("provisioning:teams/{group_id}/channels?forceSync=false"),
            Some(json!({"spaceThreadId": team_thread_id, "displayName": name, "description": "", "channelType": "Private", "sharedWithParentTeam": false})),
        ),
        ["send", method, path, body] => (
            match *method {
                "PATCH" => Method::Patch,
                "DELETE" => Method::Delete,
                "PUT" => Method::Put,
                _ => Method::Post,
            },
            (*path).to_owned(),
            Some(serde_json::from_str(body).expect("json body")),
        ),
        ["provisioning-get", path] => (Method::Get, format!("provisioning:{path}"), None),
        _ => panic!("usage: create-team NAME | create-channel TEAM_THREAD GROUP NAME | delete-channel TEAM_THREAD CHANNEL | delete-team GROUP | get PATH"),
    };
    let (url, scope) = if let Some(path) = path.strip_prefix("csa:") {
        (format!("{CSA}/{path}"), Scope::new(CSA_RESOURCE, "user_impersonation"))
    } else if let Some(path) = path.strip_prefix("graph:") {
        (format!("{GRAPH}/v1.0/{path}"), Scope::new(GRAPH, ".default"))
    } else if let Some(path) = path.strip_prefix("provisioning:") {
        (format!("{PROVISIONING}/{path}"), Scope::new(PROVISIONING_RESOURCE, ".default"))
    } else {
        (format!("{MIDDLE_TIER}/{path}"), Scope::new(SPACES, "user_impersonation"))
    };
    let mut request = match body {
        Some(body) => Request::with_body(method, url, body),
        None => Request { method, ..Request::get(url) },
    };
    request.headers = vec![
        ("x-ms-client-type".to_owned(), "TeamsV2Web".to_owned()),
        ("Accept".to_owned(), "application/json".to_owned()),
    ];
    if let Ok(anchor) = std::env::var("ANCHOR_MAILBOX") {
        request.headers.push(("X-AnchorMailbox".to_owned(), anchor));
    }
    let answers = session
        .batch(&[request], &scope)
        .await
        .expect("request");
    let answer = &answers[0];
    println!("HTTP {}", answer.status);
    let body: &Value = &answer.body;
    println!("{}", serde_json::to_string_pretty(body).unwrap_or_default());
}
