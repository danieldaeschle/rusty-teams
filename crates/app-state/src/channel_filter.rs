use store::SidebarTeam;

pub const CHANNEL_FILTER_PLACEHOLDER: &str = "Filter by channel name";
pub const UNREAD_PILL: &str = "Unread";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelFilter {
    pub text: String,
    pub unread_only: bool,
}

impl ChannelFilter {
    pub fn is_active(&self) -> bool {
        self.unread_only || !self.text.trim().is_empty()
    }

    pub fn apply(&self, teams: &[SidebarTeam]) -> Vec<SidebarTeam> {
        let needle = self.text.trim().to_lowercase();
        teams
            .iter()
            .filter_map(|entry| {
                let team_matches =
                    needle.is_empty() || entry.team.name.to_lowercase().contains(&needle);
                let channels: Vec<_> = entry
                    .channels
                    .iter()
                    .filter(|channel| team_matches || channel.name.to_lowercase().contains(&needle))
                    .filter(|channel| !self.unread_only || channel.unread)
                    .cloned()
                    .collect();
                (!channels.is_empty()).then(|| SidebarTeam {
                    channels,
                    hidden: false,
                    hidden_channel_ids: Vec::new(),
                    ..entry.clone()
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use store::{ChannelRecord, TeamRecord, TeamRights};

    use super::*;

    fn channel(id: &str, name: &str, unread: bool) -> ChannelRecord {
        ChannelRecord {
            id: id.to_owned(),
            team_id: "t".to_owned(),
            name: name.to_owned(),
            membership_type: None,
            last_message_at: None,
            unread,
        }
    }

    fn team(name: &str, channels: Vec<ChannelRecord>) -> SidebarTeam {
        SidebarTeam {
            team: TeamRecord {
                id: name.to_owned(),
                name: name.to_owned(),
            },
            channels,
            hidden: true,
            rights: TeamRights::default(),
            hidden_channel_ids: vec!["c2".to_owned()],
            notifications: HashMap::new(),
            mentions: HashMap::new(),
        }
    }

    fn names(teams: &[SidebarTeam]) -> Vec<(String, Vec<String>)> {
        teams
            .iter()
            .map(|entry| {
                let channels = entry
                    .channels
                    .iter()
                    .map(|channel| channel.name.clone())
                    .collect();
                (entry.team.name.clone(), channels)
            })
            .collect()
    }

    #[test]
    fn text_matches_channel_names_or_the_whole_team() {
        let teams = vec![
            team(
                "Platform",
                vec![
                    channel("c1", "General", false),
                    channel("c2", "Releases", true),
                ],
            ),
            team("Release crew", vec![channel("c3", "General", false)]),
        ];
        let filter = ChannelFilter {
            text: " release".to_owned(),
            unread_only: false,
        };
        let filtered = filter.apply(&teams);
        assert_eq!(
            names(&filtered),
            [
                ("Platform".to_owned(), vec!["Releases".to_owned()]),
                ("Release crew".to_owned(), vec!["General".to_owned()]),
            ]
        );
        assert!(
            filtered
                .iter()
                .all(|entry| !entry.hidden && entry.hidden_channel_ids.is_empty())
        );
    }

    #[test]
    fn unread_keeps_only_unread_channels() {
        let teams = vec![
            team(
                "Platform",
                vec![
                    channel("c1", "General", false),
                    channel("c2", "Releases", true),
                ],
            ),
            team("Quiet", vec![channel("c3", "General", false)]),
        ];
        let filter = ChannelFilter {
            text: String::new(),
            unread_only: true,
        };
        assert_eq!(
            names(&filter.apply(&teams)),
            [("Platform".to_owned(), vec!["Releases".to_owned()])]
        );
        assert!(!ChannelFilter::default().is_active());
    }
}
