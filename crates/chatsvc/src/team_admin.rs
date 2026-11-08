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
pub struct ExistingChannel<'a> {
    pub team_thread_id: &'a str,
    pub group_id: &'a str,
    pub channel_id: &'a str,
    pub substrate_group_id: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelEdit<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
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

pub fn channel_edit_body(edit: ChannelEdit<'_>) -> Value {
    let mut body = json!({ "displayName": edit.name });
    if let Some(description) = edit.description {
        body["description"] = json!(description);
    }
    body
}

fn substrate_channel_id(substrate_group_id: &str, tenant_id: &str) -> String {
    if substrate_group_id.starts_with("OID:") {
        substrate_group_id.to_owned()
    } else {
        format!("OID:{substrate_group_id}@{tenant_id}")
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
                let request = Request::with_body(Method::Post, url, private_channel_body(channel));
                self.provisioning
                    .send(with_anchor(request, mailbox))
                    .await?
            }
        };
        ensure_success(&answer)
    }

    pub async fn update_channel(
        &self,
        channel: ExistingChannel<'_>,
        edit: ChannelEdit<'_>,
        mailbox: Mailbox<'_>,
    ) -> Result<()> {
        let body = channel_edit_body(edit);
        let answer = match channel.substrate_group_id {
            None => {
                self.middle_tier
                    .send(with_headers(Request::with_body(
                        Method::Patch,
                        self.middle_tier_channel_url(channel),
                        body,
                    )))
                    .await?
            }
            Some(substrate_group_id) => {
                let request = Request::with_body(
                    Method::Patch,
                    self.provisioning_channel_url(channel, substrate_group_id, mailbox),
                    body,
                );
                self.provisioning
                    .send(with_anchor(request, mailbox))
                    .await?
            }
        };
        ensure_success(&answer)
    }

    pub async fn delete_channel(
        &self,
        channel: ExistingChannel<'_>,
        mailbox: Mailbox<'_>,
    ) -> Result<()> {
        let answer = match channel.substrate_group_id {
            None => {
                self.middle_tier
                    .send(with_headers(Request::with_body(
                        Method::Delete,
                        self.middle_tier_channel_url(channel),
                        json!({}),
                    )))
                    .await?
            }
            Some(substrate_group_id) => {
                let request = Request::delete(self.provisioning_channel_url(
                    channel,
                    substrate_group_id,
                    mailbox,
                ));
                self.provisioning
                    .send(with_anchor(request, mailbox))
                    .await?
            }
        };
        ensure_success(&answer)
    }

    fn middle_tier_channel_url(&self, channel: ExistingChannel<'_>) -> String {
        format!(
            "https://teams.cloud.microsoft/api/mt/{}/beta/teams/{}/channels/{}",
            self.region,
            encode(channel.team_thread_id),
            encode(channel.channel_id)
        )
    }

    fn provisioning_channel_url(
        &self,
        channel: ExistingChannel<'_>,
        substrate_group_id: &str,
        mailbox: Mailbox<'_>,
    ) -> String {
        format!(
            "https://teams.cloud.microsoft/fabric/{}/templates/api/teams/{}/channels/{}?forceSync=false",
            self.region,
            channel.group_id,
            encode(&substrate_channel_id(substrate_group_id, mailbox.tenant_id))
        )
    }
}

fn with_anchor(request: Request, mailbox: Mailbox<'_>) -> Request {
    let mut request = with_headers(request);
    request
        .headers
        .push((ANCHOR_HEADER.to_owned(), mailbox.anchor()));
    request
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

    const STANDARD: ExistingChannel<'static> = ExistingChannel {
        team_thread_id: "19:team@thread.skype",
        group_id: "group-1",
        channel_id: "19:channel@thread.skype",
        substrate_group_id: None,
    };

    #[tokio::test]
    async fn standard_channel_edits_and_deletes_go_to_the_middle_tier() {
        let middle_tier = Recorder::default();
        let provisioning = Recorder::default();
        let admin = TeamAdmin::with_transports(&middle_tier, &provisioning, "emea");
        let edit = ChannelEdit {
            name: "Renamed",
            description: None,
        };
        admin.update_channel(STANDARD, edit, MAILBOX).await.unwrap();
        admin.delete_channel(STANDARD, MAILBOX).await.unwrap();
        let requests = middle_tier.requests.lock().unwrap();
        let url = "https://teams.cloud.microsoft/api/mt/emea/beta/teams/19%3Ateam%40thread.skype/channels/19%3Achannel%40thread.skype";
        assert_eq!(
            (requests[0].method, requests[0].url.as_str()),
            (Method::Patch, url)
        );
        assert_eq!(
            requests[0].body.as_ref().unwrap(),
            &json!({"displayName": "Renamed"})
        );
        assert_eq!(
            (requests[1].method, requests[1].url.as_str()),
            (Method::Delete, url)
        );
        assert!(provisioning.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn private_channel_edits_and_deletes_go_to_provisioning_by_substrate_id() {
        let middle_tier = Recorder::default();
        let provisioning = Recorder::default();
        let admin = TeamAdmin::with_transports(&middle_tier, &provisioning, "emea");
        let private = ExistingChannel {
            substrate_group_id: Some("substrate-1"),
            ..STANDARD
        };
        let edit = ChannelEdit {
            name: "Renamed",
            description: Some("About"),
        };
        admin.update_channel(private, edit, MAILBOX).await.unwrap();
        admin.delete_channel(private, MAILBOX).await.unwrap();
        let requests = provisioning.requests.lock().unwrap();
        let url = "https://teams.cloud.microsoft/fabric/emea/templates/api/teams/group-1/channels/OID%3Asubstrate-1%40tenant?forceSync=false";
        assert_eq!(
            (requests[0].method, requests[0].url.as_str()),
            (Method::Patch, url)
        );
        assert_eq!(
            requests[0].body.as_ref().unwrap(),
            &json!({"displayName": "Renamed", "description": "About"})
        );
        assert_eq!(
            (requests[1].method, requests[1].url.as_str()),
            (Method::Delete, url)
        );
        assert!(
            requests[1]
                .headers
                .contains(&(ANCHOR_HEADER.to_owned(), "OID:me@tenant".to_owned()))
        );
        assert!(middle_tier.requests.lock().unwrap().is_empty());
    }
}
