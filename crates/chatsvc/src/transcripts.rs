use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;
use session::{Scope, Session};

use crate::messages::encode;

const RECORDING_EVENT: &str = "#microsoft.graph.callRecordingEventMessageDetail";
const RECORDING_SUCCESS: &str = "success";
const MESSAGES_SCOPE: &str = "Chat.Read";
const SITE_SCOPE: &str = ".default";
const MAX_MESSAGE_PAGES: usize = 40;
const UNKNOWN_SPEAKER: &str = "?";

#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptTurn {
    pub speaker: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeetingTranscript {
    pub meeting_thread_id: String,
    pub recording_name: Option<String>,
    pub created_at: Option<String>,
    pub turns: Vec<TranscriptTurn>,
}

#[derive(Debug, thiserror::Error)]
pub enum TranscriptError {
    #[error("no finished recording in this meeting chat")]
    NoRecording,
    #[error("the recording has no transcript")]
    NoTranscript,
    #[error("no access to the recording or its transcript")]
    AccessDenied,
    #[error(transparent)]
    Request(session::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    pub url: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptInfo {
    pub id: String,
    pub created_at: Option<String>,
}

pub struct Transcripts<'a> {
    session: &'a Session,
}

impl<'a> Transcripts<'a> {
    pub fn new(session: &'a Session) -> Self {
        Transcripts { session }
    }

    pub async fn for_meeting_chat(
        &self,
        thread_id: &str,
    ) -> Result<MeetingTranscript, TranscriptError> {
        let recording = self.find_recording(thread_id).await?;
        let site = site_origin(&recording.url);
        let site_scope = Scope::new(site.clone(), SITE_SCOPE);
        let item_url = self.item_url(&recording.url, &site, &site_scope).await?;
        let listing = self
            .get(
                &format!("{item_url}/media/transcripts"),
                &site_scope,
                TranscriptError::NoTranscript,
            )
            .await?;
        let transcript = newest_transcript(parse_transcript_list(&listing))
            .ok_or(TranscriptError::NoTranscript)?;
        let content = self
            .get(
                &format!(
                    "{item_url}/media/transcripts/{}/streamContent?format=json",
                    encode(&transcript.id)
                ),
                &site_scope,
                TranscriptError::NoTranscript,
            )
            .await?;
        Ok(MeetingTranscript {
            meeting_thread_id: thread_id.to_owned(),
            recording_name: recording.name,
            created_at: transcript.created_at,
            turns: parse_turns(&content),
        })
    }

    async fn find_recording(&self, thread_id: &str) -> Result<Recording, TranscriptError> {
        let scope = Scope::graph(MESSAGES_SCOPE);
        let mut url = format!(
            "{}/v1.0/chats/{}/messages?$top=50&$orderby=createdDateTime desc",
            session::GRAPH,
            encode(thread_id)
        );
        for _ in 0..MAX_MESSAGE_PAGES {
            let page = self.get(&url, &scope, TranscriptError::NoRecording).await?;
            if let Some(recording) = find_recording(&page) {
                return Ok(recording);
            }
            match page.get("@odata.nextLink").and_then(Value::as_str) {
                Some(next) => url = next.to_owned(),
                None => break,
            }
        }
        Err(TranscriptError::NoRecording)
    }

    async fn item_url(
        &self,
        recording_url: &str,
        site: &str,
        site_scope: &Scope,
    ) -> Result<String, TranscriptError> {
        let item = self
            .get(
                &format!(
                    "{site}/_api/v2.1/shares/{}/driveItem?$select=id,parentReference",
                    share_id(recording_url)
                ),
                site_scope,
                TranscriptError::NoRecording,
            )
            .await?;
        let drive_id = item
            .pointer("/parentReference/driveId")
            .and_then(Value::as_str);
        let item_id = item.get("id").and_then(Value::as_str);
        match (drive_id, item_id) {
            (Some(drive_id), Some(item_id)) => Ok(format!(
                "{site}/_api/v2.1/drives/{drive_id}/items/{item_id}"
            )),
            _ => Err(TranscriptError::NoRecording),
        }
    }

    pub(crate) async fn get(
        &self,
        url: &str,
        scope: &Scope,
        not_found: TranscriptError,
    ) -> Result<Value, TranscriptError> {
        match self
            .session
            .request(session::Method::Get, url, scope, None)
            .await
        {
            Ok(answer) => Ok(answer.body),
            Err(session::Error::Api {
                status: 403 | 401, ..
            }) => Err(TranscriptError::AccessDenied),
            Err(session::Error::Api { status: 404, .. }) => Err(not_found),
            Err(error) => Err(TranscriptError::Request(error)),
        }
    }
}

pub fn find_recording(messages_json: &Value) -> Option<Recording> {
    messages_json
        .get("value")?
        .as_array()?
        .iter()
        .find_map(|message| {
            let detail = message.get("eventDetail")?;
            let matches = detail.get("@odata.type").and_then(Value::as_str)
                == Some(RECORDING_EVENT)
                && detail.get("callRecordingStatus").and_then(Value::as_str)
                    == Some(RECORDING_SUCCESS);
            let url = detail.get("callRecordingUrl").and_then(Value::as_str)?;
            if !matches || !url.starts_with("https://") {
                return None;
            }
            Some(Recording {
                url: url.to_owned(),
                name: detail
                    .get("callRecordingDisplayName")
                    .and_then(Value::as_str)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned),
            })
        })
}

pub fn find_recording_url(messages_json: &Value) -> Option<String> {
    find_recording(messages_json).map(|recording| recording.url)
}

pub fn share_id(url: &str) -> String {
    format!("u!{}", URL_SAFE_NO_PAD.encode(url))
}

pub fn site_origin(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("https", url));
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    format!("{scheme}://{host}")
}

pub fn parse_transcript_list(listing_json: &Value) -> Vec<TranscriptInfo> {
    listing_json
        .get("value")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| {
            entry
                .get("isVisible")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        })
        .filter_map(|entry| {
            Some(TranscriptInfo {
                id: entry
                    .get("id")?
                    .as_str()
                    .filter(|id| !id.is_empty())?
                    .to_owned(),
                created_at: entry
                    .get("createdDateTime")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

pub fn newest_transcript(transcripts: Vec<TranscriptInfo>) -> Option<TranscriptInfo> {
    transcripts
        .into_iter()
        .max_by(|left, right| left.created_at.cmp(&right.created_at))
}

pub fn parse_turns(content_json: &Value) -> Vec<TranscriptTurn> {
    let turns = content_json
        .get("entries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let text = entry.get("text").and_then(Value::as_str)?.trim();
            if text.is_empty() {
                return None;
            }
            let speaker = entry
                .get("speakerDisplayName")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .unwrap_or(UNKNOWN_SPEAKER);
            Some(TranscriptTurn {
                speaker: speaker.to_owned(),
                start_seconds: offset_seconds(entry.get("startOffset")),
                end_seconds: offset_seconds(entry.get("endOffset")),
                text: text.to_owned(),
            })
        })
        .collect();
    merge_consecutive_turns(turns)
}

pub fn merge_consecutive_turns(turns: Vec<TranscriptTurn>) -> Vec<TranscriptTurn> {
    let mut merged: Vec<TranscriptTurn> = Vec::new();
    for turn in turns {
        match merged.last_mut() {
            Some(last) if last.speaker == turn.speaker => {
                last.text.push(' ');
                last.text.push_str(&turn.text);
                last.end_seconds = turn.end_seconds;
            }
            _ => merged.push(turn),
        }
    }
    merged
}

fn offset_seconds(offset: Option<&Value>) -> f64 {
    offset
        .and_then(Value::as_str)
        .map(|text| {
            text.split(':').fold(0.0, |total, part| {
                total * 60.0 + part.parse::<f64>().unwrap_or(0.0)
            })
        })
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/tests/fixtures/transcripts/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn finds_the_successful_recording_only() {
        let recording = find_recording(&fixture("messages.json")).unwrap();
        assert!(recording.url.ends_with("Weekly%20sync.mp4"));
        assert_eq!(
            recording.name.as_deref(),
            Some("Weekly sync-20260302_100000-Meeting Recording.mp4")
        );
        assert_eq!(
            find_recording_url(&fixture("messages.json")),
            Some(recording.url)
        );
    }

    #[test]
    fn no_recording_in_chat_without_event() {
        assert_eq!(
            find_recording_url(&serde_json::json!({"value": [{"id": "1"}]})),
            None
        );
        assert_eq!(find_recording_url(&serde_json::json!({})), None);
    }

    #[test]
    fn share_id_is_unpadded_base64url() {
        assert_eq!(
            share_id("https://a.example/x?y=>>"),
            "u!aHR0cHM6Ly9hLmV4YW1wbGUveD95PT4-"
        );
        assert!(!share_id("https://a.example/ab").contains('='));
    }

    #[test]
    fn site_origin_drops_path() {
        assert_eq!(
            site_origin("https://contoso-my.sharepoint.com/personal/a/b.mp4?e=1"),
            "https://contoso-my.sharepoint.com"
        );
    }

    #[test]
    fn list_skips_hidden_and_idless_and_picks_newest() {
        let transcripts = parse_transcript_list(&fixture("list.json"));
        assert_eq!(transcripts.len(), 2);
        assert_eq!(newest_transcript(transcripts).unwrap().id, "t-new");
        assert_eq!(newest_transcript(Vec::new()), None);
    }

    #[test]
    fn turns_merge_same_speaker_and_parse_offsets() {
        let turns = parse_turns(&fixture("content.json"));
        assert_eq!(turns.len(), 4);
        assert_eq!(turns[0].speaker, "Alex Example");
        assert_eq!(turns[0].text, "Welcome everyone. Let us start.");
        assert_eq!(turns[0].start_seconds, 1.5);
        assert_eq!(turns[0].end_seconds, 7.0);
        assert_eq!(turns[1].end_seconds, 69.25);
        assert_eq!(turns[2].speaker, "?");
        assert_eq!(turns[2].start_seconds, 3600.0);
    }

    #[test]
    fn missing_entries_give_no_turns() {
        assert!(parse_turns(&serde_json::json!({})).is_empty());
    }
}
