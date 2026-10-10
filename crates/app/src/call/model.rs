use std::sync::Arc;
use std::time::{Duration, Instant};

use calling::{
    CallCommand, CallState, CallUpdate, CameraDevice, CaptionEntry, CaptionState, ContentShare, DeviceChoice, DeviceLists, EndKind, EndReason,
    HoldState, MeetingTarget, Progress, Reaction, RosterEntry, ShareKind, ShareSource, SpeakingDetector, VideoHub,
};
use tokio::sync::mpsc::UnboundedSender;

use super::background::BackgroundPick;
use super::pictures::CallPictures;

pub const TEST_CALL_TITLE: &str = "Test call";
pub const ECHO_NAME: &str = "Teams echo";
pub const ECHO_MRI: &str = calling::signaling::ECHO_BOT_MRI;
pub const MAX_TILES: usize = 9;
const ORGID_PREFIX: &str = "8:orgid:";
const WIDE_TILES: usize = 2;
const MEDIUM_TILES: usize = 4;
pub const REACTION_SHOWN: Duration = Duration::from_secs(3);
pub const CHAT_OPEN_TILES: usize = 6;
pub const CAPTION_SHOWN: Duration = Duration::from_secs(4);
const CAPTION_LINES: usize = 2;

pub struct ActiveCall {
    pub id: u64,
    pub model: CallModel,
    pub commands: UnboundedSender<CallCommand>,
    pub viewing: bool,
    pub conversation_id: Option<String>,
    pub video: Arc<VideoHub>,
    pub pictures: CallPictures,
    pub stage_fullscreen: bool,
    pub chat_open: bool,
    pub meeting_target: Option<MeetingTarget>,
    pub pending_move: Option<PendingMove>,
    pub consult: Option<ConsultCall>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingMove {
    pub target: MeetingTarget,
    pub title: String,
    pub breakout: Option<BreakoutState>,
}

pub struct ConsultCall {
    pub id: u64,
    pub target: (String, String),
    pub commands: UnboundedSender<CallCommand>,
    pub state: CallState,
    pub replacement: Option<String>,
    pub transferring: bool,
}

impl ConsultCall {
    pub fn connected(&self) -> bool {
        matches!(self.state, CallState::Connected { .. })
    }

    pub fn ready_to_transfer(&self) -> bool {
        self.connected() && self.replacement.is_some()
    }

    pub fn status_text(&self) -> String {
        let name = &self.target.1;
        match &self.state {
            CallState::Connected { .. } => format!("Consulting {name}"),
            _ => format!("Calling {name}..."),
        }
    }
}

impl ActiveCall {
    pub fn new(id: u64, model: CallModel, commands: UnboundedSender<CallCommand>, conversation_id: Option<String>, video: Arc<VideoHub>) -> Self {
        ActiveCall {
            id,
            model,
            commands,
            viewing: true,
            conversation_id,
            video,
            pictures: CallPictures::default(),
            stage_fullscreen: false,
            chat_open: false,
            meeting_target: None,
            pending_move: None,
            consult: None,
        }
    }

    pub fn chat_thread(&self) -> Option<&str> {
        chat_thread(self.model.kind, self.model.meeting_chat.as_deref(), self.conversation_id.as_deref())
    }
}

pub fn chat_thread<'a>(kind: CallKind, meeting_chat: Option<&'a str>, conversation_id: Option<&'a str>) -> Option<&'a str> {
    match kind {
        CallKind::Meeting => meeting_chat.or(conversation_id),
        CallKind::Test | CallKind::Direct | CallKind::Group => conversation_id,
    }
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
    pub hand: Option<u64>,
    pub spotlight: Option<u64>,
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
            hand: None,
            spotlight: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BreakoutState {
    pub room_name: String,
    pub main: Option<MeetingTarget>,
    pub main_title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Focus {
    pub mri: String,
    pub own: bool,
    pub spotlight: bool,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionLine {
    pub id: String,
    pub speaker: String,
    pub text: String,
    pub at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridLayout {
    pub shown_others: usize,
    pub hidden: usize,
}

pub fn grid_layout(others: usize, max_tiles: usize) -> GridLayout {
    let total = others + 1;
    if total <= max_tiles {
        return GridLayout {
            shown_others: others,
            hidden: 0,
        };
    }
    let shown_others = max_tiles - 2;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionChip {
    pub mri: String,
    pub reaction: Reaction,
    pub since: Instant,
    pub serial: u64,
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
    pub share_sound: bool,
    pub own_hand: Option<u64>,
    pub own_spotlight: Option<u64>,
    pub organizer: bool,
    pub pinned: Option<String>,
    pub captions: CaptionState,
    pub caption_lines: Vec<CaptionLine>,
    pub background: BackgroundPick,
    pub recording: bool,
    pub consent_required: bool,
    pub hold: HoldState,
    pub whiteboard: Option<ContentShare>,
    pub breakout: Option<BreakoutState>,
    pub main_meeting: Option<MeetingTarget>,
    pub reactions: Vec<ReactionChip>,
    reaction_serial: u64,
    pub meeting_chat: Option<String>,
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
            share_sound: false,
            own_hand: None,
            own_spotlight: None,
            organizer: false,
            pinned: None,
            captions: CaptionState::Off,
            caption_lines: Vec::new(),
            background: BackgroundPick::None,
            recording: false,
            consent_required: false,
            hold: HoldState::Active,
            whiteboard: None,
            breakout: None,
            main_meeting: None,
            reactions: Vec::new(),
            reaction_serial: 0,
            meeting_chat: None,
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
            CallUpdate::LocalShare(label) => {
                if label.is_none() {
                    self.share_sound = false;
                }
                self.local_share = label;
            }
            CallUpdate::ShareSources(sources) => self.share_sources = sources,
            CallUpdate::ShareSound(active) => self.share_sound = active,
            CallUpdate::Reaction { mri, reaction } => self.note_reaction(mri, reaction, Instant::now()),
            CallUpdate::MeetingChat(thread) => self.meeting_chat = Some(thread),
            CallUpdate::Captions(state) => {
                if state != CaptionState::On {
                    self.caption_lines.clear();
                }
                self.captions = state;
            }
            CallUpdate::Caption(entry) => self.note_caption(entry, Instant::now()),
            CallUpdate::Recording(on) => self.recording = on,
            CallUpdate::ConsentRequired(required) => self.consent_required = required,
            CallUpdate::Hold(hold) => self.hold = hold,
            CallUpdate::Whiteboard(share) => self.whiteboard = share,
            CallUpdate::BreakoutRoom { main } => self.main_meeting = Some(main),
            CallUpdate::BlurTiming(_)
            | CallUpdate::Organizer { .. }
            | CallUpdate::WhiteboardUrl(_)
            | CallUpdate::BreakoutMove(_)
            | CallUpdate::ReplacementLink(_) => {}
        }
    }

    pub fn note_caption(&mut self, entry: CaptionEntry, at: Instant) {
        if self.captions == CaptionState::Off {
            return;
        }
        let speaker = if entry.display_name.is_empty() { "Someone".to_owned() } else { entry.display_name };
        let line = CaptionLine { id: entry.id, speaker, text: entry.text, at };
        match self.caption_lines.last_mut() {
            Some(last) if last.id == line.id => *last = line,
            _ => self.caption_lines.push(line),
        }
        let surplus = self.caption_lines.len().saturating_sub(CAPTION_LINES);
        self.caption_lines.drain(..surplus);
    }

    pub fn visible_captions(&self, now: Instant) -> Vec<&CaptionLine> {
        self.caption_lines.iter().filter(|line| now.saturating_duration_since(line.at) < CAPTION_SHOWN).collect()
    }

    pub fn captions_on(&self) -> bool {
        matches!(self.captions, CaptionState::Starting | CaptionState::On)
    }

    fn silence(&mut self) {
        self.local_speaking = false;
        self.remote_speaking = false;
        self.speaking_mris.clear();
        self.local_detector.reset();
        self.remote_detector.reset();
    }

    pub fn note_reaction(&mut self, mri: String, reaction: Reaction, at: Instant) {
        self.reactions.retain(|chip| chip.mri != mri && at.saturating_duration_since(chip.since) < REACTION_SHOWN);
        self.reaction_serial += 1;
        self.reactions.push(ReactionChip { mri, reaction, since: at, serial: self.reaction_serial });
    }

    pub fn chip_of(&self, mri: &str, now: Instant) -> Option<&ReactionChip> {
        self.reactions
            .iter()
            .find(|chip| chip.mri == mri && now.saturating_duration_since(chip.since) < REACTION_SHOWN)
    }

    pub fn own_chip(&self, now: Instant) -> Option<&ReactionChip> {
        self.chip_of(self.own_mri.as_deref()?, now)
    }

    pub fn hand_position(&self, rank: u64) -> usize {
        let raised = self.tiles.iter().filter_map(|tile| tile.hand).chain(self.own_hand);
        1 + raised.filter(|other| *other < rank).count()
    }

    pub fn can_manage(&self) -> bool {
        self.can_end_meeting || self.organizer
    }

    pub fn organizes_meeting(&self) -> bool {
        self.kind == CallKind::Meeting && self.can_manage()
    }

    pub fn can_lower_hands(&self) -> bool {
        self.can_manage()
    }

    pub fn lobby_guests(&self) -> Vec<&Tile> {
        self.tiles.iter().filter(|tile| tile.state == TileState::InLobby).collect()
    }

    pub fn can_admit(&self) -> bool {
        self.can_manage() && self.is_live() && !self.lobby
    }

    pub fn is_pinned(&self, mri: &str) -> bool {
        self.pinned.as_deref() == Some(mri)
    }

    pub fn toggle_pin(&mut self, mri: &str) {
        self.pinned = if self.is_pinned(mri) { None } else { Some(mri.to_owned()) };
    }

    pub fn is_spotlighted(&self, mri: &str) -> bool {
        self.own_mri.as_deref() == Some(mri) && self.own_spotlight.is_some()
            || self.tiles.iter().any(|tile| tile.mri == mri && tile.spotlight.is_some())
    }

    pub fn focus(&self) -> Option<Focus> {
        let own = |mri: &str| self.own_mri.as_deref() == Some(mri);
        let on_stage = |mri: &str| own(mri) || self.visible_tiles().iter().any(|tile| tile.mri == mri && tile.state == TileState::Present);
        if let Some(pinned) = self.pinned.as_deref().filter(|mri| on_stage(mri)) {
            return Some(Focus { mri: pinned.to_owned(), own: own(pinned), spotlight: self.is_spotlighted(pinned), pinned: true });
        }
        let remote = self.tiles.iter().filter(|tile| tile.state == TileState::Present).filter_map(|tile| Some((tile.spotlight?, tile.mri.as_str())));
        let (_, mri) = remote.chain(self.own_spotlight.zip(self.own_mri.as_deref())).min_by_key(|(rank, _)| *rank)?;
        Some(Focus { mri: mri.to_owned(), own: own(mri), spotlight: true, pinned: false })
    }

    pub fn any_hand_raised(&self) -> bool {
        self.own_hand.is_some() || self.tiles.iter().any(|tile| tile.hand.is_some())
    }

    pub fn own_cell_index(&self, remote_cells: usize) -> usize {
        let Some(own) = self.own_hand else { return remote_cells };
        let ahead = self.visible_tiles().iter().filter(|tile| tile.hand.is_some_and(|rank| rank < own)).count();
        ahead.min(remote_cells)
    }

    fn apply_roster(&mut self, entries: &[RosterEntry]) {
        let own_entry = entries.iter().find(|entry| Some(&entry.mri) == self.own_mri.as_ref());
        self.own_hand = own_entry.and_then(|entry| entry.hand.as_ref().map(|hand| hand.rank));
        self.own_spotlight = own_entry.and_then(|entry| entry.spotlight.as_ref().map(|spotlight| spotlight.rank));
        if let Some(entry) = own_entry {
            self.organizer = entry.organizer;
        }
        for entry in entries.iter().filter(|entry| Some(&entry.mri) != self.own_mri.as_ref()) {
            let state = if entry.in_lobby { TileState::InLobby } else { TileState::Present };
            match self.tiles.iter_mut().find(|tile| tile.mri == entry.mri) {
                Some(tile) => {
                    tile.state = state;
                    tile.muted = entry.muted;
                    tile.has_video = entry.has_video;
                    tile.hand = entry.hand.as_ref().map(|hand| hand.rank);
                    tile.spotlight = entry.spotlight.as_ref().map(|spotlight| spotlight.rank);
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
                    hand: entry.hand.as_ref().map(|hand| hand.rank),
                    spotlight: entry.spotlight.as_ref().map(|spotlight| spotlight.rank),
                }),
            }
        }
        let own_mri = self.own_mri.clone();
        let listed = |mri: &str| entries.iter().any(|entry| entry.mri == mri);
        if self.pinned.as_deref().is_some_and(|mri| !listed(mri) && Some(mri) != own_mri.as_deref()) {
            self.pinned = None;
        }
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
        tiles.sort_by_key(|tile| {
            let state = match tile.state {
                TileState::Present => 0,
                TileState::Invited => 1,
                _ => 2,
            };
            (!self.is_pinned(&tile.mri), tile.hand.is_none(), tile.hand.unwrap_or(0), state)
        });
        tiles
    }

    pub fn strip_tiles(&self) -> Vec<&Tile> {
        let mut tiles = self.visible_tiles();
        tiles.sort_by_key(|tile| (!self.is_pinned(&tile.mri), tile.hand.is_none(), tile.hand.unwrap_or(0), !self.tile_speaking(tile)));
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

    pub fn whiteboard_label(&self) -> Option<String> {
        let share = self.whiteboard.as_ref()?;
        let name = share
            .presenter
            .as_deref()
            .and_then(|presenter| self.tiles.iter().find(|tile| tile.mri == presenter))
            .map(|tile| tile.name.as_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("Someone");
        Some(format!("{name} is sharing a whiteboard"))
    }

    pub fn whiteboard_url(&self) -> Option<&str> {
        self.whiteboard.as_ref()?.url.as_deref()
    }

    pub fn is_one_to_one(&self) -> bool {
        self.kind == CallKind::Direct
    }

    pub fn can_hold(&self) -> bool {
        self.is_one_to_one() && self.is_live() && self.hold != HoldState::Remote
    }

    pub fn can_transfer(&self) -> bool {
        self.is_one_to_one() && self.is_live() && self.hold == HoldState::Active
    }

    pub fn can_record(&self) -> bool {
        self.organizes_meeting() && self.is_live() && !self.lobby
    }

    pub fn can_open_whiteboard(&self) -> bool {
        self.kind == CallKind::Meeting && self.is_live() && !self.lobby
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

    pub fn can_react(&self) -> bool {
        self.is_live() && !self.lobby
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
            hand: None,
            spotlight: None,
            organizer: false,
        }
    }

    fn spotlit(mri: &str, name: &str, rank: u64) -> RosterEntry {
        RosterEntry {
            spotlight: Some(calling::Spotlight { state_id: format!("sp-{rank}"), rank }),
            ..entry(mri, name, false, false)
        }
    }

    fn organizer_entry(mri: &str) -> RosterEntry {
        RosterEntry { organizer: true, ..entry(mri, "Me", false, false) }
    }

    fn raised(mri: &str, name: &str, rank: u64) -> RosterEntry {
        RosterEntry {
            hand: Some(calling::RaisedHand { state_id: format!("s-{rank}"), rank }),
            ..entry(mri, name, false, false)
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
        assert_eq!(grid_layout(0, MAX_TILES), GridLayout { shown_others: 0, hidden: 0 });
        assert_eq!(grid_layout(8, MAX_TILES), GridLayout { shown_others: 8, hidden: 0 });
        assert_eq!(grid_layout(9, MAX_TILES), GridLayout { shown_others: 7, hidden: 2 });
        assert_eq!(grid_layout(30, MAX_TILES), GridLayout { shown_others: 7, hidden: 23 });
    }

    #[test]
    fn an_open_chat_panel_leaves_room_for_fewer_tiles() {
        assert_eq!(grid_layout(5, CHAT_OPEN_TILES), GridLayout { shown_others: 5, hidden: 0 });
        assert_eq!(grid_layout(6, CHAT_OPEN_TILES), GridLayout { shown_others: 4, hidden: 2 });
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

    #[test]
    fn raised_hands_get_queue_positions_by_raise_time_and_sort_first() {
        let mut model = meeting_with_me();
        model.apply(CallUpdate::Roster(vec![
            entry("8:orgid:a", "Ana", false, false),
            raised("8:orgid:b", "Bo", 9),
            raised("8:orgid:c", "Cy", 4),
            raised("8:orgid:me", "Me", 6),
        ]));
        assert_eq!(model.own_hand, Some(6));
        assert!(model.any_hand_raised());
        let names: Vec<&str> = model.visible_tiles().iter().map(|tile| tile.name.as_str()).collect();
        assert_eq!(names, vec!["Cy", "Bo", "Ana"]);
        let positions: Vec<usize> = [4, 6, 9].into_iter().map(|rank| model.hand_position(rank)).collect();
        assert_eq!(positions, vec![1, 2, 3]);
        assert_eq!(model.own_cell_index(3), 1);
        let strip: Vec<&str> = model.strip_tiles().iter().map(|tile| tile.name.as_str()).collect();
        assert_eq!(strip, vec!["Cy", "Bo", "Ana"]);
    }

    #[test]
    fn lowering_clears_the_hand_everywhere() {
        let mut model = meeting_with_me();
        model.apply(CallUpdate::Roster(vec![raised("8:orgid:a", "Ana", 1), raised("8:orgid:me", "Me", 2)]));
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:a", "Ana", false, false), entry("8:orgid:me", "Me", false, false)]));
        assert!(!model.any_hand_raised());
        assert_eq!(model.own_cell_index(1), 1);
    }

    #[test]
    fn only_organizers_may_lower_hands() {
        assert!(!CallModel::meeting("Standup", false).can_lower_hands());
        assert!(CallModel::meeting("Standup", true).can_lower_hands());
    }

    #[test]
    fn a_reaction_shows_for_three_seconds_on_its_sender_and_the_latest_wins() {
        let mut model = meeting_with_me();
        let start = Instant::now();
        model.note_reaction("8:orgid:a".into(), Reaction::Heart, start);
        model.note_reaction("8:orgid:me".into(), Reaction::Like, start);
        let reaction_at = |model: &CallModel, mri: &str, seconds: u64| {
            model.chip_of(mri, start + Duration::from_secs(seconds)).map(|chip| chip.reaction)
        };
        assert_eq!(reaction_at(&model, "8:orgid:a", 2), Some(Reaction::Heart));
        assert_eq!(model.own_chip(start + Duration::from_secs(2)).map(|chip| chip.reaction), Some(Reaction::Like));
        assert_eq!(reaction_at(&model, "8:orgid:a", 3), None);
        model.note_reaction("8:orgid:a".into(), Reaction::Laugh, start + Duration::from_secs(1));
        assert_eq!(reaction_at(&model, "8:orgid:a", 2), Some(Reaction::Laugh));
        assert_eq!(model.reactions.len(), 2);
    }

    #[test]
    fn the_chat_panel_picks_the_meeting_thread_or_the_call_chat() {
        assert_eq!(chat_thread(CallKind::Meeting, Some("19:meeting_x"), Some("19:list")), Some("19:meeting_x"));
        assert_eq!(chat_thread(CallKind::Meeting, None, Some("19:list")), Some("19:list"));
        assert_eq!(chat_thread(CallKind::Direct, Some("19:meeting_x"), Some("19:dm")), Some("19:dm"));
        assert_eq!(chat_thread(CallKind::Group, None, Some("19:group")), Some("19:group"));
        assert_eq!(chat_thread(CallKind::Test, None, None), None);
    }

    #[test]
    fn computer_sound_follows_the_share() {
        let mut model = connected_model();
        model.apply(CallUpdate::LocalShare(Some("Screen".into())));
        model.apply(CallUpdate::ShareSound(true));
        assert!(model.share_sound);
        model.apply(CallUpdate::LocalShare(None));
        assert!(!model.share_sound);
    }

    #[test]
    fn lobby_guests_are_listed_for_organizers_who_may_admit() {
        let mut model = meeting_with_me();
        model.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        model.apply(CallUpdate::Roster(vec![
            entry("8:orgid:a", "Ana", false, false),
            entry("8:orgid:g1", "Gast Eins", false, true),
            entry("8:orgid:g2", "Gast Zwei", false, true),
        ]));
        let waiting: Vec<&str> = model.lobby_guests().iter().map(|tile| tile.name.as_str()).collect();
        assert_eq!(waiting, vec!["Gast Eins", "Gast Zwei"]);
        assert!(!model.can_admit());
        model.apply(CallUpdate::Roster(vec![organizer_entry("8:orgid:me"), entry("8:orgid:g1", "Gast Eins", false, true)]));
        assert!(model.organizer && model.can_admit() && model.organizes_meeting());
        let mut owner = CallModel::meeting("Standup", true);
        owner.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        assert!(owner.can_admit());
        owner.apply(CallUpdate::Lobby(true));
        assert!(!owner.can_admit());
    }

    #[test]
    fn a_spotlight_takes_the_stage_and_the_lowest_rank_wins() {
        let mut model = meeting_with_me();
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:a", "Ana", false, false), spotlit("8:orgid:b", "Bo", 4), spotlit("8:orgid:c", "Cy", 2)]));
        assert_eq!(model.focus(), Some(Focus { mri: "8:orgid:c".into(), own: false, spotlight: true, pinned: false }));
        assert!(model.is_spotlighted("8:orgid:b") && !model.is_spotlighted("8:orgid:a"));
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:a", "Ana", false, false), spotlit("8:orgid:b", "Bo", 4), entry("8:orgid:c", "Cy", false, false)]));
        assert_eq!(model.focus().map(|focus| focus.mri), Some("8:orgid:b".to_owned()));
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:a", "Ana", false, false)]));
        assert_eq!(model.focus(), None);
    }

    #[test]
    fn spotlighting_yourself_puts_your_own_tile_on_the_stage() {
        let mut model = meeting_with_me();
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:a", "Ana", false, false), spotlit("8:orgid:me", "Me", 1)]));
        assert_eq!(model.focus(), Some(Focus { mri: "8:orgid:me".into(), own: true, spotlight: true, pinned: false }));
    }

    #[test]
    fn a_pin_is_local_beats_the_spotlight_and_ends_when_the_person_leaves() {
        let mut model = meeting_with_me();
        model.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:a", "Ana", false, false), spotlit("8:orgid:b", "Bo", 1)]));
        model.toggle_pin("8:orgid:a");
        assert_eq!(model.focus(), Some(Focus { mri: "8:orgid:a".into(), own: false, spotlight: false, pinned: true }));
        let names: Vec<&str> = model.strip_tiles().iter().map(|tile| tile.name.as_str()).collect();
        assert_eq!(names[0], "Ana");
        model.toggle_pin("8:orgid:b");
        assert_eq!(model.focus().map(|focus| (focus.mri, focus.spotlight, focus.pinned)), Some(("8:orgid:b".to_owned(), true, true)));
        model.toggle_pin("8:orgid:b");
        assert_eq!(model.pinned, None);
        model.toggle_pin("8:orgid:a");
        model.apply(CallUpdate::Roster(vec![spotlit("8:orgid:b", "Bo", 1)]));
        assert_eq!(model.pinned, None);
    }

    fn caption(id: &str, speaker: &str, text: &str, is_final: bool) -> CaptionEntry {
        CaptionEntry { id: id.into(), user_id: "u".into(), display_name: speaker.into(), text: text.into(), is_final }
    }

    #[test]
    fn captions_keep_two_lines_update_partials_and_fade_after_four_seconds() {
        let mut model = meeting_with_me();
        let start = Instant::now();
        model.note_caption(caption("1", "Ana", "ignored while off", false), start);
        assert!(model.caption_lines.is_empty());
        model.apply(CallUpdate::Captions(CaptionState::On));
        model.note_caption(caption("1", "Ana", "Hello", false), start);
        model.note_caption(caption("1", "Ana", "Hello there", true), start);
        assert_eq!(model.caption_lines.len(), 1);
        assert_eq!(model.caption_lines[0].text, "Hello there");
        model.note_caption(caption("2", "", "Next", false), start + Duration::from_secs(1));
        model.note_caption(caption("3", "Bo", "Third", false), start + Duration::from_secs(2));
        let texts: Vec<&str> = model.caption_lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(texts, vec!["Next", "Third"]);
        assert_eq!(model.caption_lines[0].speaker, "Someone");
        assert_eq!(model.visible_captions(start + Duration::from_secs(3)).len(), 2);
        assert_eq!(model.visible_captions(start + Duration::from_millis(5500)).len(), 1);
        assert!(model.visible_captions(start + Duration::from_secs(7)).is_empty());
        model.apply(CallUpdate::Captions(CaptionState::Off));
        assert!(model.caption_lines.is_empty() && !model.captions_on());
    }

    fn one_to_one() -> CallModel {
        let mut model = CallModel::people("Bea", &[("8:orgid:bea".into(), "Bea".into())]);
        model.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        model
    }

    #[test]
    fn hold_and_transfer_belong_to_one_to_one_calls_and_follow_the_hold_state() {
        let mut model = one_to_one();
        assert!(model.can_hold() && model.can_transfer());
        model.apply(CallUpdate::Hold(HoldState::Local));
        assert!(model.can_hold() && !model.can_transfer());
        model.apply(CallUpdate::Hold(HoldState::Remote));
        assert!(!model.can_hold() && !model.can_transfer());
        let meeting = meeting_with_me();
        assert!(!meeting.can_hold() && !meeting.can_transfer());
        assert!(!CallModel::people("Group", &[("8:orgid:a".into(), "A".into()), ("8:orgid:b".into(), "B".into())]).can_hold());
    }

    #[test]
    fn recording_and_consent_follow_the_call_updates_and_only_organizers_may_record() {
        let mut model = meeting_with_me();
        model.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        assert!(!model.can_record());
        model.apply(CallUpdate::Roster(vec![organizer_entry("8:orgid:me")]));
        assert!(model.can_record());
        model.apply(CallUpdate::Recording(true));
        model.apply(CallUpdate::ConsentRequired(true));
        assert!(model.recording && model.consent_required);
        model.apply(CallUpdate::ConsentRequired(false));
        assert!(model.recording && !model.consent_required);
        model.apply(CallUpdate::Recording(false));
        assert!(!model.recording);
    }

    #[test]
    fn a_whiteboard_share_names_the_presenter_and_offers_the_browser_only_with_a_link() {
        let mut model = meeting_with_me();
        model.apply(CallUpdate::Roster(vec![entry("8:orgid:ana", "Ana", false, false)]));
        assert_eq!(model.whiteboard_label(), None);
        let share = |presenter: &str, url: Option<&str>| calling::ContentShare {
            session_id: "s".into(),
            presenter: Some(presenter.into()),
            subject: "Whiteboard".into(),
            url: url.map(str::to_owned),
            whiteboard: true,
        };
        model.apply(CallUpdate::Whiteboard(Some(share("8:orgid:ana", Some("https://app.whiteboard.microsoft.com/x")))));
        assert_eq!(model.whiteboard_label().as_deref(), Some("Ana is sharing a whiteboard"));
        assert_eq!(model.whiteboard_url(), Some("https://app.whiteboard.microsoft.com/x"));
        model.apply(CallUpdate::Whiteboard(Some(share("8:orgid:stranger", None))));
        assert_eq!(model.whiteboard_label().as_deref(), Some("Someone is sharing a whiteboard"));
        assert_eq!(model.whiteboard_url(), None);
        model.apply(CallUpdate::Whiteboard(None));
        assert_eq!(model.whiteboard_label(), None);
    }

    #[test]
    fn a_consultation_can_transfer_once_connected_with_a_replacement_link() {
        let (handle, _control) = calling::call_channel();
        let mut consult = ConsultCall {
            id: 3,
            target: ("8:orgid:ana".into(), "Ana".into()),
            commands: handle.commands,
            state: CallState::Connecting,
            replacement: None,
            transferring: false,
        };
        assert_eq!(consult.status_text(), "Calling Ana...");
        assert!(!consult.ready_to_transfer());
        consult.state = CallState::Connected { since: Instant::now() };
        assert_eq!(consult.status_text(), "Consulting Ana");
        assert!(!consult.ready_to_transfer());
        consult.replacement = Some("https://cc/replacement".into());
        assert!(consult.ready_to_transfer());
    }
}
