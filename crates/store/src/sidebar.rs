use std::cmp::Ordering;

use rusqlite::params;

use crate::error::Result;
use crate::models::{ChannelRecord, Sidebar, SidebarTeam, TeamLayoutRecord, TeamRecord};
use crate::store::Store;
use crate::teams::channel_from_row;

const GENERAL_FALLBACK_NAME: &str = "General";

struct TeamRow {
    team: TeamRecord,
    position: Option<i64>,
    hidden: bool,
}

struct ChannelRow {
    channel: ChannelRecord,
    general: bool,
    hidden: bool,
}

impl Store {
    pub fn sidebar(&self) -> Result<Sidebar> {
        let chats = self.recent_chats(usize::MAX >> 1)?;
        let connection = self.lock()?;
        let mut team_rows: Vec<TeamRow> = connection
            .prepare_cached(
                "SELECT teams.id, teams.name, team_layout.position, COALESCE(team_layout.hidden, 0)
                 FROM teams LEFT JOIN team_layout ON team_layout.team_id = teams.id",
            )?
            .query_map([], |row| {
                Ok(TeamRow {
                    team: TeamRecord {
                        id: row.get(0)?,
                        name: row.get(1)?,
                    },
                    position: row.get(2)?,
                    hidden: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        team_rows.sort_by(|left, right| {
            match (left.position, right.position) {
                (Some(left), Some(right)) => left.cmp(&right),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
            .then_with(|| compare_names(&left.team.name, &right.team.name))
            .then_with(|| left.team.id.cmp(&right.team.id))
        });
        let mut channel_rows: Vec<ChannelRow> = connection
            .prepare_cached(
                "SELECT id, team_id, name, membership_type, last_message_at, unread,
                        COALESCE(channel_layout.general, name = ?1), COALESCE(channel_layout.hidden, 0)
                 FROM channels LEFT JOIN channel_layout ON channel_layout.channel_id = channels.id",
            )?
            .query_map(params![GENERAL_FALLBACK_NAME], |row| {
                Ok(ChannelRow {
                    channel: channel_from_row(row)?,
                    general: row.get(6)?,
                    hidden: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        channel_rows.sort_by(|left, right| {
            right
                .general
                .cmp(&left.general)
                .then_with(|| compare_names(&left.channel.name, &right.channel.name))
                .then_with(|| left.channel.id.cmp(&right.channel.id))
        });
        let mut teams: Vec<SidebarTeam> = team_rows
            .into_iter()
            .map(|row| SidebarTeam {
                team: row.team,
                channels: Vec::new(),
                hidden: row.hidden,
                hidden_channel_ids: Vec::new(),
            })
            .collect();
        for row in channel_rows {
            if let Some(team) = teams
                .iter_mut()
                .find(|team| team.team.id == row.channel.team_id)
            {
                if row.hidden {
                    team.hidden_channel_ids.push(row.channel.id.clone());
                }
                team.channels.push(row.channel);
            }
        }
        Ok(Sidebar { chats, teams })
    }

    pub fn replace_team_layout(&self, layout: &[TeamLayoutRecord]) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM team_layout", [])?;
        transaction.execute("DELETE FROM channel_layout", [])?;
        {
            let mut team_insert = transaction.prepare_cached(
                "INSERT OR REPLACE INTO team_layout (team_id, position, hidden) VALUES (?1, ?2, ?3)",
            )?;
            let mut channel_insert = transaction.prepare_cached(
                "INSERT OR REPLACE INTO channel_layout (channel_id, general, hidden) VALUES (?1, ?2, ?3)",
            )?;
            for (position, team) in layout.iter().enumerate() {
                team_insert.execute(params![team.team_id, position as i64, team.hidden])?;
                for channel in &team.channels {
                    channel_insert.execute(params![
                        channel.channel_id,
                        channel.general,
                        channel.hidden
                    ])?;
                }
            }
        }
        Ok(transaction.commit()?)
    }
}

fn compare_names(left: &str, right: &str) -> Ordering {
    sort_key(left).cmp(&sort_key(right))
}

fn sort_key(name: &str) -> (bool, String) {
    let trimmed = name.trim_start();
    let starts_with_word = trimmed
        .chars()
        .next()
        .is_some_and(char::is_alphanumeric);
    let mut folded = String::with_capacity(trimmed.len());
    for character in trimmed.chars().flat_map(char::to_lowercase) {
        match character {
            'ä' | 'á' | 'à' | 'â' => folded.push('a'),
            'ö' | 'ó' | 'ò' | 'ô' => folded.push('o'),
            'ü' | 'ú' | 'ù' | 'û' => folded.push('u'),
            'é' | 'è' | 'ê' | 'ë' => folded.push('e'),
            'ß' => folded.push_str("ss"),
            other => folded.push(other),
        }
    }
    (starts_with_word, folded)
}

#[cfg(test)]
mod tests {
    use super::compare_names;
    use std::cmp::Ordering;

    #[test]
    fn names_compare_like_a_person_reads_them() {
        assert_eq!(compare_names("alpha", "Beta"), Ordering::Less);
        assert_eq!(compare_names("Übergabe", "Zoll"), Ordering::Less);
        assert_eq!(compare_names("Übergabe", "Tests"), Ordering::Greater);
        assert_eq!(compare_names("🚩 Rollout", "Alpha"), Ordering::Less);
    }
}
