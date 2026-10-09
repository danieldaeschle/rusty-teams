use serde_json::{Value, json};
use session::{ApiResponse, Method, Request, Session};

use crate::cards::{CardTransport, DEFAULT_MT_REGION, SessionCardTransport, spaces_scope};
use crate::error::{Error, Result};

const FORBIDDEN_STATUS: u16 = 403;
const RESULT_LIMIT: &str = "27";
const RATING: &str = "g";
const LANGUAGE: &str = "de-de";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gif {
    pub url: String,
    pub preview_url: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub preview_width: u32,
    pub preview_height: u32,
}

pub struct Gifs<T: CardTransport = SessionCardTransport> {
    transport: T,
    url: String,
}

impl Gifs<SessionCardTransport> {
    pub fn new(session: &Session) -> Self {
        Gifs::with_transport(SessionCardTransport::new(session), DEFAULT_MT_REGION)
    }
}

impl<T: CardTransport> Gifs<T> {
    pub fn with_transport(transport: T, mt_region: &str) -> Self {
        Gifs {
            transport,
            url: format!(
                "https://teams.cloud.microsoft/api/mt/{mt_region}/beta/commands/com.microsoft.teamspace.inputextension.giphy/execute?version=1.0"
            ),
        }
    }

    /// An empty query returns the trending GIFs.
    pub async fn search(&self, query: &str) -> Result<Vec<Gif>> {
        let mut request = Request::with_body(Method::Post, &self.url, search_body(query));
        request.headers = vec![(
            "content-type".to_owned(),
            "application/json;charset=UTF-8".to_owned(),
        )];
        let answer = self.transport.send(request, spaces_scope()).await?;
        parse_answer(answer, &self.url)
    }
}

fn search_body(query: &str) -> Value {
    json!([
        {"name": "query", "value": query},
        {"name": "rating", "value": RATING},
        {"name": "limit", "value": RESULT_LIMIT},
        {"name": "lang", "value": LANGUAGE},
    ])
}

fn parse_answer(answer: ApiResponse, url: &str) -> Result<Vec<Gif>> {
    if answer.status == FORBIDDEN_STATUS {
        return Err(Error::GifsDisabled);
    }
    if !answer.is_success() {
        return Err(Error::Session(session::Error::api(
            answer.status,
            url,
            answer.body,
        )));
    }
    Ok(parse_gifs(&answer.body))
}

pub fn parse_gifs(body: &Value) -> Vec<Gif> {
    body.pointer("/result/0/result")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_gif)
        .collect()
}

fn parse_gif(entry: &Value) -> Option<Gif> {
    let field = |name: &str| {
        entry
            .get("fieldValues")?
            .as_array()?
            .iter()
            .find(|field| field.get("fieldName").and_then(Value::as_str) == Some(name))?
            .get("fieldValue")?
            .as_str()
    };
    let number = |name: &str| field(name)?.parse::<u32>().ok().filter(|size| *size > 0);
    let url = entry.get("composeValue")?.as_str()?.to_owned();
    Some(Gif {
        preview_url: field("previewImageFW")
            .or_else(|| field("previewImage"))
            .map_or_else(|| url.clone(), str::to_owned),
        title: field("title").unwrap_or_default().to_owned(),
        width: number("width")?,
        height: number("height")?,
        preview_width: number("previewImageFWWidth").unwrap_or(1),
        preview_height: number("previewImageFWHeight").unwrap_or(1),
        url,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use session::Scope;

    use super::*;

    struct Canned {
        status: u16,
        body: Value,
        sent: Mutex<Vec<(Request, Scope)>>,
    }

    impl CardTransport for Canned {
        async fn send(&self, request: Request, scope: Scope) -> Result<ApiResponse> {
            self.sent.lock().unwrap().push((request, scope));
            Ok(ApiResponse {
                status: self.status,
                body: self.body.clone(),
                retry_after: None,
            })
        }
    }

    fn gifs(status: u16, body: Value) -> Gifs<Canned> {
        Gifs::with_transport(
            Canned {
                status,
                body,
                sent: Mutex::new(Vec::new()),
            },
            "emea",
        )
    }

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/giphy_search.json")).unwrap()
    }

    #[tokio::test]
    async fn search_posts_the_query_to_the_giphy_command() {
        let gifs = gifs(200, fixture());
        gifs.search("thumbs up").await.unwrap();
        let sent = gifs.transport.sent.lock().unwrap();
        let (request, scope) = &sent[0];
        assert_eq!(request.method, Method::Post);
        assert_eq!(
            request.url,
            "https://teams.cloud.microsoft/api/mt/emea/beta/commands/com.microsoft.teamspace.inputextension.giphy/execute?version=1.0"
        );
        assert_eq!(request.body.as_ref().unwrap()[0]["value"], "thumbs up");
        assert_eq!(request.body.as_ref().unwrap()[2]["value"], "27");
        assert_eq!(*scope, spaces_scope());
    }

    #[tokio::test]
    async fn results_carry_urls_titles_and_sizes() {
        let found = gifs(200, fixture()).search("x").await.unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].title, "Baby Thank You GIF");
        assert!(found[0].url.ends_with("/giphy.gif"));
        assert!(found[0].preview_url.ends_with("/200w.gif"));
        assert_eq!((found[0].width, found[0].height), (358, 360));
        assert_eq!(
            (found[0].preview_width, found[0].preview_height),
            (200, 202)
        );
    }

    #[tokio::test]
    async fn forbidden_means_disabled() {
        let outcome = gifs(403, json!({"errorCode": "Forbidden"}))
            .search("")
            .await;
        assert!(matches!(outcome, Err(Error::GifsDisabled)));
    }

    #[tokio::test]
    async fn other_failures_are_api_errors() {
        let outcome = gifs(500, Value::Null).search("").await;
        assert!(matches!(
            outcome,
            Err(Error::Session(session::Error::Api { status: 500, .. }))
        ));
    }

    #[test]
    fn entries_without_a_size_are_skipped() {
        let body = json!({"result": [{"result": [{"composeValue": "https://media0.giphy.com/a.gif", "fieldValues": []}]}]});
        assert!(parse_gifs(&body).is_empty());
    }
}
