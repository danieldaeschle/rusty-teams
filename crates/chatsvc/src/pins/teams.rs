use serde_json::Value;
use session::Request;

use super::Pins;
use super::chats::ensure_success;
use super::transport::CsaTransport;
use crate::channel_notifications::{ChannelNotifications, parse_property};
use crate::error::{Error, Result};

const TEAMS_QUERY: &str = "isPrefetch=false&enableMembershipSummary=true";
const WIKI_DEFINITION_ID: &str = "com.microsoft.teamspace.tab.wiki";
const TAB_URL_KEYS: [&str; 3] = ["websiteUrl", "componentUrl", "url"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelTab {
    pub id: String,
    pub name: String,
    pub definition_id: String,
    pub open_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelLayout {
    pub channel_id: String,
    pub general: bool,
    pub hidden: bool,
    pub tabs: Vec<ChannelTab>,
    pub notifications: Option<ChannelNotifications>,
    pub followed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamLayout {
    pub team_id: String,
    pub hidden: bool,
    pub channels: Vec<ChannelLayout>,
}

impl<T: CsaTransport> Pins<T> {
    pub async fn team_layout(&self) -> Result<Vec<TeamLayout>> {
        let answer = self
            .transport
            .send(Request::get(format!("{}?{TEAMS_QUERY}", self.base_url)))
            .await?;
        ensure_success(&answer)?;
        parse_team_layout(&answer.body)
    }
}

pub fn parse_team_layout(body: &Value) -> Result<Vec<TeamLayout>> {
    let teams = body
        .get("teams")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::UnexpectedAnswer("teams missing".into()))?;
    Ok(teams.iter().filter_map(parse_team).collect())
}

fn flag(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

fn tab_open_url(settings: &Value) -> Option<String> {
    TAB_URL_KEYS
        .iter()
        .filter_map(|key| settings.get(key).and_then(Value::as_str))
        .map(str::trim)
        .find(|url| url.starts_with("http") && !url.contains('{'))
        .map(str::to_owned)
}

fn parse_tab(tab: &Value) -> Option<(i64, ChannelTab)> {
    let definition_id = tab.get("definitionId")?.as_str()?;
    if definition_id == WIKI_DEFINITION_ID {
        return None;
    }
    let order = tab
        .get("order")
        .and_then(|order| order.as_i64().or_else(|| order.as_str()?.parse().ok()))
        .unwrap_or(i64::MAX);
    let settings = tab.get("settings").unwrap_or(&Value::Null);
    Some((
        order,
        ChannelTab {
            id: tab.get("id")?.as_str()?.to_owned(),
            name: tab.get("name")?.as_str()?.to_owned(),
            definition_id: definition_id.to_owned(),
            open_url: tab_open_url(settings),
        },
    ))
}

pub fn parse_channel_tabs(channel: &Value) -> Vec<ChannelTab> {
    let mut tabs: Vec<(i64, ChannelTab)> = channel
        .get("tabs")
        .and_then(Value::as_array)
        .map(|tabs| tabs.iter().filter_map(parse_tab).collect())
        .unwrap_or_default();
    tabs.sort_by_key(|(order, _)| *order);
    tabs.into_iter().map(|(_, tab)| tab).collect()
}

fn parse_team(team: &Value) -> Option<TeamLayout> {
    if flag(team, "isDeleted") == Some(true) {
        return None;
    }
    let general_shown = flag(team, "isGeneralChannelFavorite").unwrap_or(true);
    let channels = team
        .get("channels")
        .and_then(Value::as_array)
        .map(|channels| {
            channels
                .iter()
                .filter(|channel| flag(channel, "isDeleted") != Some(true))
                .filter_map(|channel| {
                    let general = flag(channel, "isGeneral").unwrap_or(false);
                    let shown = if general {
                        general_shown
                    } else {
                        flag(channel, "isFavorite").unwrap_or(true)
                    };
                    Some(ChannelLayout {
                        channel_id: channel.get("id")?.as_str()?.to_owned(),
                        general,
                        hidden: !shown,
                        tabs: parse_channel_tabs(channel),
                        notifications: channel
                            .get("channelNotificationSettings")
                            .and_then(Value::as_str)
                            .and_then(parse_property),
                        followed: flag(channel, "isFollowed").unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(TeamLayout {
        team_id: team.get("id")?.as_str()?.to_owned(),
        hidden: flag(team, "isFavorite") == Some(false),
        channels,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn general_visibility_follows_the_team_flag_only() {
        let body = json!({"teams": [
            {"id": "19:a@thread.tacv2", "isGeneralChannelFavorite": false, "channels": [
                {"id": "19:a@thread.tacv2", "isGeneral": true, "isFavorite": true}
            ]},
            {"id": "19:b@thread.tacv2", "isGeneralChannelFavorite": true, "channels": [
                {"id": "19:b@thread.tacv2", "isGeneral": true, "isFavorite": false}
            ]},
            {"id": "19:c@thread.tacv2", "channels": [
                {"id": "19:c@thread.tacv2", "isGeneral": true, "isFavorite": false}
            ]}
        ]});
        let layout = parse_team_layout(&body).unwrap();
        assert!(layout[0].channels[0].hidden);
        assert!(!layout[1].channels[0].hidden);
        assert!(!layout[2].channels[0].hidden);
    }

    #[test]
    fn keeps_the_service_order_and_reads_hidden_flags() {
        let body = json!({"teams": [
            {"id": "19:b@thread.tacv2", "isFavorite": true, "channels": [
                {"id": "19:b2@thread.tacv2", "isFavorite": false},
                {"id": "19:b@thread.tacv2", "isGeneral": true, "isFavorite": true}
            ]},
            {"id": "19:gone@thread.tacv2", "isDeleted": true, "channels": []},
            {"id": "19:a@thread.tacv2", "isFavorite": false, "isGeneralChannelFavorite": false, "channels": [
                {"id": "19:a@thread.tacv2", "isGeneral": true},
                {"id": "19:a2@thread.tacv2"}
            ]}
        ]});
        let layout = parse_team_layout(&body).unwrap();
        let ids: Vec<&str> = layout.iter().map(|team| team.team_id.as_str()).collect();
        assert_eq!(ids, ["19:b@thread.tacv2", "19:a@thread.tacv2"]);
        assert!(!layout[0].hidden);
        assert!(layout[1].hidden);
        assert_eq!(
            layout[0].channels,
            [
                ChannelLayout {
                    channel_id: "19:b2@thread.tacv2".into(),
                    general: false,
                    hidden: true,
                    tabs: Vec::new(),
                    notifications: None,
                    followed: false,
                },
                ChannelLayout {
                    channel_id: "19:b@thread.tacv2".into(),
                    general: true,
                    hidden: false,
                    tabs: Vec::new(),
                    notifications: None,
                    followed: false,
                },
            ]
        );
        assert!(layout[1].channels[0].hidden);
        assert!(!layout[1].channels[1].hidden);
    }

    #[test]
    fn tabs_follow_the_order_field_and_hide_wiki() {
        let channel = json!({"id": "19:a@thread.tacv2", "tabs": [
            {"id": "t3", "name": "Notes", "definitionId": "0d820ecd-def2-4297-adad-78056cde7c78",
             "order": 30, "settings": {
                "url": "https://onenote.example/{tid}/content",
                "websiteUrl": "https://onenote.example/web"}},
            {"id": "t0", "name": "Wiki", "definitionId": "com.microsoft.teamspace.tab.wiki", "order": 5},
            {"id": "t1", "name": "Docs", "definitionId": "com.microsoft.teamspace.tab.web",
             "order": 10, "settings": {"url": "https://docs.example", "websiteUrl": "https://docs.example"}},
            {"id": "t2", "name": "Plan", "definitionId": "com.microsoft.teamspace.tab.planner",
             "order": "20", "settings": {"url": "https://tasks.example/{locale}"}}
        ]});
        let tabs = parse_channel_tabs(&channel);
        let names: Vec<&str> = tabs.iter().map(|tab| tab.name.as_str()).collect();
        assert_eq!(names, ["Docs", "Plan", "Notes"]);
        assert_eq!(tabs[0].open_url.as_deref(), Some("https://docs.example"));
        assert_eq!(tabs[1].open_url, None);
        assert_eq!(
            tabs[2].open_url.as_deref(),
            Some("https://onenote.example/web")
        );
    }

    #[test]
    fn reads_the_notification_string_and_the_followed_flag() {
        let body = json!({"teams": [
            {"id": "19:a@thread.tacv2", "channels": [
                {"id": "19:a1", "channelNotificationSettings": "{\"allNewPosts\":\"On\",\"dskNotif\":\"On\",\"includeReplies\":true}", "isFollowed": true},
                {"id": "19:a2", "channelNotificationSettings": null, "isFollowed": true},
                {"id": "19:a3"}
            ]}
        ]});
        let channels = &parse_team_layout(&body).unwrap()[0].channels;
        assert_eq!(
            channels[0].notifications,
            Some(ChannelNotifications {
                level: crate::ChannelNotificationLevel::BannerAndFeed,
                include_replies: true
            })
        );
        assert_eq!(
            (channels[1].notifications, channels[1].followed),
            (None, true)
        );
        assert_eq!(
            (channels[2].notifications, channels[2].followed),
            (None, false)
        );
    }

    #[test]
    fn a_channel_without_tabs_has_none() {
        assert!(parse_channel_tabs(&json!({"id": "19:a@thread.tacv2"})).is_empty());
    }

    #[test]
    fn rejects_an_answer_without_teams() {
        assert!(parse_team_layout(&json!({"chats": []})).is_err());
    }
}
