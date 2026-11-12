use chatsvc::compose_copilot::{Adjustment, ComposeAction, ComposeCopilot, ComposeRewrite};
use session::{DEFAULT_ENDPOINT, Method, Scope, Session};

const DRAFT: &str = "<p>hallo zusammen, wollte kurz fragen ob das treffen morgen noch steht oder ob wir es verschieben</p>";

#[tokio::main]
async fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let action = match arguments.first().map(String::as_str) {
        Some("proofread") => ComposeAction::Proofread,
        Some("custom") => ComposeAction::Custom(arguments[1..].join(" ")),
        Some(label) => Adjustment::ALL
            .into_iter()
            .find(|adjustment| adjustment.label().eq_ignore_ascii_case(label))
            .map_or(ComposeAction::Rewrite, ComposeAction::Adjust),
        None => ComposeAction::Rewrite,
    };
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let organization = session
        .request(
            Method::Get,
            "https://graph.microsoft.com/v1.0/organization?$select=id",
            &Scope::graph("User.Read"),
            None,
        )
        .await
        .expect("organization");
    let rewrite = ComposeRewrite {
        tenant_id: organization.body["value"][0]["id"]
            .as_str()
            .expect("tenant id")
            .to_owned(),
        thread_id: "48:notes".to_owned(),
        user_html: DRAFT.to_owned(),
        action,
        ui_language: "en-gb".to_owned(),
        time_zone: "Europe/Berlin".to_owned(),
    };
    match ComposeCopilot::new(&session).rewrite(&rewrite).await {
        Ok(answer) => println!("answer: {answer}"),
        Err(error) => println!("error: {error}"),
    }
}
