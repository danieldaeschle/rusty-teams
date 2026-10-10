use serde_json::Value;

const ACTIVE: &str = "active";
const AUDIO: &str = "audio";
const VIDEO: &str = "video";
const SCREEN: &str = "applicationsharing-video";
const SENDING_DIRECTIONS: [&str; 2] = ["sendonly", "sendrecv"];
const RAISE_HANDS: &str = "raiseHands";
const SPOTLIGHT: &str = "spotlight";
const ORGANIZER_ROLES: [&str; 2] = ["organizer", "coorganizer"];
const BOT_PREFIX: &str = "28:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedState {
    pub state_id: String,
    pub rank: u64,
}

pub type RaisedHand = PublishedState;
pub type Spotlight = PublishedState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionBot {
    pub mri: String,
    pub command_url: Option<String>,
    pub active: bool,
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
    pub captions: Option<CaptionBot>,
    version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
        organizer: is_organizer_role(participant),
        captions: mri.starts_with(BOT_PREFIX).then(|| caption_bot(mri, &endpoints)).flatten(),
        version,
    }
}

fn is_organizer_role(participant: &Value) -> bool {
    ["meetingRole", "role"]
        .iter()
        .filter_map(|key| participant[*key].as_str())
        .any(|role| ORGANIZER_ROLES.contains(&role.to_ascii_lowercase().as_str()))
}

fn caption_bot(mri: &str, endpoints: &[&Value]) -> Option<CaptionBot> {
    let metadata: Vec<&Value> = endpoints.iter().map(|endpoint| &endpoint["endpointMetadata"]).filter(|metadata| metadata.is_object()).collect();
    let command_url = metadata.iter().find_map(|metadata| metadata["commandUrl"].as_str()).map(str::to_owned);
    let known = command_url.is_some() || metadata.iter().any(|metadata| metadata["processingModes"].is_object());
    let active = metadata.iter().any(|metadata| metadata["processingModes"]["closedCaptions"]["state"].as_str() == Some("Active"));
    known.then(|| CaptionBot { mri: mri.to_owned(), command_url, active })
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
    fn caption_speakers_resolve_by_mri_or_object_id() {
        let mut roster = Roster::default();
        roster.apply(&delta(json!({"8:orgid:abc-123": participant("Ana", 1, "active", false, 201)})));
        assert_eq!(roster.name_of("8:orgid:abc-123"), Some("Ana"));
        assert_eq!(roster.name_of("abc-123"), Some("Ana"));
        assert_eq!(roster.name_of("zzz"), None);
    }
}
