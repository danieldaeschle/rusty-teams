use std::sync::Arc;
use std::time::{Duration, Instant};

use calling::{
    CallCommand, CallState, CallUpdate, CameraDevice, DeviceChoice, DeviceLists, EndKind, EndReason, Progress, RosterEntry,
    ShareKind, ShareSource, SpeakingDetector, VideoHub,
};
use tokio::sync::mpsc::UnboundedSender;

use super::pictures::CallPictures;

pub const TEST_CALL_TITLE: &str = "Test call";
pub const ECHO_NAME: &str = "Teams echo";
pub const ECHO_MRI: &str = calling::signaling::ECHO_BOT_MRI;
pub const MAX_TILES: usize = 9;
const ORGID_PREFIX: &str = "8:orgid:";
const WIDE_TILES: usize = 2;
const MEDIUM_TILES: usize = 4;

pub struct ActiveCall {
    pub id: u64,
    pub model: CallModel,
    pub commands: UnboundedSender<CallCommand>,
    pub viewing: bool,
    pub conversation_id: Option<String>,
    pub video: Arc<VideoHub>,
    pub pictures: CallPictures,
    pub stage_fullscreen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    Test,
    Direct,
    Group,
    Meeting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileState {
    Invited,
    Present,
    InLobby,
    Left,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub mri: String,
    pub user_id: Option<String>,
    pub name: String,
    pub muted: bool,
    pub state: TileState,
    pub invited: bool,
    pub has_video: bool,
}

impl Tile {
    pub fn invited(mri: &str, name: &str) -> Tile {
        Tile {
            mri: mri.to_owned(),
            user_id: mri.strip_prefix(ORGID_PREFIX).map(str::to_owned),
            name: name.to_owned(),
            muted: false,
            state: TileState::Invited,
            invited: true,
            has_video: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridLayout {
    pub shown_others: usize,
    pub hidden: usize,
}

pub fn grid_layout(others: usize) -> GridLayout {
    let total = others + 1;
    if total <= MAX_TILES {
        return GridLayout {
            shown_others: others,
            hidden: 0,
        };
    }
    let shown_others = MAX_TILES - 2;
    GridLayout {
        shown_others,
        hidden: others - shown_others,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileSize {
    pub width: f32,
    pub height: f32,
    pub avatar: f32,
}

pub fn tile_size(cells: usize) -> TileSize {
    if cells <= WIDE_TILES {
        TileSize {
            width: 260.,
            height: 210.,
            avatar: 96.,
        }
    } else if cells <= MEDIUM_TILES {
        TileSize {
            width: 220.,
            height: 180.,
            avatar: 72.,
        }
    } else {
        TileSize {
            width: 190.,
            height: 150.,
            avatar: 56.,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CallModel {
    pub title: String,
    pub kind: CallKind,
    pub peer_name: String,
    pub state: CallState,
    pub progress: Option<Progress>,
    pub muted: bool,
    pub listen_only: bool,
    pub lobby: bool,
    pub can_end_meeting: bool,
    pub incoming: bool,
    pub input: DeviceChoice,
    pub output: DeviceChoice,
    pub devices: DeviceLists,
    pub local_speaking: bool,
    pub remote_speaking: bool,
    pub speaking_mris: Vec<String>,
    pub connected_since: Option<Instant>,
    pub own_mri: Option<String>,
    pub tiles: Vec<Tile>,
    pub screen_sharer: Option<String>,
    pub camera_on: bool,
    pub cameras: Vec<CameraDevice>,
    pub camera: DeviceChoice,
    pub local_share: Option<String>,
    pub share_sources: Vec<ShareSource>,
    local_detector: SpeakingDetector,
    remote_detector: SpeakingDetector,
}

impl CallModel {
    fn base(kind: CallKind, title: &str, peer_name: &str, tiles: Vec<Tile>) -> Self {
        CallModel {
            title: title.to_owned(),
            kind,
            peer_name: peer_name.to_owned(),
            state: CallState::Connecting,
            progress: None,
            muted: false,
            listen_only: false,
            lobby: false,
            can_end_meeting: false,
            incoming: false,
            input: DeviceChoice::SystemDefault,
            output: DeviceChoice::SystemDefault,
            devices: DeviceLists::default(),
            local_speaking: false,
            remote_speaking: false,
            speaking_mris: Vec::new(),
            connected_since: None,
            own_mri: None,
            tiles,
            screen_sharer: None,
            camera_on: false,
            cameras: Vec::new(),
            camera: DeviceChoice::SystemDefault,
            local_share: None,
            share_sources: Vec::new(),
            local_detector: SpeakingDetector::default(),
            remote_detector: SpeakingDetector::default(),
        }
    }

    pub fn test() -> Self {
        Self::base(
            CallKind::Test,
            TEST_CALL_TITLE,
            ECHO_NAME,
            vec![Tile::invited(ECHO_MRI, ECHO_NAME)],
        )
    }

    pub fn people(title: &str, callees: &[(String, String)]) -> Self {
        let kind = if callees.len() == 1 { CallKind::Direct } else { CallKind::Group };
        let peer_name = match callees {
            [(_, name)] => name.clone(),
            _ => title.to_owned(),
        };
        let tiles = callees.iter().map(|(mri, name)| Tile::invited(mri, name)).collect();
        Self::base(kind, title, &peer_name, tiles)
    }

    pub fn meeting(title: &str, can_end_meeting: bool) -> Self {
        let mut model = Self::base(CallKind::Meeting, title, title, Vec::new());
        model.can_end_meeting = can_end_meeting;
        model
    }

    pub fn incoming(title: &str, caller_mri: &str, caller_name: &str) -> Self {
        let mut model = Self::base(
            CallKind::Direct,
            title,
            caller_name,
            vec![Tile {
                state: TileState::Present,
                ..Tile::invited(caller_mri, caller_name)
            }],
        );
        model.incoming = true;
        model
    }

    pub fn apply(&mut self, update: CallUpdate) {
        match update {
            CallUpdate::State(state) => {
                if let Some(since) = state.started_at() {
                    self.connected_since = Some(since);
                }
                if !matches!(state, CallState::Connected { .. }) {
                    self.silence();
                }
                self.state = state;
            }
            CallUpdate::Muted(muted) => {
                self.muted = muted;
                if muted {
                    self.local_speaking = false;
                    self.local_detector.reset();
                }
            }
            CallUpdate::Levels { local, remote } => {
                self.local_speaking = !self.muted && self.local_detector.update(local);
                self.remote_speaking = self.remote_detector.update(remote);
            }
            CallUpdate::Devices(devices) => self.devices = devices,
            CallUpdate::Selected { input, output } => {
                self.input = input;
                self.output = output;
            }
            CallUpdate::ListenOnly(listen_only) => self.listen_only = listen_only,
            CallUpdate::Stats { .. } => {}
            CallUpdate::Progress(progress) => self.progress = Some(progress),
            CallUpdate::Roster(entries) => self.apply_roster(&entries),
            CallUpdate::Speakers(mris) => self.speaking_mris = mris,
            CallUpdate::Lobby(lobby) => self.lobby = lobby,
            CallUpdate::OwnIdentity { mri } => self.own_mri = Some(mri),
            CallUpdate::ScreenShare(sharer) => self.screen_sharer = sharer,
            CallUpdate::VideoReady | CallUpdate::Notice(_) => {}
            CallUpdate::Camera(on) => self.camera_on = on,
            CallUpdate::Cameras(cameras) => self.cameras = cameras,
            CallUpdate::LocalShare(label) => self.local_share = label,
            CallUpdate::ShareSources(sources) => self.share_sources = sources,
        }
    }

    fn silence(&mut self) {
        self.local_speaking = false;
        self.remote_speaking = false;
        self.speaking_mris.clear();
        self.local_detector.reset();
        self.remote_detector.reset();
    }

    fn apply_roster(&mut self, entries: &[RosterEntry]) {
        for entry in entries.iter().filter(|entry| Some(&entry.mri) != self.own_mri.as_ref()) {
            let state = if entry.in_lobby { TileState::InLobby } else { TileState::Present };
            match self.tiles.iter_mut().find(|tile| tile.mri == entry.mri) {
                Some(tile) => {
                    tile.state = state;
                    tile.muted = entry.muted;
                    tile.has_video = entry.has_video;
                    if tile.name.is_empty() {
                        tile.name = entry.display_name.clone();
                    }
                }
                None => self.tiles.push(Tile {
                    mri: entry.mri.clone(),
                    user_id: entry.mri.strip_prefix(ORGID_PREFIX).map(str::to_owned),
                    name: entry.display_name.clone(),
                    muted: entry.muted,
                    state,
                    invited: false,
                    has_video: entry.has_video,
                }),
            }
        }
        let own_mri = self.own_mri.clone();
        let listed = |mri: &str| entries.iter().any(|entry| entry.mri == mri);
        self.tiles.retain_mut(|tile| {
            if listed(&tile.mri) || Some(&tile.mri) == own_mri.as_ref() {
                return true;
            }
            match (tile.invited, tile.state) {
                (true, TileState::Invited) => true,
                (true, _) => {
                    tile.state = TileState::Left;
                    true
                }
                (false, _) => false,
            }
        });
    }

    pub fn visible_tiles(&self) -> Vec<&Tile> {
        let mut tiles: Vec<&Tile> = self.tiles.iter().filter(|tile| tile.state != TileState::InLobby).collect();
        tiles.sort_by_key(|tile| match tile.state {
            TileState::Present => 0,
            TileState::Invited => 1,
            _ => 2,
        });
        tiles
    }

    pub fn strip_tiles(&self) -> Vec<&Tile> {
        let mut tiles = self.visible_tiles();
        tiles.sort_by_key(|tile| !self.tile_speaking(tile));
        tiles
    }

    pub fn sharing_label(&self) -> Option<String> {
        let sharer = self.screen_sharer.as_ref()?;
        let name = self
            .tiles
            .iter()
            .find(|tile| &tile.mri == sharer)
            .map(|tile| tile.name.as_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("Someone");
        Some(format!("{name} is sharing"))
    }

    pub fn tile_speaking(&self, tile: &Tile) -> bool {
        if self.speaking_mris.contains(&tile.mri) {
            return true;
        }
        let only_remote = self.tiles.len() == 1;
        only_remote && tile.state != TileState::Left && self.remote_speaking
    }

    pub fn tile_caption(&self, tile: &Tile) -> Option<&'static str> {
        match tile.state {
            TileState::Invited => Some(match self.progress {
                Some(Progress::Ringing) => "Ringing",
                _ => "Calling...",
            }),
            TileState::Left => Some("Left"),
            TileState::Present | TileState::InLobby => None,
        }
    }

    pub fn elapsed(&self, now: Instant) -> Option<Duration> {
        self.connected_since
            .map(|since| now.saturating_duration_since(since))
    }

    pub fn connecting_text(&self) -> &'static str {
        match (self.kind, self.progress) {
            (CallKind::Meeting, _) => "Joining...",
            (_, _) if self.incoming => "Connecting...",
            (_, Some(Progress::Ringing)) => "Ringing",
            _ => "Calling...",
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            CallState::Connecting | CallState::Connected { .. } | CallState::Reconnecting { .. }
        )
    }

    pub fn is_connecting(&self) -> bool {
        matches!(self.state, CallState::Connecting)
    }

    pub fn is_reconnecting(&self) -> bool {
        matches!(self.state, CallState::Reconnecting { .. })
    }

    pub fn ended_reason(&self) -> Option<&EndReason> {
        match &self.state {
            CallState::Ended { reason } => Some(reason),
            _ => None,
        }
    }

    pub fn timer_text(&self, now: Instant) -> String {
        self.elapsed(now).map(format_elapsed).unwrap_or_default()
    }

    pub fn can_use_camera(&self) -> bool {
        !self.cameras.is_empty()
    }

    pub fn can_share(&self) -> bool {
        !self.share_sources.is_empty() && self.is_live()
    }

    fn is_live(&self) -> bool {
        matches!(self.state, CallState::Connected { .. } | CallState::Reconnecting { .. })
    }

    pub fn screens(&self) -> Vec<&ShareSource> {
        self.share_sources.iter().filter(|source| source.kind == ShareKind::Screen).collect()
    }

    pub fn windows(&self) -> Vec<&ShareSource> {
        self.share_sources.iter().filter(|source| source.kind == ShareKind::Window).collect()
    }

    pub fn can_unmute(&self) -> bool {
        !self.listen_only
    }
}

pub fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

pub fn ended_notice(reason: &EndReason, elapsed: Option<Duration>, peer_name: &str) -> String {
    let ended = || match elapsed {
        Some(elapsed) => format!("Call ended {}", format_elapsed(elapsed)),
        None => "Call ended".to_owned(),
    };
    match reason {
        EndReason::Cancelled => "Call cancelled".to_owned(),
        EndReason::Dropped => "Call dropped".to_owned(),
        EndReason::Failed(message) => format!("Call failed: {message}"),
        EndReason::LocalHangup => ended(),
        EndReason::Remote(EndKind::Declined) => format!("{peer_name} declined"),
        EndReason::Remote(EndKind::NoAnswer) => "No answer".to_owned(),
        EndReason::Remote(EndKind::Unavailable) => format!("{peer_name} is not available"),
        EndReason::Remote(_) => ended(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connected_model() -> CallModel {
        let mut model = CallModel::test();
        model.apply(CallUpdate::State(CallState::Connected {
            since: Instant::now(),
        }));
        model
    }

    fn entry(mri: &str, name: &str, muted: bool, in_lobby: bool) -> RosterEntry {
        RosterEntry {
            mri: mri.into(),
            display_name: name.into(),
            muted,
            in_lobby,
            has_video: false,
            sharing: false,
        }
    }

    fn meeting_with_me() -> CallModel {
        let mut model = CallModel::meeting("Standup", false);
        model.apply(CallUpdate::OwnIdentity { mri: "8:orgid:me".into() });
        model
    }

    #[test]
    fn elapsed_text_is_minutes_and_seconds() {
        assert_eq!(format_elapsed(Duration::from_secs(42)), "00:42");
        assert_eq!(format_elapsed(Duration::from_secs(61)), "01:01");
        assert_eq!(format_elapsed(Duration::from_secs(3725)), "1:02:05");
    }

    #[test]
    fn levels_drive_the_speaking_rings() {
        let mut model = connected_model();
        model.apply(CallUpdate::Levels {
            local: 0.0,
            remote: 0.4,
        });
        assert!(model.remote_speaking);
        assert!(!model.local_speaking);
        model.apply(CallUpdate::Levels {
            local: 0.4,
            remote: 0.0,
        });
        assert!(model.local_speaking);
    }

    #[test]
    fn muting_switches_the_local_ring_off_at_once() {
        let mut model = connected_model();
        model.apply(CallUpdate::Levels {
            local: 0.5,
            remote: 0.0,
        });
        assert!(model.local_speaking);
        model.apply(CallUpdate::Muted(true));
        assert!(!model.local_speaking);
        model.apply(CallUpdate::Levels {
            local: 0.5,
            remote: 0.0,
        });
        assert!(!model.local_speaking);
    }

    #[test]
    fn reconnecting_keeps_the_timer_and_clears_the_rings() {
        let mut model = connected_model();
        let since = model.connected_since.unwrap();
        model.apply(CallUpdate::Levels {
            local: 0.5,
            remote: 0.5,
        });
        model.apply(CallUpdate::State(CallState::Reconnecting { since }));
        assert!(model.is_reconnecting());
        assert_eq!(model.connected_since, Some(since));
        assert!(!model.remote_speaking && !model.local_speaking);
    }

    #[test]
    fn notices_for_every_end_reason() {
        let elapsed = Some(Duration::from_secs(42));
        let notice = |reason: EndReason, elapsed| ended_notice(&reason, elapsed, "Bea");
        assert_eq!(notice(EndReason::LocalHangup, elapsed), "Call ended 00:42");
        assert_eq!(notice(EndReason::Remote(EndKind::Normal), elapsed), "Call ended 00:42");
        assert_eq!(notice(EndReason::Remote(EndKind::Normal), None), "Call ended");
        assert_eq!(notice(EndReason::Cancelled, None), "Call cancelled");
        assert_eq!(notice(EndReason::Dropped, elapsed), "Call dropped");
        assert_eq!(notice(EndReason::Failed("no route".into()), None), "Call failed: no route");
        assert_eq!(notice(EndReason::Remote(EndKind::Declined), None), "Bea declined");
        assert_eq!(notice(EndReason::Remote(EndKind::NoAnswer), None), "No answer");
        assert_eq!(notice(EndReason::Remote(EndKind::Unavailable), None), "Bea is not available");
        assert_eq!(notice(EndReason::Remote(EndKind::Cancelled), None), "Call ended");
        assert_eq!(notice(EndReason::Remote(EndKind::Other), None), "Call ended");
        assert_eq!(notice(EndReason::Remote(EndKind::AnsweredElsewhere), None), "Call ended");
    }

    #[test]
    fn the_connecting_text_follows_the_kind_of_call() {
        let mut direct = CallModel::people("Bea", &[("8:orgid:b".into(), "Bea".into())]);
        assert_eq!(direct.connecting_text(), "Calling...");
        direct.apply(CallUpdate::Progress(Progress::Ringing));
        assert_eq!(direct.connecting_text(), "Ringing");
        assert_eq!(CallModel::meeting("Standup", false).connecting_text(), "Joining...");
        assert_eq!(CallModel::incoming("Bea", "8:orgid:b", "Bea").connecting_text(), "Connecting...");
        assert_eq!(CallModel::test().connecting_text(), "Calling...");
    }

    #[test]
    fn listen_only_blocks_unmuting() {
        let mut model = connected_model();
        assert!(model.can_unmute());
        model.apply(CallUpdate::ListenOnly(true));
        assert!(!model.can_unmute());
    }

    #[test]
    fn callees_start_invited_and_follow_the_progress() {
        let mut model = CallModel::people("Bea", &[("8:orgid:b".into(), "Bea".into())]);
        assert_eq!(model.kind, CallKind::Direct);
        assert_eq!(model.peer_name, "Bea");
        assert_eq!(model.tile_caption(&model.tiles[0]), Some("Calling..."));
        model.apply(CallUpdate::Progress(Progress::Ringing));
        assert_eq!(model.tile_caption(&model.tiles[0]), Some("Ringing"));
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:b", "Bea", false, false)]));
        assert_eq!(model.tiles[0].state, TileState::Present);
        assert_eq!(model.tile_caption(&model.tiles[0]), None);
    }

    #[test]
    fn several_callees_make_a_group_call_named_after_the_chat() {
        let model = CallModel::people(
            "Retro team",
            &[("8:orgid:b".into(), "Bea".into()), ("8:orgid:c".into(), "Cy".into())],
        );
        assert_eq!(model.kind, CallKind::Group);
        assert_eq!(model.peer_name, "Retro team");
        assert_eq!(model.tiles.len(), 2);
    }

    #[test]
    fn roster_adds_guests_updates_mute_and_drops_leavers() {
        let mut model = meeting_with_me();
        model.apply(CallUpdate::Roster(vec![
            entry("8:orgid:me", "Me", false, false),
            entry("8:orgid:a", "Ana", true, false),
            entry("8:orgid:b", "Bo", false, false),
        ]));
        assert_eq!(model.tiles.len(), 2);
        assert!(model.tiles.iter().find(|tile| tile.name == "Ana").unwrap().muted);
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:me", "Me", false, false), entry("8:orgid:b", "Bo", false, false)]));
        assert_eq!(model.tiles.len(), 1);
        assert_eq!(model.tiles[0].name, "Bo");
    }

    #[test]
    fn an_invited_callee_who_leaves_stays_as_left() {
        let mut model = CallModel::people("Bea", &[("8:orgid:b".into(), "Bea".into())]);
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:b", "Bea", false, false)]));
        model.apply(CallUpdate::Roster(Vec::new()));
        assert_eq!(model.tiles[0].state, TileState::Left);
        assert_eq!(model.tile_caption(&model.tiles[0]), Some("Left"));
    }

    #[test]
    fn lobby_guests_get_no_tile() {
        let mut model = meeting_with_me();
        model.apply(CallUpdate::Roster(vec![
            entry("8:orgid:a", "Ana", false, false),
            entry("8:orgid:g", "Guest", false, true),
        ]));
        let names: Vec<&str> = model.visible_tiles().iter().map(|tile| tile.name.as_str()).collect();
        assert_eq!(names, vec!["Ana"]);
    }

    #[test]
    fn the_lobby_flag_follows_the_call() {
        let mut model = meeting_with_me();
        model.apply(CallUpdate::Lobby(true));
        assert!(model.lobby);
        model.apply(CallUpdate::Lobby(false));
        assert!(!model.lobby);
    }

    #[test]
    fn speakers_light_their_own_tile_and_a_lone_remote_follows_the_level() {
        let mut model = meeting_with_me();
        model.apply(CallUpdate::Roster(vec![
            entry("8:orgid:a", "Ana", false, false),
            entry("8:orgid:b", "Bo", false, false),
        ]));
        model.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        model.apply(CallUpdate::Speakers(vec!["8:orgid:b".into()]));
        let tiles: Vec<Tile> = model.tiles.clone();
        assert!(!model.tile_speaking(&tiles[0]));
        assert!(model.tile_speaking(&tiles[1]));
        let mut lone = CallModel::people("Bea", &[("8:orgid:b".into(), "Bea".into())]);
        lone.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        lone.apply(CallUpdate::Levels { local: 0.0, remote: 0.5 });
        assert!(lone.tile_speaking(&lone.tiles[0].clone()));
    }

    #[test]
    fn more_than_nine_cells_collapse_into_a_plus_n_tile() {
        assert_eq!(grid_layout(0), GridLayout { shown_others: 0, hidden: 0 });
        assert_eq!(grid_layout(8), GridLayout { shown_others: 8, hidden: 0 });
        assert_eq!(grid_layout(9), GridLayout { shown_others: 7, hidden: 2 });
        assert_eq!(grid_layout(30), GridLayout { shown_others: 7, hidden: 23 });
    }

    #[test]
    fn crowded_calls_get_smaller_tiles() {
        assert_eq!(tile_size(2).avatar, 96.);
        assert_eq!(tile_size(4).avatar, 72.);
        assert_eq!(tile_size(9).avatar, 56.);
    }

    #[test]
    fn the_stage_label_names_the_sharer_and_the_strip_puts_the_speaker_first() {
        let mut model = meeting_with_me();
        model.apply(CallUpdate::Roster(vec![
            entry("8:orgid:a", "Ana", false, false),
            entry("8:orgid:b", "Bo", false, false),
        ]));
        assert_eq!(model.sharing_label(), None);
        model.apply(CallUpdate::ScreenShare(Some("8:orgid:b".into())));
        assert_eq!(model.sharing_label().as_deref(), Some("Bo is sharing"));
        model.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        model.apply(CallUpdate::Speakers(vec!["8:orgid:b".into()]));
        let names: Vec<&str> = model.strip_tiles().iter().map(|tile| tile.name.as_str()).collect();
        assert_eq!(names, vec!["Bo", "Ana"]);
        model.apply(CallUpdate::ScreenShare(None));
        assert_eq!(model.sharing_label(), None);
    }

    #[test]
    fn roster_entries_carry_the_camera_flag_to_the_tile() {
        let mut model = meeting_with_me();
        let mut with_camera = entry("8:orgid:a", "Ana", false, false);
        with_camera.has_video = true;
        model.apply(CallUpdate::Roster(vec![with_camera]));
        assert!(model.tiles[0].has_video);
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:a", "Ana", false, false)]));
        assert!(!model.tiles[0].has_video);
    }

    #[test]
    fn present_guests_sort_before_waiting_ones() {
        let mut model = CallModel::people(
            "Group",
            &[("8:orgid:b".into(), "Bea".into()), ("8:orgid:c".into(), "Cy".into())],
        );
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:c", "Cy", false, false)]));
        let names: Vec<&str> = model.visible_tiles().iter().map(|tile| tile.name.as_str()).collect();
        assert_eq!(names, vec!["Cy", "Bea"]);
    }
}
