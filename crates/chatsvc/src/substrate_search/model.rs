use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

const READ_FLAG: &str = "IsRead";
const MESSAGE_ID_MARKER: &str = ";messageid=";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Suggestions {
    pub people: Vec<SearchPerson>,
    pub chats: Vec<SearchChat>,
    pub channels: Vec<SearchChannel>,
    pub teams: Vec<SearchTeam>,
    pub messages: Vec<SearchMessage>,
    pub files: Vec<SearchFile>,
    pub events: Vec<SearchEvent>,
    pub texts: Vec<SearchText>,
    pub best_matches: Vec<BestMatch>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueryResults {
    pub messages: ResultPage<SearchMessage>,
    pub people: ResultPage<SearchPerson>,
    pub chats: ResultPage<SearchChat>,
    pub channels: ResultPage<SearchChannel>,
    pub files: ResultPage<SearchFile>,
    pub search_terms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResultPage<T> {
    pub total: u64,
    pub more_available: bool,
    pub items: Vec<T>,
}

impl<T> Default for ResultPage<T> {
    fn default() -> Self {
        ResultPage {
            total: 0,
            more_available: false,
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BestMatch {
    pub group: String,
    pub reference_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchPerson {
    pub reference_id: Option<String>,
    pub user_id: Option<String>,
    pub mri: Option<String>,
    pub display_name: String,
    pub given_name: Option<String>,
    pub surname: Option<String>,
    pub email_addresses: Vec<String>,
    pub user_principal_name: Option<String>,
    pub job_title: Option<String>,
    pub department: Option<String>,
    pub company_name: Option<String>,
    pub office_location: Option<String>,
    pub people_subtype: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatMember {
    pub display_name: String,
    pub mri: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchChat {
    pub reference_id: Option<String>,
    pub thread_id: String,
    pub name: Option<String>,
    pub thread_type: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub total_members: u32,
    pub members: Vec<ChatMember>,
    pub matching_members: Vec<ChatMember>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchChannel {
    pub reference_id: Option<String>,
    pub thread_id: String,
    pub name: String,
    pub team_name: Option<String>,
    pub team_id: Option<String>,
    pub channel_type: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchTeam {
    pub reference_id: Option<String>,
    pub thread_id: Option<String>,
    pub team_id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchMessage {
    pub reference_id: Option<String>,
    pub conversation_id: String,
    pub message_id: String,
    pub root_message_id: Option<String>,
    pub sent_at: Option<DateTime<Utc>>,
    pub sender_name: Option<String>,
    pub sender_address: Option<String>,
    pub sender_mri: Option<String>,
    pub preview: String,
    pub highlighted_summary: Option<String>,
    pub thread_type: Option<String>,
    pub thread_sub_type: Option<String>,
    pub is_read: Option<bool>,
    pub is_mentioned: bool,
    pub has_attachments: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchFile {
    pub reference_id: Option<String>,
    pub name: String,
    pub extension: Option<String>,
    pub size: Option<u64>,
    pub url: Option<String>,
    pub author: Option<String>,
    pub modified_at: Option<DateTime<Utc>>,
    pub shared_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchEvent {
    pub reference_id: Option<String>,
    pub subject: String,
    pub organizer_name: Option<String>,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub location: Option<String>,
    pub teams_meeting_url: Option<String>,
    pub web_link: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchText {
    pub reference_id: Option<String>,
    pub text: String,
    pub query_text: Option<String>,
    pub kind: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct WireSuggestions {
    groups: Vec<WireGroup>,
    layout_hints: WireLayoutHints,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct WireGroup {
    #[serde(rename = "Type")]
    group_type: String,
    suggestions: Vec<Value>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct WireLayoutHints {
    best_match_suggestions: Vec<WireBestMatch>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct WireBestMatch {
    group: String,
    reference_id: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct WireQuery {
    entity_sets: Vec<WireEntitySet>,
    search_terms: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct WireEntitySet {
    entity_type: String,
    result_sets: Vec<WireResultSet>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct WireResultSet {
    total: u64,
    more_results_available: bool,
    results: Vec<WireResult>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct WireResult {
    reference_id: Option<String>,
    hit_highlighted_summary: Option<String>,
    source: Value,
}

pub fn parse_suggestions(body: &Value) -> Suggestions {
    let wire: WireSuggestions = WireSuggestions::deserialize(body).unwrap_or_default();
    let mut suggestions = Suggestions {
        best_matches: wire
            .layout_hints
            .best_match_suggestions
            .into_iter()
            .map(|hint| BestMatch {
                group: hint.group,
                reference_id: hint.reference_id,
            })
            .collect(),
        ..Suggestions::default()
    };
    for group in wire.groups {
        let items = group.suggestions.iter();
        match group.group_type.as_str() {
            "People" => suggestions
                .people
                .extend(items.filter_map(|item| person(item))),
            "Chat" => suggestions
                .chats
                .extend(items.filter_map(|item| chat(item))),
            "Channel" => suggestions
                .channels
                .extend(items.filter_map(|item| channel(item))),
            "Team" => suggestions
                .teams
                .extend(items.filter_map(|item| team(item))),
            "Message" => suggestions
                .messages
                .extend(items.filter_map(|item| message(item, None))),
            "File" => suggestions
                .files
                .extend(items.filter_map(|item| file(item))),
            "Event" => suggestions
                .events
                .extend(items.filter_map(|item| event(item))),
            "Text" => suggestions
                .texts
                .extend(items.filter_map(|item| text(item))),
            _ => {}
        }
    }
    suggestions
}

pub fn parse_query_results(body: &Value) -> QueryResults {
    let wire: WireQuery = WireQuery::deserialize(body).unwrap_or_default();
    let mut results = QueryResults {
        search_terms: wire.search_terms,
        ..QueryResults::default()
    };
    for entity_set in wire.entity_sets {
        for result_set in entity_set.result_sets {
            match entity_set.entity_type.as_str() {
                "Message" => fill(&mut results.messages, &result_set, |result| {
                    message(&result.source, result.hit_highlighted_summary.clone())
                }),
                "People" => fill(&mut results.people, &result_set, |result| {
                    person(&result.source)
                }),
                "Chat" => fill(&mut results.chats, &result_set, |result| {
                    chat(&result.source)
                }),
                "TeamsChannel" | "Channel" => fill(&mut results.channels, &result_set, |result| {
                    channel(&result.source)
                }),
                "File" => fill(&mut results.files, &result_set, |result| {
                    file(&result.source)
                }),
                _ => {}
            }
        }
    }
    results
}

fn fill<T>(
    page: &mut ResultPage<T>,
    result_set: &WireResultSet,
    convert: impl Fn(&WireResult) -> Option<T>,
) {
    page.total = page.total.max(result_set.total);
    page.more_available |= result_set.more_results_available;
    page.items.extend(
        result_set
            .results
            .iter()
            .filter_map(|result| convert(&with_reference(result))),
    );
}

fn with_reference(result: &WireResult) -> WireResult {
    let mut source = result.source.clone();
    if let (Some(fields), Some(reference_id)) = (source.as_object_mut(), &result.reference_id) {
        fields
            .entry("ReferenceId")
            .or_insert_with(|| Value::String(reference_id.clone()));
    }
    WireResult {
        reference_id: result.reference_id.clone(),
        hit_highlighted_summary: result.hit_highlighted_summary.clone(),
        source,
    }
}

fn string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn time(value: &Value, key: &str) -> Option<DateTime<Utc>> {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|parsed| parsed.with_timezone(&Utc))
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn members(value: &Value, key: &str) -> Vec<ChatMember> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|member| {
                    Some(ChatMember {
                        display_name: string(member, "DisplayName")?,
                        mri: string(member, "MRI"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn person(value: &Value) -> Option<SearchPerson> {
    Some(SearchPerson {
        reference_id: string(value, "ReferenceId"),
        user_id: string(value, "ExternalDirectoryObjectId"),
        mri: string(value, "MRI"),
        display_name: string(value, "DisplayName")?,
        given_name: string(value, "GivenName"),
        surname: string(value, "Surname"),
        email_addresses: strings(value, "EmailAddresses"),
        user_principal_name: string(value, "UserPrincipalName"),
        job_title: string(value, "JobTitle"),
        department: string(value, "Department"),
        company_name: string(value, "CompanyName"),
        office_location: string(value, "OfficeLocation"),
        people_subtype: string(value, "PeopleSubtype"),
    })
}

fn chat(value: &Value) -> Option<SearchChat> {
    Some(SearchChat {
        reference_id: string(value, "ReferenceId"),
        thread_id: string(value, "ThreadId")?,
        name: string(value, "Name"),
        thread_type: string(value, "ThreadType"),
        last_message_at: time(value, "LastMessageTime"),
        total_members: value
            .get("TotalChatMembersCount")
            .and_then(Value::as_u64)
            .and_then(|count| u32::try_from(count).ok())
            .unwrap_or_default(),
        members: members(value, "ChatMembers"),
        matching_members: members(value, "MatchingMembers"),
    })
}

fn channel(value: &Value) -> Option<SearchChannel> {
    Some(SearchChannel {
        reference_id: string(value, "ReferenceId"),
        thread_id: string(value, "ThreadId")?,
        name: string(value, "Name")?,
        team_name: string(value, "TeamName"),
        team_id: string(value, "GroupId"),
        channel_type: string(value, "ChannelType"),
    })
}

fn team(value: &Value) -> Option<SearchTeam> {
    Some(SearchTeam {
        reference_id: string(value, "ReferenceId"),
        thread_id: string(value, "ThreadId"),
        team_id: string(value, "GroupId"),
        name: string(value, "Name")?,
    })
}

fn message(value: &Value, highlighted_summary: Option<String>) -> Option<SearchMessage> {
    let extensions = value.get("Extensions").unwrap_or(&Value::Null);
    let conversation_id = string(value, "ClientThreadId")?;
    let message_id = string(value, "InternetMessageId")?;
    let sender = value
        .get("From")
        .or_else(|| value.get("Sender"))
        .unwrap_or(&Value::Null);
    let sender_email = sender.get("EmailAddress").unwrap_or(&Value::Null);
    let root_message_id = string(value, "ParentMessageId")
        .or_else(|| {
            string(
                extensions,
                "SkypeSpaces_ConversationPost_Extension_ParentMessageId",
            )
        })
        .or_else(|| {
            string(value, "ClientConversationId").and_then(|id| {
                id.split_once(MESSAGE_ID_MARKER)
                    .map(|(_, root)| root.to_owned())
            })
        });
    let metadata = strings(value, "Metadata");
    Some(SearchMessage {
        reference_id: string(value, "ReferenceId"),
        conversation_id,
        message_id,
        root_message_id,
        sent_at: time(value, "DateTimeSent"),
        sender_name: string(sender_email, "Name"),
        sender_address: string(sender_email, "Address"),
        sender_mri: string(sender, "MRI").or_else(|| {
            string(
                extensions,
                "SkypeSpaces_ConversationPost_Extension_FromSkypeInternalId",
            )
        }),
        preview: string(value, "Preview").unwrap_or_default(),
        highlighted_summary,
        thread_type: string(value, "ThreadType").or_else(|| {
            string(
                extensions,
                "SkypeSpaces_ConversationPost_Extension_ThreadType",
            )
        }),
        thread_sub_type: string(value, "ThreadSubType"),
        is_read: value
            .get("Metadata")
            .map(|_| metadata.iter().any(|flag| flag == READ_FLAG)),
        is_mentioned: value
            .pointer("/MentionsPreview/IsMentioned")
            .and_then(Value::as_bool)
            .unwrap_or_default(),
        has_attachments: value
            .get("HasAttachments")
            .and_then(Value::as_bool)
            .unwrap_or_default(),
    })
}

fn file(value: &Value) -> Option<SearchFile> {
    Some(SearchFile {
        reference_id: string(value, "ReferenceId"),
        name: string(value, "FileName")
            .or_else(|| string(value, "Filename"))
            .or_else(|| string(value, "Title"))?,
        extension: string(value, "FileExtension"),
        size: value
            .get("FileSize")
            .or_else(|| value.get("Size"))
            .and_then(Value::as_u64),
        url: string(value, "AccessUrl").or_else(|| string(value, "Path")),
        author: string(value, "Author"),
        modified_at: time(value, "DateModified").or_else(|| time(value, "LastModifiedTime")),
        shared_at: time(value, "SharedDateTime"),
    })
}

fn event(value: &Value) -> Option<SearchEvent> {
    Some(SearchEvent {
        reference_id: string(value, "ReferenceId"),
        subject: string(value, "Subject")?,
        organizer_name: string(value, "OrganizerName"),
        start: time(value, "Start"),
        end: time(value, "End"),
        location: string(value, "Location"),
        teams_meeting_url: string(value, "SkypeTeamsMeetingUrl"),
        web_link: string(value, "WebLink"),
    })
}

fn text(value: &Value) -> Option<SearchText> {
    Some(SearchText {
        reference_id: string(value, "ReferenceId"),
        text: string(value, "Text")?,
        query_text: string(value, "QueryText"),
        kind: string(value, "TextSuggestionType"),
    })
}
