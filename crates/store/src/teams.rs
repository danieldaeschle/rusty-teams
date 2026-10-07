use rusqlite::{OptionalExtension, Row, params};

use crate::error::Result;
use crate::models::{ChannelRecord, TeamRecord};
use crate::store::{Store, json_ids};
use crate::time::{optional_from_millis, optional_to_millis};

const CHANNEL_COLUMNS: &str = "id, team_id, name, membership_type, last_message_at, unread";

impl Store {
    pub fn upsert_teams(&self, teams: &[TeamRecord]) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        {
            let mut upsert = transaction.prepare_cached(
                "INSERT INTO teams (id, name) VALUES (?1, ?2) ON CONFLICT (id) DO UPDATE SET name = excluded.name",
            )?;
            for team in teams {
                upsert.execute(params![team.id, team.name])?;
            }
        }
        Ok(transaction.commit()?)
    }

    pub fn upsert_channels(&self, channels: &[ChannelRecord]) -> Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        {
            let mut upsert = transaction.prepare_cached(
                "INSERT INTO channels (id, team_id, name, membership_type, last_message_at, unread)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (id) DO UPDATE SET
                    team_id = excluded.team_id, name = excluded.name, membership_type = excluded.membership_type,
                    last_message_at = COALESCE(excluded.last_message_at, channels.last_message_at),
                    unread = excluded.unread",
            )?;
            for channel in channels {
                upsert.execute(params![
                    channel.id,
                    channel.team_id,
                    channel.name,
                    channel.membership_type,
                    optional_to_millis(channel.last_message_at),
                    channel.unread,
                ])?;
            }
        }
        Ok(transaction.commit()?)
    }

    pub fn remove_teams_except(&self, kept_team_ids: &[String]) -> Result<usize> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let kept = json_ids(kept_team_ids);
        let removed = transaction.execute(
            "DELETE FROM teams WHERE id NOT IN (SELECT value FROM json_each(?1))",
            [kept],
        )?;
        transaction.commit()?;
        Ok(removed)
    }

    pub fn remove_channels_except(
        &self,
        team_id: &str,
        kept_channel_ids: &[String],
    ) -> Result<usize> {
        let connection = self.lock()?;
        Ok(connection.execute(
            "DELETE FROM channels WHERE team_id = ?1 AND id NOT IN (SELECT value FROM json_each(?2))",
            params![team_id, json_ids(kept_channel_ids)],
        )?)
    }

    pub fn team(&self, team_id: &str) -> Result<Option<TeamRecord>> {
        let connection = self.lock()?;
        Ok(connection
            .query_row(
                "SELECT id, name FROM teams WHERE id = ?1",
                [team_id],
                |row| {
                    Ok(TeamRecord {
                        id: row.get(0)?,
                        name: row.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn channel(&self, channel_id: &str) -> Result<Option<ChannelRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(&format!(
            "SELECT {CHANNEL_COLUMNS} FROM channels WHERE id = ?1"
        ))?;
        let mut channels = statement
            .query_map([channel_id], channel_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(channels.pop())
    }
}

pub(crate) fn channel_from_row(row: &Row<'_>) -> rusqlite::Result<ChannelRecord> {
    Ok(ChannelRecord {
        id: row.get(0)?,
        team_id: row.get(1)?,
        name: row.get(2)?,
        membership_type: row.get(3)?,
        last_message_at: optional_from_millis(row.get(4)?),
        unread: row.get(5)?,
    })
}
