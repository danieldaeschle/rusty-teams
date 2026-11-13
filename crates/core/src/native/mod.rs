mod chat;
mod markup;
mod message;
mod source;

pub use chat::{NativeMember, chat_type, graph_chat, graph_chat_from_aggregator};
pub use message::graph_message;
pub use source::{NativeChats, known_profile};

#[cfg(test)]
mod event_tests {
    use chatsvc::graph_events::event_detail;

    use crate::call_event::{CallEvent, CallPhase, CallType};
    use crate::roster_event::{RosterChange, RosterEvent, SharedHistory};

    #[test]
    fn an_ended_call_lists_participants_and_the_longest_duration() {
        let content = r#"<ended/><partlist alt="" count="2"><part identity="28:ann"><name>8:orgid:ann</name><displayName>Ann</displayName><duration>61</duration></part><part identity="8:orgid:bob"><name>8:orgid:bob</name><displayName></displayName><duration>125</duration></part></partlist><callEventType>meeting</callEventType><callId>call-1</callId>"#;
        let detail =
            event_detail("Event/Call", content, "19:meeting_x@thread.v2", Some("ann")).unwrap();
        let event = CallEvent::from_graph(&detail).unwrap();
        assert_eq!(event.phase, CallPhase::Ended);
        assert_eq!(event.call_type, CallType::Meeting);
        assert_eq!(event.call_id, "call-1");
        assert_eq!(event.duration_seconds, Some(125));
        assert_eq!(event.participant_ids, vec!["ann", "bob"]);
        assert_eq!(event.initiator_id.as_deref(), Some("ann"));
    }

    #[test]
    fn a_call_without_end_is_a_started_call() {
        let content = "<partlist alt=\"\"></partlist><meetingDetails><meetingType>Scheduled</meetingType></meetingDetails><callId>call-3</callId>";
        let detail = event_detail("Event/Call", content, "19:a@thread.v2", Some("ann")).unwrap();
        let event = CallEvent::from_graph(&detail).unwrap();
        assert_eq!(event.phase, CallPhase::Started);
        assert_eq!(event.call_type, CallType::Meeting);
    }

    #[test]
    fn a_saved_recording_names_its_file_and_link() {
        let content = r#"<URIObject type="Video.2/CallRecording.1"><RecordingStatus status="Success" code="200"></RecordingStatus><Title>Daily</Title><a href="https://contoso.sharepoint.com/:v:/rec">Play</a><OriginalName v="Daily-20261104_103230-Meeting Recording.mp4" /><MeetingOrganizerId value="8:orgid:boss" /><Identifiers><Id type="callId" value="call-2" /></Identifiers><RecordingContent duration="0:08:51.02"></RecordingContent></URIObject>"#;
        let detail = event_detail(
            "RichText/Media_CallRecording",
            content,
            "19:a@thread.v2",
            None,
        )
        .unwrap();
        let event = CallEvent::from_graph(&detail).unwrap();
        assert_eq!(event.phase, CallPhase::RecordingSaved);
        assert_eq!(event.call_id, "call-2");
        assert_eq!(event.duration_seconds, Some(531));
        let recording = event.recording.unwrap();
        assert_eq!(recording.title().as_deref(), Some("Daily"));
        assert_eq!(
            recording.url.as_deref(),
            Some("https://contoso.sharepoint.com/:v:/rec")
        );
        assert_eq!(recording.organizer_id.as_deref(), Some("boss"));
    }

    #[test]
    fn added_members_carry_the_shared_history() {
        let content = "<addmember><eventtime>1</eventtime><initiator>8:orgid:boss</initiator><target>8:orgid:ann</target><detailedtargetinfo><id>8:orgid:ann</id></detailedtargetinfo><sharehistorytime>0</sharehistorytime></addmember>";
        let detail =
            event_detail("ThreadActivity/AddMember", content, "19:a@thread.v2", None).unwrap();
        let event = RosterEvent::from_graph(&detail, None).unwrap();
        assert_eq!(event.change, RosterChange::Added);
        assert_eq!(event.member_ids, vec!["ann"]);
        assert_eq!(event.initiator_id.as_deref(), Some("boss"));
        assert_eq!(event.history, SharedHistory::All);
    }

    #[test]
    fn leaving_and_renaming_read_like_graph() {
        let left = r#"{"eventtime":1,"initiator":"8:orgid:ann","members":[{"friendlyname":"Ann","id":"8:orgid:ann"}]}"#;
        let detail =
            event_detail("ThreadActivity/MemberLeft", left, "19:a@thread.v2", None).unwrap();
        assert_eq!(
            RosterEvent::from_graph(&detail, None).unwrap().change,
            RosterChange::Left
        );
        let topic = "<topicupdate><eventtime>1</eventtime><initiator>8:orgid:ann</initiator><value>Plan &amp; do</value></topicupdate>";
        let detail =
            event_detail("ThreadActivity/TopicUpdate", topic, "19:a@thread.v2", None).unwrap();
        let renamed = RosterEvent::from_graph(&detail, None).unwrap();
        assert_eq!(renamed.topic.as_deref(), Some("Plan & do"));
        assert!(
            event_detail(
                "ThreadActivity/TopicUpdate",
                topic,
                "19:meeting_x@thread.v2",
                None
            )
            .is_none()
        );
    }
}
