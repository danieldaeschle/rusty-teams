use rusqlite::{Connection, Transaction};

use crate::error::{Error, Result};
use crate::search;

struct Migration {
    script: &'static str,
    after: Option<fn(&Transaction<'_>) -> Result<()>>,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        script: include_str!("migrations/0001_initial.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0002_delta_and_meta.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0003_previews_avatars_folders.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0004_images_and_search.sql"),
        after: Some(search::backfill),
    },
    Migration {
        script: include_str!("migrations/0005_team_layout.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0006_presence.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0007_sender_application.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0008_activity.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0009_muted.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0010_name_lookup_indexes.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0011_message_links.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0012_outbox_drafts.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0013_message_subject.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0014_folder_expanded.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0015_channel_tabs.sql"),
        after: None,
    },
    Migration {
        script: include_str!("migrations/0016_channel_notifications.sql"),
        after: None,
    },
];

pub fn latest_version() -> i64 {
    MIGRATIONS.len() as i64
}

pub fn migrate(connection: &mut Connection) -> Result<()> {
    let found: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if found > latest_version() {
        return Err(Error::SchemaTooNew {
            found,
            supported: latest_version(),
        });
    }
    for (index, migration) in MIGRATIONS.iter().enumerate().skip(found as usize) {
        let transaction = connection.transaction()?;
        transaction.execute_batch(migration.script)?;
        if let Some(after) = migration.after {
            after(&transaction)?;
        }
        transaction.pragma_update(None, "user_version", index as i64 + 1)?;
        transaction.commit()?;
    }
    Ok(())
}
