use std::time::{Duration, Instant};

use calling::video_frame::bgra_from_i420;
use calling::video_pattern::{PatternKind, pattern_frame};
use calling::{
    AudioDevice, BackgroundChoice, BreakoutMove, CallCommand, CallControl, CallHandle, CallState, CallUpdate, Caller, CaptionEntry, CaptionState,
    ContentShare, DeviceChoice, DeviceLists, EndReason, HoldState, IncomingRing, MeetingTarget, Progress, PublishedState, RaisedHand, Reaction, RosterEntry,
    ShareKind, ShareSource, VideoKey, call_channel,
};
use calling::CameraDevice;

use super::demo_scene::self_view;
use tokio::time::{interval, sleep};

use super::model::{ECHO_MRI, ECHO_NAME};

const CONNECT_DELAY: Duration = Duration::from_millis(2200);
const RING_START: Duration = Duration::from_millis(1400);
const ANSWER_DELAY: Duration = Duration::from_millis(3200);
const INCOMING_CONNECT_DELAY: Duration = Duration::from_millis(600);
const LOBBY_WAIT: Duration = Duration::from_secs(6);
const LEVEL_PERIOD: Duration = Duration::from_millis(200);
const CYCLE_TICKS: u32 = 40;
const REMOTE_SPEAKS: std::ops::Range<u32> = 0..18;
const LOCAL_SPEAKS: std::ops::Range<u32> = 24..32;
const SPEAKING_LEVEL: f32 = 0.3;
const SPEAKER_TICKS: u32 = 10;
const VIDEO_PERIOD: Duration = Duration::from_millis(125);
const DEMO_CAMERAS: usize = 4;
const SCREEN_EVERY_TICKS: u32 = 4;
const SHARE_START: Duration = Duration::from_secs(10);
const SHARE_END: Duration = Duration::from_secs(45);
const REACTION_EVERY_TICKS: u32 = 20;
const DEMO_FIRST_HANDS: [(usize, u64); 2] = [(2, 1), (0, 2)];
const DEMO_WAITING: [&str; 3] = ["Anja Vogel", "Matteo Conti", "Hannah Weiss"];
const CAPTION_WORDS_PER_TICK: usize = 2;
const CAPTION_HOLD_TICKS: usize = 14;
const DEMO_CAPTIONS: [(usize, &str); 4] = [
    (0, "Thanks everyone for joining, let us start with the release status."),
    (1, "The build is green and the new call controls are ready for review."),
    (2, "Can we admit the two people from the lobby before we begin?"),
    (0, "Yes, I am letting them in now and we will share the plan on screen."),
];
const GUEST_PHASE: u32 = 37;
const SELF_PHASE: u32 = 211;
pub const DEMO_OWN_MRI: &str = "8:orgid:demo-me";
pub const DEMO_RING_ID: u64 = 9001;
const DEMO_CALLER: (&str, &str) = ("8:orgid:demo-mara", "Mara Lindqvist");
const DEMO_GUESTS: [&str; 11] = [
    "Mara Lindqvist",
    "Jonas Ortega",
    "Priya Nair",
    "Lea Schneider",
    "Tobias Klein",
    "Ines Duarte",
    "Noor Haddad",
    "Felix Brandt",
    "Yuki Tanaka",
    "Carlos Mendez",
    "Sofia Rossi",
];

pub type Person = (String, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DemoScene {
    #[default]
    Plain,
    Call,
    Recording,
    Consent,
    Hold,
    Held,
    Breakout,
    Whiteboard,
}

impl DemoScene {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.trim() {
            "call" => DemoScene::Call,
            "recording" => DemoScene::Recording,
            "consent" => DemoScene::Consent,
            "hold" => DemoScene::Hold,
            "held" => DemoScene::Held,
            "breakout" => DemoScene::Breakout,
            "whiteboard" => DemoScene::Whiteboard,
            _ => return None,
        })
    }

    pub fn is_one_to_one(self) -> bool {
        matches!(self, DemoScene::Call | DemoScene::Hold | DemoScene::Held)
    }
}

const BREAKOUT_AT_TICKS: u32 = 20;
const WHITEBOARD_AT_TICKS: u32 = 5;
const DEMO_BOARD_URL: &str = "https://app.whiteboard.microsoft.com/me/whiteboards/demo";
const DEMO_REPLACEMENT_LINK: &str = "https://demo.invalid/replacement";
const DEMO_ROOM_THREAD: &str = "19:meeting_demo-room-1@thread.v2";

pub enum DemoCall {
    Test,
    People(Vec<Person>),
    Meeting { guests: Vec<Person>, lobby: bool, organizer: bool },
    Incoming(Person),
}

pub fn demo_guests(count: usize) -> Vec<Person> {
    DEMO_GUESTS
        .iter()
        .take(count)
        .enumerate()
        .map(|(index, name)| (format!("8:orgid:demo-guest-{index}"), (*name).to_owned()))
        .collect()
}

pub fn demo_caller() -> Person {
    (DEMO_CALLER.0.to_owned(), DEMO_CALLER.1.to_owned())
}

pub fn demo_video_ring() -> IncomingRing {
    IncomingRing { video: true, ..demo_ring() }
}

pub fn demo_ring() -> IncomingRing {
    let (mri, display_name) = demo_caller();
    IncomingRing {
        ring_id: DEMO_RING_ID,
        caller: Caller { mri, display_name },
        thread_id: Some("demo-chat-mara".to_owned()),
        video: false,
        is_group: false,
        subject: None,
    }
}

fn device(index: u16, name: &str) -> AudioDevice {
    AudioDevice {
        index,
        guid: format!("demo-{index}"),
        name: name.to_owned(),
    }
}

fn demo_cameras() -> Vec<CameraDevice> {
    vec![
        CameraDevice { key: "0".into(), name: "Integrated Camera".into() },
        CameraDevice { key: "1".into(), name: "Studio Webcam".into() },
    ]
}

fn demo_share_sources() -> Vec<ShareSource> {
    let window = |id: u64, title: &str| ShareSource { id, kind: ShareKind::Window, title: title.to_owned() };
    vec![
        ShareSource { id: 1, kind: ShareKind::Screen, title: String::new() },
        window(2, "Release notes - Docs"),
        window(3, "Standup board"),
        window(4, "Terminal"),
    ]
}

fn demo_devices() -> DeviceLists {
    DeviceLists {
        inputs: vec![device(0, "Headset Microphone"), device(1, "Built-in Microphone")],
        outputs: vec![device(0, "Headset"), device(1, "Speakers")],
    }
}

pub fn start_demo_call(script: DemoCall) -> CallHandle {
    start_demo_scene_call(script, DemoScene::Plain)
}

pub fn start_demo_scene_call(script: DemoCall, scene: DemoScene) -> CallHandle {
    let (handle, control) = call_channel();
    crate::runtime::handle().spawn(run_demo_call(control, script, scene));
    handle
}

fn roster_of(people: &[Person]) -> Vec<RosterEntry> {
    people
        .iter()
        .enumerate()
        .map(|(index, (mri, name))| RosterEntry {
            mri: mri.clone(),
            display_name: name.clone(),
            muted: index % 3 == 1,
            in_lobby: false,
            has_video: index < DEMO_CAMERAS,
            sharing: false,
            hand: None,
            spotlight: None,
            organizer: false,
        })
        .collect()
}

async fn run_demo_call(mut control: CallControl, script: DemoCall, scene: DemoScene) {
    control.send(CallUpdate::OwnIdentity {
        mri: DEMO_OWN_MRI.to_owned(),
    });
    let (people, lobby) = match &script {
        DemoCall::Test => (vec![(ECHO_MRI.to_owned(), ECHO_NAME.to_owned())], false),
        DemoCall::People(callees) => (callees.clone(), false),
        DemoCall::Meeting { guests, lobby, .. } => (guests.clone(), *lobby),
        DemoCall::Incoming(caller) => (vec![caller.clone()], false),
    };
    if !ring_until_answered(&mut control, &script).await {
        control.send(CallUpdate::State(CallState::Ended {
            reason: EndReason::Cancelled,
        }));
        return;
    }
    control.send(CallUpdate::ListenOnly(false));
    control.send(CallUpdate::Devices(demo_devices()));
    control.send(CallUpdate::Cameras(demo_cameras()));
    control.send(CallUpdate::ShareSources(demo_share_sources()));
    control.send(CallUpdate::Selected {
        input: DeviceChoice::SystemDefault,
        output: DeviceChoice::SystemDefault,
    });
    control.send(CallUpdate::State(CallState::Connected {
        since: Instant::now(),
    }));
    if lobby {
        control.send(CallUpdate::Lobby(true));
        if control.until_hangup(sleep(LOBBY_WAIT)).await.is_err() {
            end(&control, EndReason::LocalHangup);
            return;
        }
        control.send(CallUpdate::Lobby(false));
    }
    let mut roster = roster_of(&people);
    if matches!(script, DemoCall::People(_)) {
        control.send(CallUpdate::ReplacementLink(DEMO_REPLACEMENT_LINK.to_owned()));
    }
    if matches!(script, DemoCall::Meeting { organizer: true, .. }) {
        roster.push(demo_organizer());
        roster.extend(DEMO_WAITING.iter().enumerate().map(|(index, name)| demo_waiting(index, name)));
    }
    if matches!(script, DemoCall::Meeting { .. }) {
        for (index, rank) in DEMO_FIRST_HANDS {
            if let Some(entry) = roster.get_mut(index) {
                entry.hand = Some(demo_hand(rank));
            }
        }
    }
    control.send(CallUpdate::Roster(roster.clone()));
    let video = !matches!(script, DemoCall::Test);
    let sharer = matches!(script, DemoCall::Meeting { .. }).then(|| people.get(1).map(|person| person.0.clone())).flatten();
    start_scene(&control, scene);
    speak_until_hangup(control, people, roster, video, sharer, scene).await;
}

async fn ring_until_answered(control: &mut CallControl, script: &DemoCall) -> bool {
    let steps: Vec<(Option<Progress>, Duration)> = match script {
        DemoCall::People(_) => vec![
            (Some(Progress::Calling), RING_START),
            (Some(Progress::Ringing), ANSWER_DELAY - RING_START),
        ],
        DemoCall::Incoming(_) => vec![(None, INCOMING_CONNECT_DELAY)],
        DemoCall::Test | DemoCall::Meeting { .. } => vec![(None, CONNECT_DELAY)],
    };
    for (progress, wait) in steps {
        if let Some(progress) = progress {
            control.send(CallUpdate::Progress(progress));
        }
        if control.until_hangup(sleep(wait)).await.is_err() {
            return false;
        }
    }
    true
}

fn publish_demo_video(control: &CallControl, people: &[Person], tick: u32, sharing: bool, sharer: Option<&str>, camera: Option<bool>) {
    if let Some(blur) = camera {
        control.video.publish(VideoKey::LocalCamera, self_view((tick + SELF_PHASE) as usize, blur));
    }
    for (index, (mri, _)) in people.iter().take(DEMO_CAMERAS).enumerate() {
        let frame = pattern_frame(PatternKind::Camera, tick + index as u32 * GUEST_PHASE);
        control.video.publish(VideoKey::Person(mri.clone()), bgra_from_i420(frame));
    }
    if sharing && sharer.is_some() && tick.is_multiple_of(SCREEN_EVERY_TICKS) {
        control.video.publish(VideoKey::Screen, bgra_from_i420(pattern_frame(PatternKind::Screen, tick)));
    }
}

fn start_scene(control: &CallControl, scene: DemoScene) {
    match scene {
        DemoScene::Recording => control.send(CallUpdate::Recording(true)),
        DemoScene::Consent => {
            control.send(CallUpdate::Recording(true));
            control.send(CallUpdate::ConsentRequired(true));
        }
        DemoScene::Hold => control.send(CallUpdate::Hold(HoldState::Local)),
        DemoScene::Held => control.send(CallUpdate::Hold(HoldState::Remote)),
        DemoScene::Call | DemoScene::Breakout | DemoScene::Whiteboard | DemoScene::Plain => {}
    }
}

fn scene_tick(control: &CallControl, scene: DemoScene, ticks: u32, people: &[Person]) {
    match scene {
        DemoScene::Breakout if ticks == BREAKOUT_AT_TICKS => control.send(CallUpdate::BreakoutMove(BreakoutMove {
            room_name: "Room 1".to_owned(),
            target: MeetingTarget { thread_id: DEMO_ROOM_THREAD.to_owned(), tenant_id: "demo-tenant".to_owned(), organizer_id: "demo-me".to_owned(), meeting_data: None },
            returning: false,
        })),
        DemoScene::Whiteboard if ticks == WHITEBOARD_AT_TICKS => control.send(CallUpdate::Whiteboard(Some(ContentShare {
            session_id: "demo-whiteboard".to_owned(),
            presenter: people.first().map(|person| person.0.clone()),
            subject: "Whiteboard".to_owned(),
            url: Some(DEMO_BOARD_URL.to_owned()),
            whiteboard: true,
        }))),
        _ => {}
    }
}

async fn speak_until_hangup(mut control: CallControl, people: Vec<Person>, mut roster: Vec<RosterEntry>, video: bool, sharer: Option<String>, scene: DemoScene) {
    let started = Instant::now();
    let mut video_ticks = 0u32;
    let mut sharing = false;
    let mut camera_on = false;
    let mut blur = false;
    let mut captions_on = false;
    let mut caption_ticks = 0usize;
    let mut video_tick = interval(VIDEO_PERIOD);
    let mut muted = false;
    let mut input = DeviceChoice::SystemDefault;
    let mut output = DeviceChoice::SystemDefault;
    let mut ticks = 0u32;
    let mut share_sound = false;
    let mut tick = interval(LEVEL_PERIOD);
    loop {
        tokio::select! {
            _ = video_tick.tick(), if video || camera_on => {
                let elapsed = started.elapsed();
                let share_now = sharer.is_some() && (SHARE_START..SHARE_END).contains(&elapsed);
                if share_now != sharing {
                    sharing = share_now;
                    control.send(CallUpdate::ScreenShare(sharer.clone().filter(|_| sharing)));
                }
                publish_demo_video(&control, &people, video_ticks, sharing, sharer.as_deref(), camera_on.then_some(blur));
                video_ticks += 1;
            }
            _ = tick.tick() => {
                let phase = ticks % CYCLE_TICKS;
                let remote_speaks = REMOTE_SPEAKS.contains(&phase);
                control.send(CallUpdate::Levels {
                    local: if LOCAL_SPEAKS.contains(&phase) && !muted { SPEAKING_LEVEL } else { 0.0 },
                    remote: if remote_speaks { SPEAKING_LEVEL } else { 0.0 },
                });
                if people.len() > 1 && ticks.is_multiple_of(SPEAKER_TICKS) {
                    let speaker = &people[(ticks / SPEAKER_TICKS) as usize % people.len()];
                    control.send(CallUpdate::Speakers(vec![speaker.0.clone()]));
                }
                if ticks % REACTION_EVERY_TICKS == REACTION_EVERY_TICKS / 2
                    && let Some((mri, _)) = people.get((ticks / REACTION_EVERY_TICKS) as usize % people.len().max(1))
                {
                    let reaction = Reaction::ALL[(ticks / REACTION_EVERY_TICKS) as usize % Reaction::ALL.len()];
                    control.send(CallUpdate::Reaction { mri: mri.clone(), reaction });
                }
                if captions_on {
                    if let Some(entry) = demo_caption(&people, caption_ticks) {
                        control.send(CallUpdate::Caption(entry));
                    }
                    caption_ticks += 1;
                }
                scene_tick(&control, scene, ticks, &people);
                ticks += 1;
            }
            command = control.recv() => match command {
                Some(CallCommand::Mute(command)) => {
                    muted = command.target(muted);
                    control.send(CallUpdate::Muted(muted));
                }
                Some(CallCommand::SelectInput(choice)) => {
                    input = choice;
                    control.send(CallUpdate::Selected { input: input.clone(), output: output.clone() });
                }
                Some(CallCommand::SelectOutput(choice)) => {
                    output = choice;
                    control.send(CallUpdate::Selected { input: input.clone(), output: output.clone() });
                }
                Some(CallCommand::RefreshDevices) => control.send(CallUpdate::Devices(demo_devices())),
                Some(CallCommand::SetCamera(on)) => {
                    camera_on = on;
                    control.send(CallUpdate::Camera(on));
                }
                Some(CallCommand::SelectCamera(_)) => {}
                Some(CallCommand::StartShare(source)) => {
                    control.send(CallUpdate::LocalShare(Some(source.label())));
                    if share_sound {
                        control.send(CallUpdate::ShareSound(true));
                    }
                }
                Some(CallCommand::StopShare) => control.send(CallUpdate::LocalShare(None)),
                Some(CallCommand::SetShareSound(on)) => share_sound = on,
                Some(CallCommand::SetHand(raised)) => {
                    set_demo_hand(&mut roster, DEMO_OWN_MRI, raised);
                    control.send(CallUpdate::Roster(roster.clone()));
                }
                Some(CallCommand::LowerHand { mri }) => {
                    set_demo_hand(&mut roster, &mri, false);
                    control.send(CallUpdate::Roster(roster.clone()));
                }
                Some(CallCommand::LowerAllHands) => {
                    roster.iter_mut().for_each(|entry| entry.hand = None);
                    control.send(CallUpdate::Roster(roster.clone()));
                }
                Some(CallCommand::SendReaction(reaction)) => {
                    control.send(CallUpdate::Reaction { mri: DEMO_OWN_MRI.to_owned(), reaction });
                }
                Some(CallCommand::Admit { mri }) => {
                    roster.iter_mut().filter(|entry| entry.mri == mri).for_each(|entry| entry.in_lobby = false);
                    control.send(CallUpdate::Roster(roster.clone()));
                }
                Some(CallCommand::AdmitAll) => {
                    roster.iter_mut().for_each(|entry| entry.in_lobby = false);
                    control.send(CallUpdate::Roster(roster.clone()));
                }
                Some(CallCommand::Deny { mri } | CallCommand::RemoveParticipant { mri }) => {
                    roster.retain(|entry| entry.mri != mri);
                    control.send(CallUpdate::Roster(roster.clone()));
                }
                Some(CallCommand::MuteParticipant { mri }) => {
                    roster.iter_mut().filter(|entry| entry.mri == mri).for_each(|entry| entry.muted = true);
                    control.send(CallUpdate::Roster(roster.clone()));
                }
                Some(CallCommand::MuteAll) => {
                    roster.iter_mut().filter(|entry| entry.mri != DEMO_OWN_MRI).for_each(|entry| entry.muted = true);
                    control.send(CallUpdate::Roster(roster.clone()));
                }
                Some(CallCommand::Spotlight { mri }) => {
                    let rank = roster.iter().filter_map(|entry| entry.spotlight.as_ref().map(|state| state.rank)).max().unwrap_or(0) + 1;
                    if let Some(entry) = roster.iter_mut().find(|entry| entry.mri == mri) {
                        entry.spotlight = Some(demo_spotlight(rank));
                    }
                    control.send(CallUpdate::Roster(roster.clone()));
                }
                Some(CallCommand::StopSpotlight { mri }) => {
                    roster.iter_mut().filter(|entry| entry.mri == mri).for_each(|entry| entry.spotlight = None);
                    control.send(CallUpdate::Roster(roster.clone()));
                }
                Some(CallCommand::SetCaptions(on)) => {
                    captions_on = on;
                    caption_ticks = 0;
                    control.send(CallUpdate::Captions(if on { CaptionState::On } else { CaptionState::Off }));
                }
                Some(CallCommand::SetBackground(choice)) => blur = choice != BackgroundChoice::None,
                Some(CallCommand::SetRecording { on, .. }) => control.send(CallUpdate::Recording(on)),
                Some(CallCommand::ConsentToRecording) => control.send(CallUpdate::ConsentRequired(false)),
                Some(CallCommand::Hold(hold)) => control.send(CallUpdate::Hold(if hold { HoldState::Local } else { HoldState::Active })),
                Some(CallCommand::Transfer { .. }) => {
                    control.send(CallUpdate::Notice("Call transferred".to_owned()));
                    end(&control, EndReason::LocalHangup);
                    return;
                }
                Some(CallCommand::OpenWhiteboard { .. }) => control.send(CallUpdate::WhiteboardUrl(DEMO_BOARD_URL.to_owned())),
                Some(CallCommand::RefreshShareSources) => control.send(CallUpdate::ShareSources(demo_share_sources())),
                Some(CallCommand::Hangup | CallCommand::EndMeeting) | None => {
                    end(&control, EndReason::LocalHangup);
                    return;
                }
            },
        }
    }
}

fn demo_organizer() -> RosterEntry {
    RosterEntry {
        mri: DEMO_OWN_MRI.to_owned(),
        display_name: "You".to_owned(),
        muted: false,
        in_lobby: false,
        has_video: false,
        sharing: false,
        hand: None,
        spotlight: None,
        organizer: true,
    }
}

fn demo_waiting(index: usize, name: &str) -> RosterEntry {
    RosterEntry {
        mri: format!("8:orgid:demo-waiting-{index}"),
        display_name: name.to_owned(),
        muted: false,
        in_lobby: true,
        has_video: false,
        sharing: false,
        hand: None,
        spotlight: None,
        organizer: false,
    }
}

fn demo_spotlight(rank: u64) -> PublishedState {
    PublishedState { state_id: format!("demo-spotlight-{rank}"), rank }
}

fn demo_caption(people: &[Person], tick: usize) -> Option<CaptionEntry> {
    let line = tick / (CAPTION_HOLD_TICKS * 2) % DEMO_CAPTIONS.len();
    let within = tick % (CAPTION_HOLD_TICKS * 2);
    let (speaker, text) = DEMO_CAPTIONS[line];
    let words: Vec<&str> = text.split(' ').collect();
    let shown = ((within + 1) * CAPTION_WORDS_PER_TICK).min(words.len());
    let (mri, name) = people.get(speaker % people.len().max(1))?;
    Some(CaptionEntry {
        id: format!("demo-caption-{}", tick / (CAPTION_HOLD_TICKS * 2)),
        user_id: mri.clone(),
        display_name: name.clone(),
        text: words[..shown].join(" "),
        is_final: shown == words.len(),
    })
}

fn demo_hand(rank: u64) -> RaisedHand {
    RaisedHand { state_id: format!("demo-hand-{rank}"), rank }
}

fn set_demo_hand(roster: &mut Vec<RosterEntry>, mri: &str, raised: bool) {
    let next_rank = roster.iter().filter_map(|entry| entry.hand.as_ref().map(|hand| hand.rank)).max().unwrap_or(0) + 1;
    if !raised {
        if let Some(entry) = roster.iter_mut().find(|entry| entry.mri == mri) {
            entry.hand = None;
        }
        roster.retain(|entry| entry.mri != DEMO_OWN_MRI || entry.hand.is_some());
        return;
    }
    match roster.iter_mut().find(|entry| entry.mri == mri) {
        Some(entry) => entry.hand = Some(demo_hand(next_rank)),
        None => roster.push(RosterEntry {
            mri: mri.to_owned(),
            display_name: "You".to_owned(),
            muted: false,
            in_lobby: false,
            has_video: false,
            sharing: false,
            hand: Some(demo_hand(next_rank)),
            spotlight: None,
            organizer: false,
        }),
    }
}

fn end(control: &CallControl, reason: EndReason) {
    control.send(CallUpdate::State(CallState::Ended { reason }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_guests_have_distinct_mris_and_names() {
        let guests = demo_guests(11);
        assert_eq!(guests.len(), 11);
        let mut mris: Vec<&String> = guests.iter().map(|guest| &guest.0).collect();
        mris.sort();
        mris.dedup();
        assert_eq!(mris.len(), 11);
    }

    #[test]
    fn demo_hands_queue_in_order_and_lowering_drops_the_own_entry() {
        let mut roster = roster_of(&demo_guests(3));
        set_demo_hand(&mut roster, "8:orgid:demo-guest-1", true);
        set_demo_hand(&mut roster, DEMO_OWN_MRI, true);
        let ranks: Vec<u64> = roster.iter().filter_map(|entry| entry.hand.as_ref().map(|hand| hand.rank)).collect();
        assert_eq!(ranks, vec![1, 2]);
        set_demo_hand(&mut roster, DEMO_OWN_MRI, false);
        assert!(roster.iter().all(|entry| entry.mri != DEMO_OWN_MRI));
    }

    #[test]
    fn the_demo_ring_names_a_caller_and_a_chat() {
        let ring = demo_ring();
        assert_eq!(ring.caller.display_name, "Mara Lindqvist");
        assert_eq!(ring.thread_id.as_deref(), Some("demo-chat-mara"));
    }
}
