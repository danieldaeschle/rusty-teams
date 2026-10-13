use calling::{MeetingRole, Modality};

use super::model::{CallKind, CallModel, Tile, TileState};

const BOT_PREFIX: &str = "28:";
const FLAT_TITLE: &str = "In this call";
const OWN_NAME: &str = "You";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonRow {
    pub mri: String,
    pub name: String,
    pub user_id: Option<String>,
    pub own: bool,
    pub label: Option<&'static str>,
    pub muted: bool,
    pub mic_blocked: bool,
    pub camera_on: bool,
    pub camera_blocked: bool,
    pub hand: Option<usize>,
    pub ringing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyRow {
    pub mri: String,
    pub name: String,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeopleGroup {
    pub title: &'static str,
    pub rows: Vec<PersonRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PeopleSections {
    pub count: usize,
    pub lobby: Vec<LobbyRow>,
    pub hands: Vec<PersonRow>,
    pub groups: Vec<PeopleGroup>,
}

pub fn role_label(role: MeetingRole) -> &'static str {
    match role {
        MeetingRole::Organizer => "Organizer",
        MeetingRole::Presenter => "Presenter",
        MeetingRole::Attendee => "Attendee",
    }
}

fn group_title(role: MeetingRole) -> &'static str {
    match role {
        MeetingRole::Organizer => "Organizers",
        MeetingRole::Presenter => "Presenters",
        MeetingRole::Attendee => "Attendees",
    }
}

fn tile_row(model: &CallModel, tile: &Tile, role: Option<MeetingRole>) -> PersonRow {
    let attendee = role != Some(MeetingRole::Organizer) && role != Some(MeetingRole::Presenter);
    PersonRow {
        mri: tile.mri.clone(),
        name: tile.name.clone(),
        user_id: tile.user_id.clone(),
        own: false,
        label: role.map(role_label),
        muted: tile.muted,
        mic_blocked: attendee && !model.attendee_allowed(&tile.mri, Modality::Mic),
        camera_on: tile.has_video,
        camera_blocked: attendee && !model.attendee_allowed(&tile.mri, Modality::Camera),
        hand: tile.hand.map(|rank| model.hand_position(rank)),
        ringing: tile.state == TileState::Invited,
    }
}

fn own_row(model: &CallModel, role: Option<MeetingRole>) -> Option<PersonRow> {
    let mri = model.own_mri.clone()?;
    Some(PersonRow {
        mri,
        name: OWN_NAME.to_owned(),
        user_id: None,
        own: true,
        label: role.map(role_label),
        muted: model.muted,
        mic_blocked: false,
        camera_on: model.camera_on,
        camera_blocked: false,
        hand: model.own_hand.map(|rank| model.hand_position(rank)),
        ringing: false,
    })
}

fn by_name(rows: &mut [PersonRow]) {
    rows.sort_by_key(|row| (!row.own, row.name.to_lowercase()));
}

pub fn people_sections(model: &CallModel) -> PeopleSections {
    let in_meeting = model.kind == CallKind::Meeting;
    let own_role = in_meeting.then(|| {
        model
            .own_role
            .or_else(|| model.can_manage().then_some(MeetingRole::Organizer))
            .unwrap_or(MeetingRole::Attendee)
    });
    let mut rows: Vec<(MeetingRole, PersonRow)> = Vec::new();
    let mut lobby = Vec::new();
    for tile in model.tiles.iter().filter(|tile| !tile.mri.starts_with(BOT_PREFIX)) {
        match tile.state {
            TileState::InLobby => lobby.push(LobbyRow {
                mri: tile.mri.clone(),
                name: tile.name.clone(),
                user_id: tile.user_id.clone(),
            }),
            TileState::Present | TileState::Invited => {
                let role = in_meeting.then(|| tile.role.unwrap_or(MeetingRole::Attendee));
                rows.push((role.unwrap_or(MeetingRole::Attendee), tile_row(model, tile, role)));
            }
            TileState::Left => {}
        }
    }
    if let Some(own) = own_row(model, own_role) {
        rows.push((own_role.unwrap_or(MeetingRole::Attendee), own));
    }
    let count = rows.len();
    let mut hands: Vec<PersonRow> = rows
        .iter()
        .filter(|(_, row)| row.hand.is_some())
        .map(|(_, row)| row.clone())
        .collect();
    hands.sort_by_key(|row| row.hand);
    let groups = if in_meeting {
        [MeetingRole::Organizer, MeetingRole::Presenter, MeetingRole::Attendee]
            .into_iter()
            .filter_map(|role| {
                let mut group: Vec<PersonRow> = rows
                    .iter()
                    .filter(|(row_role, _)| *row_role == role)
                    .map(|(_, row)| row.clone())
                    .collect();
                by_name(&mut group);
                (!group.is_empty()).then_some(PeopleGroup { title: group_title(role), rows: group })
            })
            .collect()
    } else {
        let mut flat: Vec<PersonRow> = rows.into_iter().map(|(_, row)| row).collect();
        by_name(&mut flat);
        vec![PeopleGroup { title: FLAT_TITLE, rows: flat }]
    };
    PeopleSections { count, lobby, hands, groups }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use calling::{AttendeeLimits, CallState, CallUpdate, MeetingCapability, PublishedState, RaisedHand, RosterEntry};

    use super::*;

    fn entry(mri: &str, name: &str, role: Option<MeetingRole>) -> RosterEntry {
        RosterEntry { mri: mri.into(), display_name: name.into(), role, ..RosterEntry::default() }
    }

    fn raised(mut entry: RosterEntry, rank: u64) -> RosterEntry {
        entry.hand = Some(RaisedHand { state_id: format!("s-{rank}"), rank });
        entry
    }

    fn meeting(entries: Vec<RosterEntry>) -> CallModel {
        let mut model = CallModel::meeting("Standup", true);
        model.apply(CallUpdate::OwnIdentity { mri: "8:orgid:me".into() });
        model.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        model.apply(CallUpdate::Roster(entries));
        model
    }

    fn titles(sections: &PeopleSections) -> Vec<&'static str> {
        sections.groups.iter().map(|group| group.title).collect()
    }

    fn names(group: &PeopleGroup) -> Vec<&str> {
        group.rows.iter().map(|row| row.name.as_str()).collect()
    }

    #[test]
    fn a_meeting_groups_by_role_and_keeps_only_the_groups_that_exist() {
        let model = meeting(vec![
            entry("8:orgid:me", "Me", Some(MeetingRole::Organizer)),
            entry("8:orgid:a", "Ana", Some(MeetingRole::Attendee)),
            entry("8:orgid:b", "bo", Some(MeetingRole::Attendee)),
            entry("8:orgid:c", "Cy", None),
        ]);
        let sections = people_sections(&model);
        assert_eq!(titles(&sections), vec!["Organizers", "Attendees"]);
        assert_eq!(names(&sections.groups[0]), vec!["You"]);
        assert_eq!(names(&sections.groups[1]), vec!["Ana", "bo", "Cy"]);
        assert_eq!(sections.count, 4);
        let presenter = meeting(vec![entry("8:orgid:p", "Pia", Some(MeetingRole::Presenter))]);
        assert_eq!(titles(&people_sections(&presenter)), vec!["Organizers", "Presenters"]);
    }

    #[test]
    fn direct_and_group_calls_show_one_flat_list_without_role_labels() {
        let mut model = CallModel::people("Team", &[("8:orgid:a".into(), "Ana".into()), ("8:orgid:b".into(), "Bo".into())]);
        model.apply(CallUpdate::OwnIdentity { mri: "8:orgid:me".into() });
        model.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        let sections = people_sections(&model);
        assert_eq!(titles(&sections), vec!["In this call"]);
        assert_eq!(names(&sections.groups[0]), vec!["You", "Ana", "Bo"]);
        assert!(sections.groups[0].rows.iter().all(|row| row.label.is_none()));
        assert!(sections.groups[0].rows[1].ringing);
    }

    #[test]
    fn raised_hands_queue_in_raise_order_and_the_lobby_stays_out_of_the_groups() {
        let mut waiting = entry("8:orgid:w", "Wen", None);
        waiting.in_lobby = true;
        let model = meeting(vec![
            entry("8:orgid:me", "Me", Some(MeetingRole::Organizer)),
            raised(entry("8:orgid:a", "Ana", None), 7),
            raised(entry("8:orgid:b", "Bo", None), 3),
            waiting,
        ]);
        let sections = people_sections(&model);
        let queue: Vec<(&str, Option<usize>)> = sections.hands.iter().map(|row| (row.name.as_str(), row.hand)).collect();
        assert_eq!(queue, vec![("Bo", Some(1)), ("Ana", Some(2))]);
        assert_eq!(sections.lobby.len(), 1);
        assert_eq!(sections.lobby[0].name, "Wen");
        assert_eq!(sections.count, 3);
    }

    #[test]
    fn bots_stay_out_and_the_own_role_falls_back_to_the_organizer_flag() {
        let model = meeting(vec![entry("28:recorder", "Recorder", Some(MeetingRole::Presenter))]);
        let sections = people_sections(&model);
        assert_eq!(sections.count, 1);
        assert_eq!(sections.groups[0].title, "Organizers");
        assert_eq!(sections.groups[0].rows[0].label, Some("Organizer"));
    }

    #[test]
    fn only_attendees_show_blocked_microphones() {
        let blocked = AttendeeLimits {
            mic_blocked: Some(PublishedState { state_id: "x".into(), rank: 1 }),
            ..AttendeeLimits::default()
        };
        let mut limited = entry("8:orgid:a", "Ana", Some(MeetingRole::Attendee));
        limited.limits = blocked.clone();
        let mut presenter = entry("8:orgid:p", "Pia", Some(MeetingRole::Presenter));
        presenter.limits = blocked;
        let mut model = meeting(vec![limited, presenter, entry("8:orgid:b", "Bo", Some(MeetingRole::Attendee))]);
        model.apply(CallUpdate::Capability(MeetingCapability { attendee_camera_off: true, ..MeetingCapability::default() }));
        let sections = people_sections(&model);
        let attendees = sections.groups.iter().find(|group| group.title == "Attendees").unwrap();
        assert!(attendees.rows[0].mic_blocked && !attendees.rows[1].mic_blocked);
        assert!(attendees.rows.iter().all(|row| row.camera_blocked));
        let presenters = sections.groups.iter().find(|group| group.title == "Presenters").unwrap();
        assert!(!presenters.rows[0].mic_blocked && !presenters.rows[0].camera_blocked);
    }

    #[test]
    fn a_one_to_one_call_turns_into_a_group_when_a_third_person_joins() {
        let mut model = CallModel::people("Ana", &[("8:orgid:a".into(), "Ana".into())]);
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:a", "Ana", None)]));
        assert_eq!(model.kind, CallKind::Direct);
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:a", "Ana", None), entry("8:orgid:b", "Bo", None)]));
        assert_eq!(model.kind, CallKind::Group);
    }
}
