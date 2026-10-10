use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use session::{ApiResponse, GRAPH, Method, Request, Scope, Session};

use crate::error::{Error, Result};
use crate::reaction::{DEFAULT_SKIN_TONE, Reaction, reaction_message};
use crate::relay::ic3_scope;
use crate::timeline::Timeline;
use crate::trouter_events::{CallbackLinks, acceptance_links};
use crate::video_layout::MediaDescription;

pub const EPCONV_URL: &str = "https://api-emea.flightproxy.teams.microsoft.com/api/v2/epconv";
pub const ECHO_BOT_MRI: &str = "28:cf28171e-fcfd-47e4-a1d6-79460b0b3ca0";
pub const CHATSVC_REGION: &str = "emea";
const CLIENT_HEADER: &str = "SkypeSpaces/1415/26091712213/os=windows; osVer=NT 10.0; deviceType=computer; browser=chrome; browserVer=155.0.0.0/TsCallingVersion=2026.36.01.5/Ovb=97a9cc3f0dd74681da27f28dfcebcf05da30ef6e";
const PLATFORM_NAME: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/155.0.0.0 Safari/537.36";
const SDP_CONTENT_TYPE: &str = "application/sdp-ngc-1.0";
const ENDPOINT_CAPABILITIES: u64 = 73463;
const CLIENT_ENDPOINT_CAPABILITIES: u64 = 47150826;
const ALLOWED_HOST_SUFFIXES: [&str; 2] = [".flightproxy.teams.microsoft.com", ".teams.microsoft.com"];
const BACKEND_SUFFIX: &str = ".skype.com";
const FLIGHTPROXY_EP: &str = "https://api.flightproxy.teams.microsoft.com/api/v2/ep";
const CONVERSATION_LINK_NAMES: [&str; 9] = [
    "conversationEnd",
    "conversationUpdate",
    "localParticipantUpdate",
    "addParticipantSuccess",
    "addParticipantFailure",
    "addModalitySuccess",
    "addModalityFailure",
    "confirmUnmute",
    "receiveMessage",
];
const SUBSCRIBE_LINK_NAMES: [&str; 6] = [
    "conversationEnd",
    "conversationUpdate",
    "localParticipantUpdate",
    "addParticipantSuccess",
    "addParticipantFailure",
    "receiveMessage",
];
const CALL_LINK_NAMES: [&str; 5] = ["progress", "mediaAnswer", "acceptance", "redirection", "end"];
const AUDIO_MODALITIES: [&str; 1] = ["Audio"];
const MEETING_MESSAGE_ID: &str = "0";
const RAISE_HANDS: &str = "raiseHands";
const CANCEL_CODE: i64 = 487;
const DECLINE_CODE: i64 = 603;

#[derive(Debug, Clone)]
pub struct Participant {
    pub mri: String,
    pub display_name: String,
    pub endpoint_id: String,
    pub participant_id: String,
    pub language_id: String,
}

impl Participant {
    pub fn wire(&self) -> Value {
        json!({
            "id": self.mri,
            "displayName": self.display_name,
            "endpointId": self.endpoint_id,
            "participantId": self.participant_id,
            "languageId": self.language_id,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callee {
    pub mri: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeetingTarget {
    pub thread_id: String,
    pub tenant_id: String,
    pub organizer_id: String,
    pub meeting_data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InviteTarget {
    Echo,
    People { callees: Vec<Callee>, thread_id: String },
    Meeting(MeetingTarget),
}

#[derive(Debug, Clone)]
pub struct TenantRouting {
    pub region: String,
    pub partition: String,
    pub ring: String,
}

impl Default for TenantRouting {
    fn default() -> Self {
        TenantRouting {
            region: "de".into(),
            partition: "de01".into(),
            ring: "general".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Conversation {
    pub controller: String,
    pub links: BTreeMap<String, String>,
    pub chat_thread: Option<String>,
}

impl Conversation {
    pub fn link(&self, name: &str) -> Result<&str> {
        self.links
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| Error::Signaling(format!("conversation has no '{name}' link")))
    }
}

fn host_of(url: &str) -> Result<&str> {
    url.strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?']).next())
        .map(|authority| authority.split(':').next().unwrap_or(authority))
        .ok_or_else(|| Error::Signaling("link is not https".into()))
}

/// Bearer requests only go to Teams hosts; links come from server answers.
pub fn ensure_teams_url(url: &str) -> Result<()> {
    let host = host_of(url)?;
    if ALLOWED_HOST_SUFFIXES.iter().any(|suffix| host.ends_with(suffix)) || host == "teams.microsoft.com" {
        Ok(())
    } else {
        Err(Error::Signaling(format!("refusing link to host {host}")))
    }
}

fn is_backend_host(host: &str) -> bool {
    host.len() > BACKEND_SUFFIX.len()
        && host.ends_with(BACKEND_SUFFIX)
        && !host.starts_with('.')
        && !host.contains("..")
        && host
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-')
}

/// Push links may name a backend host directly; the browser always reaches those through flightproxy.
pub fn routed_url(url: &str) -> Result<String> {
    let host = host_of(url)?;
    if !is_backend_host(host) {
        ensure_teams_url(url)?;
        return Ok(url.to_owned());
    }
    let rest = &url["https://".len()..];
    let path = rest.find(['/', '?']).map_or("", |position| &rest[position..]);
    Ok(format!("{FLIGHTPROXY_EP}/{host}{path}"))
}

const FAILURE_BODY_CHARS: usize = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveReason {
    Hangup,
    Cancel,
}

pub struct Signaling {
    session: Session,
    poll_session: Session,
    chain_id: String,
    routing: TenantRouting,
    timeline: Timeline,
    endpoint_state_sequence: std::sync::atomic::AtomicU32,
    state_sequence: std::sync::atomic::AtomicU32,
}

pub struct Invitation<'a> {
    pub from: &'a Participant,
    pub offer_sdp: &'a str,
    pub media_leg_id: &'a str,
    pub callbacks: &'a CallbackLinks,
    pub target: &'a InviteTarget,
}

pub struct Answer<'a> {
    pub from: &'a Participant,
    pub answer_sdp: &'a str,
    pub media_leg_id: &'a str,
    pub callbacks: &'a CallbackLinks,
    pub modalities: &'a [String],
}

pub struct AttachRequest<'a> {
    pub from: &'a Participant,
    pub callbacks: &'a CallbackLinks,
    pub controller: Option<&'a str>,
    pub needs_media: bool,
}

#[derive(Debug, Clone)]
pub struct Attached {
    pub offer_sdp: Option<String>,
    pub from_mixer: bool,
    pub modalities: Vec<String>,
    pub links: BTreeMap<String, String>,
    pub conversation: Option<Conversation>,
}

impl Signaling {
    /// `poll_session` is a second CDP connection so the broker long poll never blocks the call requests.
    pub fn new(session: Session, poll_session: Session, routing: TenantRouting, timeline: Timeline) -> Self {
        Signaling {
            session,
            poll_session,
            chain_id: uuid::Uuid::new_v4().to_string(),
            routing,
            timeline,
            endpoint_state_sequence: std::sync::atomic::AtomicU32::new(2),
            state_sequence: std::sync::atomic::AtomicU32::new(0),
        }
    }

    fn request(&self, method: Method, url: &str, body: Option<Value>) -> Request {
        let mut request = match body {
            Some(body) => Request::with_body(method, url, body),
            None => Request { method, ..Request::get(url) },
        };
        request.headers = vec![
            ("X-Microsoft-Skype-Chain-ID".into(), self.chain_id.clone()),
            ("X-Microsoft-Skype-Message-ID".into(), uuid::Uuid::new_v4().to_string()),
            ("X-Microsoft-Skype-Client".into(), CLIENT_HEADER.into()),
            ("MS-Teams-Region".into(), self.routing.region.clone()),
            ("MS-Teams-Partition".into(), self.routing.partition.clone()),
            ("MS-Teams-Ring".into(), self.routing.ring.clone()),
            ("X-MS-Migration".into(), "True".into()),
            ("content-type".into(), "application/json".into()),
        ];
        request
    }

    async fn exchange(&self, session: &Session, label: &str, mut request: Request) -> Result<ApiResponse> {
        request.url = routed_url(&request.url)?;
        let response = session.batch(&[request], &ic3_scope()).await?.remove(0);
        self.timeline.record(label, format!("HTTP {}", response.status));
        if !response.is_success() {
            let reason: String = response.body.to_string().chars().take(FAILURE_BODY_CHARS).collect();
            return Err(Error::Signaling(format!("{label} answered HTTP {} {reason}", response.status)));
        }
        Ok(response)
    }

    async fn send(&self, label: &str, method: Method, url: &str, body: Option<Value>) -> Result<ApiResponse> {
        self.exchange(&self.session, label, self.request(method, url, body)).await
    }

    pub async fn post_json(&self, label: &str, url: &str, body: Value) -> Result<()> {
        self.send(label, Method::Post, url, Some(body)).await.map(|_| ())
    }

    pub async fn post_empty(&self, label: &str, url: &str) -> Result<()> {
        self.send(label, Method::Post, url, None).await.map(|_| ())
    }

    pub async fn create_conversation(&self, invitation: &Invitation<'_>) -> Result<Conversation> {
        let response = self
            .send("POST epconv", Method::Post, EPCONV_URL, Some(epconv_body(invitation)))
            .await?;
        conversation_from(&response.body)
    }

    pub async fn subscribe_meeting(
        &self,
        from: &Participant,
        callbacks: &CallbackLinks,
        meeting: &MeetingTarget,
    ) -> Result<Conversation> {
        let body = subscribe_body(from, callbacks, meeting);
        let response = self.send("POST epconv subscribe", Method::Post, EPCONV_URL, Some(body)).await?;
        conversation_from(&response.body)
    }

    /// Joins with the offer; the answer may restate the conversation links, otherwise the subscribed ones stay.
    pub async fn join_meeting(&self, subscribed: &Conversation, invitation: &Invitation<'_>) -> Result<Conversation> {
        let response = self
            .send("POST join", Method::Post, &subscribed.controller, Some(epconv_body(invitation)))
            .await?;
        let mut links = subscribed.links.clone();
        if let Ok(joined) = conversation_from(&response.body) {
            links.extend(joined.links);
        }
        Ok(Conversation {
            controller: subscribed.controller.clone(),
            links,
            chat_thread: subscribed.chat_thread.clone(),
        })
    }

    pub fn next_endpoint_sequence(&self) -> u32 {
        self.endpoint_state_sequence.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
    }

    pub async fn update_endpoint_state(&self, conversation: &Conversation, from: &Participant, muted: bool) -> Result<()> {
        let body = json!({
            "from": from.wire(),
            "endpointState": {
                "endpointStateSequenceNumber": self.next_endpoint_sequence(),
                "endpointProperties": {"additionalEndpointProperties": {"infoShownInReportMode": "FullInformation"}},
                "state": {"isMuted": muted},
            },
        });
        let url = conversation.link("updateEndpointState")?;
        self.post_json("POST updateEndpointState", url, body).await
    }

    fn next_state_sequence(&self) -> u32 {
        self.state_sequence.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
    }

    pub async fn raise_hand(&self, conversation: &Conversation, from: &Participant) -> Result<Option<String>> {
        let body = raise_hand_body(from.wire(), self.next_state_sequence());
        let response = self.send("POST publishState", Method::Post, conversation.link("publishState")?, Some(body)).await?;
        Ok(response.body["publishStateResponse"]["stateId"].as_str().map(str::to_owned))
    }

    pub async fn lower_hands(&self, conversation: &Conversation, from: &Participant, state_ids: &[String]) -> Result<()> {
        let body = remove_states_body(from.wire(), self.next_state_sequence(), state_ids);
        self.post_json("POST removeState", conversation.link("removeState")?, body).await
    }

    pub async fn lower_all_hands(&self, conversation: &Conversation, from: &Participant) -> Result<()> {
        let body = remove_all_hands_body(from.wire(), self.next_state_sequence());
        self.post_json("POST removeState", conversation.link("removeState")?, body).await
    }

    pub async fn send_reaction(&self, conversation: &Conversation, from: &Participant, reaction: Reaction) -> Result<()> {
        let body = reaction_message(from.wire(), reaction, &uuid::Uuid::new_v4().to_string(), &uuid::Uuid::new_v4().to_string());
        self.post_json("POST sendMessage", conversation.link("sendMessage")?, body).await
    }

    pub async fn update_media_descriptions(&self, url: &str, descriptions: &[MediaDescription], request_id: u32) -> Result<()> {
        let body = json!({
            "UpdateMediaDescriptions": {
                "mediaDescriptions": {
                    "descriptions": descriptions.iter().map(MediaDescription::wire).collect::<Vec<_>>(),
                    "requestId": request_id,
                },
            },
        });
        self.post_json("POST updateMediaDescriptions", url, body).await
    }

    pub async fn add_echo_bot(
        &self,
        conversation: &Conversation,
        from: &Participant,
        thread_id: Option<&str>,
        callbacks: &CallbackLinks,
    ) -> Result<()> {
        let body = json!({
            "disableUnmute": false,
            "participants": {
                "from": from.wire(),
                "to": [{"id": ECHO_BOT_MRI, "participantId": uuid::Uuid::new_v4().to_string()}],
            },
            "participantInvitationData": {},
            "replacementDetails": null,
            "groupContext": null,
            "groupChat": thread_id.map(|thread_id| json!({"threadId": thread_id, "messageId": null})),
            "links": {
                "addParticipantSuccess": callbacks.conversation("addParticipantSuccess"),
                "addParticipantFailure": callbacks.conversation("addParticipantFailure"),
            },
        });
        let url = conversation.link("addParticipantAndModality")?;
        self.post_json("POST add (echo bot)", url, body).await
    }

    pub async fn update_endpoint_metadata(&self, conversation: &Conversation, from: &Participant) -> Result<()> {
        let body = json!({
            "participants": {"from": from.wire()},
            "endpointMetadata": {"holographicCapabilities": 3},
        });
        let url = conversation.link("updateEndpointMetadata")?;
        self.send("PUT updateEndpointMetadata", Method::Put, url, Some(body)).await.map(|_| ())
    }

    /// One broker long poll; answers the next subscribe URL.
    pub async fn poll_broker(&self, url: &str) -> Result<Option<String>> {
        let mut request = self.request(Method::Get, url, None);
        request.headers.push(("X-UseBatching".into(), "1".into()));
        let response = self.exchange(&self.poll_session, "GET broker subscribe", request).await?;
        Ok(response.body["nextSubscribeUrl"].as_str().map(str::to_owned))
    }

    pub async fn leave(&self, conversation: &Conversation, from: &Participant, reason: LeaveReason) -> Result<()> {
        let url = conversation.link("leave")?;
        self.post_json("POST leave", url, leave_body(from, reason)).await
    }

    pub async fn end_for_all(&self, conversation: &Conversation, from: &Participant) -> Result<()> {
        self.send(
            "DELETE conversation",
            Method::Delete,
            &conversation.controller,
            Some(end_for_all_body(from)),
        )
        .await
        .map(|_| ())
    }

    pub async fn attach(&self, url: &str, request: &AttachRequest<'_>) -> Result<Attached> {
        let response = self
            .send("POST attach", Method::Post, url, Some(attach_body(request)))
            .await?;
        attached_from(&response.body)
    }

    pub async fn send_ringing(&self, url: &str, from: &Participant) -> Result<()> {
        self.post_json("POST progress", url, ringing_body(from)).await
    }

    pub async fn accept(&self, url: &str, answer: &Answer<'_>) -> Result<()> {
        self.post_json("POST acceptance", url, acceptance_body(answer)).await
    }

    pub async fn decline(&self, call_leg_url: &str) -> Result<()> {
        self.send("DELETE callLeg (decline)", Method::Delete, call_leg_url, Some(decline_body()))
            .await
            .map(|_| ())
    }

    pub async fn keep_call_alive(&self, call_leg_url: &str) -> Result<()> {
        self.post_json("POST callLeg keep-alive", call_leg_url, json!({"callParticipantUpdate": {}}))
            .await
    }

    pub async fn keep_conversation_alive(
        &self,
        conversation: &Conversation,
        from: &Participant,
        callbacks: &CallbackLinks,
    ) -> Result<()> {
        let url = conversation.link("notificationLinks")?;
        self.post_json("POST notificationLinks", url, notification_links_body(from, callbacks)).await
    }
}

fn link_map(names: &[&str], make: impl Fn(&str) -> String) -> Map<String, Value> {
    names
        .iter()
        .map(|name| ((*name).to_owned(), Value::String(make(name))))
        .collect()
}

fn roster_request(callbacks: &CallbackLinks) -> Value {
    json!({"type": "Delta", "rosterUpdate": callbacks.conversation("rosterUpdate")})
}

fn conversation_properties() -> Value {
    json!({
        "allowConversationWithoutHost": true,
        "enableGroupCallEventMessages": true,
        "enableGroupCallUpgradeMessage": false,
        "enableGroupCallMeetupGeneration": false,
    })
}

fn endpoint_state() -> Value {
    json!({
        "endpointStateSequenceNumber": 2,
        "endpointProperties": {"additionalEndpointProperties": {"infoShownInReportMode": "FullInformation"}},
    })
}

fn media_content(sdp: &str, media_leg_id: &str, with_descriptions: bool) -> Value {
    let mut content = json!({
        "contentType": SDP_CONTENT_TYPE,
        "blob": sdp,
        "clientLocation": "DE",
        "mediaLegId": media_leg_id,
    });
    if with_descriptions {
        content["mediaDescriptions"] = json!({"descriptions": [], "requestId": 1});
    }
    content
}

fn media_controller_links(callbacks: &CallbackLinks) -> Value {
    json!({
        "controlVideoStreaming": callbacks.call("controlVideoStreaming"),
        "csrcInfo": callbacks.call("csrcInfo"),
    })
}

fn group_chat(target: &InviteTarget) -> Value {
    match target {
        InviteTarget::Echo => Value::Null,
        InviteTarget::People { thread_id, .. } => json!({"threadId": thread_id, "messageId": null}),
        InviteTarget::Meeting(meeting) => json!({"threadId": meeting.thread_id, "messageId": MEETING_MESSAGE_ID}),
    }
}

fn callee_wire(callee: &Callee) -> Value {
    json!({"id": callee.mri, "displayName": callee.display_name})
}

pub fn epconv_body(invitation: &Invitation<'_>) -> Value {
    let callbacks = invitation.callbacks;
    let target = invitation.target;
    let to: Vec<Value> = match target {
        InviteTarget::People { callees, .. } => callees.iter().map(callee_wire).collect(),
        InviteTarget::Echo | InviteTarget::Meeting(_) => Vec::new(),
    };
    let mut request = json!({
        "conversationType": null,
        "subject": null,
        "suppressDialout": true,
        "roster": roster_request(callbacks),
        "properties": conversation_properties(),
        "links": link_map(&CONVERSATION_LINK_NAMES, |name| callbacks.conversation(name)),
    });
    if matches!(target, InviteTarget::Echo) {
        request["scenario"] = json!("UserInitiatedTestCall");
    }
    let (meeting_info, meeting_data) = match target {
        InviteTarget::Meeting(meeting) => (
            json!({"tenantId": meeting.tenant_id, "organizerId": meeting.organizer_id}),
            meeting.meeting_data.clone().unwrap_or(Value::Null),
        ),
        _ => (Value::Null, Value::Null),
    };
    json!({
        "conversationRequest": request,
        "contentSharing": null,
        "participants": {"from": invitation.from.wire(), "to": to},
        "capabilities": null,
        "endpointCapabilities": ENDPOINT_CAPABILITIES,
        "clientEndpointCapabilities": CLIENT_ENDPOINT_CAPABILITIES,
        "endpointMetadata": {"holographicCapabilities": 3},
        "groupContext": null,
        "groupChat": group_chat(target),
        "meetingInfo": meeting_info,
        "meetingData": meeting_data,
        "endpointState": endpoint_state(),
        "callInvitation": {
            "callModalities": AUDIO_MODALITIES,
            "replaces": null,
            "transferor": null,
            "clientTransferContext": null,
            "customContext": null,
            "links": link_map(&CALL_LINK_NAMES, |name| callbacks.call(name)),
            "clientContentForMediaController": media_controller_links(callbacks),
            "pstnContent": {"emergencyCallCountry": "", "platformName": PLATFORM_NAME, "publicApiCall": false},
            "emergencyContent": null,
            "mediaContent": media_content(invitation.offer_sdp, invitation.media_leg_id, true),
        },
    })
}

pub fn subscribe_body(from: &Participant, callbacks: &CallbackLinks, meeting: &MeetingTarget) -> Value {
    json!({
        "conversationRequest": {
            "roster": roster_request(callbacks),
            "properties": conversation_properties(),
            "links": link_map(&SUBSCRIBE_LINK_NAMES, |name| callbacks.conversation(name)),
        },
        "participants": {"from": from.wire()},
        "groupChat": {"threadId": meeting.thread_id, "messageId": MEETING_MESSAGE_ID},
        "meetingInfo": {"tenantId": meeting.tenant_id, "organizerId": meeting.organizer_id},
        "endpointState": endpoint_state(),
    })
}

pub fn leave_body(from: &Participant, reason: LeaveReason) -> Value {
    let call_end = match reason {
        LeaveReason::Hangup => json!({"code": 0, "subCode": 0, "phrase": "CallEndReasonLocalUserInitiated", "resultCategories": ["Success"]}),
        LeaveReason::Cancel => json!({"code": CANCEL_CODE, "subCode": 0, "phrase": "CallEndReasonLocalUserInitiated"}),
    };
    json!({
        "participants": {"from": from.wire()},
        "conversationTransactionEnd": {"reason": "noError", "code": 0, "phrase": "ConversationEndNoModalityConnected"},
        "callTransactionEnd": call_end,
    })
}

pub fn end_for_all_body(from: &Participant) -> Value {
    json!({
        "participants": {"from": from.wire()},
        "conversationTransactionEnd": {"reason": "noError", "code": 0, "phrase": "ConversationEndForAllInitiated"},
        "callTransactionEnd": {"code": 0, "phrase": "CallEndReasonLocalUserInitiated"},
    })
}

pub fn decline_body() -> Value {
    json!({"callEnd": {"code": DECLINE_CODE, "subCode": 0, "phrase": "CallEndReasonLocalUserInitiated", "resultCategories": ["Success"]}})
}

pub fn ringing_body(from: &Participant) -> Value {
    json!({"callProgress": {"sender": from.wire(), "status": "ringing", "phrase": "ringing"}})
}

pub fn attach_body(request: &AttachRequest<'_>) -> Value {
    let callbacks = request.callbacks;
    let mut body = json!({
        "attach": {
            "requireMediaContent": request.needs_media,
            "links": {"end": callbacks.call("end")},
        },
        "capabilities": null,
        "endpointCapabilities": ENDPOINT_CAPABILITIES,
    });
    if let Some(controller) = request.controller {
        body["additionalActions"] = json!([{
            "name": "join",
            "url": controller,
            "waitForResponse": true,
            "input": {
                "conversationRequest": {
                    "roster": roster_request(callbacks),
                    "links": link_map(&SUBSCRIBE_LINK_NAMES, |name| callbacks.conversation(name)),
                },
                "participants": {"from": request.from.wire()},
                "endpointMetadata": {"holographicCapabilities": 3},
                "endpointCapabilities": ENDPOINT_CAPABILITIES,
            },
        }]);
    }
    body
}

pub fn acceptance_body(answer: &Answer<'_>) -> Value {
    let callbacks = answer.callbacks;
    json!({
        "callAcceptance": {
            "acceptedBy": answer.from.wire(),
            "acceptedCallModalities": answer.modalities,
            "endpointCapabilities": ENDPOINT_CAPABILITIES,
            "clientEndpointCapabilities": CLIENT_ENDPOINT_CAPABILITIES,
            "links": acceptance_links(callbacks),
            "clientContentForMediaController": media_controller_links(callbacks),
            "mediaContent": media_content(answer.answer_sdp, answer.media_leg_id, false),
            "pstnContent": {"emergencyCallCountry": "", "platformName": PLATFORM_NAME, "publicApiCall": false},
            "callKeepAliveInterval": null,
        },
    })
}

pub fn renegotiation_body(
    from: &Participant,
    callbacks: &CallbackLinks,
    offer_sdp: &str,
    media_leg_id: &str,
) -> Value {
    json!({
        "mediaNegotiation": {
            "callModalities": AUDIO_MODALITIES,
            "sender": from.wire(),
            "links": {
                "mediaAnswer": callbacks.call("mediaAnswer"),
                "rejection": callbacks.call("rejection"),
            },
            "mediaContent": renegotiation_content(offer_sdp, media_leg_id),
        },
    })
}

fn renegotiation_content(sdp: &str, media_leg_id: &str) -> Value {
    let mut content = media_content(sdp, media_leg_id, true);
    content["newOffer"] = json!(true);
    content
}

pub fn escalation_answer_body(
    from: &Participant,
    callbacks: &CallbackLinks,
    answer_sdp: &str,
    media_leg_id: &str,
) -> Value {
    json!({
        "mediaAnswer": {
            "callModalities": AUDIO_MODALITIES,
            "sender": from.wire(),
            "links": {"mediaAcknowledgement": callbacks.call("mediaAcknowledgement")},
            "clientContentForMediaController": media_controller_links(callbacks),
            "mediaContent": media_content(answer_sdp, media_leg_id, false),
        },
    })
}

pub fn notification_links_body(from: &Participant, callbacks: &CallbackLinks) -> Value {
    json!({
        "participants": {"from": from.wire()},
        "roster": roster_request(callbacks),
        "links": link_map(&CONVERSATION_LINK_NAMES, |name| callbacks.conversation(name)),
    })
}

fn conversation_from(body: &Value) -> Result<Conversation> {
    let controller = body["conversationController"]
        .as_str()
        .ok_or_else(|| Error::Signaling("answer without conversationController".into()))?
        .to_owned();
    let links = body["links"]
        .as_object()
        .map(|links| {
            links
                .iter()
                .filter_map(|(name, url)| Some((name.clone(), url.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    Ok(Conversation { controller, links, chat_thread: chat_thread_id(body) })
}

pub fn raise_hand_body(from: Value, sequence_number: u32) -> Value {
    json!({
        "from": from,
        "publishedState": {
            "stateType": RAISE_HANDS,
            "level": "user",
            "content": {"skinTone": DEFAULT_SKIN_TONE},
            "sequenceNumber": sequence_number,
        },
    })
}

pub fn remove_states_body(from: Value, sequence_number: u32, state_ids: &[String]) -> Value {
    json!({"from": from, "sequenceNumber": sequence_number, "scope": "specified", "stateIds": state_ids})
}

pub fn remove_all_hands_body(from: Value, sequence_number: u32) -> Value {
    json!({"from": from, "sequenceNumber": sequence_number, "scope": "all", "stateType": RAISE_HANDS})
}

pub fn chat_thread_id(body: &Value) -> Option<String> {
    body["activeModalities"]["groupChat"]["threadId"].as_str().map(str::to_owned)
}

fn attached_from(body: &Value) -> Result<Attached> {
    let invitation = &body["callInvitation"];
    if invitation.is_null() {
        return Err(Error::Signaling("attach answer without callInvitation".into()));
    }
    let links = invitation["links"]
        .as_object()
        .map(|links| {
            links
                .iter()
                .filter_map(|(name, url)| Some((name.clone(), url.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let modalities = invitation["callModalities"]
        .as_array()
        .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    Ok(Attached {
        offer_sdp: invitation["mediaContent"]["blob"].as_str().map(str::to_owned),
        from_mixer: invitation["mediaContent"]["fromMixer"].as_bool().unwrap_or(false),
        modalities,
        links,
        conversation: conversation_from(&body["additionalActionResponses"][0]["output"]).ok(),
    })
}

#[derive(Debug, Clone)]
pub struct SelfIdentity {
    pub object_id: String,
    pub display_name: String,
}

pub async fn fetch_self(session: &Session) -> Result<SelfIdentity> {
    let response = session
        .request(
            Method::Get,
            &format!("{GRAPH}/v1.0/me?$select=id,displayName"),
            &Scope::graph("User.Read"),
            None,
        )
        .await?;
    Ok(SelfIdentity {
        object_id: response.body["id"]
            .as_str()
            .ok_or_else(|| Error::Signaling("graph /me without id".into()))?
            .to_owned(),
        display_name: response.body["displayName"].as_str().unwrap_or_default().to_owned(),
    })
}

/// The 1:1 chat with the Echo bot; Teams sends it as `groupChat` when adding the bot.
pub async fn find_echo_bot_thread(session: &Session, self_object_id: &str) -> Result<Option<String>> {
    let bot_id = ECHO_BOT_MRI.trim_start_matches("28:");
    for thread_id in [
        format!("19:{self_object_id}_{bot_id}@unq.gbl.spaces"),
        format!("19:{bot_id}_{self_object_id}@unq.gbl.spaces"),
    ] {
        let url = format!(
            "https://teams.cloud.microsoft/api/chatsvc/{CHATSVC_REGION}/v1/users/ME/conversations/{}?view=msnp24Equivalent",
            thread_id.replace(':', "%3A").replace('@', "%40")
        );
        let response = session.batch(&[Request::get(url)], &ic3_scope()).await?.remove(0);
        if response.is_success() {
            return Ok(Some(thread_id));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raising_publishes_a_raise_hands_state_and_lowering_names_the_state() {
        let raised = raise_hand_body(json!({"id": "8:orgid:me"}), 4);
        assert_eq!(raised["publishedState"]["stateType"], "raiseHands");
        assert_eq!(raised["publishedState"]["level"], "user");
        assert_eq!(raised["publishedState"]["sequenceNumber"], 4);
        let lowered = remove_states_body(json!({"id": "8:orgid:me"}), 5, &["s-1".to_owned()]);
        assert_eq!(lowered["scope"], "specified");
        assert_eq!(lowered["stateIds"][0], "s-1");
        let all = remove_all_hands_body(json!({"id": "8:orgid:me"}), 6);
        assert_eq!(all["scope"], "all");
        assert_eq!(all["stateType"], "raiseHands");
    }

    #[test]
    fn the_meeting_chat_thread_comes_from_the_group_chat_modality() {
        let body = json!({"activeModalities": {"groupChat": {"threadId": "19:meeting_abc@thread.v2"}}});
        assert_eq!(chat_thread_id(&body).as_deref(), Some("19:meeting_abc@thread.v2"));
        assert_eq!(chat_thread_id(&json!({"activeModalities": {}})), None);
    }

    fn from() -> Participant {
        Participant {
            mri: "8:orgid:00000000-0000-0000-0000-000000000001".into(),
            display_name: "Test".into(),
            endpoint_id: "e".into(),
            participant_id: "p".into(),
            language_id: "en-gb".into(),
        }
    }

    fn callbacks() -> CallbackLinks {
        CallbackLinks::new("https://pub-ent-x-f.trouter.teams.microsoft.com:3443/v4/f/abc/", "call-1")
    }

    fn body_for(target: &InviteTarget) -> Value {
        let from = from();
        let callbacks = callbacks();
        epconv_body(&Invitation {
            from: &from,
            offer_sdp: "v=0\r\n",
            media_leg_id: "AB",
            callbacks: &callbacks,
            target,
        })
    }

    fn callee(mri: &str, name: &str) -> Callee {
        Callee {
            mri: mri.into(),
            display_name: name.into(),
        }
    }

    #[test]
    fn only_teams_hosts_get_the_bearer() {
        assert!(ensure_teams_url("https://api.flightproxy.teams.microsoft.com/api/v2/ep/conv-x/conv/1/leave?i=1").is_ok());
        assert!(ensure_teams_url(EPCONV_URL).is_ok());
        assert!(ensure_teams_url("https://teams.microsoft.com/trap/tokens").is_ok());
        assert!(ensure_teams_url("https://conv-euno-06-prod-aks.conv.skype.com/conv/1").is_err());
        assert!(ensure_teams_url("https://evil.example/?x=.teams.microsoft.com").is_err());
        assert!(ensure_teams_url("http://api.flightproxy.teams.microsoft.com/x").is_err());
    }

    #[test]
    fn backend_links_go_through_flightproxy() {
        assert_eq!(
            routed_url("https://cc-euno-06-prod-aks.cc.skype.com/cc/v1/active/1/attach?i=2").unwrap(),
            "https://api.flightproxy.teams.microsoft.com/api/v2/ep/cc-euno-06-prod-aks.cc.skype.com/cc/v1/active/1/attach?i=2"
        );
        assert_eq!(
            routed_url("https://api.flightproxy.teams.microsoft.com/api/v2/ep/x/y").unwrap(),
            "https://api.flightproxy.teams.microsoft.com/api/v2/ep/x/y"
        );
        assert!(routed_url("https://evil.skype.com.example/x").is_err());
        assert!(routed_url("https://Evil.skype.com/x").is_err());
        assert!(routed_url("https://user@cc.skype.com/x").is_err());
        assert!(routed_url("http://cc.skype.com/x").is_err());
        assert!(routed_url("https://.skype.com/x").is_err());
    }

    #[test]
    fn echo_body_matches_the_captured_test_call_shape() {
        let body = body_for(&InviteTarget::Echo);
        assert_eq!(body["conversationRequest"]["scenario"], "UserInitiatedTestCall");
        assert_eq!(body["participants"]["to"], json!([]));
        assert_eq!(body["groupChat"], Value::Null);
        assert_eq!(body["callInvitation"]["callModalities"], json!(["Audio"]));
        assert_eq!(body["callInvitation"]["mediaContent"]["contentType"], SDP_CONTENT_TYPE);
        assert!(body["callInvitation"]["links"]["acceptance"].as_str().unwrap().ends_with("/call/acceptance/"));
        assert_eq!(body["conversationRequest"]["links"].as_object().unwrap().len(), 9);
    }

    #[test]
    fn one_to_one_body_names_the_callee_and_the_chat() {
        let target = InviteTarget::People {
            callees: vec![callee("8:orgid:b", "Bea")],
            thread_id: "19:a_b@unq.gbl.spaces".into(),
        };
        let body = body_for(&target);
        assert_eq!(body["participants"]["to"], json!([{"id": "8:orgid:b", "displayName": "Bea"}]));
        assert_eq!(body["groupChat"], json!({"threadId": "19:a_b@unq.gbl.spaces", "messageId": null}));
        assert!(body["conversationRequest"].get("scenario").is_none());
        assert_eq!(body["meetingInfo"], Value::Null);
        assert_eq!(body["callInvitation"]["callModalities"], json!(["Audio"]));
    }

    #[test]
    fn group_body_invites_every_callee_on_the_group_thread() {
        let target = InviteTarget::People {
            callees: vec![callee("8:orgid:b", "Bea"), callee("8:orgid:c", "Cy"), callee("8:orgid:d", "Di")],
            thread_id: "19:group@thread.v2".into(),
        };
        let body = body_for(&target);
        let to = body["participants"]["to"].as_array().unwrap();
        assert_eq!(to.len(), 3);
        assert_eq!(to[2]["id"], "8:orgid:d");
        assert_eq!(body["groupChat"]["threadId"], "19:group@thread.v2");
        assert_eq!(body["groupChat"]["messageId"], Value::Null);
    }

    #[test]
    fn meeting_join_body_carries_thread_and_meeting_info() {
        let target = InviteTarget::Meeting(MeetingTarget {
            thread_id: "19:meeting_x@thread.v2".into(),
            tenant_id: "tenant".into(),
            organizer_id: "organizer".into(),
            meeting_data: Some(json!({"meetingCode": "123"})),
        });
        let body = body_for(&target);
        assert_eq!(body["groupChat"], json!({"threadId": "19:meeting_x@thread.v2", "messageId": "0"}));
        assert_eq!(body["meetingInfo"], json!({"tenantId": "tenant", "organizerId": "organizer"}));
        assert_eq!(body["meetingData"]["meetingCode"], "123");
        assert_eq!(body["participants"]["to"], json!([]));
        assert!(body["conversationRequest"].get("scenario").is_none());
        assert!(body["endpointState"]["endpointProperties"].get("preheatProperties").is_none());
    }

    #[test]
    fn meeting_subscribe_is_roster_only() {
        let meeting = MeetingTarget {
            thread_id: "19:meeting_x@thread.v2".into(),
            tenant_id: "tenant".into(),
            organizer_id: "organizer".into(),
            meeting_data: None,
        };
        let body = subscribe_body(&from(), &callbacks(), &meeting);
        assert!(body.get("callInvitation").is_none());
        assert!(body["participants"].get("to").is_none());
        assert_eq!(body["conversationRequest"]["links"].as_object().unwrap().len(), 6);
        assert_eq!(body["groupChat"]["messageId"], "0");
        assert_eq!(body["meetingInfo"]["organizerId"], "organizer");
    }

    #[test]
    fn leave_bodies_differ_by_reason() {
        let hangup = leave_body(&from(), LeaveReason::Hangup);
        assert_eq!(hangup["callTransactionEnd"]["code"], 0);
        let cancel = leave_body(&from(), LeaveReason::Cancel);
        assert_eq!(cancel["callTransactionEnd"]["code"], 487);
        assert_eq!(cancel["callTransactionEnd"]["phrase"], "CallEndReasonLocalUserInitiated");
        assert_eq!(cancel["conversationTransactionEnd"]["phrase"], "ConversationEndNoModalityConnected");
        let end = end_for_all_body(&from());
        assert_eq!(end["conversationTransactionEnd"]["phrase"], "ConversationEndForAllInitiated");
    }

    #[test]
    fn decline_uses_603() {
        let body = decline_body();
        assert_eq!(body["callEnd"]["code"], 603);
        assert_eq!(body["callEnd"]["resultCategories"], json!(["Success"]));
    }

    #[test]
    fn attach_asks_for_media_and_joins_the_conversation() {
        let from = from();
        let callbacks = callbacks();
        let body = attach_body(&AttachRequest {
            from: &from,
            callbacks: &callbacks,
            controller: Some("https://conv/controller"),
            needs_media: true,
        });
        assert_eq!(body["attach"]["requireMediaContent"], true);
        assert!(body["attach"]["links"]["end"].as_str().unwrap().ends_with("/call/end/"));
        let action = &body["additionalActions"][0];
        assert_eq!(action["name"], "join");
        assert_eq!(action["url"], "https://conv/controller");
        assert_eq!(action["waitForResponse"], true);
        assert_eq!(action["input"]["conversationRequest"]["links"].as_object().unwrap().len(), 6);
        let without = attach_body(&AttachRequest {
            from: &from,
            callbacks: &callbacks,
            controller: None,
            needs_media: false,
        });
        assert!(without.get("additionalActions").is_none());
        assert_eq!(without["attach"]["requireMediaContent"], false);
    }

    #[test]
    fn attach_answer_yields_offer_links_and_conversation() {
        let body = json!({
            "callInvitation": {
                "links": {"acceptance": "https://cc/accept", "callLeg": "https://cc/leg", "progress": "https://cc/progress"},
                "mediaContent": {"blob": "v=0\r\n"},
                "callModalities": ["Audio"],
            },
            "additionalActionResponses": [{"output": {"conversationController": "https://conv/c", "links": {"leave": "https://conv/leave"}}}],
        });
        let attached = attached_from(&body).unwrap();
        assert_eq!(attached.offer_sdp.as_deref(), Some("v=0\r\n"));
        assert_eq!(attached.links["callLeg"], "https://cc/leg");
        assert_eq!(attached.modalities, vec!["Audio".to_owned()]);
        let conversation = attached.conversation.unwrap();
        assert_eq!(conversation.controller, "https://conv/c");
        assert_eq!(conversation.link("leave").unwrap(), "https://conv/leave");
        assert!(attached_from(&json!({})).is_err());
    }

    #[test]
    fn acceptance_carries_the_answer_and_callback_links() {
        let from = from();
        let callbacks = callbacks();
        let modalities = vec!["Audio".to_owned()];
        let body = acceptance_body(&Answer {
            from: &from,
            answer_sdp: "v=0\r\n",
            media_leg_id: "AB",
            callbacks: &callbacks,
            modalities: &modalities,
        });
        let accepted = &body["callAcceptance"];
        assert_eq!(accepted["acceptedBy"]["id"], from.mri);
        assert_eq!(accepted["acceptedCallModalities"], json!(["Audio"]));
        assert_eq!(accepted["mediaContent"]["blob"], "v=0\r\n");
        assert_eq!(accepted["links"].as_object().unwrap().len(), 7);
        assert!(accepted["links"]["mediaRenegotiation"].as_str().unwrap().ends_with("/call/mediaRenegotiation/"));
        assert_eq!(accepted["callKeepAliveInterval"], Value::Null);
    }

    #[test]
    fn renegotiation_messages_name_their_reply_links() {
        let from = from();
        let callbacks = callbacks();
        let offer = renegotiation_body(&from, &callbacks, "v=0\r\n", "AB");
        let negotiation = &offer["mediaNegotiation"];
        assert_eq!(negotiation["mediaContent"]["newOffer"], true);
        assert!(negotiation["links"]["mediaAnswer"].as_str().unwrap().ends_with("/call/mediaAnswer/"));
        assert!(negotiation["links"]["rejection"].as_str().unwrap().ends_with("/call/rejection/"));
        let answer = escalation_answer_body(&from, &callbacks, "v=0\r\n", "AB");
        assert!(answer["mediaAnswer"]["links"]["mediaAcknowledgement"]
            .as_str()
            .unwrap()
            .ends_with("/call/mediaAcknowledgement/"));
    }

    #[test]
    fn ringing_progress_names_the_sender() {
        let body = ringing_body(&from());
        assert_eq!(body["callProgress"]["status"], "ringing");
        assert_eq!(body["callProgress"]["sender"]["id"], from().mri);
    }
}
