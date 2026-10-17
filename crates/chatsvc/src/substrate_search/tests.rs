use std::collections::VecDeque;
use std::sync::Mutex;

use serde_json::{Value, json};
use session::{ApiResponse, Method, Request};

use super::*;

const SUGGESTIONS: &str = include_str!("../../tests/fixtures/substrate_search/suggestions.json");
const QUERY: &str = include_str!("../../tests/fixtures/substrate_search/query.json");
const USER_ID: &str = "11111111-2222-3333-4444-555555555555";

#[derive(Default)]
struct Recorder {
    requests: Mutex<Vec<Request>>,
    answers: Mutex<VecDeque<ApiResponse>>,
}

impl Recorder {
    fn answering(status: u16, body: Value) -> Self {
        let recorder = Recorder::default();
        recorder.answers.lock().unwrap().push_back(ApiResponse {
            status,
            body,
            retry_after: None,
        });
        recorder
    }

    fn only_request(&self) -> Request {
        let requests = self.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        requests[0].clone()
    }
}

impl CsaTransport for &Recorder {
    async fn send(&self, request: Request) -> Result<ApiResponse> {
        self.requests.lock().unwrap().push(request);
        Ok(self
            .answers
            .lock()
            .unwrap()
            .pop_front()
            .expect("no answer left"))
    }
}

fn client(recorder: &Recorder) -> SubstrateSearch<&Recorder> {
    let identity = SearchIdentity {
        user_id: USER_ID.to_owned(),
        tenant_id: "tenant".to_owned(),
    };
    SubstrateSearch::with_transport(recorder, identity, "https://substrate.test/")
}

fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn fixture(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[tokio::test]
async fn typeahead_posts_the_entity_requests_with_teams_headers() {
    let recorder = Recorder::answering(200, fixture(SUGGESTIONS));
    let search_session = SearchSession::new();
    client(&recorder)
        .suggestions(&search_session, "eri")
        .await
        .unwrap();
    let request = recorder.only_request();
    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.url,
        "https://substrate.test/search/api/v1/suggestions?scenario=powerbar&setflight=EnableMessageImageSearch%2CEnableTeamsChannelDomainPowerbar"
    );
    let body = request.body.clone().unwrap();
    assert_eq!(body["Cvid"], search_session.correlation_id());
    assert_eq!(body["EntityRequests"][0]["Query"]["QueryString"], "eri");
    assert_eq!(header(&request, "x-client-version"), Some("T2.1"));
    assert_eq!(header(&request, "X-ClientType"), Some("MicrosoftTeams"));
    assert_eq!(
        header(&request, "x-anchormailbox"),
        Some(format!("Oid:{USER_ID}@tenant").as_str())
    );
    assert_eq!(
        header(&request, "x-routingparameter-sessionkey"),
        Some(format!("OID:{USER_ID}@tenant").as_str())
    );
    assert!(header(&request, "x-client-flights").is_some());
}

#[tokio::test]
async fn empty_text_asks_for_the_zero_query_state() {
    let recorder = Recorder::answering(200, json!({"Groups": []}));
    let search_session = SearchSession::new();
    client(&recorder)
        .suggestions(&search_session, " ")
        .await
        .unwrap();
    let request = recorder.only_request();
    assert_eq!(request.method, Method::Get);
    assert!(request.url.starts_with(&format!(
        "https://substrate.test/search/api/v1/suggestions?scenario=powerbar&query=&cvid={}&logicalId=",
        search_session.correlation_id()
    )));
}

#[tokio::test]
async fn query_posts_to_the_search_service_with_version_one() {
    let recorder = Recorder::answering(200, fixture(QUERY));
    let results = client(&recorder)
        .query(
            &SearchSession::new(),
            &QueryRequest::new("erika", QueryScope::Messages)
                .with_filters(vec![MessageFilter::MentionsMe]),
        )
        .await
        .unwrap();
    let request = recorder.only_request();
    assert_eq!(
        request.url,
        "https://substrate.test/searchservice/api/v2/query"
    );
    assert_eq!(header(&request, "x-client-version"), Some("1"));
    assert_eq!(
        request.body.clone().unwrap()["EntityRequests"][0]["query"]["queryString"],
        "NOT (isClientSoftDeleted:TRUE) AND mentions:11111111222233334444555555555555 AND erika"
    );
    assert_eq!(results.messages.total, 22770);
}

#[tokio::test]
async fn init_warms_up_with_the_session_cvid() {
    let recorder = Recorder::answering(200, json!({}));
    let search_session = SearchSession::new();
    client(&recorder).init(&search_session).await.unwrap();
    assert_eq!(
        recorder.only_request().url,
        format!(
            "https://substrate.test/search/api/v2/init?cvid={}&scenario=powerbar",
            search_session.correlation_id()
        )
    );
}

#[tokio::test]
async fn failed_answer_is_an_error() {
    let recorder = Recorder::answering(401, json!({"error": "denied"}));
    let outcome = client(&recorder)
        .suggestions(&SearchSession::new(), "x")
        .await;
    assert!(outcome.is_err());
}

#[test]
fn suggestions_fixture_parses_every_group() {
    let suggestions = parse_suggestions(&fixture(SUGGESTIONS));
    let person = &suggestions.people[0];
    assert_eq!(person.display_name, "Erika Mustermann");
    assert_eq!(person.user_id.as_deref(), Some(USER_ID));
    assert_eq!(person.job_title.as_deref(), Some("Engineer"));
    assert_eq!(person.email_addresses, ["erika.mustermann@contoso.example"]);

    let chat = &suggestions.chats[0];
    assert_eq!(chat.thread_id, "19:group01@thread.v2");
    assert_eq!(chat.name, None);
    assert_eq!(chat.total_members, 3);
    assert_eq!(chat.members.len(), 2);
    assert_eq!(chat.matching_members[0].display_name, "Erika Mustermann");

    assert_eq!(
        suggestions.channels[0].team_name.as_deref(),
        Some("Platform")
    );
    assert_eq!(suggestions.teams[0].name, "Platform");

    let read = &suggestions.messages[0];
    assert_eq!(read.conversation_id, "19:group01@thread.v2");
    assert_eq!(read.message_id, "1760600081000");
    assert_eq!(read.sender_name.as_deref(), Some("Max Muster"));
    assert_eq!(read.sender_mri.as_deref(), Some("8:orgid:aaaa"));
    assert_eq!(read.is_read, Some(true));
    assert_eq!(suggestions.messages[1].is_read, Some(false));

    assert_eq!(suggestions.files[0].name, "Roadmap.docx");
    assert_eq!(suggestions.files[0].size, Some(2048));
    assert_eq!(suggestions.events[0].subject, "Release sync");
    assert_eq!(suggestions.texts[0].kind.as_deref(), Some("PeopleAsText"));
    assert_eq!(suggestions.best_matches.len(), 2);
    assert_eq!(suggestions.best_matches[0].group, "People");
}

#[test]
fn query_fixture_maps_channel_replies_to_their_root() {
    let results = parse_query_results(&fixture(QUERY));
    assert_eq!(results.search_terms, ["erika"]);
    assert_eq!(
        results.people.items[0].office_location.as_deref(),
        Some("Building 1")
    );
    assert!(results.chats.more_available);
    assert_eq!(results.chats.items[0].reference_id.as_deref(), Some("q.2"));

    let message = &results.messages.items[0];
    assert_eq!(message.conversation_id, "19:channel01@thread.tacv2");
    assert_eq!(message.message_id, "1760600000000");
    assert_eq!(message.root_message_id.as_deref(), Some("1760500000000"));
    assert_eq!(message.thread_type.as_deref(), Some("topic"));
    assert_eq!(message.sender_mri.as_deref(), Some("8:orgid:aaaa"));
    assert_eq!(message.sender_name.as_deref(), Some("Max Muster"));
    assert!(message.is_mentioned);
    assert!(message.has_attachments);
    assert_eq!(message.is_read, None);
    let highlighted = split_highlights(message.highlighted_summary.as_deref().unwrap());
    assert_eq!(&highlighted.text[highlighted.hits[0].clone()], "Erika");

    assert_eq!(results.files.total, 393);
    assert_eq!(results.files.items[0].name, "Notes");
    assert_eq!(results.files.items[0].size, Some(12));
}

#[test]
fn unknown_shape_parses_to_empty_results() {
    assert_eq!(parse_suggestions(&json!([])), Suggestions::default());
    assert_eq!(
        parse_query_results(&json!({"EntitySets": 3})),
        QueryResults::default()
    );
}
