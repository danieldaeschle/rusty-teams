use serde_json::Value;

use crate::signaling::MeetingTarget;
use crate::trouter_events::find_key;

const MEETUP_PATH: &str = "/l/meetup-join/";

#[derive(Debug, Clone, PartialEq)]
pub struct BreakoutMove {
    pub room_name: String,
    pub target: MeetingTarget,
    pub returning: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomAssignment {
    pub name: String,
    pub open: bool,
    pub join_url: String,
    pub version: u64,
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes.get(index + 1..index + 3).and_then(|pair| std::str::from_utf8(pair).ok()).and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                index += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn context_of(decoded_url: &str) -> Option<Value> {
    let start = decoded_url.find('{')?;
    let mut depth = 0usize;
    for (offset, letter) in decoded_url[start..].char_indices() {
        match letter {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&decoded_url[start..=start + offset]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

/// `https://<host>/l/meetup-join/<thread>/<messageId>?context={"Tid":..,"Oid":..}`; the host does not matter.
pub fn target_from_join_url(url: &str) -> Option<MeetingTarget> {
    let decoded = percent_decoded(url);
    let after = &decoded[decoded.find(MEETUP_PATH)? + MEETUP_PATH.len()..];
    let thread_id = after.split(['/', '?']).next().filter(|thread| thread.starts_with("19:"))?.to_owned();
    let context = context_of(after)?;
    Some(MeetingTarget {
        thread_id,
        tenant_id: context["Tid"].as_str()?.to_owned(),
        organizer_id: context["Oid"].as_str()?.to_owned(),
        meeting_data: None,
    })
}

/// `<tenantId>/<organizerId>/<threadId with the first ":" written as "_">/<messageId>`.
pub fn target_from_coordinates(coordinates: &str) -> Option<MeetingTarget> {
    let mut parts = coordinates.split('/');
    let (tenant_id, organizer_id, thread) = (parts.next()?, parts.next()?, parts.next()?);
    if tenant_id.is_empty() || organizer_id.is_empty() {
        return None;
    }
    let thread_id = thread.replacen('_', ":", 1);
    thread_id.starts_with("19:").then(|| MeetingTarget {
        thread_id,
        tenant_id: tenant_id.to_owned(),
        organizer_id: organizer_id.to_owned(),
        meeting_data: None,
    })
}

pub fn room_assignment(body: &Value) -> Option<RoomAssignment> {
    let properties = find_key(body, "mainMeetingProperties")?;
    let room = properties["assignedBreakoutRooms"].as_array()?.first()?;
    Some(RoomAssignment {
        name: room["name"].as_str().unwrap_or("a breakout room").to_owned(),
        open: room["isOpen"].as_bool().unwrap_or(false),
        join_url: room["meetingJoinUrl"].as_str()?.to_owned(),
        version: properties["breakoutRoomVersion"].as_u64().unwrap_or_default(),
    })
}

pub fn main_meeting_target(conversation_update: &Value) -> Option<MeetingTarget> {
    let properties = find_key(conversation_update, "breakoutRoomProperties")?;
    target_from_join_url(properties["mainMeetingJoinUrl"].as_str()?)
}

/// Mode 1: the server rings the client with a "replaces" call that names the room or the main meeting.
pub fn invite_move(body: &Value) -> Option<BreakoutMove> {
    let data = find_key(body, "breakoutRoomData")?;
    let returning = data["isMainMeetingInvite"].as_bool().unwrap_or(false);
    if !returning && !data["isRoomInvite"].as_bool().unwrap_or(false) {
        return None;
    }
    let target = target_from_coordinates(data["relatedMeetingCoords"].as_str()?)?;
    let room_name = data["meetingName"].as_str().filter(|name| !name.is_empty()).map_or_else(
        || if returning { "the main meeting".to_owned() } else { "a breakout room".to_owned() },
        str::to_owned,
    );
    Some(BreakoutMove { room_name, target, returning })
}

/// Mode 2: no invite, the room simply opens; moves once per room opening.
#[derive(Debug, Default)]
pub struct RoomWatcher {
    version: u64,
    joined: Option<String>,
}

impl RoomWatcher {
    pub fn apply(&mut self, assignment: Option<RoomAssignment>) -> Option<BreakoutMove> {
        let assignment = assignment?;
        if assignment.version < self.version {
            return None;
        }
        self.version = assignment.version;
        if !assignment.open {
            self.joined = None;
            return None;
        }
        if self.joined.as_deref() == Some(assignment.join_url.as_str()) {
            return None;
        }
        let target = target_from_join_url(&assignment.join_url)?;
        self.joined = Some(assignment.join_url);
        Some(BreakoutMove { room_name: assignment.name, target, returning: false })
    }

    pub fn entered(&mut self, join_url: &str) {
        self.joined = Some(join_url.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ROOM_URL: &str = "https://teams.microsoft.com/l/meetup-join/19%3ameeting_Room1abc%40thread.v2/0?context=%7b%22Tid%22%3a%22tenant-1%22%2c%22Oid%22%3a%22org-1%22%7d";

    fn room_update(open: bool, version: u64) -> Value {
        json!({"breakoutDetails": {"mainMeetingProperties": {
            "breakoutRoomVersion": version,
            "breakoutRoomManagers": [],
            "allBreakoutRooms": [{"name": "Room 1", "isOpen": open, "meetingJoinUrl": ROOM_URL, "invitees": []}],
            "assignedBreakoutRooms": [{"name": "Room 1", "isOpen": open, "meetingJoinUrl": ROOM_URL, "invitees": [{"id": "8:orgid:me"}], "eTag": "1", "moveJitter": 3}],
        }}})
    }

    #[test]
    fn a_meetup_join_link_becomes_a_meeting_target() {
        let target = target_from_join_url(ROOM_URL).unwrap();
        assert_eq!(target.thread_id, "19:meeting_Room1abc@thread.v2");
        assert_eq!(target.tenant_id, "tenant-1");
        assert_eq!(target.organizer_id, "org-1");
        let plain = "https://teams.cloud.microsoft/l/meetup-join/19:meeting_x@thread.v2/0?context={\"Tid\":\"t\",\"Oid\":\"o\"}";
        assert_eq!(target_from_join_url(plain).unwrap().organizer_id, "o");
        assert!(target_from_join_url("https://example.com/other").is_none());
        assert!(target_from_join_url("https://teams.microsoft.com/l/meetup-join/19:x@thread.v2/0").is_none());
    }

    #[test]
    fn coordinates_name_the_meeting_with_the_first_colon_replaced() {
        let target = target_from_coordinates("tenant-1/org-1/19_meeting_Room1abc@thread.v2/0").unwrap();
        assert_eq!(target.thread_id, "19:meeting_Room1abc@thread.v2");
        assert_eq!((target.tenant_id.as_str(), target.organizer_id.as_str()), ("tenant-1", "org-1"));
        assert!(target_from_coordinates("tenant-1/org-1").is_none());
        assert!(target_from_coordinates("t/o/nonsense/0").is_none());
    }

    #[test]
    fn the_assigned_room_is_read_from_the_participant_update() {
        let assignment = room_assignment(&room_update(true, 5)).unwrap();
        assert_eq!(assignment, RoomAssignment { name: "Room 1".into(), open: true, join_url: ROOM_URL.into(), version: 5 });
        assert!(room_assignment(&json!({"breakoutDetails": {"mainMeetingProperties": {"assignedBreakoutRooms": []}}})).is_none());
        assert!(room_assignment(&json!({"other": 1})).is_none());
    }

    #[test]
    fn an_opening_room_moves_you_once() {
        let mut watcher = RoomWatcher::default();
        assert!(watcher.apply(room_assignment(&room_update(false, 1))).is_none());
        let moved = watcher.apply(room_assignment(&room_update(true, 2))).unwrap();
        assert_eq!(moved.room_name, "Room 1");
        assert_eq!(moved.target.thread_id, "19:meeting_Room1abc@thread.v2");
        assert!(!moved.returning);
        assert!(watcher.apply(room_assignment(&room_update(true, 3))).is_none());
        assert!(watcher.apply(room_assignment(&room_update(false, 4))).is_none());
        assert!(watcher.apply(room_assignment(&room_update(true, 5))).is_some());
    }

    #[test]
    fn old_versions_and_the_room_you_are_already_in_do_not_move_you() {
        let mut watcher = RoomWatcher::default();
        assert!(watcher.apply(room_assignment(&room_update(true, 9))).is_some());
        assert!(watcher.apply(room_assignment(&room_update(false, 4))).is_none());
        let mut inside = RoomWatcher::default();
        inside.entered(ROOM_URL);
        assert!(inside.apply(room_assignment(&room_update(true, 1))).is_none());
    }

    fn invite(room: bool, main: bool) -> Value {
        json!({"callNotification": {"callType": "replaces", "invitationData": {"breakoutRoomData": {
            "isRoomInvite": room, "isMainMeetingInvite": main, "meetingName": "Room 2",
            "relatedMeetingCoords": "tenant-1/org-1/19_meeting_Room2@thread.v2/0", "shouldAutoAcceptInvite": true, "correlationId": "c"}},
            "meetingInfo": {"tenantId": "tenant-1", "organizerId": "org-1"}}})
    }

    #[test]
    fn a_room_invite_names_the_room_and_a_main_invite_sends_you_back() {
        let moved = invite_move(&invite(true, false)).unwrap();
        assert_eq!(moved.room_name, "Room 2");
        assert_eq!(moved.target.thread_id, "19:meeting_Room2@thread.v2");
        assert!(!moved.returning);
        let back = invite_move(&invite(false, true)).unwrap();
        assert!(back.returning);
        assert!(invite_move(&invite(false, false)).is_none());
        assert!(invite_move(&json!({"callNotification": {}})).is_none());
    }

    #[test]
    fn the_room_properties_carry_the_way_back() {
        let update = json!({"meetingDetails": {"breakoutDetails": {"breakoutRoomProperties": {"mainMeetingJoinUrl": ROOM_URL, "remainingRoomDuration": 600}}}});
        assert_eq!(main_meeting_target(&update).unwrap().tenant_id, "tenant-1");
        assert!(main_meeting_target(&json!({"meetingDetails": {}})).is_none());
    }
}
