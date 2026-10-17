use chatsvc::substrate_search::{
    MessageFilter, QueryRequest, QueryResults, QueryScope, SearchIdentity, SearchSession,
    SubstrateSearch, Suggestions,
};
use session::{DEFAULT_ENDPOINT, Method, Scope, Session};

fn print_suggestions(label: &str, suggestions: &Suggestions) {
    println!(
        "# {label}: people {} chats {} channels {} teams {} messages {} files {} events {} texts {} best {}",
        suggestions.people.len(),
        suggestions.chats.len(),
        suggestions.channels.len(),
        suggestions.teams.len(),
        suggestions.messages.len(),
        suggestions.files.len(),
        suggestions.events.len(),
        suggestions.texts.len(),
        suggestions.best_matches.len()
    );
}

fn print_results(label: &str, results: &QueryResults) {
    println!(
        "# {label}: messages {}/{} people {}/{} chats {}/{} channels {}/{} files {}/{}",
        results.messages.items.len(),
        results.messages.total,
        results.people.items.len(),
        results.people.total,
        results.chats.items.len(),
        results.chats.total,
        results.channels.items.len(),
        results.channels.total,
        results.files.items.len(),
        results.files.total
    );
}

#[tokio::main]
async fn main() {
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let text = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "release".to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let graph_scope = Scope::graph("User.Read");
    let me = session
        .request(
            Method::Get,
            "https://graph.microsoft.com/v1.0/me?$select=id",
            &graph_scope,
            None,
        )
        .await
        .expect("me");
    let organization = session
        .request(
            Method::Get,
            "https://graph.microsoft.com/v1.0/organization?$select=id",
            &graph_scope,
            None,
        )
        .await
        .expect("organization");
    let identity = SearchIdentity {
        user_id: me.body["id"].as_str().expect("user id").to_owned(),
        tenant_id: organization.body["value"][0]["id"]
            .as_str()
            .expect("tenant id")
            .to_owned(),
    };
    let search = SubstrateSearch::new(&session, identity);
    let search_session = SearchSession::new();
    match search.init(&search_session).await {
        Ok(()) => println!("# init: ok"),
        Err(error) => println!("# init: {error}"),
    }
    match search.suggestions(&search_session, "").await {
        Ok(suggestions) => print_suggestions("zero query", &suggestions),
        Err(error) => println!("# zero query: {error}"),
    }
    let typeahead = search.suggestions(&search_session, &text).await;
    match &typeahead {
        Ok(suggestions) => print_suggestions("typeahead", suggestions),
        Err(error) => println!("# typeahead: {error}"),
    }
    for scope in [QueryScope::All, QueryScope::Messages, QueryScope::People] {
        match search
            .query(&search_session, &QueryRequest::new(&text, scope))
            .await
        {
            Ok(results) => print_results(&format!("query {scope:?}"), &results),
            Err(error) => println!("# query {scope:?}: {error}"),
        }
    }
    let chat_thread = typeahead.ok().and_then(|suggestions| {
        suggestions
            .messages
            .first()
            .map(|hit| hit.conversation_id.clone())
    });
    if let Some(thread_id) = chat_thread {
        let request = QueryRequest::new(&text, QueryScope::Messages)
            .with_filters(vec![MessageFilter::InChat { thread_id }]);
        match search.query(&search_session, &request).await {
            Ok(results) => print_results("find in chat", &results),
            Err(error) => println!("# find in chat: {error}"),
        }
    }
}
