use chatsvc::{Attendance, AttendanceRecord, Occurrence, RecordingDetails, TranscriptTurn};
use chrono::{DateTime, Duration, Utc};

pub const DEMO_RECAP_CHAT: &str = "demo-chat-meeting-standup";
const ORGANIZER_ID: &str = "demo-me";
const OCCURRENCE_DAYS: [i64; 3] = [0, 1, 2];
const DEMO_RECORDING_URL: &str = "https://demo.sharepoint.example/personal/demo/Recordings/standup";
const SHORT_EXPIRY_DAYS: i64 = 5;
const LONG_EXPIRY_DAYS: i64 = 58;

const LINES: [(&str, f64, &str); 7] = [
    ("Jonas Ortega", 4.0, "Morning everyone. Quick round, then we are done in ten."),
    ("Priya Nair", 21.0, "Yesterday I finished the export flow. Today I pick up the retry logic."),
    ("Mara Lindqvist", 49.0, "I am blocked on the staging certificate. Can someone ping IT?"),
    ("Jonas Ortega", 63.0, "I will ping them right after this. Anything else on the release?"),
    ("Tobias Klein", 88.0, "The release notes are drafted, I need a second pair of eyes."),
    ("Priya Nair", 104.0, "I can review them before lunch."),
    ("Jonas Ortega", 121.0, "Great. Thanks all, see you tomorrow."),
];

pub fn occurrences(conversation_id: &str, now: DateTime<Utc>) -> Vec<Occurrence> {
    if conversation_id != DEMO_RECAP_CHAT {
        return Vec::new();
    }
    OCCURRENCE_DAYS
        .into_iter()
        .map(|days_ago| Occurrence {
            message_id: format!("demo-recap-{days_ago}"),
            started_at: Some(now - Duration::days(days_ago) - Duration::hours(1)),
            recording_url: format!("{DEMO_RECORDING_URL}-{days_ago}.mp4"),
            recording_name: Some(format!(
                "Daily standup-{days_ago}-Meeting Recording.mp4"
            )),
            duration_seconds: Some(920 + days_ago as u64 * 65),
            organizer_id: Some(ORGANIZER_ID.to_owned()),
            audio_only: false,
        })
        .collect()
}

pub fn details(occurrence: &Occurrence, now: DateTime<Utc>) -> RecordingDetails {
    let latest = occurrence.message_id.ends_with('0');
    RecordingDetails {
        item_url: occurrence.recording_url.clone(),
        audio_only: occurrence.message_id.ends_with('2'),
        transcript_only: false,
        duration_seconds: occurrence.duration_seconds,
        expires_at: Some(
            now + Duration::days(if latest {
                LONG_EXPIRY_DAYS
            } else {
                SHORT_EXPIRY_DAYS
            }) - Duration::minutes(1),
        ),
        transcript_id: Some("demo-transcript".to_owned()),
    }
}

pub fn transcript() -> Vec<TranscriptTurn> {
    LINES
        .iter()
        .map(|(speaker, start_seconds, text)| TranscriptTurn {
            speaker: (*speaker).to_owned(),
            start_seconds: *start_seconds,
            end_seconds: start_seconds + 12.0,
            text: (*text).to_owned(),
        })
        .collect()
}

pub fn attendance(now: DateTime<Utc>) -> Attendance {
    let start = now - Duration::hours(1);
    let record = |name: &str, joined: i64, left: i64| AttendanceRecord {
        name: name.to_owned(),
        joined_at: Some(start + Duration::minutes(joined)),
        left_at: Some(start + Duration::minutes(left)),
        duration_seconds: ((left - joined) * 60) as u64,
    };
    Attendance::Report(vec![
        record("Jonas Ortega", 0, 16),
        record("Priya Nair", 1, 16),
        record("Mara Lindqvist", 2, 15),
        record("Tobias Klein", 5, 12),
    ])
}

pub const DEMO_FILE_BYTES: &[u8] = b"Rusty Teams demo file\n";
