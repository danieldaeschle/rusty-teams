use std::collections::HashSet;
use std::time::{Duration, Instant};

use graph::User;

use crate::engine::{Conversation, META_USER_ID, SyncEngine};
use crate::error::Result;
use crate::remote::Remote;

const DIRECTORY_CACHE_TTL: Duration = Duration::from_secs(300);
const DIRECTORY_CACHE_MAX_QUERIES: usize = 64;
const DIRECTORY_MIN_QUERY_CHARS: usize = 2;
const RECENT_SENDERS_SCANNED: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonSource {
    Member,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonCandidate {
    pub user_id: String,
    pub display_name: String,
    pub mail: Option<String>,
    pub job_title: Option<String>,
    pub source: PersonSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MentionCandidate {
    Person(PersonCandidate),
    Channel { channel_id: String, name: String },
    Team { team_id: String, name: String },
}

impl MentionCandidate {
    pub fn display_name(&self) -> &str {
        match self {
            MentionCandidate::Person(person) => &person.display_name,
            MentionCandidate::Channel { name, .. } | MentionCandidate::Team { name, .. } => name,
        }
    }

    /// Ready to pass to the send, reply and edit calls.
    pub fn to_mention(&self) -> crate::MentionInput {
        match self {
            MentionCandidate::Person(person) => {
                crate::MentionInput::user(&person.user_id, &person.display_name)
            }
            MentionCandidate::Channel { channel_id, name } => {
                crate::MentionInput::channel(channel_id, name)
            }
            MentionCandidate::Team { team_id, name } => crate::MentionInput::team(team_id, name),
        }
    }
}

pub(crate) struct DirectoryCache {
    entries: std::collections::HashMap<String, (Instant, Vec<User>)>,
}

impl DirectoryCache {
    pub(crate) fn new() -> Self {
        DirectoryCache {
            entries: std::collections::HashMap::new(),
        }
    }

    fn get(&self, query: &str) -> Option<Vec<User>> {
        let (stored_at, users) = self.entries.get(query)?;
        (stored_at.elapsed() < DIRECTORY_CACHE_TTL).then(|| users.clone())
    }

    fn put(&mut self, query: &str, users: Vec<User>) {
        if self.entries.len() >= DIRECTORY_CACHE_MAX_QUERIES {
            self.entries
                .retain(|_, (stored_at, _)| stored_at.elapsed() < DIRECTORY_CACHE_TTL);
        }
        if self.entries.len() >= DIRECTORY_CACHE_MAX_QUERIES {
            self.entries.clear();
        }
        self.entries
            .insert(query.to_owned(), (Instant::now(), users));
    }
}

impl<R: Remote> SyncEngine<R> {
    /// Directory search, answers are cached in memory for five minutes per query.
    pub async fn search_people(&self, query: &str) -> Result<Vec<User>> {
        let key = query.trim().to_lowercase();
        if let Some(cached) = self.directory_lock().get(&key) {
            return Ok(cached);
        }
        let users = self.remote.search_people(query).await?;
        self.directory_lock().put(&key, users.clone());
        Ok(users)
    }

    /// Autocomplete for `@`: members of the conversation first, then (channels) the channel and its team,
    /// then the directory. A failing directory search still returns the local part.
    pub async fn mention_candidates(
        &self,
        conversation_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MentionCandidate>> {
        let conversation = self.resolve(conversation_id)?;
        let query = query.trim().trim_start_matches('@').trim().to_lowercase();
        let my_user_id = self.store.meta(META_USER_ID)?.unwrap_or_default();
        let mut seen: HashSet<String> = HashSet::from([my_user_id]);

        let mut candidates: Vec<MentionCandidate> = ranked(
            self.local_people(conversation_id)?
                .into_iter()
                .filter(|person| seen.insert(person.user_id.clone()))
                .map(MentionCandidate::Person),
            &query,
        );
        if let Conversation::Channel { team_id } = &conversation {
            candidates.extend(ranked(
                self.scope_entries(conversation_id, team_id)?,
                &query,
            ));
        }
        let local_people = candidates
            .iter()
            .filter(|candidate| matches!(candidate, MentionCandidate::Person(_)))
            .count();
        if query.chars().count() >= DIRECTORY_MIN_QUERY_CHARS
            && local_people < limit
            && let Ok(users) = self.search_people(&query).await
        {
            let directory = users
                .into_iter()
                .filter(|user| seen.insert(user.id.clone()))
                .map(directory_person)
                .map(MentionCandidate::Person);
            candidates.extend(ranked(directory, &query));
        }
        candidates.truncate(limit);
        Ok(candidates)
    }

    fn local_people(&self, conversation_id: &str) -> Result<Vec<PersonCandidate>> {
        let member = |user_id: &str, display_name: &str| PersonCandidate {
            user_id: user_id.to_owned(),
            display_name: display_name.to_owned(),
            mail: None,
            job_title: None,
            source: PersonSource::Member,
        };
        if let Some(chat) = self.store.chat(conversation_id)? {
            return Ok(chat
                .members
                .iter()
                .filter_map(|record| Some(member(record.user_id.as_deref()?, &record.display_name)))
                .collect());
        }
        let mut recent = self
            .store
            .messages(conversation_id, None, RECENT_SENDERS_SCANNED)?;
        recent.reverse();
        Ok(recent
            .iter()
            .filter_map(|record| {
                Some(member(
                    record.sender_id.as_deref()?,
                    record.sender_name.as_deref()?,
                ))
            })
            .collect())
    }

    fn scope_entries(
        &self,
        channel_id: &str,
        team_id: &str,
    ) -> Result<impl Iterator<Item = MentionCandidate> + use<R>> {
        let channel = self
            .store
            .channel(channel_id)?
            .map(|channel| MentionCandidate::Channel {
                channel_id: channel.id,
                name: channel.name,
            });
        let team = self
            .store
            .team(team_id)?
            .map(|team| MentionCandidate::Team {
                team_id: team.id,
                name: team.name,
            });
        Ok(channel.into_iter().chain(team))
    }

    fn directory_lock(&self) -> std::sync::MutexGuard<'_, DirectoryCache> {
        self.directory_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn directory_person(user: User) -> PersonCandidate {
    PersonCandidate {
        display_name: user
            .display_name
            .or_else(|| user.mail.clone())
            .or(user.user_principal_name)
            .unwrap_or_default(),
        mail: user.mail,
        job_title: user.job_title,
        user_id: user.id,
        source: PersonSource::Directory,
    }
}

/// Keeps matches only. Best match first, the incoming order breaks ties.
fn ranked(
    candidates: impl Iterator<Item = MentionCandidate>,
    query: &str,
) -> Vec<MentionCandidate> {
    let mut scored: Vec<(u8, MentionCandidate)> = candidates
        .filter_map(|candidate| Some((match_tier(&candidate, query)?, candidate)))
        .collect();
    scored.sort_by_key(|(tier, _)| *tier);
    scored.into_iter().map(|(_, candidate)| candidate).collect()
}

fn match_tier(candidate: &MentionCandidate, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(0);
    }
    let name = candidate.display_name().to_lowercase();
    if name.starts_with(query) {
        return Some(0);
    }
    if name
        .split(|character: char| !character.is_alphanumeric())
        .any(|word| word.starts_with(query))
    {
        return Some(1);
    }
    if name.contains(query) {
        return Some(2);
    }
    let MentionCandidate::Person(person) = candidate else {
        return None;
    };
    let mail = person.mail.as_deref()?.to_lowercase();
    if mail.starts_with(query) {
        Some(3)
    } else {
        mail.contains(query).then_some(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(user_id: &str, name: &str, mail: Option<&str>) -> MentionCandidate {
        MentionCandidate::Person(PersonCandidate {
            user_id: user_id.to_owned(),
            display_name: name.to_owned(),
            mail: mail.map(str::to_owned),
            job_title: None,
            source: PersonSource::Directory,
        })
    }

    fn names(candidates: Vec<MentionCandidate>) -> Vec<String> {
        candidates
            .iter()
            .map(|candidate| candidate.display_name().to_owned())
            .collect()
    }

    #[test]
    fn prefix_beats_word_start_beats_contains() {
        let ranked = ranked(
            [
                person("1", "Maria Adams", None),
                person("2", "Madame Curie", None),
                person("3", "Ada Example", None),
                person("4", "Nobody", None),
            ]
            .into_iter(),
            "ada",
        );
        assert_eq!(
            names(ranked),
            ["Ada Example", "Maria Adams", "Madame Curie"]
        );
    }

    #[test]
    fn mail_matches_rank_after_name_matches() {
        let ranked = ranked(
            [
                person("1", "Zed", Some("ada@example.com")),
                person("2", "Ada", None),
            ]
            .into_iter(),
            "ada",
        );
        assert_eq!(names(ranked), ["Ada", "Zed"]);
    }

    #[test]
    fn empty_query_keeps_the_incoming_order() {
        let ranked = ranked(
            [person("1", "B", None), person("2", "A", None)].into_iter(),
            "",
        );
        assert_eq!(names(ranked), ["B", "A"]);
    }

    #[test]
    fn candidates_convert_to_the_matching_mention() {
        let team = MentionCandidate::Team {
            team_id: "t".to_owned(),
            name: "Squad".to_owned(),
        };
        assert_eq!(team.to_mention(), crate::MentionInput::team("t", "Squad"));
    }

    #[test]
    fn the_directory_cache_expires_and_is_bounded() {
        let mut cache = DirectoryCache::new();
        cache.put("ada", Vec::new());
        assert!(cache.get("ada").is_some());
        assert!(cache.get("bo").is_none());
        for index in 0..DIRECTORY_CACHE_MAX_QUERIES + 1 {
            cache.put(&format!("q{index}"), Vec::new());
        }
        assert!(cache.entries.len() <= DIRECTORY_CACHE_MAX_QUERIES);
    }
}
