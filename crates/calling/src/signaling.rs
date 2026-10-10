use std::collections::BTreeMap;

use serde_json::{Value, json};
use session::{ApiResponse, GRAPH, Method, Request, Scope, Session};

use crate::error::{Error, Result};
use crate::relay::ic3_scope;
use crate::timeline::Timeline;
use crate::trouter_events::CallbackLinks;

pub const EPCONV_URL: &str = "https://api-emea.flightproxy.teams.microsoft.com/api/v2/epconv";
pub const ECHO_BOT_MRI: &str = "28:cf28171e-fcfd-47e4-a1d6-79460b0b3ca0";
pub const CHATSVC_REGION: &str = "emea";
const CLIENT_HEADER: &str = "SkypeSpaces/1415/26091712213/os=windows; osVer=NT 10.0; deviceType=computer; browser=chrome; browserVer=155.0.0.0/TsCallingVersion=2026.36.01.5/Ovb=97a9cc3f0dd74681da27f28dfcebcf05da30ef6e";
const PLATFORM_NAME: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/155.0.0.0 Safari/537.36";
const SDP_CONTENT_TYPE: &str = "application/sdp-ngc-1.0";
const ENDPOINT_CAPABILITIES: u64 = 73463;
const CLIENT_ENDPOINT_CAPABILITIES: u64 = 47150826;
const ALLOWED_HOST_SUFFIXES: [&str; 2] = [".flightproxy.teams.microsoft.com", ".teams.microsoft.com"];

#[derive(Debug, Clone)]
pub struct Participant {
    pub mri: String,
    pub display_name: String,
    pub endpoint_id: String,
    pub participant_id: String,
    pub language_id: String,
}

impl Participant {
    fn wire(&self) -> Value {
        json!({
            "id": self.mri,
            "displayName": self.display_name,
            "endpointId": self.endpoint_id,
            "participantId": self.participant_id,
            "languageId": self.language_id,
        })
    }
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
}

impl Conversation {
    pub fn link(&self, name: &str) -> Result<&str> {
        self.links
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| Error::Signaling(format!("conversation has no '{name}' link")))
    }
}

/// Bearer requests only go to Teams hosts; links come from server answers.
pub fn ensure_teams_url(url: &str) -> Result<()> {
    let host = url
        .strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?']).next())
        .map(|authority| authority.split(':').next().unwrap_or(authority))
        .ok_or_else(|| Error::Signaling("link is not https".into()))?;
    if ALLOWED_HOST_SUFFIXES.iter().any(|suffix| host.ends_with(suffix)) || host == "teams.microsoft.com" {
        Ok(())
    } else {
        Err(Error::Signaling(format!("refusing link to host {host}")))
    }
}

pub struct Signaling {
    session: Session,
    poll_session: Session,
    chain_id: String,
    routing: TenantRouting,
    timeline: Timeline,
    endpoint_state_sequence: std::sync::atomic::AtomicU32,
}

pub struct EchoInvitation<'a> {
    pub from: &'a Participant,
    pub offer_sdp: &'a str,
    pub media_leg_id: &'a str,
    pub callbacks: &'a CallbackLinks,
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

    async fn exchange(&self, session: &Session, label: &str, request: Request) -> Result<ApiResponse> {
        ensure_teams_url(&request.url)?;
        let response = session.batch(&[request], &ic3_scope()).await?.remove(0);
        self.timeline.record(label, format!("HTTP {}", response.status));
        if !response.is_success() {
            return Err(Error::Signaling(format!("{label} answered HTTP {}", response.status)));
        }
        Ok(response)
    }

    pub async fn create_conversation(&self, invitation: &EchoInvitation<'_>) -> Result<Conversation> {
        let body = epconv_body(invitation);
        let response = self
            .exchange(&self.session, "POST epconv", self.request(Method::Post, EPCONV_URL, Some(body)))
            .await?;
        conversation_from(&response.body)
    }

    pub async fn update_endpoint_state(&self, conversation: &Conversation, from: &Participant, muted: bool) -> Result<()> {
        let sequence = self
            .endpoint_state_sequence
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let body = json!({
            "from": from.wire(),
            "endpointState": {
                "endpointStateSequenceNumber": sequence,
                "endpointProperties": {"additionalEndpointProperties": {"infoShownInReportMode": "FullInformation"}},
                "state": {"isMuted": muted},
            },
        });
        let url = conversation.link("updateEndpointState")?;
        self.exchange(&self.session, "POST updateEndpointState", self.request(Method::Post, url, Some(body)))
            .await
            .map(|_| ())
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
        self.exchange(&self.session, "POST add (echo bot)", self.request(Method::Post, url, Some(body)))
            .await
            .map(|_| ())
    }

    pub async fn update_endpoint_metadata(&self, conversation: &Conversation, from: &Participant) -> Result<()> {
        let body = json!({
            "participants": {"from": from.wire()},
            "endpointMetadata": {"holographicCapabilities": 3},
        });
        let url = conversation.link("updateEndpointMetadata")?;
        self.exchange(&self.session, "PUT updateEndpointMetadata", self.request(Method::Put, url, Some(body)))
            .await
            .map(|_| ())
    }

    /// One broker long poll; answers the next subscribe URL.
    pub async fn poll_broker(&self, url: &str) -> Result<Option<String>> {
        let mut request = self.request(Method::Get, url, None);
        request.headers.push(("X-UseBatching".into(), "1".into()));
        let response = self.exchange(&self.poll_session, "GET broker subscribe", request).await?;
        Ok(response.body["nextSubscribeUrl"].as_str().map(str::to_owned))
    }

    pub async fn leave(&self, conversation: &Conversation, from: &Participant) -> Result<()> {
        let body = json!({
            "participants": {"from": from.wire()},
            "conversationTransactionEnd": {"reason": "noError", "code": 0, "phrase": "ConversationEndNoModalityConnected"},
            "callTransactionEnd": {"code": 0, "subCode": 0, "phrase": "CallEndReasonLocalUserInitiated", "resultCategories": ["Success"]},
        });
        let url = conversation.link("leave")?;
        self.exchange(&self.session, "POST leave", self.request(Method::Post, url, Some(body)))
            .await
            .map(|_| ())
    }
}

pub fn epconv_body(invitation: &EchoInvitation<'_>) -> Value {
    let callbacks = invitation.callbacks;
    let conversation_links: serde_json::Map<String, Value> = [
        "conversationEnd",
        "conversationUpdate",
        "localParticipantUpdate",
        "addParticipantSuccess",
        "addParticipantFailure",
        "addModalitySuccess",
        "addModalityFailure",
        "confirmUnmute",
        "receiveMessage",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), Value::String(callbacks.conversation(name))))
    .collect();
    let call_links: serde_json::Map<String, Value> = ["progress", "mediaAnswer", "acceptance", "redirection", "end"]
        .into_iter()
        .map(|name| (name.to_owned(), Value::String(callbacks.call(name))))
        .collect();
    json!({
        "conversationRequest": {
            "conversationType": null,
            "subject": null,
            "suppressDialout": true,
            "roster": {"type": "Delta", "rosterUpdate": callbacks.conversation("rosterUpdate")},
            "properties": {
                "allowConversationWithoutHost": true,
                "enableGroupCallEventMessages": true,
                "enableGroupCallUpgradeMessage": false,
                "enableGroupCallMeetupGeneration": false,
            },
            "links": conversation_links,
            "scenario": "UserInitiatedTestCall",
        },
        "contentSharing": null,
        "participants": {"from": invitation.from.wire(), "to": []},
        "capabilities": null,
        "endpointCapabilities": ENDPOINT_CAPABILITIES,
        "clientEndpointCapabilities": CLIENT_ENDPOINT_CAPABILITIES,
        "endpointMetadata": {"holographicCapabilities": 3},
        "groupContext": null,
        "groupChat": null,
        "meetingInfo": null,
        "meetingData": null,
        "endpointState": {
            "endpointStateSequenceNumber": 2,
            "endpointProperties": {"additionalEndpointProperties": {"infoShownInReportMode": "FullInformation"}},
        },
        "callInvitation": {
            "callModalities": ["Audio"],
            "replaces": null,
            "transferor": null,
            "clientTransferContext": null,
            "customContext": null,
            "links": call_links,
            "clientContentForMediaController": {
                "controlVideoStreaming": callbacks.call("controlVideoStreaming"),
                "csrcInfo": callbacks.call("csrcInfo"),
            },
            "pstnContent": {"emergencyCallCountry": "", "platformName": PLATFORM_NAME, "publicApiCall": false},
            "emergencyContent": null,
            "mediaContent": {
                "contentType": SDP_CONTENT_TYPE,
                "blob": invitation.offer_sdp,
                "clientLocation": "DE",
                "mediaDescriptions": {"descriptions": [], "requestId": 1},
                "mediaLegId": invitation.media_leg_id,
            },
        },
    })
}

fn conversation_from(body: &Value) -> Result<Conversation> {
    let controller = body["conversationController"]
        .as_str()
        .ok_or_else(|| Error::Signaling("epconv answer without conversationController".into()))?
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
    Ok(Conversation { controller, links })
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
    fn only_teams_hosts_get_the_bearer() {
        assert!(ensure_teams_url("https://api.flightproxy.teams.microsoft.com/api/v2/ep/conv-x/conv/1/leave?i=1").is_ok());
        assert!(ensure_teams_url(EPCONV_URL).is_ok());
        assert!(ensure_teams_url("https://teams.microsoft.com/trap/tokens").is_ok());
        assert!(ensure_teams_url("https://conv-euno-06-prod-aks.conv.skype.com/conv/1").is_err());
        assert!(ensure_teams_url("https://evil.example/?x=.teams.microsoft.com").is_err());
        assert!(ensure_teams_url("http://api.flightproxy.teams.microsoft.com/x").is_err());
    }

    #[test]
    fn epconv_body_matches_the_captured_test_call_shape() {
        let from = Participant {
            mri: "8:orgid:00000000-0000-0000-0000-000000000001".into(),
            display_name: "Test".into(),
            endpoint_id: "e".into(),
            participant_id: "p".into(),
            language_id: "en-gb".into(),
        };
        let callbacks = CallbackLinks::new("https://pub-ent-x-f.trouter.teams.microsoft.com:3443/v4/f/abc/", "call-1");
        let body = epconv_body(&EchoInvitation { from: &from, offer_sdp: "v=0\r\n", media_leg_id: "AB", callbacks: &callbacks });
        assert_eq!(body["conversationRequest"]["scenario"], "UserInitiatedTestCall");
        assert_eq!(body["participants"]["to"], json!([]));
        assert_eq!(body["callInvitation"]["mediaContent"]["contentType"], SDP_CONTENT_TYPE);
        assert!(body["callInvitation"]["links"]["acceptance"].as_str().unwrap().ends_with("/call/acceptance/"));
        assert_eq!(body["conversationRequest"]["links"].as_object().unwrap().len(), 9);
    }
}
