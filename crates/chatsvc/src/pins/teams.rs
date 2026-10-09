use serde_json::Value;
use session::Request;

use super::Pins;
use super::chats::ensure_success;
use super::transport::CsaTransport;
use crate::error::{Error, Result};

const TEAMS_QUERY: &str = "isPrefetch=false&enableMembershipSummary=true";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelLayout {
    pub channel_id: String,
    pub general: bool,
    pub hidden: bool,
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
                    hidden: true
                },
                ChannelLayout {
                    channel_id: "19:b@thread.tacv2".into(),
                    general: true,
                    hidden: false
                },
            ]
        );
        assert!(layout[1].channels[0].hidden);
        assert!(!layout[1].channels[1].hidden);
    }

    #[test]
    fn rejects_an_answer_without_teams() {
        assert!(parse_team_layout(&json!({"chats": []})).is_err());
    }
}
