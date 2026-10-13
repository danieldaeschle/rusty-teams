use std::collections::{BTreeSet, HashSet};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use serde_json::Value;
use session::{Method, Request, Scope, Session};

use crate::messages::encode;
use crate::pins::DEFAULT_REGION;
use crate::transcripts::{
    TranscriptError, TranscriptTurn, Transcripts, newest_transcript, parse_transcript_list,
    parse_turns, share_id, site_origin,
};

const RECORDING_EVENT: &str = "#microsoft.graph.callRecordingEventMessageDetail";
const RECORDING_SUCCESS: &str = "success";
const MESSAGES_SCOPE: &str = "Chat.Read";
const SITE_SCOPE: &str = ".default";
const ATTENDANCE_SCOPE: &str = "OnlineMeetingArtifact.Read.All";
const MAX_MESSAGE_PAGES: usize = 40;
const AUDIO_ONLY_MARKER: &str = "videoContent=\"CurtainOnly\"";
const ORIGINAL_NAME_OPEN: &str = "<OriginalName v=\"";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub message_id: String,
    pub started_at: Option<DateTime<Utc>>,
    pub recording_url: String,
    pub recording_name: Option<String>,
    pub duration_seconds: Option<u64>,
    pub organizer_id: Option<String>,
    pub audio_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingDetails {
    pub item_url: String,
    pub audio_only: bool,
    pub transcript_only: bool,
    pub duration_seconds: Option<u64>,
    pub expires_at: Option<DateTime<Utc>>,
    pub transcript_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TranscriptFormat {
    Vtt,
    Docx,
}

impl TranscriptFormat {
    pub fn query(self) -> &'static str {
        match self {
            TranscriptFormat::Vtt => "vtt",
            TranscriptFormat::Docx => "docx",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttendanceRecord {
    pub name: String,
    pub joined_at: Option<DateTime<Utc>>,
    pub left_at: Option<DateTime<Utc>>,
    pub duration_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attendance {
    TurnedOff,
    Report(Vec<AttendanceRecord>),
}

pub struct Recap<'a> {
    session: &'a Session,
    transcripts: Transcripts<'a>,
}

impl<'a> Recap<'a> {
    pub fn new(session: &'a Session) -> Self {
        Recap {
            session,
            transcripts: Transcripts::new(session),
        }
    }

    pub async fn occurrences(&self, thread_id: &str) -> Result<Vec<Occurrence>, TranscriptError> {
        let scope = Scope::graph(MESSAGES_SCOPE);
        let mut url = format!(
            "{}/v1.0/chats/{}/messages?$top=50&$orderby=createdDateTime desc",
            session::GRAPH,
            encode(thread_id)
        );
        let mut found = Vec::new();
        for _ in 0..MAX_MESSAGE_PAGES {
            let page = self
                .transcripts
                .get(&url, &scope, TranscriptError::NoRecording)
                .await?;
            found.extend(parse_occurrences(&page));
            match page.get("@odata.nextLink").and_then(Value::as_str) {
                Some(next) => url = next.to_owned(),
                None => break,
            }
        }
        let audio_only_names = self.audio_only_names(thread_id).await;
        let occurrences = sorted_occurrences(with_audio_only(found, &audio_only_names));
        let deleted = self.deleted_recordings(&occurrences).await;
        Ok(without_deleted(occurrences, &deleted))
    }

    async fn audio_only_names(&self, thread_id: &str) -> HashSet<String> {
        let scope = Scope::new(session::IC3, "Teams.AccessAsUser.All");
        let mut url = format!(
            "https://teams.cloud.microsoft/api/chatsvc/{DEFAULT_REGION}/v1/users/ME/conversations/{}/messages?view=msnp24Equivalent&pageSize=200",
            encode(thread_id)
        );
        let mut names = HashSet::new();
        for _ in 0..MAX_MESSAGE_PAGES {
            let Ok(page) = self
                .transcripts
                .get(&url, &scope, TranscriptError::NoRecording)
                .await
            else {
                break;
            };
            names.extend(parse_audio_only_names(&page));
            match page
                .pointer("/_metadata/backwardLink")
                .and_then(Value::as_str)
                .filter(|link| !link.is_empty() && *link != url)
            {
                Some(next) => url = next.to_owned(),
                None => break,
            }
        }
        names
    }

    async fn deleted_recordings(&self, occurrences: &[Occurrence]) -> HashSet<String> {
        let mut deleted = HashSet::new();
        let sites: BTreeSet<String> = occurrences
            .iter()
            .map(|occurrence| site_origin(&occurrence.recording_url))
            .collect();
        for site in sites {
            let urls: Vec<&str> = occurrences
                .iter()
                .map(|occurrence| occurrence.recording_url.as_str())
                .filter(|url| site_origin(url) == site)
                .collect();
            let requests: Vec<Request> = urls
                .iter()
                .map(|url| {
                    Request::get(format!(
                        "{site}/_api/v2.1/shares/{}/driveItem?$select=id",
                        share_id(url)
                    ))
                })
                .collect();
            let Ok(answers) = self
                .session
                .batch(&requests, &Scope::new(site.clone(), SITE_SCOPE))
                .await
            else {
                continue;
            };
            deleted.extend(
                urls.iter()
                    .zip(answers)
                    .filter(|(_, answer)| answer.status == 404)
                    .map(|(url, _)| (*url).to_owned()),
            );
        }
        deleted
    }

    pub async fn details(
        &self,
        occurrence: &Occurrence,
    ) -> Result<RecordingDetails, TranscriptError> {
        let site = site_origin(&occurrence.recording_url);
        let site_scope = Scope::new(site.clone(), SITE_SCOPE);
        let item = self
            .transcripts
            .get(
                &format!(
                    "{site}/_api/v2.1/shares/{}/driveItem?$select=id,parentReference,video,file,media",
                    share_id(&occurrence.recording_url)
                ),
                &site_scope,
                TranscriptError::NoRecording,
            )
            .await?;
        let item_url = item_url(&site, &item).ok_or(TranscriptError::NoRecording)?;
        let expiration = self
            .transcripts
            .get(
                &format!("{item_url}/listItem?$expand=fields($select=_ExpirationDate)"),
                &site_scope,
                TranscriptError::NoRecording,
            )
            .await
            .ok();
        let listing = self
            .transcripts
            .get(
                &format!("{item_url}/media/transcripts"),
                &site_scope,
                TranscriptError::NoTranscript,
            )
            .await
            .ok();
        Ok(RecordingDetails {
            audio_only: occurrence.audio_only || is_audio_only(&item),
            transcript_only: is_transcript_only(&item),
            duration_seconds: item_duration_seconds(&item).or(occurrence.duration_seconds),
            expires_at: expiration.as_ref().and_then(parse_expiration),
            transcript_id: listing
                .as_ref()
                .and_then(|listing| newest_transcript(parse_transcript_list(listing)))
                .map(|transcript| transcript.id),
            item_url,
        })
    }

    pub async fn transcript(
        &self,
        details: &RecordingDetails,
    ) -> Result<Vec<TranscriptTurn>, TranscriptError> {
        let transcript_id = details
            .transcript_id
            .as_deref()
            .ok_or(TranscriptError::NoTranscript)?;
        let site_scope = Scope::new(site_origin(&details.item_url), SITE_SCOPE);
        let content = self
            .transcripts
            .get(
                &transcript_url(&details.item_url, transcript_id, "json"),
                &site_scope,
                TranscriptError::NoTranscript,
            )
            .await?;
        Ok(parse_turns(&content))
    }

    pub async fn transcript_file(
        &self,
        details: &RecordingDetails,
        format: TranscriptFormat,
    ) -> Result<Vec<u8>, TranscriptError> {
        let transcript_id = details
            .transcript_id
            .as_deref()
            .ok_or(TranscriptError::NoTranscript)?;
        let site_scope = Scope::new(site_origin(&details.item_url), SITE_SCOPE);
        let request = Request::binary_get(transcript_url(
            &details.item_url,
            transcript_id,
            format.query(),
        ));
        let answer = self
            .session
            .send(request, &site_scope)
            .await
            .map_err(request_failure)?;
        decode_binary(&answer.body).ok_or(TranscriptError::NoTranscript)
    }

    pub async fn delete_recording(
        &self,
        details: &RecordingDetails,
    ) -> Result<(), TranscriptError> {
        let site_scope = Scope::new(site_origin(&details.item_url), SITE_SCOPE);
        self.session
            .request(Method::Delete, &details.item_url, &site_scope, None)
            .await
            .map(|_| ())
            .map_err(request_failure)
    }

    pub async fn attendance(&self, thread_id: &str) -> Result<Attendance, TranscriptError> {
        let graph_scope = Scope::graph(MESSAGES_SCOPE);
        let chat = match self
            .transcripts
            .get(
                &format!(
                    "{}/v1.0/chats/{}?$select=onlineMeetingInfo",
                    session::GRAPH,
                    encode(thread_id)
                ),
                &graph_scope,
                TranscriptError::NoRecording,
            )
            .await
        {
            Ok(chat) => chat,
            Err(TranscriptError::NoRecording | TranscriptError::AccessDenied) => {
                return Ok(Attendance::TurnedOff);
            }
            Err(error) => return Err(error),
        };
        let Some(join_url) = chat
            .pointer("/onlineMeetingInfo/joinWebUrl")
            .and_then(Value::as_str)
        else {
            return Ok(Attendance::TurnedOff);
        };
        let scope = Scope::graph(ATTENDANCE_SCOPE);
        let lookup = format!(
            "{}/v1.0/me/onlineMeetings?$filter={}&$select=id",
            session::GRAPH,
            encode(&format!("JoinWebUrl eq '{}'", join_url.replace('\'', "''")))
        );
        let meeting_id = match self
            .transcripts
            .get(&lookup, &scope, TranscriptError::NoRecording)
            .await
        {
            Ok(found) => found
                .pointer("/value/0/id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            Err(TranscriptError::NoRecording | TranscriptError::AccessDenied) => None,
            Err(error) => return Err(error),
        };
        let Some(meeting_id) = meeting_id else {
            return Ok(Attendance::TurnedOff);
        };
        let reports_url = format!(
            "{}/v1.0/me/onlineMeetings/{}/attendanceReports",
            session::GRAPH,
            encode(&meeting_id)
        );
        let reports = match self
            .transcripts
            .get(&reports_url, &scope, TranscriptError::NoRecording)
            .await
        {
            Ok(reports) => reports,
            Err(TranscriptError::NoRecording | TranscriptError::AccessDenied) => {
                return Ok(Attendance::TurnedOff);
            }
            Err(error) => return Err(error),
        };
        let Some(report_id) = reports
            .pointer("/value/0/id")
            .and_then(Value::as_str)
        else {
            return Ok(Attendance::TurnedOff);
        };
        let report = self
            .transcripts
            .get(
                &format!(
                    "{reports_url}/{}?$expand=attendanceRecords",
                    encode(report_id)
                ),
                &scope,
                TranscriptError::NoRecording,
            )
            .await?;
        Ok(Attendance::Report(parse_attendance(&report)))
    }
}

fn request_failure(error: session::Error) -> TranscriptError {
    match error {
        session::Error::Api {
            status: 403 | 401, ..
        } => TranscriptError::AccessDenied,
        error => TranscriptError::Request(error),
    }
}

fn transcript_url(item_url: &str, transcript_id: &str, format: &str) -> String {
    format!(
        "{item_url}/media/transcripts/{}/streamContent?format={format}",
        encode(transcript_id)
    )
}

fn item_url(site: &str, item: &Value) -> Option<String> {
    let drive_id = item
        .pointer("/parentReference/driveId")
        .and_then(Value::as_str)?;
    let item_id = item.get("id").and_then(Value::as_str)?;
    Some(format!("{site}/_api/v2.1/drives/{drive_id}/items/{item_id}"))
}

fn decode_binary(body: &Value) -> Option<Vec<u8>> {
    STANDARD.decode(body.get("base64")?.as_str()?).ok()
}

pub fn parse_occurrences(messages_json: &Value) -> Vec<Occurrence> {
    messages_json
        .get("value")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|message| {
            let detail = message.get("eventDetail")?;
            let finished = detail.get("@odata.type").and_then(Value::as_str)
                == Some(RECORDING_EVENT)
                && detail.get("callRecordingStatus").and_then(Value::as_str)
                    == Some(RECORDING_SUCCESS);
            let url = detail.get("callRecordingUrl").and_then(Value::as_str)?;
            if !finished || !url.starts_with("https://") {
                return None;
            }
            Some(Occurrence {
                message_id: message.get("id").and_then(Value::as_str)?.to_owned(),
                started_at: message
                    .get("createdDateTime")
                    .and_then(Value::as_str)
                    .and_then(|stamp| DateTime::parse_from_rfc3339(stamp).ok())
                    .map(|stamp| stamp.with_timezone(&Utc)),
                recording_url: url.to_owned(),
                recording_name: detail
                    .get("callRecordingDisplayName")
                    .and_then(Value::as_str)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned),
                duration_seconds: detail
                    .get("callRecordingDuration")
                    .and_then(Value::as_str)
                    .and_then(parse_iso_duration),
                organizer_id: detail
                    .pointer("/meetingOrganizer/user/id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                audio_only: false,
            })
        })
        .collect()
}

pub fn parse_audio_only_names(chatsvc_messages_json: &Value) -> HashSet<String> {
    chatsvc_messages_json
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|message| {
            message.get("messagetype").and_then(Value::as_str) == Some("RichText/Media_CallRecording")
        })
        .filter_map(|message| message.get("content").and_then(Value::as_str))
        .filter(|content| content.contains(AUDIO_ONLY_MARKER))
        .filter_map(|content| {
            let start = content.find(ORIGINAL_NAME_OPEN)? + ORIGINAL_NAME_OPEN.len();
            let length = content[start..].find('"')?;
            Some(content[start..start + length].to_owned())
        })
        .collect()
}

pub fn with_audio_only(
    occurrences: Vec<Occurrence>,
    audio_only_names: &HashSet<String>,
) -> Vec<Occurrence> {
    occurrences
        .into_iter()
        .map(|occurrence| Occurrence {
            audio_only: occurrence
                .recording_name
                .as_ref()
                .is_some_and(|name| audio_only_names.contains(name)),
            ..occurrence
        })
        .collect()
}

pub fn sorted_occurrences(mut occurrences: Vec<Occurrence>) -> Vec<Occurrence> {
    occurrences.sort_by_key(|occurrence| std::cmp::Reverse(occurrence.started_at));
    occurrences.dedup_by(|left, right| left.recording_url == right.recording_url);
    occurrences
}

pub fn without_deleted(occurrences: Vec<Occurrence>, deleted: &HashSet<String>) -> Vec<Occurrence> {
    occurrences
        .into_iter()
        .filter(|occurrence| !deleted.contains(&occurrence.recording_url))
        .collect()
}

pub fn parse_iso_duration(text: &str) -> Option<u64> {
    let time = text.strip_prefix("PT")?;
    let mut total = 0.0;
    let mut number = String::new();
    for character in time.chars() {
        let unit = match character {
            'H' => 3600.0,
            'M' => 60.0,
            'S' => 1.0,
            digit => {
                number.push(digit);
                continue;
            }
        };
        total += number.parse::<f64>().ok()? * unit;
        number.clear();
    }
    number.is_empty().then_some(total as u64)
}

pub fn parse_expiration(listing_json: &Value) -> Option<DateTime<Utc>> {
    let stamp = listing_json
        .pointer("/fields/_ExpirationDate")?
        .as_str()?;
    DateTime::parse_from_rfc3339(stamp)
        .ok()
        .map(|stamp| stamp.with_timezone(&Utc))
}

pub fn is_audio_only(item_json: &Value) -> bool {
    item_json
        .get("video")
        .and_then(|video| video.get("width"))
        .and_then(Value::as_u64)
        .is_none_or(|width| width == 0)
}

pub fn is_transcript_only(item_json: &Value) -> bool {
    item_json
        .pointer("/media/mediaSource/contentType")
        .and_then(Value::as_str)
        == Some("Transcript")
}

pub fn item_duration_seconds(item_json: &Value) -> Option<u64> {
    item_json
        .pointer("/video/duration")
        .and_then(Value::as_u64)
        .map(|milliseconds| milliseconds / 1000)
}

pub fn days_until_expiry(expires_at: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
    let remaining = expires_at - now;
    (remaining.num_seconds() + 86_399).div_euclid(86_400).max(0)
}

pub fn parse_attendance(report_json: &Value) -> Vec<AttendanceRecord> {
    let parse_stamp = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .and_then(|stamp| DateTime::parse_from_rfc3339(stamp).ok())
            .map(|stamp| stamp.with_timezone(&Utc))
    };
    report_json
        .get("attendanceRecords")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|record| {
            let intervals: Vec<&Value> = record
                .get("attendanceIntervals")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .collect();
            let name = record
                .pointer("/identity/displayName")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .or_else(|| record.get("emailAddress").and_then(Value::as_str))
                .unwrap_or("?");
            AttendanceRecord {
                name: name.to_owned(),
                joined_at: intervals
                    .iter()
                    .filter_map(|interval| parse_stamp(interval.get("joinDateTime")))
                    .min(),
                left_at: intervals
                    .iter()
                    .filter_map(|interval| parse_stamp(interval.get("leaveDateTime")))
                    .max(),
                duration_seconds: record
                    .get("totalAttendanceInSeconds")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/tests/fixtures/recap/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn occurrences_keep_finished_recordings_newest_first() {
        let occurrences = sorted_occurrences(parse_occurrences(&fixture("messages.json")));
        assert_eq!(occurrences.len(), 2);
        assert_eq!(occurrences[0].message_id, "2002");
        assert_eq!(occurrences[0].duration_seconds, Some(1832));
        assert_eq!(occurrences[0].organizer_id.as_deref(), Some("user-organizer"));
        assert_eq!(occurrences[1].message_id, "2001");
        assert_eq!(occurrences[1].duration_seconds, None);
    }

    #[test]
    fn deleted_recordings_drop_out_of_the_occurrences() {
        let occurrences = sorted_occurrences(parse_occurrences(&fixture("messages.json")));
        let deleted: HashSet<String> = [occurrences[0].recording_url.clone()].into();
        let remaining = without_deleted(occurrences, &deleted);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].message_id, "2001");
    }

    #[test]
    fn iso_durations_parse_to_seconds() {
        assert_eq!(parse_iso_duration("PT1H2M3S"), Some(3723));
        assert_eq!(parse_iso_duration("PT45M"), Some(2700));
        assert_eq!(parse_iso_duration("PT12.5S"), Some(12));
        assert_eq!(parse_iso_duration("1H"), None);
        assert_eq!(parse_iso_duration("PT5"), None);
    }

    #[test]
    fn expiration_reads_the_list_item_field() {
        let expires_at = parse_expiration(&fixture("expiration.json")).unwrap();
        assert_eq!(expires_at.to_rfc3339(), "2026-05-01T08:00:00+00:00");
        assert_eq!(parse_expiration(&serde_json::json!({"fields": {}})), None);
    }

    #[test]
    fn days_until_expiry_rounds_up_and_floors_at_zero() {
        let now = DateTime::parse_from_rfc3339("2026-04-24T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let in_hours = |hours| now + chrono::Duration::hours(hours);
        assert_eq!(days_until_expiry(in_hours(7 * 24), now), 7);
        assert_eq!(days_until_expiry(in_hours(36), now), 2);
        assert_eq!(days_until_expiry(in_hours(-5), now), 0);
    }

    #[test]
    fn audio_only_when_the_item_has_no_video_size() {
        assert!(is_audio_only(&serde_json::json!({"file": {}})));
        assert!(!is_audio_only(
            &serde_json::json!({"video": {"width": 1280, "duration": 90000}})
        ));
        assert_eq!(
            item_duration_seconds(&serde_json::json!({"video": {"duration": 90000}})),
            Some(90)
        );
    }

    #[test]
    fn audio_only_recordings_are_found_by_the_curtain_marker_in_the_chat_event() {
        let names = parse_audio_only_names(&fixture("chatsvc_messages.json"));
        assert_eq!(
            names,
            HashSet::from(["Weekly sync-20260309_100000-Meeting Recording.mp4".to_owned()])
        );
        let occurrences = with_audio_only(
            sorted_occurrences(parse_occurrences(&fixture("messages.json"))),
            &names,
        );
        assert!(occurrences[0].audio_only);
        assert!(!occurrences[1].audio_only);
    }

    #[test]
    fn transcript_only_items_are_told_apart_by_their_media_source() {
        assert!(is_transcript_only(&fixture("drive_item_transcript.json")));
        assert!(!is_transcript_only(&fixture("drive_item.json")));
        assert!(!is_transcript_only(&fixture("drive_item_recording.json")));
    }

    #[test]
    fn recording_item_gives_url_duration_and_video() {
        let item = fixture("drive_item.json");
        assert_eq!(
            item_url("https://contoso-my.sharepoint.com", &item).as_deref(),
            Some("https://contoso-my.sharepoint.com/_api/v2.1/drives/drive-1/items/item-1")
        );
        assert!(!is_audio_only(&item));
        assert_eq!(item_duration_seconds(&item), Some(53));
    }

    #[test]
    fn transcript_urls_request_the_wanted_format() {
        assert_eq!(
            transcript_url("https://a.example/_api/v2.1/drives/d/items/i", "t 1", "vtt"),
            "https://a.example/_api/v2.1/drives/d/items/i/media/transcripts/t%201/streamContent?format=vtt"
        );
        assert_eq!(TranscriptFormat::Docx.query(), "docx");
    }

    #[test]
    fn binary_answers_decode_from_base64() {
        let body = serde_json::json!({"base64": "V0VCVlRU", "contentType": "text/vtt"});
        assert_eq!(decode_binary(&body), Some(b"WEBVTT".to_vec()));
        assert_eq!(decode_binary(&serde_json::json!({})), None);
    }

    #[test]
    fn attendance_spans_first_join_to_last_leave() {
        let records = parse_attendance(&fixture("attendance.json"));
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "Alex Example");
        assert_eq!(records[0].duration_seconds, 1500);
        assert_eq!(
            records[0].joined_at.unwrap().to_rfc3339(),
            "2026-03-02T10:00:00+00:00"
        );
        assert_eq!(
            records[0].left_at.unwrap().to_rfc3339(),
            "2026-03-02T10:30:00+00:00"
        );
        assert_eq!(records[1].name, "sam@example.com");
    }
}
