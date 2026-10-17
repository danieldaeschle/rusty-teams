mod highlight;
mod kql;
mod model;
mod request;

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use session::{ApiResponse, Method, Request, Scope, Session};

use crate::error::{Error, Result};
use crate::pins::{CsaTransport, SessionTransport};

pub use highlight::{HighlightedText, split_highlights};
pub use kql::{MessageFilter, message_query_string};
pub use model::{
    BestMatch, ChatMember, QueryResults, ResultPage, SearchChannel, SearchChat, SearchEvent,
    SearchFile, SearchMessage, SearchPerson, SearchTeam, SearchText, Suggestions,
    parse_query_results, parse_suggestions,
};
pub use request::{QueryRequest, QueryScope};

pub const SUBSTRATE: &str = "https://substrate.office.com";
pub const SUBSTRATE_SEARCH_SCOPE: &str = "SubstrateSearch-Internal.ReadWrite";
const SCENARIO: &str = "powerbar";
const SUGGESTION_CLIENT_VERSION: &str = "T2.1";
const QUERY_CLIENT_VERSION: &str = "1";
const SUGGESTION_FLIGHTS: &str = "IncludeHistoryInBMDynamic,PeopleKeywordAsTextSuggestion,TDPeopleSynthetic,EnableKql,EnableSelfSuggestion";
const SUGGESTION_SET_FLIGHTS: &str = "EnableMessageImageSearch,EnableTeamsChannelDomainPowerbar";

/// Without the anchor mailbox headers Substrate answers 200 with empty result sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchIdentity {
    pub user_id: String,
    pub tenant_id: String,
}

/// One search session per opened search box: Teams sends the same cvid for every keystroke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSession {
    correlation_id: String,
}

impl SearchSession {
    pub fn new() -> Self {
        SearchSession {
            correlation_id: new_id(),
        }
    }

    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }
}

impl Default for SearchSession {
    fn default() -> Self {
        SearchSession::new()
    }
}

pub struct SubstrateSearch<T: CsaTransport = SessionTransport> {
    transport: T,
    identity: SearchIdentity,
    base_url: String,
}

impl SubstrateSearch<SessionTransport> {
    pub fn new(session: &Session, identity: SearchIdentity) -> Self {
        let transport =
            SessionTransport::with_scope(session, Scope::new(SUBSTRATE, SUBSTRATE_SEARCH_SCOPE));
        SubstrateSearch::with_transport(transport, identity, SUBSTRATE)
    }
}

impl<T: CsaTransport> SubstrateSearch<T> {
    pub fn with_transport(transport: T, identity: SearchIdentity, base_url: &str) -> Self {
        SubstrateSearch {
            transport,
            identity,
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }

    pub async fn init(&self, search_session: &SearchSession) -> Result<()> {
        let url = format!(
            "{}/search/api/v2/init?cvid={}&scenario={SCENARIO}",
            self.base_url,
            search_session.correlation_id()
        );
        let answer = self
            .send(Request::get(url), search_session, SUGGESTION_CLIENT_VERSION)
            .await?;
        ensure_success(&answer)
    }

    /// An empty text returns the zero-query state: top people, recent searches, chats.
    pub async fn suggestions(
        &self,
        search_session: &SearchSession,
        text: &str,
    ) -> Result<Suggestions> {
        let logical_id = new_id();
        let correlation_id = search_session.correlation_id();
        let mut request = if text.trim().is_empty() {
            Request::get(format!(
                "{}/search/api/v1/suggestions?scenario={SCENARIO}&query=&cvid={correlation_id}&logicalId={logical_id}",
                self.base_url
            ))
        } else {
            Request::with_body(
                Method::Post,
                format!(
                    "{}/search/api/v1/suggestions?scenario={SCENARIO}&setflight={}",
                    self.base_url,
                    utf8_percent_encode(SUGGESTION_SET_FLIGHTS, NON_ALPHANUMERIC)
                ),
                request::suggestion_body(text, correlation_id, &logical_id),
            )
        };
        request
            .headers
            .push(("x-client-flights".to_owned(), SUGGESTION_FLIGHTS.to_owned()));
        let answer = self
            .send(request, search_session, SUGGESTION_CLIENT_VERSION)
            .await?;
        ensure_success(&answer)?;
        Ok(parse_suggestions(&answer.body))
    }

    pub async fn query(
        &self,
        search_session: &SearchSession,
        query: &QueryRequest,
    ) -> Result<QueryResults> {
        let body = request::query_body(
            query,
            &self.identity.user_id,
            search_session.correlation_id(),
            &new_id(),
        );
        let request = Request::with_body(
            Method::Post,
            format!("{}/searchservice/api/v2/query", self.base_url),
            body,
        );
        let answer = self
            .send(request, search_session, QUERY_CLIENT_VERSION)
            .await?;
        ensure_success(&answer)?;
        Ok(parse_query_results(&answer.body))
    }

    async fn send(
        &self,
        mut request: Request,
        search_session: &SearchSession,
        client_version: &str,
    ) -> Result<ApiResponse> {
        request
            .headers
            .extend(self.headers(search_session, client_version));
        self.transport.send(request).await
    }

    fn headers(
        &self,
        search_session: &SearchSession,
        client_version: &str,
    ) -> Vec<(String, String)> {
        let SearchIdentity { user_id, tenant_id } = &self.identity;
        vec![
            ("x-client-version".to_owned(), client_version.to_owned()),
            ("X-ClientType".to_owned(), "MicrosoftTeams".to_owned()),
            ("X-ClientScenario".to_owned(), "TeamsSearch".to_owned()),
            ("client-request-id".to_owned(), new_id()),
            (
                "client-session-id".to_owned(),
                search_session.correlation_id().to_owned(),
            ),
            (
                "x-anchormailbox".to_owned(),
                format!("Oid:{user_id}@{tenant_id}"),
            ),
            (
                "x-routingparameter-sessionkey".to_owned(),
                format!("OID:{user_id}@{tenant_id}"),
            ),
        ]
    }
}

fn ensure_success(answer: &ApiResponse) -> Result<()> {
    if answer.is_success() {
        Ok(())
    } else {
        Err(Error::Session(session::Error::api(
            answer.status,
            "substrate search",
            answer.body.clone(),
        )))
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests;
