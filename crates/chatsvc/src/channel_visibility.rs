use serde_json::{Value, json};
use session::{Method, Request, Session};

use crate::error::Result;
use crate::members::{threads_url, user_mri};
use crate::messages::{MessageTransport, SessionMessageTransport, encode, ensure_success};
use crate::pins::DEFAULT_REGION;

const FAVORITE_PROPERTY: &str = "favorite";
const GENERAL_FAVORITE_PROPERTY: &str = "isGeneralChannelFavorite";
const FOLLOWED_PROPERTY: &str = "isfollowed";
const PERMANENT_JOIN_QUERY: &str = "requirePermanentJoinedState=true";
const FAVORITE_MODE_HEADER: &str = "x-ms-favorite-mode";
const FAVORITE_MODE_MANUAL: &str = "manual";
const HIDDEN_MEMBERSHIP_SECONDS: i64 = 1_209_600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    Standard,
    Private,
    Shared,
}

impl ChannelKind {
    pub fn from_membership_type(membership_type: Option<&str>) -> Self {
        match membership_type.map(str::to_ascii_lowercase).as_deref() {
            Some("private") => ChannelKind::Private,
            Some("shared") => ChannelKind::Shared,
            _ => ChannelKind::Standard,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelTarget<'a> {
    pub channel_id: &'a str,
    pub general: bool,
    pub kind: ChannelKind,
}

/// The own member state of a channel conversation that decides Teams' show and hide steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelMembership {
    pub moderator: bool,
    pub explicitly_added: bool,
    pub followed: bool,
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.),
        Some(Value::Array(_) | Value::Object(_)) => true,
        _ => false,
    }
}

pub fn parse_channel_membership(conversation: &Value) -> ChannelMembership {
    let member = conversation.get("memberProperties");
    ChannelMembership {
        moderator: truthy(member.and_then(|member| member.get("isModerator"))),
        explicitly_added: truthy(member.and_then(|member| member.get("explicitlyAdded"))),
        followed: truthy(conversation.pointer(&format!("/properties/{FOLLOWED_PROPERTY}"))),
    }
}

/// Teams joins the user to a standard channel thread before it changes the channel's visibility.
pub fn joins_thread(kind: ChannelKind, membership: ChannelMembership) -> bool {
    let permanently_joined = kind == ChannelKind::Private
        || (kind == ChannelKind::Shared && membership.explicitly_added)
        || membership.moderator;
    !permanently_joined
}

pub fn membership_expiration(shown: bool, now_ms: i64) -> i64 {
    if shown {
        0
    } else {
        (now_ms + 999).div_euclid(1000) + HIDDEN_MEMBERSHIP_SECONDS
    }
}

pub struct ChannelVisibility<T: MessageTransport = SessionMessageTransport> {
    transport: T,
    conversations_url: String,
    threads_url: String,
}

impl ChannelVisibility<SessionMessageTransport> {
    pub fn new(session: &Session) -> Self {
        ChannelVisibility::with_transport(SessionMessageTransport::new(session), DEFAULT_REGION)
    }
}

impl<T: MessageTransport> ChannelVisibility<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        ChannelVisibility {
            transport,
            conversations_url: format!(
                "https://teams.cloud.microsoft/api/chatsvc/{region}/v1/users/ME/conversations"
            ),
            threads_url: threads_url(region),
        }
    }

    pub fn conversation_request(&self, conversation_id: &str) -> Request {
        Request::get(format!(
            "{}/{}?view=msnp24Equivalent",
            self.conversations_url,
            encode(conversation_id)
        ))
    }

    fn property_request(&self, conversation_id: &str, name: &str, value: Value) -> Request {
        Request::with_body(
            Method::Put,
            format!(
                "{}/{}/properties?name={name}",
                self.conversations_url,
                encode(conversation_id)
            ),
            json!({ name: value }),
        )
    }

    pub fn favorite_request(&self, channel_id: &str, shown: bool) -> Request {
        let mut request =
            self.property_request(channel_id, FAVORITE_PROPERTY, json!(shown.to_string()));
        if shown {
            request.url = format!("{}&{PERMANENT_JOIN_QUERY}", request.url);
            request
                .headers
                .push((FAVORITE_MODE_HEADER.into(), FAVORITE_MODE_MANUAL.into()));
        }
        request
    }

    pub fn general_favorite_request(&self, team_thread_id: &str, shown: bool) -> Request {
        self.property_request(
            team_thread_id,
            GENERAL_FAVORITE_PROPERTY,
            json!(shown.to_string()),
        )
    }

    pub fn team_favorite_request(&self, team_thread_id: &str, shown: bool) -> Request {
        self.property_request(team_thread_id, FAVORITE_PROPERTY, json!(shown.to_string()))
    }

    pub fn unfollow_request(&self, channel_id: &str) -> Request {
        self.property_request(channel_id, FOLLOWED_PROPERTY, json!(false))
    }

    pub fn membership_request(
        &self,
        channel_id: &str,
        own_user_id: &str,
        shown: bool,
        now_ms: i64,
    ) -> Request {
        Request::with_body(
            Method::Put,
            format!(
                "{}/{}/members/{}",
                self.threads_url,
                encode(channel_id),
                user_mri(own_user_id)
            ),
            json!({ "expiration": membership_expiration(shown, now_ms) }),
        )
    }

    /// Teams' order for a channel other than General: join step, `favorite`, then `isfollowed`.
    pub fn channel_requests(
        &self,
        target: &ChannelTarget<'_>,
        membership: ChannelMembership,
        own_user_id: &str,
        shown: bool,
        now_ms: i64,
    ) -> Vec<Request> {
        if target.general {
            return vec![self.general_favorite_request(target.channel_id, shown)];
        }
        let mut requests = Vec::new();
        if joins_thread(target.kind, membership) {
            requests.push(self.membership_request(target.channel_id, own_user_id, shown, now_ms));
        }
        requests.push(self.favorite_request(target.channel_id, shown));
        if !shown || membership.followed {
            requests.push(self.unfollow_request(target.channel_id));
        }
        requests
    }

    pub async fn set_channel_shown(
        &self,
        target: &ChannelTarget<'_>,
        own_user_id: &str,
        shown: bool,
        now_ms: i64,
    ) -> Result<()> {
        let membership = if target.general {
            ChannelMembership::default()
        } else {
            let answer = self
                .transport
                .send(self.conversation_request(target.channel_id))
                .await?;
            ensure_success(&answer)?;
            parse_channel_membership(&answer.body)
        };
        for request in self.channel_requests(target, membership, own_user_id, shown, now_ms) {
            ensure_success(&self.transport.send(request).await?)?;
        }
        Ok(())
    }

    pub async fn set_team_shown(&self, team_thread_id: &str, shown: bool) -> Result<()> {
        let answer = self
            .transport
            .send(self.team_favorite_request(team_thread_id, shown))
            .await?;
        ensure_success(&answer)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use session::ApiResponse;

    use super::*;

    const BASE: &str = "https://teams.cloud.microsoft/api/chatsvc/emea/v1";
    const CHANNEL: &str = "19:abc@thread.tacv2";
    const ENCODED: &str = "19%3Aabc%40thread.tacv2";
    const NOW_MS: i64 = 1_791_368_000_500;

    struct Canned {
        requests: Mutex<Vec<Request>>,
        answers: Mutex<VecDeque<(u16, Value)>>,
    }

    impl MessageTransport for &Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request);
            let (status, body) = self
                .answers
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or((200, Value::Null));
            Ok(ApiResponse {
                status,
                body,
                retry_after: None,
            })
        }
    }

    fn canned(answers: Vec<(u16, Value)>) -> Canned {
        Canned {
            requests: Mutex::new(Vec::new()),
            answers: Mutex::new(answers.into()),
        }
    }

    fn standard() -> ChannelTarget<'static> {
        ChannelTarget {
            channel_id: CHANNEL,
            general: false,
            kind: ChannelKind::Standard,
        }
    }

    fn summary(request: &Request) -> (String, String, Option<Value>) {
        (
            request.method.as_str().to_owned(),
            request.url.clone(),
            request.body.clone(),
        )
    }

    #[test]
    fn showing_a_standard_channel_joins_then_favorites_with_the_manual_mode() {
        let transport = canned(Vec::new());
        let client = ChannelVisibility::with_transport(&transport, "emea");
        let requests = client.channel_requests(
            &standard(),
            ChannelMembership::default(),
            "user-1",
            true,
            NOW_MS,
        );
        let summaries: Vec<_> = requests.iter().map(summary).collect();
        assert_eq!(
            summaries,
            vec![
                (
                    "PUT".to_owned(),
                    format!("{BASE}/threads/{ENCODED}/members/8:orgid:user-1"),
                    Some(json!({"expiration": 0})),
                ),
                (
                    "PUT".to_owned(),
                    format!(
                        "{BASE}/users/ME/conversations/{ENCODED}/properties?name=favorite&requirePermanentJoinedState=true"
                    ),
                    Some(json!({"favorite": "true"})),
                ),
            ]
        );
        assert_eq!(
            requests[1].headers,
            vec![("x-ms-favorite-mode".to_owned(), "manual".to_owned())]
        );
    }

    #[test]
    fn hiding_sets_a_two_week_expiration_and_unfollows() {
        let transport = canned(Vec::new());
        let client = ChannelVisibility::with_transport(&transport, "emea");
        let requests = client.channel_requests(
            &standard(),
            ChannelMembership::default(),
            "8:orgid:user-1",
            false,
            NOW_MS,
        );
        let summaries: Vec<_> = requests.iter().map(summary).collect();
        assert_eq!(
            summaries,
            vec![
                (
                    "PUT".to_owned(),
                    format!("{BASE}/threads/{ENCODED}/members/8:orgid:user-1"),
                    Some(json!({"expiration": 1_791_368_001 + 1_209_600})),
                ),
                (
                    "PUT".to_owned(),
                    format!("{BASE}/users/ME/conversations/{ENCODED}/properties?name=favorite"),
                    Some(json!({"favorite": "false"})),
                ),
                (
                    "PUT".to_owned(),
                    format!("{BASE}/users/ME/conversations/{ENCODED}/properties?name=isfollowed"),
                    Some(json!({"isfollowed": false})),
                ),
            ]
        );
        assert!(requests[1].headers.is_empty());
    }

    #[test]
    fn moderators_and_private_members_skip_the_join_step() {
        let transport = canned(Vec::new());
        let client = ChannelVisibility::with_transport(&transport, "emea");
        let moderator = ChannelMembership {
            moderator: true,
            ..ChannelMembership::default()
        };
        let requests = client.channel_requests(&standard(), moderator, "user-1", true, NOW_MS);
        assert_eq!(requests.len(), 1);
        assert!(requests[0].url.contains("name=favorite"));
        let private = ChannelTarget {
            kind: ChannelKind::Private,
            ..standard()
        };
        assert!(!joins_thread(private.kind, ChannelMembership::default()));
        let shared = ChannelMembership {
            explicitly_added: true,
            ..ChannelMembership::default()
        };
        assert!(!joins_thread(ChannelKind::Shared, shared));
        assert!(joins_thread(
            ChannelKind::Shared,
            ChannelMembership::default()
        ));
    }

    #[test]
    fn showing_a_followed_channel_also_clears_the_follow() {
        let transport = canned(Vec::new());
        let client = ChannelVisibility::with_transport(&transport, "emea");
        let followed = ChannelMembership {
            moderator: true,
            followed: true,
            ..ChannelMembership::default()
        };
        let requests = client.channel_requests(&standard(), followed, "user-1", true, NOW_MS);
        assert_eq!(requests.len(), 2);
        assert!(requests[1].url.ends_with("name=isfollowed"));
    }

    #[test]
    fn general_and_team_visibility_write_one_property() {
        let transport = canned(Vec::new());
        let client = ChannelVisibility::with_transport(&transport, "emea");
        let general = ChannelTarget {
            general: true,
            ..standard()
        };
        let requests =
            client.channel_requests(&general, ChannelMembership::default(), "u", false, NOW_MS);
        assert_eq!(
            requests.iter().map(summary).collect::<Vec<_>>(),
            vec![(
                "PUT".to_owned(),
                format!(
                    "{BASE}/users/ME/conversations/{ENCODED}/properties?name=isGeneralChannelFavorite"
                ),
                Some(json!({"isGeneralChannelFavorite": "false"})),
            )]
        );
        let team = client.team_favorite_request(CHANNEL, true);
        assert_eq!(
            summary(&team),
            (
                "PUT".to_owned(),
                format!("{BASE}/users/ME/conversations/{ENCODED}/properties?name=favorite"),
                Some(json!({"favorite": "true"})),
            )
        );
        assert!(team.headers.is_empty());
    }

    #[test]
    fn membership_follows_javascript_truthiness() {
        let membership = parse_channel_membership(&json!({
            "memberProperties": {"isModerator": true, "explicitlyAdded": "true"},
            "properties": {"isfollowed": "false"},
        }));
        assert_eq!(
            membership,
            ChannelMembership {
                moderator: true,
                explicitly_added: true,
                followed: true,
            }
        );
        assert_eq!(
            parse_channel_membership(&json!({"properties": {"isfollowed": ""}})),
            ChannelMembership::default()
        );
        assert_eq!(
            ChannelKind::from_membership_type(Some("private")),
            ChannelKind::Private
        );
        assert_eq!(
            ChannelKind::from_membership_type(None),
            ChannelKind::Standard
        );
    }

    #[tokio::test]
    async fn set_channel_shown_reads_the_conversation_first() {
        let transport = canned(vec![(
            200,
            json!({"memberProperties": {"isModerator": true}}),
        )]);
        let client = ChannelVisibility::with_transport(&transport, "emea");
        client
            .set_channel_shown(&standard(), "user-1", false, NOW_MS)
            .await
            .unwrap();
        let urls: Vec<String> = transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.url.clone())
            .collect();
        assert_eq!(
            urls,
            vec![
                format!("{BASE}/users/ME/conversations/{ENCODED}?view=msnp24Equivalent"),
                format!("{BASE}/users/ME/conversations/{ENCODED}/properties?name=favorite"),
                format!("{BASE}/users/ME/conversations/{ENCODED}/properties?name=isfollowed"),
            ]
        );
    }

    #[tokio::test]
    async fn a_failed_step_stops_the_chain() {
        let transport = canned(vec![(200, json!({})), (403, json!({}))]);
        let client = ChannelVisibility::with_transport(&transport, "emea");
        let outcome = client
            .set_channel_shown(&standard(), "user-1", true, NOW_MS)
            .await;
        assert!(outcome.is_err());
        assert_eq!(transport.requests.lock().unwrap().len(), 2);
    }
}
