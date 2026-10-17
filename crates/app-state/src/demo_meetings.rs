use chrono::{DateTime, Duration, Utc};
use store::MessageRecord;
use teams_core::{CallEvent, CallPhase, CallType, Recording};

use crate::demo::{DEMO_USER_ID, JONAS_ID, LEA_ID, MARA_ID, PRIYA_ID, TOBIAS_ID, at};
use crate::demo_recap::DEMO_RECAP_CHAT;

const CHANNEL: &str = "demo-channel-1-1";
const CHANNEL_CALL: &str = "demo-channel-meeting";
const CHANNEL_ROOT: &str = "meet-1";
const STANDUP_CALL: &str = "demo-standup-recorded";
const DEMO_RECORDING_URL: &str = "https://demo.sharepoint.example/personal/demo/Recordings";

fn event(phase: CallPhase, call_id: &str, initiator: &str, seconds: Option<u64>) -> CallEvent {
    CallEvent {
        phase,
        call_type: CallType::Meeting,
        call_id: call_id.to_owned(),
        duration_seconds: seconds,
        initiator_id: Some(initiator.to_owned()),
        initiator_name: None,
        participant_ids: Vec::new(),
        participant_names: Vec::new(),
        recording: None,
    }
}

fn recording(mut event: CallEvent, name: &str, url: Option<&str>, organizer: &str) -> CallEvent {
    event.recording = Some(Recording {
        name: Some(name.to_owned()),
        url: url.map(str::to_owned),
        organizer_id: Some(organizer.to_owned()),
    });
    event
}

fn ended(call_id: &str, initiator: &str, seconds: u64, people: &[(&str, &str)]) -> CallEvent {
    CallEvent {
        participant_ids: people.iter().map(|(id, _)| (*id).to_owned()).collect(),
        participant_names: people.iter().map(|(_, name)| (*name).to_owned()).collect(),
        ..event(CallPhase::Ended, call_id, initiator, Some(seconds))
    }
}

fn record(
    conversation: &str,
    id: &str,
    reply_to: Option<&str>,
    time: DateTime<Utc>,
    event: CallEvent,
) -> MessageRecord {
    MessageRecord {
        conversation_id: conversation.to_owned(),
        message_id: id.to_owned(),
        reply_to_id: reply_to.map(str::to_owned),
        created_at: time,
        links_json: "[]".to_owned(),
        attachments_json: "[]".to_owned(),
        reactions_json: "[]".to_owned(),
        mentions_json: "[]".to_owned(),
        call_event_json: Some(event.to_json()),
        ..MessageRecord::default()
    }
}

fn reply(id: &str, sender: (&str, &str), time: DateTime<Utc>, html: &str) -> MessageRecord {
    MessageRecord {
        conversation_id: CHANNEL.to_owned(),
        message_id: id.to_owned(),
        reply_to_id: Some(CHANNEL_ROOT.to_owned()),
        sender_id: Some(sender.0.to_owned()),
        sender_name: Some(sender.1.to_owned()),
        created_at: time,
        body_html: html.to_owned(),
        links_json: "[]".to_owned(),
        attachments_json: "[]".to_owned(),
        reactions_json: "[]".to_owned(),
        mentions_json: "[]".to_owned(),
        ..MessageRecord::default()
    }
}

fn channel_meeting() -> Vec<MessageRecord> {
    let start = at(0, 10, 30);
    let minutes = |count: i64| start + Duration::minutes(count);
    let name = "Design review-20261017_103200-Meeting Recording.mp4";
    let url = format!("{DEMO_RECORDING_URL}/design-review.mp4");
    let people = [
        (MARA_ID, "Mara Lindqvist"),
        (JONAS_ID, "Jonas Ortega"),
        (PRIYA_ID, "Priya Nair"),
        (DEMO_USER_ID, "Dana Demo"),
        (TOBIAS_ID, "Tobias Klein"),
        (LEA_ID, "Lea Schneider"),
    ];
    let thread = |id: &str, offset: i64, event: CallEvent| {
        record(CHANNEL, id, Some(CHANNEL_ROOT), minutes(offset), event)
    };
    vec![
        record(
            CHANNEL,
            CHANNEL_ROOT,
            None,
            start,
            event(CallPhase::Started, CHANNEL_CALL, MARA_ID, None),
        ),
        thread(
            "meet-1-rec",
            2,
            recording(
                event(CallPhase::RecordingStarted, CHANNEL_CALL, MARA_ID, Some(0)),
                name,
                None,
                MARA_ID,
            ),
        ),
        thread(
            "meet-1-transcript",
            2,
            event(CallPhase::TranscriptStarted, CHANNEL_CALL, MARA_ID, None),
        ),
        thread(
            "meet-1-stop",
            34,
            recording(
                event(
                    CallPhase::RecordingStopped,
                    CHANNEL_CALL,
                    MARA_ID,
                    Some(1914),
                ),
                name,
                None,
                MARA_ID,
            ),
        ),
        thread(
            "meet-1-end",
            35,
            ended(CHANNEL_CALL, MARA_ID, 2100, &people),
        ),
        thread(
            "meet-1-saved",
            36,
            recording(
                event(CallPhase::RecordingSaved, CHANNEL_CALL, MARA_ID, Some(1914)),
                name,
                Some(&url),
                MARA_ID,
            ),
        ),
        reply(
            "meet-1-r1",
            (JONAS_ID, "Jonas Ortega"),
            minutes(41),
            "<p>Thanks, the recording covers the open questions for Priya.</p>",
        ),
    ]
}

fn standup_line(id: &str, time: DateTime<Utc>, event: CallEvent) -> MessageRecord {
    record(DEMO_RECAP_CHAT, id, None, time, event)
}

fn recorded_standup() -> Vec<MessageRecord> {
    let start = at(2, 11, 45);
    let minutes = |count: i64| start + Duration::minutes(count);
    let name = "Daily standup-20261015_114602-Meeting Recording.mp4";
    let url = format!("{DEMO_RECORDING_URL}/standup-1.mp4");
    let people = [
        (JONAS_ID, "Jonas Ortega"),
        (DEMO_USER_ID, "Dana Demo"),
        (MARA_ID, "Mara Lindqvist"),
    ];
    let saved_recording = |phase: CallPhase, seconds: Option<u64>, url: Option<&str>| {
        recording(
            event(phase, STANDUP_CALL, DEMO_USER_ID, seconds),
            name,
            url,
            MARA_ID,
        )
    };
    vec![
        standup_line(
            "standup-1-start",
            start,
            event(CallPhase::Started, STANDUP_CALL, JONAS_ID, None),
        ),
        standup_line(
            "standup-1-rec",
            minutes(1),
            saved_recording(CallPhase::RecordingStarted, Some(0), None),
        ),
        standup_line(
            "standup-1-transcript",
            minutes(1),
            event(
                CallPhase::TranscriptStarted,
                STANDUP_CALL,
                DEMO_USER_ID,
                None,
            ),
        ),
        standup_line(
            "standup-1-stop",
            minutes(10),
            saved_recording(CallPhase::RecordingStopped, Some(531), None),
        ),
        standup_line(
            "standup-1-end",
            minutes(10) + Duration::seconds(40),
            ended(STANDUP_CALL, JONAS_ID, 586, &people),
        ),
        standup_line(
            "demo-recap-0",
            minutes(10) + Duration::seconds(55),
            saved_recording(CallPhase::RecordingSaved, Some(531), Some(&url)),
        ),
    ]
}

pub fn meeting_messages() -> Vec<MessageRecord> {
    channel_meeting()
        .into_iter()
        .chain(recorded_standup())
        .collect()
}
