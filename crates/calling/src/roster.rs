use serde_json::Value;

const ACTIVE: &str = "active";
const AUDIO: &str = "audio";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub mri: String,
    pub display_name: String,
    pub muted: bool,
    pub in_lobby: bool,
    pub audio_sources: Vec<u32>,
    version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterEntry {
    pub mri: String,
    pub display_name: String,
    pub muted: bool,
    pub in_lobby: bool,
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

    pub fn is_in_lobby(&self, mri: &str) -> Option<bool> {
        self.members.iter().find(|member| member.mri == mri).map(|member| member.in_lobby)
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
        version,
    }
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

    #[test]
    fn missing_name_keeps_the_known_one() {
        let mut roster = Roster::default();
        roster.apply(&delta(json!({"8:orgid:a": participant("Ana", 1, "active", false, 201)})));
        roster.apply(&delta(json!({"8:orgid:a": {"version": 2, "state": "active", "endpoints": {"ep": {"call": {}}}}})));
        assert_eq!(roster.members()[0].display_name, "Ana");
    }
}
