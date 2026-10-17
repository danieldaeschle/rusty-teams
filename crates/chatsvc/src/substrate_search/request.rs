use serde_json::{Value, json};

use super::kql::{MessageFilter, message_query_string};

const APP_NAME: &str = "Microsoft Teams";
const SCENARIO: &str = "powerbar";
const TIME_ZONE: &str = "Europe/Berlin";
const FORM_FACTOR: &str = "general.web.reactSearch";
const TAB_PAGE_SIZE: usize = 25;
const TAB_TOP_RESULTS: usize = 5;
const MESSAGE_FIELDS: [&str; 8] = [
    "Extension_SkypeSpaces_ConversationPost_Extension_FromSkypeInternalId_String",
    "Extension_SkypeSpaces_ConversationPost_Extension_FileData_String",
    "Extension_SkypeSpaces_ConversationPost_Extension_ThreadType_String",
    "Extension_SkypeSpaces_ConversationPost_Extension_SkypeGroupId_String",
    "Extension_SkypeSpaces_ConversationPost_Extension_SenderTenantId_String",
    "Extension_SkypeSpaces_ConversationPost_Extension_ParentMessageId_String",
    "Extension_SkypeSpaces_ConversationPost_Extension_ImageSrc_String",
    "Extension_SkypeSpaces_ConversationPost_Extension_AmsReferences_StringArray",
];
const PEOPLE_FIELDS: [&str; 18] = [
    "Id",
    "MRI",
    "DisplayName",
    "EmailAddresses",
    "PeopleType",
    "PeopleSubtype",
    "Phones",
    "UserPrincipalName",
    "GivenName",
    "Surname",
    "CompanyName",
    "JobTitle",
    "ImAddress",
    "ExternalDirectoryObjectId",
    "Department",
    "OfficeLocation",
    "HomeTenantName",
    "MTOLabel",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryScope {
    All,
    Messages,
    People,
    Chats,
    Channels,
    Files,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryRequest {
    pub text: String,
    pub scope: QueryScope,
    pub filters: Vec<MessageFilter>,
    pub from: usize,
    pub size: usize,
}

impl QueryRequest {
    pub fn new(text: impl Into<String>, scope: QueryScope) -> Self {
        QueryRequest {
            text: text.into(),
            scope,
            filters: Vec::new(),
            from: 0,
            size: TAB_PAGE_SIZE,
        }
    }

    pub fn with_filters(mut self, filters: Vec<MessageFilter>) -> Self {
        self.filters = filters;
        self
    }

    pub fn page(mut self, from: usize) -> Self {
        self.from = from;
        self
    }

    fn is_contextual(&self) -> bool {
        self.filters
            .iter()
            .any(|filter| matches!(filter, MessageFilter::InChat { .. }))
    }

    fn query_type(&self) -> &'static str {
        if self.is_contextual() {
            return "ContextualTypeahead";
        }
        match self.scope {
            QueryScope::All => "All",
            QueryScope::Messages => "Messages",
            QueryScope::People => "People",
            QueryScope::Chats => "Chats",
            QueryScope::Channels => "Channels",
            QueryScope::Files => "Files",
        }
    }
}

pub(super) fn suggestion_body(text: &str, correlation_id: &str, logical_id: &str) -> Value {
    let query = json!({
        "QueryString": text,
        "DisplayQueryString": text,
        "NormalizedQueryString": text,
    });
    let entity = |entity_type: &str, size: usize| json!({"EntityType": entity_type, "Size": size, "Query": query});
    let mut people = entity("People", 5);
    people["Fields"] = json!(&PEOPLE_FIELDS[..15]);
    people["Filter"] = people_filter();
    people["Provenances"] = json!(["Mailbox", "Directory"]);
    people["From"] = json!(0);
    let mut channel = entity("Channel", 4);
    channel["Filter"] = json!({"Or": [
        {"Term": {"ChannelType": "Standard"}},
        {"Term": {"ChannelType": "Shared"}},
    ]});
    json!({
        "EntityRequests": [
            people,
            entity("File", 3),
            entity("Chat", 5),
            entity("Team", 3),
            channel,
            entity("Text", 2),
            entity("Message", 11),
            entity("Event", 3),
        ],
        "Scenario": {
            "Name": SCENARIO,
            "Dimensions": [{"DimensionName": "QueryType", "DimensionValue": "PeopleCentricSearch"}],
        },
        "Cvid": correlation_id,
        "LogicalId": logical_id,
        "AppName": APP_NAME,
        "dataSource": "personScoped",
    })
}

pub(super) fn query_body(
    request: &QueryRequest,
    user_id: &str,
    correlation_id: &str,
    logical_id: &str,
) -> Value {
    let plain_query = json!({"queryString": request.text, "displayQueryString": request.text});
    let entity_requests: Vec<Value> = match request.scope {
        QueryScope::All => vec![
            message_entity(request, user_id, 3, 3),
            file_entity(&plain_query, 0, 3),
            people_entity(&plain_query, 0, 8),
            plain_entity("Chat", &plain_query, 0, 3),
            plain_entity("TeamsChannel", &plain_query, 0, 3),
        ],
        QueryScope::Messages => vec![message_entity(
            request,
            user_id,
            request.size,
            if request.from == 0 {
                TAB_TOP_RESULTS
            } else {
                0
            },
        )],
        QueryScope::People => vec![people_entity(&plain_query, request.from, request.size)],
        QueryScope::Chats => vec![plain_entity(
            "Chat",
            &plain_query,
            request.from,
            request.size,
        )],
        QueryScope::Channels => vec![plain_entity(
            "TeamsChannel",
            &plain_query,
            request.from,
            request.size,
        )],
        QueryScope::Files => vec![file_entity(&plain_query, request.from, request.size)],
    };
    json!({
        "EntityRequests": entity_requests,
        "QueryAlterationOptions": {
            "EnableAlteration": true,
            "EnableSuggestion": true,
            "SupportedRecourseDisplayTypes": ["Suggestion"],
        },
        "scenario": {
            "Name": SCENARIO,
            "Dimensions": [
                {"DimensionName": "QueryType", "DimensionValue": request.query_type()},
                {"DimensionName": "FormFactor", "DimensionValue": FORM_FACTOR},
            ],
        },
        "cvid": correlation_id,
        "logicalId": logical_id,
        "timezone": TIME_ZONE,
    })
}

fn message_entity(
    request: &QueryRequest,
    user_id: &str,
    size: usize,
    top_results_count: usize,
) -> Value {
    let query_string = message_query_string(&request.text, &request.filters, user_id);
    let mut entity = json!({
        "entityType": "Message",
        "contentSources": ["Teams"],
        "propertySet": "Optimized",
        "fields": MESSAGE_FIELDS,
        "query": {"queryString": query_string, "displayQueryString": request.text},
        "from": request.from,
        "size": size,
    });
    if top_results_count > 0 {
        entity["topResultsCount"] = json!(top_results_count);
    }
    entity
}

fn people_entity(query: &Value, from: usize, size: usize) -> Value {
    json!({
        "entityType": "People",
        "contentSources": ["Exchange"],
        "Fields": PEOPLE_FIELDS,
        "Filter": people_filter(),
        "query": query,
        "from": from,
        "size": size,
    })
}

fn file_entity(query: &Value, from: usize, size: usize) -> Value {
    json!({
        "entityType": "File",
        "contentSources": ["OneDriveBusiness", "Exchange"],
        "query": query,
        "from": from,
        "size": size,
    })
}

fn plain_entity(entity_type: &str, query: &Value, from: usize, size: usize) -> Value {
    json!({
        "entityType": entity_type,
        "contentSources": ["Teams"],
        "query": query,
        "from": from,
        "size": size,
    })
}

fn people_filter() -> Value {
    let any_term = |field: &str, values: &[&str]| {
        let terms: Vec<Value> = values
            .iter()
            .map(|value| json!({"Term": {field: value}}))
            .collect();
        json!({"Or": terms})
    };
    json!({"And": [
        any_term("PeopleType", &["Person", "Other"]),
        any_term(
            "PeopleSubtype",
            &["OrganizationUser", "MTOUser", "PersonalContact", "Guest"],
        ),
        any_term("Flags", &["NonHidden"]),
    ]})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_ask_for_every_domain_in_teams_order() {
        let body = suggestion_body("chr", "session", "request");
        let types: Vec<&str> = body["EntityRequests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entity| entity["EntityType"].as_str().unwrap())
            .collect();
        assert_eq!(
            types,
            [
                "People", "File", "Chat", "Team", "Channel", "Text", "Message", "Event"
            ]
        );
        assert_eq!(body["Cvid"], "session");
        assert_eq!(body["LogicalId"], "request");
        assert_eq!(body["EntityRequests"][0]["Query"]["QueryString"], "chr");
        assert_eq!(
            body["EntityRequests"][0]["Provenances"],
            json!(["Mailbox", "Directory"])
        );
    }

    #[test]
    fn all_scope_uses_the_teams_section_sizes() {
        let request = QueryRequest::new("release", QueryScope::All);
        let body = query_body(&request, "user", "session", "request");
        let sizes: Vec<(String, u64)> = body["EntityRequests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entity| {
                (
                    entity["entityType"].as_str().unwrap().to_owned(),
                    entity["size"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            sizes,
            [
                ("Message".to_owned(), 3),
                ("File".to_owned(), 3),
                ("People".to_owned(), 8),
                ("Chat".to_owned(), 3),
                ("TeamsChannel".to_owned(), 3),
            ]
        );
        assert_eq!(
            body["EntityRequests"][0]["query"]["queryString"],
            "NOT (isClientSoftDeleted:TRUE) AND release"
        );
        assert_eq!(
            body["EntityRequests"][0]["query"]["displayQueryString"],
            "release"
        );
        assert_eq!(body["scenario"]["Dimensions"][0]["DimensionValue"], "All");
    }

    #[test]
    fn messages_tab_pages_by_25_with_top_results_on_the_first_page_only() {
        let first = query_body(
            &QueryRequest::new("x", QueryScope::Messages),
            "user",
            "session",
            "request",
        );
        assert_eq!(first["EntityRequests"][0]["size"], 25);
        assert_eq!(first["EntityRequests"][0]["topResultsCount"], 5);
        let second = query_body(
            &QueryRequest::new("x", QueryScope::Messages).page(25),
            "user",
            "session",
            "request",
        );
        assert_eq!(second["EntityRequests"][0]["from"], 25);
        assert!(second["EntityRequests"][0].get("topResultsCount").is_none());
    }

    #[test]
    fn find_in_chat_is_a_contextual_query() {
        let request = QueryRequest::new("x", QueryScope::Messages).with_filters(vec![
            MessageFilter::InChat {
                thread_id: "19:abc@thread.v2".to_owned(),
            },
        ]);
        let body = query_body(&request, "user", "session", "request");
        assert_eq!(
            body["scenario"]["Dimensions"][0]["DimensionValue"],
            "ContextualTypeahead"
        );
    }
}
