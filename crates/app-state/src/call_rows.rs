use std::collections::{HashMap, HashSet};

use teams_core::{CallEvent, CallPhase, CallType, ImageRef, format_duration};

const MAX_AVATARS: usize = 4;
const RECORDING_FALLBACK_TITLE: &str = "Meeting recording";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallIcon {
    Call,
    Missed,
    Video,
    Record,
    RecordStop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallParticipant {
    pub user_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingCard {
    pub message_id: String,
    pub title: String,
    pub duration: Option<String>,
    pub url: Option<String>,
    pub thumbnail: Option<ImageRef>,
    pub transcript: bool,
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
    pub recording: Option<RecordingCard>,
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

/// Which call events of a timeline show as rows: Teams keeps one row per recording start and stop,
/// hides transcript notices and, for calls (not meetings), the start once the call ended.
pub struct CallTimeline {
    hidden: Vec<bool>,
    finished: HashSet<String>,
    transcripts: HashSet<String>,
    newest_open_start: Option<usize>,
}

impl CallTimeline {
    pub fn new(events: &[Option<CallEvent>]) -> Self {
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
        CallTimeline {
            hidden,
            finished,
            transcripts,
            newest_open_start,
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

fn recording_card(event: &CallEvent, message_id: &str, transcript: bool) -> Option<RecordingCard> {
    let recording = event.recording.as_ref()?;
    Some(RecordingCard {
        message_id: message_id.to_owned(),
        title: recording
            .title()
            .unwrap_or_else(|| RECORDING_FALLBACK_TITLE.to_owned()),
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
    })
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
        CallPhase::RecordingSaved => {
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
        _ => event.title(missed).to_owned(),
    }
}

pub fn call_card(
    event: &CallEvent,
    message_id: &str,
    time: &str,
    names: &CallNames,
    transcript: bool,
) -> CallCard {
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
    CallCard {
        title: title(event, missed, names),
        detail,
        icon: match (missed, event.phase, event.call_type) {
            (true, _, _) => CallIcon::Missed,
            (false, CallPhase::RecordingStarted | CallPhase::RecordingSaved, _) => CallIcon::Record,
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
        recording: (event.phase == CallPhase::RecordingSaved)
            .then(|| recording_card(event, message_id, transcript))
            .flatten(),
    }
}

#[cfg(test)]
mod tests {
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

    fn fixture_events() -> Vec<Option<CallEvent>> {
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
            .map(|message| CallEvent::from_graph(&message["eventDetail"]))
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

    #[test]
    fn recorded_meeting_shows_like_teams() {
        let events = fixture_events();
        let timeline = CallTimeline::new(&events);
        let names = names();
        let lookup = CallNames {
            my_user_id: Some("me"),
            names: &names,
        };
        let titles: Vec<String> = events
            .iter()
            .enumerate()
            .filter(|(index, _)| !timeline.hides(*index))
            .map(|(_, event)| {
                let event = event.as_ref().unwrap();
                call_card(
                    event,
                    "m",
                    "16:15",
                    &lookup,
                    timeline.has_transcript(&event.call_id),
                )
                .title
            })
            .collect();
        assert_eq!(
            titles,
            [
                "Meeting started",
                "Sam Recorder started recording",
                "Recording stopped",
                "Meeting ended",
                "Recording has been saved to Vera Organizer's OneDrive",
            ]
        );
    }

    #[test]
    fn saved_recording_becomes_a_card_with_transcript_and_thumbnail() {
        let events = fixture_events();
        let timeline = CallTimeline::new(&events);
        let saved = events.last().unwrap().as_ref().unwrap();
        let names = HashMap::new();
        let lookup = CallNames {
            my_user_id: None,
            names: &names,
        };
        let card = call_card(
            saved,
            "saved-1",
            "16:16",
            &lookup,
            timeline.has_transcript(&saved.call_id),
        );
        assert_eq!(card.title, "Recording has been saved to OneDrive");
        assert_eq!(card.icon, CallIcon::Record);
        let recording = card.recording.unwrap();
        assert_eq!(recording.title, "Weekly team meeting");
        assert_eq!(recording.duration.as_deref(), Some("13m 34s"));
        assert!(recording.transcript);
        assert_eq!(recording.message_id, "saved-1");
        assert!(recording.thumbnail.unwrap().url.contains("/thumbnails/"));
    }

    #[test]
    fn ended_meeting_lists_named_participants_up_to_four() {
        let events = fixture_events();
        let ended = events
            .iter()
            .flatten()
            .find(|event| event.phase == CallPhase::Ended)
            .unwrap();
        let names = HashMap::new();
        let lookup = CallNames {
            my_user_id: None,
            names: &names,
        };
        let card = call_card(ended, "m", "16:15", &lookup, false);
        assert_eq!(card.detail, "16m 28s \u{b7} 16:15");
        assert_eq!(card.icon, CallIcon::Video);
        let shown: Vec<&str> = card
            .participants
            .iter()
            .map(|person| person.name.as_str())
            .collect();
        assert_eq!(shown, ["Person 1", "Person 2", "Person 3", "Person 4"]);
        assert_eq!(card.extra_participants, 20);
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
    fn my_own_recording_is_saved_to_my_onedrive() {
        let mut saved = event(CallPhase::RecordingSaved, CallType::Meeting, "c");
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
        let card = call_card(&saved, "m", "9:00", &lookup, false);
        assert_eq!(card.title, "Recording has been saved to your OneDrive");
        let recording = card.recording.unwrap();
        assert_eq!(recording.title, "Meeting recording");
        assert_eq!(recording.thumbnail, None);
        assert_eq!(recording.duration, None);
    }
}
