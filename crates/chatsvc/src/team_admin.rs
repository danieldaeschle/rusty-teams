use serde_json::{Value, json};
use session::{Method, Request, SPACES, Scope, Session};

use crate::error::Result;
use crate::language_settings::with_headers;
use crate::messages::{encode, ensure_success};
use crate::pins::{CsaTransport, DEFAULT_REGION, SessionTransport};

const MIDDLE_TIER_SCOPE: &str = "user_impersonation";
const PROVISIONING_RESOURCE: &str = "https://templates.teams.microsoft.com";
const PROVISIONING_SCOPE: &str = ".default";
const ANCHOR_HEADER: &str = "X-AnchorMailbox";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPrivacy {
    Standard,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewChannel<'a> {
    pub team_thread_id: &'a str,
    pub group_id: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub privacy: ChannelPrivacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mailbox<'a> {
    pub object_id: &'a str,
    pub tenant_id: &'a str,
}

impl Mailbox<'_> {
    fn anchor(&self) -> String {
        format!("OID:{}@{}", self.object_id, self.tenant_id)
    }
}

pub fn standard_channel_body(channel: &NewChannel<'_>) -> Value {
    json!({
        "displayName": channel.name,
        "description": channel.description,
        "groupId": channel.group_id,
        "channelType": "Standard",
    })
}

pub fn private_channel_body(channel: &NewChannel<'_>) -> Value {
    json!({
        "spaceThreadId": channel.team_thread_id,
        "displayName": channel.name,
        "description": channel.description,
        "channelType": "Private",
        "sharedWithParentTeam": false,
    })
}

pub struct TeamAdmin<T: CsaTransport = SessionTransport> {
    middle_tier: T,
    provisioning: T,
    region: String,
}

impl TeamAdmin<SessionTransport> {
    pub fn new(session: &Session) -> Self {
        TeamAdmin::with_transports(
            SessionTransport::with_scope(session, Scope::new(SPACES, MIDDLE_TIER_SCOPE)),
            SessionTransport::with_scope(
                session,
                Scope::new(PROVISIONING_RESOURCE, PROVISIONING_SCOPE),
            ),
            DEFAULT_REGION,
        )
    }
}

impl<T: CsaTransport> TeamAdmin<T> {
    pub fn with_transports(middle_tier: T, provisioning: T, region: &str) -> Self {
        TeamAdmin {
            middle_tier,
            provisioning,
            region: region.to_owned(),
        }
    }

    pub async fn create_channel(
        &self,
        channel: &NewChannel<'_>,
        mailbox: Mailbox<'_>,
    ) -> Result<()> {
        let answer = match channel.privacy {
            ChannelPrivacy::Standard => {
                let url = format!(
                    "https://teams.cloud.microsoft/api/mt/{}/beta/teams/{}/channels",
                    self.region,
                    encode(channel.team_thread_id)
                );
                self.middle_tier
                    .send(with_headers(Request::with_body(
                        Method::Post,
                        url,
                        standard_channel_body(channel),
                    )))
                    .await?
            }
            ChannelPrivacy::Private => {
                let url = format!(
                    "https://teams.cloud.microsoft/fabric/{}/templates/api/teams/{}/channels?forceSync=false",
                    self.region, channel.group_id
                );
                let mut request = with_headers(Request::with_body(
                    Method::Post,
                    url,
                    private_channel_body(channel),
                ));
                request
                    .headers
                    .push((ANCHOR_HEADER.to_owned(), mailbox.anchor()));
                self.provisioning.send(request).await?
            }
        };
        ensure_success(&answer)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use session::ApiResponse;

    use super::*;

    #[derive(Default)]
    struct Recorder {
        requests: Mutex<Vec<Request>>,
    }

    impl CsaTransport for &Recorder {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request);
            Ok(ApiResponse {
                status: 200,
                retry_after: None,
                body: Value::Null,
            })
        }
    }

    fn channel(privacy: ChannelPrivacy) -> NewChannel<'static> {
        NewChannel {
            team_thread_id: "19:team@thread.skype",
            group_id: "group-1",
            name: "Planning",
            description: "",
            privacy,
        }
    }

    const MAILBOX: Mailbox<'static> = Mailbox {
        object_id: "me",
        tenant_id: "tenant",
    };

    #[tokio::test]
    async fn standard_channels_go_to_the_middle_tier() {
        let middle_tier = Recorder::default();
        let provisioning = Recorder::default();
        let admin = TeamAdmin::with_transports(&middle_tier, &provisioning, "emea");
        admin
            .create_channel(&channel(ChannelPrivacy::Standard), MAILBOX)
            .await
            .unwrap();
        let requests = middle_tier.requests.lock().unwrap();
        assert_eq!(
            requests[0].url,
            "https://teams.cloud.microsoft/api/mt/emea/beta/teams/19%3Ateam%40thread.skype/channels"
        );
        assert_eq!(
            requests[0].body.as_ref().unwrap()["channelType"],
            "Standard"
        );
        assert_eq!(requests[0].body.as_ref().unwrap()["groupId"], "group-1");
        assert!(provisioning.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn private_channels_go_to_provisioning_with_the_mailbox_anchor() {
        let middle_tier = Recorder::default();
        let provisioning = Recorder::default();
        let admin = TeamAdmin::with_transports(&middle_tier, &provisioning, "emea");
        admin
            .create_channel(&channel(ChannelPrivacy::Private), MAILBOX)
            .await
            .unwrap();
        let requests = provisioning.requests.lock().unwrap();
        assert_eq!(
            requests[0].url,
            "https://teams.cloud.microsoft/fabric/emea/templates/api/teams/group-1/channels?forceSync=false"
        );
        assert!(
            requests[0]
                .headers
                .contains(&(ANCHOR_HEADER.to_owned(), "OID:me@tenant".to_owned()))
        );
        let body = requests[0].body.as_ref().unwrap();
        assert_eq!(body["spaceThreadId"], "19:team@thread.skype");
        assert_eq!(body["channelType"], "Private");
        assert!(middle_tier.requests.lock().unwrap().is_empty());
    }
}
