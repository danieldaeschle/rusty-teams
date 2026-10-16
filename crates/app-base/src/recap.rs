use chatsvc::{Occurrence, RecordingDetails, TranscriptTurn};
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};

use crate::format;

pub const EMPTY_RECAP: &str = "No recording or transcript for this meeting.";
pub const NO_TRANSCRIPT: &str = "No transcript for this recording.";
pub const ATTENDANCE_OFF: &str = "Attendance reports are turned off for your organization.";
pub const ATTENDANCE_FAILED: &str = "Attendance could not be loaded.";
pub const DELETE_QUESTION: &str = "Delete this recording for everyone?";
const URGENT_EXPIRY_DAYS: i64 = 7;
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Load<T> {
    Loading,
    Failed(String),
    Ready(T),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(value) => Some(value),
            _ => None,
        }
    }
}

pub fn shows_recording(details: &Load<RecordingDetails>) -> bool {
    !details.ready().is_some_and(|details| details.transcript_only)
}

pub fn expiry_label(days: i64) -> String {
    match days {
        0 => "Expires today".to_owned(),
        1 => "Expires in 1 day".to_owned(),
        days => format!("Expires in {days} days"),
    }
}

pub fn expiry_is_urgent(days: i64) -> bool {
    days < URGENT_EXPIRY_DAYS
}

pub fn clock_label(seconds: u64) -> String {
    let hours = seconds / SECONDS_PER_HOUR;
    let minutes = seconds % SECONDS_PER_HOUR / SECONDS_PER_MINUTE;
    let rest = seconds % SECONDS_PER_MINUTE;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{rest:02}")
    } else {
        format!("{minutes}:{rest:02}")
    }
}

pub fn span_label(seconds: u64) -> String {
    let hours = seconds / SECONDS_PER_HOUR;
    let minutes = seconds % SECONDS_PER_HOUR / SECONDS_PER_MINUTE;
    match (hours, minutes) {
        (0, 0) => "< 1 min".to_owned(),
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    }
}

pub fn occurrence_label(
    occurrence: &Occurrence,
    today: NaiveDate,
    offset: FixedOffset,
) -> String {
    occurrence.started_at.map_or_else(
        || occurrence.recording_name.clone().unwrap_or_default(),
        |started_at| {
            let local = started_at.with_timezone(&offset);
            format!(
                "{}, {}",
                format::day_label(local.date_naive(), today),
                local.format("%H:%M")
            )
        },
    )
}

pub fn time_label(time: Option<DateTime<Utc>>, offset: FixedOffset) -> String {
    time.map_or_else(
        || "-".to_owned(),
        |time| time.with_timezone(&offset).format("%H:%M").to_string(),
    )
}

pub fn matching_turns(turns: &[TranscriptTurn], query: &str) -> Vec<usize> {
    let needle = query.trim().to_lowercase();
    turns
        .iter()
        .enumerate()
        .filter(|(_, turn)| {
            needle.is_empty()
                || turn.text.to_lowercase().contains(&needle)
                || turn.speaker.to_lowercase().contains(&needle)
        })
        .map(|(index, _)| index)
        .collect()
}

pub fn highlight_segments(text: &str, query: &str) -> Vec<(String, bool)> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return vec![(text.to_owned(), false)];
    }
    let mut segments = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        let hit = rest
            .char_indices()
            .map(|(start, _)| start)
            .find_map(|start| matched_len(&rest[start..], &needle).map(|length| (start, length)));
        match hit {
            Some((start, length)) => {
                plain.push_str(&rest[..start]);
                if !plain.is_empty() {
                    segments.push((std::mem::take(&mut plain), false));
                }
                segments.push((rest[start..start + length].to_owned(), true));
                rest = &rest[start + length..];
            }
            None => {
                plain.push_str(rest);
                rest = "";
            }
        }
    }
    if !plain.is_empty() {
        segments.push((plain, false));
    }
    segments
}

fn matched_len(text: &str, lowered_needle: &str) -> Option<usize> {
    let mut lowered = String::new();
    for (position, character) in text.char_indices() {
        lowered.extend(character.to_lowercase());
        if lowered == lowered_needle {
            return Some(position + character.len_utf8());
        }
        if !lowered_needle.starts_with(&lowered) {
            return None;
        }
    }
    None
}

pub fn save_name(occurrence: &Occurrence, extension: &str) -> String {
    let stem = occurrence
        .recording_name
        .as_deref()
        .map(|name| name.rsplit_once('.').map_or(name, |(stem, _)| stem))
        .unwrap_or("Meeting recording");
    format!("{stem}.{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(speaker: &str, text: &str) -> TranscriptTurn {
        TranscriptTurn {
            speaker: speaker.to_owned(),
            start_seconds: 0.,
            end_seconds: 1.,
            text: text.to_owned(),
        }
    }

    fn occurrence(name: Option<&str>) -> Occurrence {
        Occurrence {
            message_id: "1".to_owned(),
            started_at: None,
            recording_url: "https://a.example/x.mp4".to_owned(),
            recording_name: name.map(str::to_owned),
            duration_seconds: None,
            organizer_id: None,
            audio_only: false,
        }
    }

    #[test]
    fn transcript_only_sessions_show_no_recording_card() {
        let details = |transcript_only| RecordingDetails {
            item_url: String::new(),
            audio_only: false,
            transcript_only,
            duration_seconds: None,
            expires_at: None,
            transcript_id: None,
        };
        assert!(!shows_recording(&Load::Ready(details(true))));
        assert!(shows_recording(&Load::Ready(details(false))));
        assert!(shows_recording(&Load::Loading));
    }

    #[test]
    fn expiry_wording_and_urgency() {
        assert_eq!(expiry_label(0), "Expires today");
        assert_eq!(expiry_label(1), "Expires in 1 day");
        assert_eq!(expiry_label(42), "Expires in 42 days");
        assert!(expiry_is_urgent(6));
        assert!(!expiry_is_urgent(7));
    }

    #[test]
    fn clocks_and_spans() {
        assert_eq!(clock_label(1832), "30:32");
        assert_eq!(clock_label(3723), "1:02:03");
        assert_eq!(span_label(20), "< 1 min");
        assert_eq!(span_label(1500), "25 min");
        assert_eq!(span_label(3600), "1 h");
        assert_eq!(span_label(3900), "1 h 5 min");
    }

    #[test]
    fn search_matches_text_and_speaker_ignoring_case() {
        let turns = [turn("Alex", "Ship it"), turn("Sam", "Later"), turn("Sam", "SHIP?")];
        assert_eq!(matching_turns(&turns, ""), vec![0, 1, 2]);
        assert_eq!(matching_turns(&turns, " ship "), vec![0, 2]);
        assert_eq!(matching_turns(&turns, "sam"), vec![1, 2]);
        assert!(matching_turns(&turns, "zzz").is_empty());
    }

    #[test]
    fn highlight_splits_around_every_hit() {
        assert_eq!(
            highlight_segments("Ship it, ship", "SHIP"),
            vec![
                ("Ship".to_owned(), true),
                (" it, ".to_owned(), false),
                ("ship".to_owned(), true),
            ]
        );
        assert_eq!(
            highlight_segments("nothing", "x"),
            vec![("nothing".to_owned(), false)]
        );
        assert_eq!(highlight_segments("a", ""), vec![("a".to_owned(), false)]);
    }

    #[test]
    fn save_names_keep_the_recording_stem() {
        let named = occurrence(Some("Weekly sync-20260302-Meeting Recording.mp4"));
        assert_eq!(
            save_name(&named, "vtt"),
            "Weekly sync-20260302-Meeting Recording.vtt"
        );
        assert_eq!(save_name(&occurrence(None), "docx"), "Meeting recording.docx");
    }
}
