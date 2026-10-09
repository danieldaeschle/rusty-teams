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

const PUNCTUATION_ORDER: &str = "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

const LETTER_FOLDS: &[(&str, &str)] = &[
    ("àáâãäåāăą", "a"),
    ("æ", "ae"),
    ("çćĉċč", "c"),
    ("ďđ", "d"),
    ("èéêëēĕėęě", "e"),
    ("ĝğġģ", "g"),
    ("ĥħ", "h"),
    ("ìíîïĩīĭįıǐ", "i"),
    ("ĵ", "j"),
    ("ķ", "k"),
    ("ĺļľŀł", "l"),
    ("ñńņňŉ", "n"),
    ("òóôõöøōŏőǒ", "o"),
    ("œ", "oe"),
    ("ŕŗř", "r"),
    ("śŝşš", "s"),
    ("ß", "ss"),
    ("ţťŧ", "t"),
    ("ùúûüũūŭůűųǔ", "u"),
    ("ŵ", "w"),
    ("ýÿŷ", "y"),
    ("źżž", "z"),
];

fn primary_weights(character: char) -> Vec<(u8, u32)> {
    if character.is_whitespace() {
        return vec![(0, 0)];
    }
    if let Some(position) = PUNCTUATION_ORDER.find(character) {
        return vec![(1, position as u32)];
    }
    if character.is_ascii_digit() || character.is_alphabetic() {
        let class = if character.is_numeric() { 2 } else { 3 };
        if let Some((_, base)) = LETTER_FOLDS
            .iter()
            .find(|(variants, _)| variants.contains(character))
        {
            return base.chars().map(|letter| (class, letter as u32)).collect();
        }
        return vec![(class, character as u32)];
    }
    vec![(1, PUNCTUATION_ORDER.len() as u32 + character as u32)]
}

fn sort_key(name: &str) -> (Vec<(u8, u32)>, String) {
    let lowered: String = name
        .trim_start()
        .chars()
        .flat_map(char::to_lowercase)
        .collect();
    let primary = lowered.chars().flat_map(primary_weights).collect();
    (primary, lowered)
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

    #[test]
    fn tie_breaks_follow_locale_compare() {
        let ordered = ["x y", "x_y", "x-y", "xy"];
        for pair in ordered.windows(2) {
            assert_eq!(compare_names(pair[0], pair[1]), Ordering::Less);
        }
        assert_eq!(compare_names("_a", "-a"), Ordering::Less);
        assert_eq!(compare_names("-a", "(a"), Ordering::Less);
        assert_eq!(compare_names("(a", "#a"), Ordering::Less);
        assert_eq!(compare_names("#a", "🚩a"), Ordering::Less);
        assert_eq!(compare_names("10 x", "2 x"), Ordering::Less);
        assert_eq!(compare_names("9", "a"), Ordering::Less);
        assert_eq!(compare_names("e", "é"), Ordering::Less);
        assert_eq!(compare_names("ss", "ß"), Ordering::Less);
        assert_eq!(compare_names("a", "ä"), Ordering::Less);
        assert_eq!(compare_names("ä", "ae"), Ordering::Less);
        assert_eq!(compare_names("ø", "p"), Ordering::Less);
        assert_eq!(compare_names("o", "ø"), Ordering::Less);
        assert_eq!(compare_names("ł", "m"), Ordering::Less);
        assert_eq!(compare_names("ı", "j"), Ordering::Less);
    }
}
