use std::collections::{HashMap, HashSet};

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use store::MessageRecord;
use teams_core::{CallEvent, CallPhase, CallType, ImageRef, format_duration};

use crate::format;

const MAX_AVATARS: usize = 4;
const RECAP_FALLBACK_TITLE: &str = "Meeting";
const RECAP_DATE_FORMAT: &str = "%A, %-d %B %Y %H:%M";
const RECAP_TIME_FORMAT: &str = "%H:%M";
const AUTO_STOP_SECONDS: i64 = 5;
const WEEKDAY_STAMP_DAYS: i64 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallIcon {
    Call,
    Missed,
    Video,
    Record,
    RecordStop,
    Saved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallParticipant {
    pub user_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingCard {
    pub message_id: String,
    pub duration: Option<String>,
    pub url: Option<String>,
    pub thumbnail: Option<ImageRef>,
    pub transcript: bool,
    pub expires_in_days: Option<i64>,
}

/// The card Teams puts under the "Meeting ended" line of a meeting with a recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecapCard {
    pub title: String,
    pub date_line: String,
    pub recording: RecordingCard,
}

/// A meeting or recording event as Teams draws it: one muted line, icon in the avatar column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallNotice {
    pub stamp: String,
    pub text: String,
    pub emphasis: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallCard {
    pub title: String,
    pub detail: String,
    pub icon: CallIcon,
    pub missed: bool,
    pub started: bool,
    pub participants: Vec<CallParticipant>,
    pub extra_participants: usize,
    pub notice: Option<CallNotice>,
    pub recap: Option<RecapCard>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeetingSlot {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// What the meeting rows need beyond the messages: the chat topic, the calendar occurrences
/// and the recording expiry the Recap tab reads, keyed by the saved recording message id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeetingInfo {
    pub title: Option<String>,
    pub slots: Vec<MeetingSlot>,
    pub expiry_days: HashMap<String, i64>,
}

pub struct CallNames<'a> {
    pub my_user_id: Option<&'a str>,
    pub names: &'a HashMap<String, String>,
}

impl CallNames<'_> {
    fn name(&self, user_id: Option<&str>, given: Option<&str>) -> Option<String> {
        given
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .or_else(|| self.names.get(user_id?).cloned())
    }
}

pub struct CallClock<'a> {
    pub offset: FixedOffset,
    pub today: NaiveDate,
    pub meeting: &'a MeetingInfo,
}

struct SavedRecording {
    message_id: String,
    event: CallEvent,
}

/// Which call events of a timeline show as rows: Teams keeps one row per recording start and stop,
/// hides transcript notices and, for calls (not meetings), the start once the call ended.
pub struct CallTimeline {
    hidden: Vec<bool>,
    finished: HashSet<String>,
    transcripts: HashSet<String>,
    newest_open_start: Option<usize>,
    started_at: HashMap<String, DateTime<Utc>>,
    ended_at: HashMap<String, DateTime<Utc>>,
    saved: HashMap<String, SavedRecording>,
}

impl CallTimeline {
    pub fn new(events: &[Option<CallEvent>]) -> Self {
        Self::with_records(events, &[])
    }

    pub fn with_records(events: &[Option<CallEvent>], records: &[&MessageRecord]) -> Self {
        let finished: HashSet<String> = events
            .iter()
            .flatten()
            .filter(|event| event.phase == CallPhase::Ended)
            .map(|event| event.call_id.clone())
            .collect();
        let transcripts: HashSet<String> = events
            .iter()
            .flatten()
            .filter(|event| event.phase == CallPhase::TranscriptStarted)
            .map(|event| event.call_id.clone())
            .collect();
        let mut shown_once: HashSet<(String, CallPhase)> = HashSet::new();
        let hidden = events
            .iter()
            .map(|event| {
                let Some(event) = event else {
                    return false;
                };
                match event.phase {
                    CallPhase::TranscriptStarted => true,
                    CallPhase::RecordingStarted | CallPhase::RecordingStopped => {
                        !shown_once.insert((event.call_id.clone(), event.phase))
                    }
                    CallPhase::Started => {
                        event.call_type == CallType::Call && finished.contains(&event.call_id)
                    }
                    _ => false,
                }
            })
            .collect();
        let newest_open_start = events.iter().rposition(|event| {
            event.as_ref().is_some_and(|event| {
                event.phase == CallPhase::Started && !finished.contains(&event.call_id)
            })
        });
        let mut started_at = HashMap::new();
        let mut ended_at = HashMap::new();
        let mut saved = HashMap::new();
        for (event, record) in events.iter().zip(records) {
            let Some(event) = event else {
                continue;
            };
            match event.phase {
                CallPhase::Started => {
                    started_at
                        .entry(event.call_id.clone())
                        .or_insert(record.created_at);
                }
                CallPhase::Ended => {
                    ended_at.insert(event.call_id.clone(), record.created_at);
                }
                CallPhase::RecordingSaved => {
                    saved.insert(
                        event.call_id.clone(),
                        SavedRecording {
                            message_id: record.message_id.clone(),
                            event: event.clone(),
                        },
                    );
                }
                _ => {}
            }
        }
        CallTimeline {
            hidden,
            finished,
            transcripts,
            newest_open_start,
            started_at,
            ended_at,
            saved,
        }
    }

    pub fn hides(&self, index: usize) -> bool {
        self.hidden.get(index).copied().unwrap_or(false)
    }

    pub fn is_live_start(&self, index: usize) -> bool {
        self.newest_open_start == Some(index)
    }

    pub fn finished(&self, call_id: &str) -> bool {
        self.finished.contains(call_id)
    }

    pub fn has_transcript(&self, call_id: &str) -> bool {
        self.transcripts.contains(call_id)
    }

    /// A recording that stops with the meeting end has no one who stopped it; Teams then says
    /// "Recording stopped." instead of naming the recorder.
    fn stopped_by_meeting_end(&self, event: &CallEvent, created_at: DateTime<Utc>) -> bool {
        self.ended_at.get(&event.call_id).is_some_and(|ended_at| {
            (*ended_at - created_at).num_seconds().abs() <= AUTO_STOP_SECONDS
        })
    }

    fn recap(
        &self,
        ended: &CallEvent,
        ended_at: DateTime<Utc>,
        clock: &CallClock,
    ) -> Option<RecapCard> {
        if ended.phase != CallPhase::Ended || ended.call_type != CallType::Meeting {
            return None;
        }
        let saved = self.saved.get(&ended.call_id)?;
        let started_at = self
            .started_at
            .get(&ended.call_id)
            .copied()
            .or_else(|| {
                ended
                    .duration_seconds
                    .map(|seconds| ended_at - chrono::Duration::seconds(seconds as i64))
            })
            .unwrap_or(ended_at);
        let recording = recording_card(
            &saved.event,
            &saved.message_id,
            self.has_transcript(&ended.call_id),
            clock.meeting,
        )?;
        let title = clock
            .meeting
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .or_else(|| saved.event.recording.as_ref()?.title())
            .unwrap_or_else(|| RECAP_FALLBACK_TITLE.to_owned());
        Some(RecapCard {
            title,
            date_line: recap_date_line(started_at, ended_at, clock),
            recording,
        })
    }
}

/// Teams dates the recap card with the scheduled occurrence; without a calendar match the call's
/// own start and end stand in.
fn recap_date_line(
    started_at: DateTime<Utc>,
    ended_at: DateTime<Utc>,
    clock: &CallClock,
) -> String {
    let slot = clock
        .meeting
        .slots
        .iter()
        .filter(|slot| slot.start <= ended_at && slot.end >= started_at)
        .min_by_key(|slot| (slot.start - started_at).num_seconds().abs())
        .copied()
        .unwrap_or(MeetingSlot {
            start: started_at,
            end: ended_at,
        });
    format!(
        "{} - {}",
        slot.start
            .with_timezone(&clock.offset)
            .format(RECAP_DATE_FORMAT),
        slot.end
            .with_timezone(&clock.offset)
            .format(RECAP_TIME_FORMAT)
    )
}

/// Teams writes the weekday in full on these lines ("Thursday 11:45"), unlike message times.
fn notice_stamp(time: DateTime<Utc>, clock: &CallClock) -> String {
    let local = time.with_timezone(&clock.offset);
    let days_ago = (clock.today - local.date_naive()).num_days();
    if (2..=WEEKDAY_STAMP_DAYS).contains(&days_ago) {
        local.format("%A %H:%M").to_string()
    } else {
        format::post_time_label(time, clock.today, clock.offset)
    }
}

fn participants(event: &CallEvent, names: &CallNames) -> Vec<CallParticipant> {
    event
        .participant_ids
        .iter()
        .enumerate()
        .filter_map(|(index, user_id)| {
            let given = event.participant_names.get(index).map(String::as_str);
            Some(CallParticipant {
                user_id: user_id.clone(),
                name: names.name(Some(user_id), given)?,
            })
        })
        .collect()
}

fn recording_card(
    event: &CallEvent,
    message_id: &str,
    transcript: bool,
    meeting: &MeetingInfo,
) -> Option<RecordingCard> {
    let recording = event.recording.as_ref()?;
    Some(RecordingCard {
        message_id: message_id.to_owned(),
        duration: event
            .duration_seconds
            .filter(|seconds| *seconds > 0)
            .map(format_duration),
        url: recording.url.clone(),
        thumbnail: recording.thumbnail_url().map(|url| ImageRef {
            id: format!("recording-{message_id}"),
            url,
            width: None,
            height: None,
            alt: None,
        }),
        transcript,
        expires_in_days: meeting.expiry_days.get(message_id).copied(),
    })
}

fn day_count(days: i64) -> String {
    if days == 1 {
        "1 day".to_owned()
    } else {
        format!("{days} days")
    }
}

struct NoticeSource<'a> {
    event: &'a CallEvent,
    message_id: &'a str,
    stopped_by_meeting_end: bool,
}

fn notice_text(source: NoticeSource, names: &CallNames, meeting: &MeetingInfo) -> String {
    let NoticeSource {
        event,
        message_id,
        stopped_by_meeting_end,
    } = source;
    let initiator = || {
        names.name(
            event.initiator_id.as_deref(),
            event.initiator_name.as_deref(),
        )
    };
    match event.phase {
        CallPhase::Ended => format!("{}:", event.title(false)),
        CallPhase::RecordingStarted => match initiator() {
            Some(name) => format!("{name} started recording."),
            None => event.title(false).to_owned(),
        },
        CallPhase::RecordingStopped => match initiator().filter(|_| !stopped_by_meeting_end) {
            Some(name) => format!("{name} stopped recording."),
            None => format!("{}.", event.title(false)),
        },
        CallPhase::RecordingSaved => {
            let saved = saved_to(event, names);
            match meeting.expiry_days.get(message_id) {
                Some(days) => format!("{saved} and will expire in {}.", day_count(*days)),
                None => format!("{saved}."),
            }
        }
        _ => event.title(false).to_owned(),
    }
}

fn saved_to(event: &CallEvent, names: &CallNames) -> String {
    let organizer_id = event
        .recording
        .as_ref()
        .and_then(|recording| recording.organizer_id.as_deref());
    if organizer_id.is_some() && organizer_id == names.my_user_id {
        return "Recording has been saved to your OneDrive".to_owned();
    }
    match names.name(organizer_id, None) {
        Some(owner) => format!("Recording has been saved to {owner}'s OneDrive"),
        None => "Recording has been saved to OneDrive".to_owned(),
    }
}

fn title(event: &CallEvent, missed: bool, names: &CallNames) -> String {
    match event.phase {
        CallPhase::RecordingStarted => {
            match names.name(
                event.initiator_id.as_deref(),
                event.initiator_name.as_deref(),
            ) {
                Some(name) => format!("{name} started recording"),
                None => event.title(false).to_owned(),
            }
        }
        CallPhase::RecordingSaved => saved_to(event, names),
        _ => event.title(missed).to_owned(),
    }
}

/// Teams draws meeting and recording events as one-line notices; 1:1 and group calls keep the card.
pub fn is_notice(event: &CallEvent) -> bool {
    event.call_type == CallType::Meeting || event.is_recording()
}

pub struct CallSource<'a> {
    pub event: &'a CallEvent,
    pub message_id: &'a str,
    pub created_at: DateTime<Utc>,
    pub time: &'a str,
}

pub fn call_card(
    source: CallSource,
    names: &CallNames,
    clock: &CallClock,
    timeline: &CallTimeline,
) -> CallCard {
    let CallSource {
        event,
        message_id,
        created_at,
        time,
    } = source;
    let missed = event.missed(names.my_user_id);
    let detail = match (event.phase, event.duration_seconds) {
        (CallPhase::Ended, Some(seconds)) if !missed => {
            format!("{} \u{b7} {time}", format_duration(seconds))
        }
        _ => time.to_owned(),
    };
    let mut people = if event.phase == CallPhase::Ended && event.call_type == CallType::Meeting {
        participants(event, names)
    } else {
        Vec::new()
    };
    let extra_participants = people.len().saturating_sub(MAX_AVATARS);
    people.truncate(MAX_AVATARS);
    let notice = is_notice(event).then(|| CallNotice {
        stamp: notice_stamp(created_at, clock),
        text: notice_text(
            NoticeSource {
                event,
                message_id,
                stopped_by_meeting_end: timeline.stopped_by_meeting_end(event, created_at),
            },
            names,
            clock.meeting,
        ),
        emphasis: event
            .duration_seconds
            .filter(|_| event.phase == CallPhase::Ended)
            .map(format_duration),
    });
    CallCard {
        title: title(event, missed, names),
        detail,
        icon: match (missed, event.phase, event.call_type) {
            (true, _, _) => CallIcon::Missed,
            (false, CallPhase::RecordingStarted, _) => CallIcon::Record,
            (false, CallPhase::RecordingSaved, _) => CallIcon::Saved,
            (false, CallPhase::RecordingStopped | CallPhase::TranscriptStarted, _) => {
                CallIcon::RecordStop
            }
            (false, _, CallType::Meeting) => CallIcon::Video,
            (false, _, CallType::Call) => CallIcon::Call,
        },
        missed,
        started: event.phase == CallPhase::Started,
        participants: people,
        extra_participants,
        recap: timeline.recap(event, created_at, clock),
        notice,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use teams_core::Recording;

    use super::*;

    fn event(phase: CallPhase, call_type: CallType, call_id: &str) -> CallEvent {
        CallEvent {
            phase,
            call_type,
            call_id: call_id.to_owned(),
            duration_seconds: None,
            initiator_id: Some("organizer".to_owned()),
            initiator_name: None,
            participant_ids: Vec::new(),
            participant_names: Vec::new(),
            recording: None,
        }
    }

    fn fixture_records() -> Vec<MessageRecord> {
        let path = format!(
            "{}/../core/tests/fixtures/call_events/meeting_chat_recording.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let page: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        page["value"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .map(|message| {
                let message: graph::Message = serde_json::from_value(message.clone()).unwrap();
                teams_core::message_record("meeting-chat", &message).unwrap()
            })
            .collect()
    }

    fn names() -> HashMap<String, String> {
        HashMap::from([
            (
                "00000000-0000-4000-8000-000000000002".to_owned(),
                "Vera Organizer".to_owned(),
            ),
            (
                "00000000-0000-4000-8000-000000000004".to_owned(),
                "Sam Recorder".to_owned(),
            ),
        ])
    }

    fn cards(records: &[MessageRecord], meeting: &MeetingInfo) -> Vec<CallCard> {
        let events: Vec<Option<CallEvent>> = records.iter().map(teams_core::call_event).collect();
        let timeline = CallTimeline::with_records(&events, &records.iter().collect::<Vec<_>>());
        let names = names();
        let lookup = CallNames {
            my_user_id: Some("me"),
            names: &names,
        };
        let clock = CallClock {
            offset: FixedOffset::east_opt(0).unwrap(),
            today: NaiveDate::from_ymd_opt(2026, 10, 30).unwrap(),
            meeting,
        };
        events
            .iter()
            .zip(records)
            .enumerate()
            .filter(|(index, _)| !timeline.hides(*index))
            .map(|(_, (event, record))| {
                call_card(
                    CallSource {
                        event: event.as_ref().unwrap(),
                        message_id: &record.message_id,
                        created_at: record.created_at,
                        time: "16:15",
                    },
                    &lookup,
                    &clock,
                    &timeline,
                )
            })
            .collect()
    }

    fn notice_line(card: &CallCard) -> String {
        let notice = card.notice.as_ref().unwrap();
        match &notice.emphasis {
            Some(emphasis) => format!("{} {emphasis}", notice.text),
            None => notice.text.clone(),
        }
    }

    #[test]
    fn recorded_meeting_shows_as_teams_notices() {
        let shown = cards(&fixture_records(), &MeetingInfo::default());
        let lines: Vec<String> = shown.iter().map(notice_line).collect();
        assert_eq!(
            lines,
            [
                "Meeting started",
                "Sam Recorder started recording.",
                "Recording stopped.",
                "Meeting ended: 16m 28s",
                "Recording has been saved to Vera Organizer's OneDrive.",
            ]
        );
        let icons: Vec<CallIcon> = shown.iter().map(|card| card.icon).collect();
        assert_eq!(
            icons,
            [
                CallIcon::Video,
                CallIcon::Record,
                CallIcon::RecordStop,
                CallIcon::Video,
                CallIcon::Saved,
            ]
        );
        assert!(
            shown
                .iter()
                .all(|card| card.recap.is_none() == (card.title != "Meeting ended"))
        );
    }

    #[test]
    fn a_recording_stopped_before_the_end_names_who_stopped_it() {
        let mut records = fixture_records();
        for record in records.iter_mut() {
            if teams_core::call_event(record)
                .is_some_and(|event| event.phase == CallPhase::RecordingStopped)
            {
                record.created_at -= chrono::Duration::minutes(2);
            }
        }
        let shown = cards(&records, &MeetingInfo::default());
        assert_eq!(notice_line(&shown[2]), "Sam Recorder stopped recording.");
    }

    #[test]
    fn ended_meeting_carries_the_recap_card_of_its_recording() {
        let records = fixture_records();
        let saved_id = records.last().unwrap().message_id.clone();
        let meeting = MeetingInfo {
            title: Some("Weekly sync".to_owned()),
            slots: Vec::new(),
            expiry_days: HashMap::from([(saved_id.clone(), 56)]),
        };
        let shown = cards(&records, &meeting);
        let ended = shown
            .iter()
            .find(|card| card.title == "Meeting ended")
            .unwrap();
        let recap = ended.recap.as_ref().unwrap();
        assert_eq!(recap.title, "Weekly sync");
        assert_eq!(recap.recording.message_id, saved_id);
        assert_eq!(recap.recording.duration.as_deref(), Some("13m 34s"));
        assert_eq!(recap.recording.expires_in_days, Some(56));
        assert!(recap.recording.transcript);
        assert!(
            recap
                .recording
                .thumbnail
                .as_ref()
                .unwrap()
                .url
                .contains("/thumbnails/")
        );
        assert!(shown.iter().filter(|card| card.recap.is_some()).count() == 1);
        assert_eq!(
            notice_line(shown.last().unwrap()),
            "Recording has been saved to Vera Organizer's OneDrive and will expire in 56 days."
        );
    }

    #[test]
    fn recap_date_prefers_the_calendar_occurrence() {
        let started_at = Utc.with_ymd_and_hms(2026, 10, 15, 9, 46, 0).unwrap();
        let ended_at = Utc.with_ymd_and_hms(2026, 10, 15, 9, 55, 0).unwrap();
        let mut meeting = MeetingInfo::default();
        let clock = |meeting: &MeetingInfo| {
            recap_date_line(
                started_at,
                ended_at,
                &CallClock {
                    offset: FixedOffset::east_opt(2 * 3600).unwrap(),
                    today: NaiveDate::from_ymd_opt(2026, 10, 17).unwrap(),
                    meeting,
                },
            )
        };
        assert_eq!(clock(&meeting), "Thursday, 15 October 2026 11:46 - 11:55");
        meeting.slots = vec![
            MeetingSlot {
                start: Utc.with_ymd_and_hms(2026, 10, 14, 9, 45, 0).unwrap(),
                end: Utc.with_ymd_and_hms(2026, 10, 14, 10, 0, 0).unwrap(),
            },
            MeetingSlot {
                start: Utc.with_ymd_and_hms(2026, 10, 15, 9, 45, 0).unwrap(),
                end: Utc.with_ymd_and_hms(2026, 10, 15, 10, 0, 0).unwrap(),
            },
        ];
        assert_eq!(clock(&meeting), "Thursday, 15 October 2026 11:45 - 12:00");
    }

    #[test]
    fn notice_stamps_spell_out_the_weekday_like_teams() {
        let meeting = MeetingInfo::default();
        let clock = CallClock {
            offset: FixedOffset::east_opt(0).unwrap(),
            today: NaiveDate::from_ymd_opt(2026, 10, 17).unwrap(),
            meeting: &meeting,
        };
        let stamp = |day: u32| {
            notice_stamp(
                Utc.with_ymd_and_hms(2026, 10, day, 11, 45, 0).unwrap(),
                &clock,
            )
        };
        assert_eq!(stamp(17), "11:45");
        assert_eq!(stamp(16), "Yesterday 11:45");
        assert_eq!(stamp(15), "Thursday 11:45");
    }

    #[test]
    fn ended_meeting_lists_named_participants_up_to_four() {
        let shown = cards(&fixture_records(), &MeetingInfo::default());
        let ended = shown
            .iter()
            .find(|card| card.title == "Meeting ended")
            .unwrap();
        assert_eq!(ended.detail, "16m 28s \u{b7} 16:15");
        let shown_names: Vec<&str> = ended
            .participants
            .iter()
            .map(|person| person.name.as_str())
            .collect();
        assert_eq!(
            shown_names,
            ["Person 1", "Person 2", "Person 3", "Person 4"]
        );
        assert_eq!(ended.extra_participants, 20);
    }

    #[test]
    fn meeting_start_stays_but_a_finished_call_start_hides() {
        let events = vec![
            Some(event(CallPhase::Started, CallType::Meeting, "meeting")),
            Some(event(CallPhase::Ended, CallType::Meeting, "meeting")),
            Some(event(CallPhase::Started, CallType::Call, "call")),
            Some(event(CallPhase::Ended, CallType::Call, "call")),
            Some(event(CallPhase::Started, CallType::Meeting, "live")),
            None,
        ];
        let timeline = CallTimeline::new(&events);
        assert!(!timeline.hides(0));
        assert!(timeline.hides(2));
        assert!(!timeline.hides(5));
        assert!(timeline.is_live_start(4));
        assert!(!timeline.is_live_start(0));
        assert!(timeline.finished("meeting"));
    }

    #[test]
    fn calls_keep_their_card_and_my_recording_is_in_my_onedrive() {
        assert!(!is_notice(&event(CallPhase::Ended, CallType::Call, "c")));
        let mut saved = event(CallPhase::RecordingSaved, CallType::Call, "c");
        saved.recording = Some(Recording {
            name: None,
            url: Some("https://contoso.example/rec.mp4".to_owned()),
            organizer_id: Some("me".to_owned()),
        });
        let names = HashMap::new();
        let lookup = CallNames {
            my_user_id: Some("me"),
            names: &names,
        };
        assert_eq!(
            notice_text(
                NoticeSource {
                    event: &saved,
                    message_id: "m",
                    stopped_by_meeting_end: false,
                },
                &lookup,
                &MeetingInfo::default()
            ),
            "Recording has been saved to your OneDrive."
        );
    }
}
