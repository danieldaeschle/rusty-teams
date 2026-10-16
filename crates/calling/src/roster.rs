use serde_json::Value;

use crate::transfer::Replacement;

const ACTIVE: &str = "active";
const AUDIO: &str = "audio";
const VIDEO: &str = "video";
const SCREEN: &str = "applicationsharing-video";
const SENDING_DIRECTIONS: [&str; 2] = ["sendonly", "sendrecv"];
const RAISE_HANDS: &str = "raiseHands";
const SPOTLIGHT: &str = "spotlight";
const ROLE_KEYS: [&str; 2] = ["meetingRole", "role"];
const MIC_BLOCKED: &str = "attendeeAudioRestricted";
const MIC_RELEASED: &str = "attendeeModalitiesUnrestricted";
const CAMERA_BLOCKED: &str = "attendeeVideoRestricted";
const CAMERA_RELEASED: &str = "attendeeVideoUnrestricted";
const BOT_PREFIX: &str = "28:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedState {
    pub state_id: String,
    pub rank: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingRole {
    Organizer,
    Presenter,
    Attendee,
}

impl MeetingRole {
    pub fn parse(text: &str) -> Option<MeetingRole> {
        match text.to_ascii_lowercase().as_str() {
            "organizer" | "coorganizer" => Some(MeetingRole::Organizer),
            "presenter" => Some(MeetingRole::Presenter),
            "attendee" => Some(MeetingRole::Attendee),
            _ => None,
        }
    }

    pub fn wire(self) -> &'static str {
        match self {
            MeetingRole::Organizer => "organizer",
            MeetingRole::Presenter => "presenter",
            MeetingRole::Attendee => "attendee",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttendeeLimits {
    pub mic_blocked: Option<PublishedState>,
    pub mic_released: Option<PublishedState>,
    pub camera_blocked: Option<PublishedState>,
    pub camera_released: Option<PublishedState>,
}

pub type RaisedHand = PublishedState;
pub type Spotlight = PublishedState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionBot {
    pub mri: String,
    pub command_url: Option<String>,
    pub active: bool,
    pub recording: bool,
    pub audio_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub mri: String,
    pub display_name: String,
    pub muted: bool,
    pub in_lobby: bool,
    pub audio_sources: Vec<u32>,
    pub video_source: Option<u32>,
    pub screen_source: Option<u32>,
    pub streams: Vec<String>,
    pub hand: Option<RaisedHand>,
    pub spotlight: Option<Spotlight>,
    pub organizer: bool,
    pub role: Option<MeetingRole>,
    pub limits: AttendeeLimits,
    pub captions: Option<CaptionBot>,
    pub replacement: Option<Replacement>,
    version: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterEntry {
    pub mri: String,
    pub display_name: String,
    pub muted: bool,
    pub in_lobby: bool,
    pub has_video: bool,
    pub sharing: bool,
    pub hand: Option<RaisedHand>,
    pub spotlight: Option<Spotlight>,
    pub organizer: bool,
    pub role: Option<MeetingRole>,
    pub limits: AttendeeLimits,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roster {
    members: Vec<Member>,
    pub lobby_count: u32,
}

impl Roster {
    pub fn members(&self) -> &[Member] {
        &self.members
    }

    pub fn entries(&self) -> Vec<RosterEntry> {
        self.members
            .iter()
            .map(|member| RosterEntry {
                mri: member.mri.clone(),
                display_name: member.display_name.clone(),
                muted: member.muted,
                in_lobby: member.in_lobby,
                has_video: member.video_source.is_some(),
                sharing: member.screen_source.is_some(),
                hand: member.hand.clone(),
                spotlight: member.spotlight.clone(),
                organizer: member.organizer,
                role: member.role,
                limits: member.limits.clone(),
            })
            .collect()
    }

    pub fn apply(&mut self, body: &Value) -> bool {
        let before = self.clone();
        if let Some(count) = body["participantCounts"]["lobbyParticipants"].as_u64() {
            self.lobby_count = u32::try_from(count).unwrap_or(u32::MAX);
        }
        let Some(participants) = body["participants"].as_object() else {
            return *self != before;
        };
        for (mri, participant) in participants {
            let version = participant["version"].as_u64().unwrap_or_default();
            let known = self.members.iter().position(|member| &member.mri == mri);
            if known.is_some_and(|index| self.members[index].version > version) {
                continue;
            }
            if participant["state"].as_str() != Some(ACTIVE) {
                if let Some(index) = known {
                    self.members.remove(index);
                }
                continue;
            }
            let member = member_from(mri, participant, version, known.map(|index| &self.members[index]));
            match known {
                Some(index) => self.members[index] = member,
                None => self.members.push(member),
            }
        }
        *self != before
    }

    /// A conversation answer lists everyone present; deltas only ever name who changed.
    pub fn apply_snapshot(&mut self, snapshot: &Value) -> bool {
        let before = self.clone();
        self.apply(snapshot);
        if let (Some(sequence), Some(listed)) = (snapshot["sequenceNumber"].as_u64(), snapshot["participants"].as_object()) {
            self.members.retain(|member| listed.contains_key(&member.mri) || member.version > sequence);
        }
        *self != before
    }

    pub fn transfer_replacement(&self, own_mri: &str) -> Option<Replacement> {
        self.members
            .iter()
            .filter(|member| member.mri != own_mri && !member.mri.starts_with(BOT_PREFIX) && !member.in_lobby)
            .find_map(|member| member.replacement.clone())
    }

    pub fn hand_of(&self, mri: &str) -> Option<&RaisedHand> {
        self.members.iter().find(|member| member.mri == mri)?.hand.as_ref()
    }

    pub fn spotlight_of(&self, mri: &str) -> Option<&Spotlight> {
        self.members.iter().find(|member| member.mri == mri)?.spotlight.as_ref()
    }

    pub fn caption_bot(&self) -> Option<&CaptionBot> {
        self.members.iter().find_map(|member| member.captions.as_ref())
    }

    pub fn name_of(&self, user_id: &str) -> Option<&str> {
        let suffix = format!(":{user_id}");
        self.members
            .iter()
            .find(|member| member.mri == user_id || member.mri.ends_with(&suffix))
            .map(|member| member.display_name.as_str())
            .filter(|name| !name.is_empty())
    }

    pub fn is_organizer(&self, mri: &str) -> bool {
        self.members.iter().any(|member| member.mri == mri && member.organizer)
    }

    /// Teams web admits everyone by listing them: the server's `admitAll` leaves anonymous guests waiting.
    pub fn lobby_mris(&self) -> Vec<String> {
        self.members.iter().filter(|member| member.in_lobby).map(|member| member.mri.clone()).collect()
    }

    pub fn is_in_lobby(&self, mri: &str) -> Option<bool> {
        self.members.iter().find(|member| member.mri == mri).map(|member| member.in_lobby)
    }

    pub fn stream_summaries(&self, mri: &str) -> Vec<String> {
        self.members
            .iter()
            .find(|member| member.mri == mri)
            .map(|member| member.streams.clone())
            .unwrap_or_default()
    }

    pub fn screen_source_of(&self, mri: &str) -> Option<u32> {
        self.members.iter().find(|member| member.mri == mri)?.screen_source
    }

    pub fn video_candidates(&self, own_mri: &str) -> Vec<(String, u32)> {
        self.members
            .iter()
            .filter(|member| member.mri != own_mri && !member.in_lobby)
            .filter_map(|member| Some((member.mri.clone(), member.video_source?)))
            .collect()
    }

    pub fn screen_share(&self, own_mri: &str) -> Option<(String, u32)> {
        self.members
            .iter()
            .filter(|member| member.mri != own_mri && !member.in_lobby)
            .find_map(|member| Some((member.mri.clone(), member.screen_source?)))
    }

    pub fn mris_for_sources(&self, sources: &[u32]) -> Vec<String> {
        self.members
            .iter()
            .filter(|member| member.audio_sources.iter().any(|source| sources.contains(source)))
            .map(|member| member.mri.clone())
            .collect()
    }
}

/// Teams builds the roster of a direct call from `acceptedBy` because no mixer reports one; later mixer updates replace it.
pub fn direct_roster(members: &[(&str, &str)]) -> Value {
    let participants: serde_json::Map<String, Value> = members
        .iter()
        .map(|(mri, name)| {
            let participant = serde_json::json!({
                "version": 0,
                "state": ACTIVE,
                "details": {"id": mri, "displayName": name},
                "endpoints": {"direct": {"call": {"mediaStreams": []}}},
            });
            ((*mri).to_owned(), participant)
        })
        .collect();
    serde_json::json!({"participants": participants})
}

fn member_from(mri: &str, participant: &Value, version: u64, previous: Option<&Member>) -> Member {
    let display_name = participant["details"]["displayName"]
        .as_str()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .or_else(|| previous.map(|member| member.display_name.clone()))
        .unwrap_or_default();
    let endpoints: Vec<&Value> = participant["endpoints"]
        .as_object()
        .map(|endpoints| endpoints.values().collect())
        .unwrap_or_default();
    let in_call: Vec<&&Value> = endpoints.iter().filter(|endpoint| endpoint.get("call").is_some()).collect();
    let in_lobby = in_call.is_empty() && endpoints.iter().any(|endpoint| endpoint.get("lobby").is_some());
    let muted = !in_call.is_empty() && in_call.iter().all(|endpoint| endpoint_muted(endpoint));
    let audio_sources = in_call
        .iter()
        .flat_map(|endpoint| audio_streams(endpoint))
        .filter_map(|stream| source_id(&stream["sourceId"]))
        .collect();
    let role = ROLE_KEYS.iter().filter_map(|key| participant[*key].as_str()).find_map(MeetingRole::parse);
    Member {
        mri: mri.to_owned(),
        display_name,
        muted,
        in_lobby,
        audio_sources,
        video_source: sending_source(&in_call, VIDEO),
        screen_source: sending_source(&in_call, SCREEN),
        streams: stream_summaries(&in_call),
        hand: published_state(participant, &endpoints, RAISE_HANDS),
        spotlight: published_state(participant, &endpoints, SPOTLIGHT),
        organizer: role == Some(MeetingRole::Organizer),
        role,
        limits: AttendeeLimits {
            mic_blocked: published_state(participant, &endpoints, MIC_BLOCKED),
            mic_released: published_state(participant, &endpoints, MIC_RELEASED),
            camera_blocked: published_state(participant, &endpoints, CAMERA_BLOCKED),
            camera_released: published_state(participant, &endpoints, CAMERA_RELEASED),
        },
        captions: mri.starts_with(BOT_PREFIX).then(|| caption_bot(mri, &endpoints)).flatten(),
        replacement: in_call.iter().copied().chain(&endpoints).find_map(|endpoint| leg_replacement(endpoint)),
        version,
    }
}

fn leg_replacement(endpoint: &Value) -> Option<Replacement> {
    let link = endpoint["callLinks"]["replacement"].as_str().filter(|link| !link.is_empty())?;
    Some(Replacement {
        link: link.to_owned(),
        participant_id: endpoint["participantId"].as_str().map(str::to_owned),
    })
}

fn caption_bot(mri: &str, endpoints: &[&Value]) -> Option<CaptionBot> {
    let metadata: Vec<&Value> = endpoints.iter().map(|endpoint| &endpoint["endpointMetadata"]).filter(|metadata| metadata.is_object()).collect();
    let command_url = metadata.iter().find_map(|metadata| metadata["commandUrl"].as_str()).map(str::to_owned);
    let known = command_url.is_some() || metadata.iter().any(|metadata| metadata["processingModes"].is_object());
    let active = metadata.iter().any(|metadata| metadata["processingModes"]["closedCaptions"]["state"].as_str() == Some("Active"));
    let recording = metadata.iter().any(|metadata| metadata["processingModes"]["recording"]["state"].as_str() == Some("Active"));
    let audio_only = metadata.iter().any(|metadata| metadata["processingModes"]["recording"]["properties"]["recordVideo"].as_str() == Some("False"));
    known.then(|| CaptionBot { mri: mri.to_owned(), command_url, active, recording, audio_only })
}

fn published_state(participant: &Value, endpoints: &[&Value], state_type: &str) -> Option<PublishedState> {
    std::iter::once(participant)
        .chain(endpoints.iter().copied())
        .flat_map(|holder| holder["publishedStates"].as_array().into_iter().flatten())
        .filter(|state| state["stateType"].as_str() == Some(state_type))
        .find_map(|state| {
            let state_id = match &state["stateId"] {
                Value::String(text) => text.clone(),
                Value::Null => return None,
                other => other.to_string(),
            };
            Some(PublishedState { state_id, rank: state["typeRank"].as_u64().unwrap_or(u64::MAX) })
        })
}

fn stream_summaries(in_call: &[&&Value]) -> Vec<String> {
    in_call
        .iter()
        .flat_map(|endpoint| endpoint["call"]["mediaStreams"].as_array().into_iter().flatten())
        .map(|stream| {
            format!(
                "{} source {} {} request {}",
                stream["type"].as_str().unwrap_or("?"),
                stream["sourceId"],
                stream["direction"].as_str().unwrap_or("?"),
                stream["mdRequestId"]
            )
        })
        .collect()
}

fn sending_source(in_call: &[&&Value], stream_type: &str) -> Option<u32> {
    in_call
        .iter()
        .flat_map(|endpoint| endpoint["call"]["mediaStreams"].as_array().into_iter().flatten())
        .filter(|stream| stream["type"].as_str() == Some(stream_type))
        .filter(|stream| stream["direction"].as_str().is_some_and(|direction| SENDING_DIRECTIONS.contains(&direction)))
        .find_map(|stream| source_id(&stream["sourceId"]))
}

fn audio_streams(endpoint: &Value) -> impl Iterator<Item = &Value> {
    endpoint["call"]["mediaStreams"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|stream| stream["type"].as_str() == Some(AUDIO))
}

fn endpoint_muted(endpoint: &Value) -> bool {
    endpoint["endpointState"]["state"]["isMuted"].as_bool() == Some(true)
        || audio_streams(endpoint).any(|stream| stream["serverMuted"].as_bool() == Some(true))
}

fn source_id(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .and_then(|id| u32::try_from(id).ok())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_direct_call_lists_both_people_until_the_mixer_reports() {
        let mut roster = Roster::default();
        assert!(roster.apply(&direct_roster(&[("8:orgid:me", "Me"), ("8:orgid:bea", "Bea")])));
        assert_eq!(roster.members().len(), 2);
        assert!(!roster.apply(&direct_roster(&[("8:orgid:me", "Me"), ("8:orgid:bea", "Bea")])));
    }

    use serde_json::json;

    use super::*;

    fn delta(participants: Value) -> Value {
        json!({"type": "Delta", "sequenceNumber": 1, "participants": participants})
    }

    fn participant(name: &str, version: u64, state: &str, muted: bool, source: u32) -> Value {
        json!({
            "version": version,
            "state": state,
            "details": {"displayName": name},
            "endpoints": {"ep": {
                "call": {"mediaStreams": [{"type": "audio", "sourceId": source, "serverMuted": false},
                                           {"type": "video", "sourceId": source + 1}]},
                "endpointState": {"state": {"isMuted": muted}},
            }},
        })
    }

    #[test]
    fn active_participants_join_and_inactive_ones_leave() {
        let mut roster = Roster::default();
        assert!(roster.apply(&delta(json!({
            "8:orgid:a": participant("Ana", 1, "active", false, 201),
            "8:orgid:b": participant("Bo", 1, "active", true, 301),
        }))));
        assert_eq!(roster.members().len(), 2);
        assert!(roster.members().iter().any(|member| member.display_name == "Bo" && member.muted));
        assert!(roster.apply(&delta(json!({"8:orgid:b": participant("Bo", 2, "inactive", true, 301)}))));
        assert_eq!(roster.members().len(), 1);
        assert_eq!(roster.members()[0].mri, "8:orgid:a");
    }

    #[test]
    fn older_versions_are_ignored() {
        let mut roster = Roster::default();
        roster.apply(&delta(json!({"8:orgid:a": participant("Ana", 5, "active", false, 201)})));
        let changed = roster.apply(&delta(json!({"8:orgid:a": participant("Old", 4, "active", true, 201)})));
        assert!(!changed);
        assert_eq!(roster.members()[0].display_name, "Ana");
        assert!(!roster.members()[0].muted);
    }

    #[test]
    fn a_repeated_delta_changes_nothing() {
        let mut roster = Roster::default();
        let body = delta(json!({"8:orgid:a": participant("Ana", 1, "active", false, 201)}));
        assert!(roster.apply(&body));
        assert!(!roster.apply(&body));
    }

    #[test]
    fn server_mute_counts_as_muted() {
        let mut roster = Roster::default();
        let mut forced = participant("Ana", 1, "active", false, 201);
        forced["endpoints"]["ep"]["call"]["mediaStreams"][0]["serverMuted"] = json!(true);
        roster.apply(&delta(json!({"8:orgid:a": forced})));
        assert!(roster.members()[0].muted);
    }

    #[test]
    fn lobby_endpoints_are_marked_and_admission_clears_it() {
        let mut roster = Roster::default();
        roster.apply(&delta(json!({"8:orgid:me": {
            "version": 1, "state": "active", "details": {"displayName": "Me"},
            "endpoints": {"ep": {"lobby": {"mediaStreams": []}}},
        }})));
        assert_eq!(roster.is_in_lobby("8:orgid:me"), Some(true));
        roster.apply(&delta(json!({"8:orgid:me": participant("Me", 2, "active", true, 201)})));
        assert_eq!(roster.is_in_lobby("8:orgid:me"), Some(false));
        assert_eq!(roster.is_in_lobby("8:orgid:nobody"), None);
    }

    #[test]
    fn lobby_mris_lists_only_people_waiting() {
        let mut roster = Roster::default();
        let waiting = |name: &str| json!({"version": 1, "state": "active", "details": {"displayName": name}, "endpoints": {"ep": {"lobby": {"mediaStreams": []}}}});
        roster.apply(&delta(json!({
            "8:teamsvisitor:one": waiting("One"),
            "8:orgid:me": participant("Me", 1, "active", true, 201),
            "8:teamsvisitor:two": waiting("Two"),
        })));
        let mut waiting_mris = roster.lobby_mris();
        waiting_mris.sort();
        assert_eq!(waiting_mris, vec!["8:teamsvisitor:one".to_owned(), "8:teamsvisitor:two".to_owned()]);
    }

    #[test]
    fn lobby_counts_come_from_participant_counts() {
        let mut roster = Roster::default();
        roster.apply(&json!({"participantCounts": {"lobbyParticipants": 3, "totalParticipants": 5}}));
        assert_eq!(roster.lobby_count, 3);
    }

    #[test]
    fn speaking_sources_map_to_participants() {
        let mut roster = Roster::default();
        roster.apply(&delta(json!({
            "8:orgid:a": participant("Ana", 1, "active", false, 201),
            "8:orgid:b": participant("Bo", 1, "active", false, 301),
        })));
        assert_eq!(roster.mris_for_sources(&[301]), vec!["8:orgid:b".to_owned()]);
        assert!(roster.mris_for_sources(&[999]).is_empty());
    }

    fn sending(mut person: Value, video: Option<u32>, screen: Option<u32>) -> Value {
        let streams = person["endpoints"]["ep"]["call"]["mediaStreams"].as_array_mut().unwrap();
        streams.clear();
        streams.push(json!({"type": "audio", "label": "main-audio", "sourceId": 201, "direction": "sendrecv"}));
        streams.push(json!({"type": "video", "label": "main-video", "sourceId": video.unwrap_or(202), "direction": if video.is_some() { "sendonly" } else { "inactive" }}));
        streams.push(json!({"type": "applicationsharing-video", "label": "applicationsharing-video", "sourceId": screen.unwrap_or(212), "direction": if screen.is_some() { "sendrecv" } else { "recvonly" }}));
        person
    }

    #[test]
    fn only_sending_video_streams_are_candidates() {
        let mut roster = Roster::default();
        roster.apply(&delta(json!({
            "8:orgid:me": sending(participant("Me", 1, "active", false, 201), Some(250), None),
            "8:orgid:a": sending(participant("Ana", 1, "active", false, 301), Some(302), None),
            "8:orgid:b": sending(participant("Bo", 1, "active", false, 401), None, None),
            "8:orgid:c": sending(participant("Cy", 1, "active", false, 501), None, Some(512)),
        })));
        assert_eq!(roster.video_candidates("8:orgid:me"), vec![("8:orgid:a".to_owned(), 302)]);
        assert_eq!(roster.screen_share("8:orgid:me"), Some(("8:orgid:c".to_owned(), 512)));
        assert_eq!(roster.screen_share("8:orgid:c"), None);
        let entries = roster.entries();
        assert!(entries.iter().find(|entry| entry.display_name == "Ana").unwrap().has_video);
        assert!(!entries.iter().find(|entry| entry.display_name == "Bo").unwrap().has_video);
        assert!(entries.iter().find(|entry| entry.display_name == "Cy").unwrap().sharing);
    }

    fn with_hand(mut person: Value, state_id: &str, rank: u64) -> Value {
        person["publishedStates"] = json!([
            {"stateType": "spotlight", "stateId": "x", "typeRank": 1},
            {"stateType": "raiseHands", "content": {"skinTone": 2}, "stateId": state_id, "typeRank": rank},
        ]);
        person
    }

    #[test]
    fn published_states_carry_the_raised_hand_and_its_queue_rank() {
        let mut roster = Roster::default();
        roster.apply(&delta(json!({
            "8:orgid:a": with_hand(participant("Ana", 1, "active", false, 201), "s-1", 2),
            "8:orgid:b": with_hand(participant("Bo", 1, "active", false, 301), "s-2", 1),
            "8:orgid:c": participant("Cy", 1, "active", false, 401),
        })));
        assert_eq!(roster.hand_of("8:orgid:a"), Some(&RaisedHand { state_id: "s-1".into(), rank: 2 }));
        assert_eq!(roster.hand_of("8:orgid:b").map(|hand| hand.rank), Some(1));
        assert_eq!(roster.hand_of("8:orgid:c"), None);
        let entries = roster.entries();
        assert!(entries.iter().find(|entry| entry.display_name == "Bo").unwrap().hand.is_some());
    }

    #[test]
    fn lowering_removes_the_key_and_the_hand() {
        let mut roster = Roster::default();
        roster.apply(&delta(json!({"8:orgid:a": with_hand(participant("Ana", 1, "active", false, 201), "s-1", 1)})));
        assert!(roster.apply(&delta(json!({"8:orgid:a": participant("Ana", 2, "active", false, 201)}))));
        assert_eq!(roster.hand_of("8:orgid:a"), None);
    }

    #[test]
    fn endpoint_level_published_states_count_too() {
        let mut roster = Roster::default();
        let mut person = participant("Ana", 1, "active", false, 201);
        person["endpoints"]["ep"]["publishedStates"] = json!([{"stateType": "raiseHands", "stateId": 7, "typeRank": 3}]);
        roster.apply(&delta(json!({"8:orgid:a": person})));
        assert_eq!(roster.hand_of("8:orgid:a"), Some(&RaisedHand { state_id: "7".into(), rank: 3 }));
    }

    #[test]
    fn missing_name_keeps_the_known_one() {
        let mut roster = Roster::default();
        roster.apply(&delta(json!({"8:orgid:a": participant("Ana", 1, "active", false, 201)})));
        roster.apply(&delta(json!({"8:orgid:a": {"version": 2, "state": "active", "endpoints": {"ep": {"call": {}}}}})));
        assert_eq!(roster.members()[0].display_name, "Ana");
    }

    fn lobby_guest(name: &str, version: u64) -> Value {
        json!({"version": version, "state": "active", "details": {"displayName": name}, "endpoints": {"ep": {"lobby": {"mediaStreams": []}}}})
    }

    #[test]
    fn people_in_the_lobby_are_listed_with_their_names_and_counted() {
        let mut roster = Roster::default();
        let mut body = delta(json!({
            "8:orgid:me": participant("Me", 1, "active", false, 201),
            "8:orgid:g1": lobby_guest("Gast Eins", 1),
            "8:orgid:g2": lobby_guest("Gast Zwei", 1),
        }));
        body["participantCounts"] = json!({"lobbyParticipants": 2});
        roster.apply(&body);
        assert_eq!(roster.lobby_count, 2);
        let waiting: Vec<String> = roster.entries().into_iter().filter(|entry| entry.in_lobby).map(|entry| entry.display_name).collect();
        assert_eq!(waiting, vec!["Gast Eins".to_owned(), "Gast Zwei".to_owned()]);
        assert!(roster.video_candidates("8:orgid:me").is_empty());
        roster.apply(&delta(json!({"8:orgid:g1": participant("Gast Eins", 2, "active", true, 301)})));
        assert_eq!(roster.is_in_lobby("8:orgid:g1"), Some(false));
        roster.apply(&delta(json!({"8:orgid:g2": participant("Gast Zwei", 2, "inactive", true, 401)})));
        assert_eq!(roster.is_in_lobby("8:orgid:g2"), None);
    }

    fn with_spotlight(mut person: Value, state_id: &str, rank: u64) -> Value {
        person["publishedStates"] = json!([{"stateType": "spotlight", "content": {}, "stateId": state_id, "typeRank": rank}]);
        person
    }

    #[test]
    fn a_spotlight_state_marks_the_participant_and_vanishes_when_removed() {
        let mut roster = Roster::default();
        roster.apply(&delta(json!({
            "8:orgid:a": with_spotlight(participant("Ana", 1, "active", false, 201), "sp-1", 1),
            "8:orgid:b": participant("Bo", 1, "active", false, 301),
        })));
        assert_eq!(roster.spotlight_of("8:orgid:a"), Some(&Spotlight { state_id: "sp-1".into(), rank: 1 }));
        assert_eq!(roster.spotlight_of("8:orgid:b"), None);
        assert_eq!(roster.hand_of("8:orgid:a"), None);
        assert!(roster.entries().iter().find(|entry| entry.display_name == "Ana").unwrap().spotlight.is_some());
        roster.apply(&delta(json!({"8:orgid:a": participant("Ana", 2, "active", false, 201)})));
        assert_eq!(roster.spotlight_of("8:orgid:a"), None);
    }

    #[test]
    fn the_organizer_role_comes_from_the_meeting_role() {
        let mut roster = Roster::default();
        let mut host = participant("Me", 1, "active", false, 201);
        host["meetingRole"] = json!("organizer");
        let mut presenter = participant("Ana", 1, "active", false, 301);
        presenter["meetingRole"] = json!("presenter");
        presenter["role"] = json!("admin");
        let mut co_organizer = participant("Bo", 1, "active", false, 401);
        co_organizer["meetingRole"] = json!("coorganizer");
        roster.apply(&delta(json!({"8:orgid:me": host, "8:orgid:a": presenter, "8:orgid:b": co_organizer})));
        assert!(roster.is_organizer("8:orgid:me"));
        assert!(!roster.is_organizer("8:orgid:a"));
        assert!(roster.is_organizer("8:orgid:b"));
    }

    #[test]
    fn roles_and_attendee_limits_reach_the_entries() {
        let mut roster = Roster::default();
        let mut presenter = participant("Ana", 1, "active", false, 201);
        presenter["meetingRole"] = json!("presenter");
        let mut limited = participant("Bo", 1, "active", false, 301);
        limited["meetingRole"] = json!("attendee");
        limited["publishedStates"] = json!([
            {"stateType": "attendeeAudioRestricted", "stateId": "s-1", "typeRank": 1},
            {"stateType": "attendeeVideoUnrestricted", "stateId": "s-2", "typeRank": 2},
        ]);
        roster.apply(&delta(json!({"8:orgid:a": presenter, "8:orgid:b": limited})));
        let entries = roster.entries();
        assert_eq!(entries[0].role, Some(MeetingRole::Presenter));
        assert_eq!(entries[1].role, Some(MeetingRole::Attendee));
        assert_eq!(entries[1].limits.mic_blocked.as_ref().map(|state| state.state_id.as_str()), Some("s-1"));
        assert_eq!(entries[1].limits.camera_released.as_ref().map(|state| state.state_id.as_str()), Some("s-2"));
        assert_eq!(entries[1].limits.camera_blocked, None);
        assert_eq!(entries[0].limits, AttendeeLimits::default());
    }

    fn recorder_bot(version: u64, active: bool) -> Value {
        json!({"version": version, "state": "active", "details": {"displayName": "Recorder"}, "meetingRole": "presenter", "endpoints": {"ep": {
            "call": {"mediaStreams": []},
            "endpointMetadata": {
                "commandUrl": "https://api.flightproxy.teams.microsoft.com/api/v2/ep/recorder/v2/oncommand/1",
                "processingModes": {"closedCaptions": {"state": if active { "Active" } else { "Inactive" }}},
            },
        }}})
    }

    #[test]
    fn the_caption_bot_is_found_by_its_command_url_and_state() {
        let mut roster = Roster::default();
        roster.apply(&delta(json!({"8:orgid:a": participant("Ana", 1, "active", false, 201)})));
        assert_eq!(roster.caption_bot(), None);
        roster.apply(&delta(json!({"28:bot": recorder_bot(1, false)})));
        let bot = roster.caption_bot().unwrap();
        assert_eq!(bot.mri, "28:bot");
        assert!(bot.command_url.as_deref().unwrap().ends_with("/v2/oncommand/1"));
        assert!(!bot.active);
        roster.apply(&delta(json!({"28:bot": recorder_bot(2, true)})));
        assert!(roster.caption_bot().unwrap().active);
        roster.apply(&delta(json!({"28:bot": {"version": 3, "state": "inactive"}})));
        assert_eq!(roster.caption_bot(), None);
    }

    #[test]
    fn an_audio_only_recording_shows_in_the_bot_properties() {
        let mut bot = recorder_bot(1, false);
        bot["endpoints"]["ep"]["endpointMetadata"]["processingModes"]["recording"] = json!({"state": "Active", "properties": {"recordVideo": "False", "exportFormatType": "Mp4"}});
        let mut roster = Roster::default();
        roster.apply(&delta(json!({"28:bot": bot})));
        let found = roster.caption_bot().unwrap();
        assert!(found.recording && found.audio_only);
        roster.apply(&delta(json!({"28:bot": recorder_bot(2, false)})));
        assert!(!roster.caption_bot().unwrap().audio_only);
    }

    const JOIN_ANSWER: &str = include_str!("../tests/fixtures/meeting_join_answer.json");

    #[test]
    fn a_late_joiner_sees_everyone_from_the_join_answer() {
        let answer: Value = serde_json::from_str(JOIN_ANSWER).unwrap();
        let mut roster = Roster::default();
        assert!(roster.apply_snapshot(&answer["roster"]));
        let mut names: Vec<String> = roster.entries().into_iter().map(|entry| entry.display_name).collect();
        names.sort();
        assert_eq!(names, ["Organizer", "Test Guest 1", "Test Guest 2", "Test Guest 3"]);
        assert_eq!(roster.video_candidates("8:orgid:00000000-0000-0000-0000-00000000000a").len(), 3);
        let own_delta = delta(json!({"8:orgid:00000000-0000-0000-0000-00000000000a": participant("Organizer", 10, "active", true, 840)}));
        roster.apply(&own_delta);
        assert_eq!(roster.members().len(), 4);
    }

    #[test]
    fn a_snapshot_drops_people_it_no_longer_lists_unless_a_newer_delta_added_them() {
        let mut roster = Roster::default();
        roster.apply(&delta(json!({
            "8:orgid:gone": participant("Gone", 3, "active", false, 201),
            "8:orgid:new": participant("New", 12, "active", false, 301),
        })));
        let snapshot = json!({"type": "Delta", "sequenceNumber": 9, "participants": {"8:orgid:a": participant("Ana", 4, "active", false, 401)}});
        assert!(roster.apply_snapshot(&snapshot));
        let mut mris: Vec<&str> = roster.members().iter().map(|member| member.mri.as_str()).collect();
        mris.sort();
        assert_eq!(mris, ["8:orgid:a", "8:orgid:new"]);
    }

    #[test]
    fn caption_speakers_resolve_by_mri_or_object_id() {
        let mut roster = Roster::default();
        roster.apply(&delta(json!({"8:orgid:abc-123": participant("Ana", 1, "active", false, 201)})));
        assert_eq!(roster.name_of("8:orgid:abc-123"), Some("Ana"));
        assert_eq!(roster.name_of("abc-123"), Some("Ana"));
        assert_eq!(roster.name_of("zzz"), None);
    }

    #[test]
    fn the_transfer_replacement_is_the_other_persons_call_leg() {
        let mut roster = Roster::default();
        let leg = |participant_id: &str, link: &str| json!({
            "version": 1,
            "state": "active",
            "endpoints": {"ep": {"participantId": participant_id, "callLinks": {"replacement": link}, "call": {}}},
        });
        roster.apply(&delta(json!({
            "8:orgid:me": leg("leg-me", "https://cc/me/replacement"),
            "28:bot": leg("leg-bot", "https://cc/bot/replacement"),
            "8:orgid:ana": leg("leg-ana", "https://cc/ana/replacement"),
        })));
        assert_eq!(
            roster.transfer_replacement("8:orgid:me"),
            Some(Replacement { link: "https://cc/ana/replacement".into(), participant_id: Some("leg-ana".into()) })
        );
        assert_eq!(roster.transfer_replacement("8:orgid:ana").map(|replacement| replacement.participant_id), Some(Some("leg-me".into())));
        let mut without_links = Roster::default();
        without_links.apply(&delta(json!({"8:orgid:ana": participant("Ana", 1, "active", false, 10)})));
        assert_eq!(without_links.transfer_replacement("8:orgid:me"), None);
    }
}
