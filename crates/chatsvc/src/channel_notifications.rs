use std::collections::HashMap;

use serde_json::{Value, json};
use session::{Method, Request, Scope, Session};

use crate::error::{Error, Result};
use crate::pins::{CsaTransport, DEFAULT_REGION, SessionTransport};

pub const NSS_RESOURCE: &str = "https://uis.teams.microsoft.com";
const NSS_SCOPE: &str = "user_impersonation";
pub(crate) const PROPERTY_NAME: &str = "channelNotificationSettings";
const NSS_BATCH_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelNotificationLevel {
    BannerAndFeed,
    #[default]
    Feed,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelNotifications {
    pub level: ChannelNotificationLevel,
    pub include_replies: bool,
}

impl ChannelNotifications {
    pub fn from_followed(followed: bool) -> Self {
        if followed {
            ChannelNotifications {
                level: ChannelNotificationLevel::BannerAndFeed,
                include_replies: true,
            }
        } else {
            ChannelNotifications::default()
        }
    }

    pub fn property_value(self) -> String {
        let mut settings = match self.level {
            ChannelNotificationLevel::BannerAndFeed => {
                json!({"allNewPosts": "On", "dskNotif": "On"})
            }
            ChannelNotificationLevel::Feed => json!({"allNewPosts": "On"}),
            ChannelNotificationLevel::Off => json!({"allNewPosts": "Off"}),
        };
        settings["includeReplies"] =
            json!(self.include_replies && self.level != ChannelNotificationLevel::Off);
        settings.to_string()
    }
}

pub fn parse_property(raw: &str) -> Option<ChannelNotifications> {
    let settings: Value = serde_json::from_str(raw).ok()?;
    let level = match (
        settings.get("allNewPosts").and_then(Value::as_str)?,
        settings.get("dskNotif").and_then(Value::as_str),
    ) {
        ("On", Some("On")) => ChannelNotificationLevel::BannerAndFeed,
        ("On", _) => ChannelNotificationLevel::Feed,
        _ => ChannelNotificationLevel::Off,
    };
    Some(ChannelNotifications {
        level,
        include_replies: settings
            .get("includeReplies")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

pub fn parse_nss_batch(body: &Value) -> HashMap<String, ChannelNotifications> {
    body.get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|result| {
            let settings = result
                .get("settings")
                .filter(|settings| settings.is_object())?;
            let level = match settings.get("allNewPosts").and_then(Value::as_str)? {
                "BannerAndFeed" => ChannelNotificationLevel::BannerAndFeed,
                "Feed" => ChannelNotificationLevel::Feed,
                _ => ChannelNotificationLevel::Off,
            };
            Some((
                result.get("channelId")?.as_str()?.to_owned(),
                ChannelNotifications {
                    level,
                    include_replies: settings
                        .get("includeReplies")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                },
            ))
        })
        .collect()
}

pub struct ChannelSettings<T: CsaTransport = SessionTransport> {
    transport: T,
    url: String,
}

impl ChannelSettings<SessionTransport> {
    pub fn new(session: &Session) -> Self {
        ChannelSettings::with_transport(
            SessionTransport::with_scope(session, Scope::new(NSS_RESOURCE, NSS_SCOPE)),
            DEFAULT_REGION,
        )
    }
}

impl<T: CsaTransport> ChannelSettings<T> {
    pub fn with_transport(transport: T, region: &str) -> Self {
        ChannelSettings {
            transport,
            url: format!(
                "https://teams.cloud.microsoft/api/nss/{region}/v1/me/notificationSettings/channels/batch"
            ),
        }
    }

    pub async fn fetch(
        &self,
        channel_ids: &[String],
    ) -> Result<HashMap<String, ChannelNotifications>> {
        let mut settings = HashMap::new();
        for batch in channel_ids.chunks(NSS_BATCH_LIMIT) {
            let answer = self
                .transport
                .send(Request::with_body(
                    Method::Post,
                    &self.url,
                    json!({"channelIds": batch}),
                ))
                .await?;
            if !answer.is_success() {
                return Err(Error::UnexpectedAnswer(format!(
                    "channel settings answered {}",
                    answer.status
                )));
            }
            settings.extend(parse_nss_batch(&answer.body));
        }
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use session::ApiResponse;

    use super::*;
    use crate::conversations::Conversations;
    use crate::messages::MessageTransport;

    struct Canned {
        answers: Mutex<VecDeque<(u16, Value)>>,
        requests: Mutex<Vec<Request>>,
    }

    impl CsaTransport for &Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            self.requests.lock().unwrap().push(request);
            let (status, body) = self.answers.lock().unwrap().pop_front().expect("exhausted");
            Ok(ApiResponse {
                status,
                body,
                retry_after: None,
            })
        }
    }

    impl MessageTransport for &Canned {
        async fn send(&self, request: Request) -> Result<ApiResponse> {
            CsaTransport::send(self, request).await
        }
    }

    fn canned(answers: Vec<(u16, Value)>) -> Canned {
        Canned {
            answers: Mutex::new(answers.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    #[test]
    fn property_maps_the_three_levels() {
        let banner =
            parse_property(r#"{"allNewPosts":"On","dskNotif":"On","includeReplies":true}"#);
        assert_eq!(
            banner,
            Some(ChannelNotifications {
                level: ChannelNotificationLevel::BannerAndFeed,
                include_replies: true
            })
        );
        let feed = parse_property(r#"{"allNewPosts":"On","latestCnsVersion":1}"#).unwrap();
        assert_eq!(feed.level, ChannelNotificationLevel::Feed);
        assert!(!feed.include_replies);
        let off = parse_property(r#"{"allNewPosts":"Off","includeReplies":false}"#).unwrap();
        assert_eq!(off.level, ChannelNotificationLevel::Off);
    }

    #[test]
    fn unreadable_property_is_missing() {
        assert_eq!(parse_property("not json"), None);
        assert_eq!(parse_property(r#"{"includeReplies":true}"#), None);
    }

    #[test]
    fn followed_without_a_value_means_banner_with_replies() {
        assert_eq!(
            ChannelNotifications::from_followed(true),
            ChannelNotifications {
                level: ChannelNotificationLevel::BannerAndFeed,
                include_replies: true
            }
        );
        assert_eq!(
            ChannelNotifications::from_followed(false).level,
            ChannelNotificationLevel::Feed
        );
    }

    #[test]
    fn nss_batch_skips_channels_without_settings() {
        let body = json!({"results": [
            {"channelId": "19:a", "success": true, "settings": {"allNewPosts": "BannerAndFeed", "includeReplies": true}},
            {"channelId": "19:b", "success": true, "settings": {"allNewPosts": "Feed"}},
            {"channelId": "19:c", "success": true, "settings": {"allNewPosts": "Off"}},
            {"channelId": "19:d", "success": true, "settings": null},
            {"channelId": "19:e", "success": false, "settings": null, "errorMessage": "x"},
        ]});
        let parsed = parse_nss_batch(&body);
        assert_eq!(parsed.len(), 3);
        assert_eq!(
            parsed["19:a"].level,
            ChannelNotificationLevel::BannerAndFeed
        );
        assert!(parsed["19:a"].include_replies);
        assert_eq!(parsed["19:b"].level, ChannelNotificationLevel::Feed);
        assert_eq!(parsed["19:c"].level, ChannelNotificationLevel::Off);
    }

    #[tokio::test]
    async fn fetch_splits_into_batches_of_one_hundred() {
        let transport = canned(vec![
            (200, json!({"results": []})),
            (200, json!({"results": []})),
        ]);
        let ids: Vec<String> = (0..101).map(|index| format!("19:{index}")).collect();
        ChannelSettings::with_transport(&transport, "emea")
            .fetch(&ids)
            .await
            .unwrap();
        let requests = transport.requests.lock().unwrap();
        let sizes: Vec<usize> = requests
            .iter()
            .map(|request| {
                request.body.as_ref().unwrap()["channelIds"]
                    .as_array()
                    .unwrap()
                    .len()
            })
            .collect();
        assert_eq!(sizes, [100, 1]);
    }

    #[tokio::test]
    async fn write_puts_the_settings_as_a_json_string() {
        let transport = canned(vec![
            (200, Value::Null),
            (200, Value::Null),
            (200, Value::Null),
        ]);
        let conversations = Conversations::with_transport(&transport, "emea");
        for (level, include_replies) in [
            (ChannelNotificationLevel::BannerAndFeed, false),
            (ChannelNotificationLevel::Feed, true),
            (ChannelNotificationLevel::Off, true),
        ] {
            conversations
                .set_channel_notifications(
                    "19:a@thread.tacv2",
                    ChannelNotifications {
                        level,
                        include_replies,
                    },
                )
                .await
                .unwrap();
        }
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[0].method, Method::Put);
        assert_eq!(
            requests[0].url,
            "https://teams.cloud.microsoft/api/chatsvc/emea/v1/users/ME/conversations/19%3Aa%40thread.tacv2/properties?name=channelNotificationSettings"
        );
        assert_eq!(
            requests[0].body,
            Some(
                json!({"channelNotificationSettings": "{\"allNewPosts\":\"On\",\"dskNotif\":\"On\",\"includeReplies\":false}"})
            )
        );
        assert_eq!(
            requests[1].body,
            Some(
                json!({"channelNotificationSettings": "{\"allNewPosts\":\"On\",\"includeReplies\":true}"})
            )
        );
        assert_eq!(
            requests[2].body,
            Some(
                json!({"channelNotificationSettings": "{\"allNewPosts\":\"Off\",\"includeReplies\":false}"})
            )
        );
    }
}
