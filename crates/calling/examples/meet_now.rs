use std::path::PathBuf;
use std::time::{Duration, Instant};

use calling::meeting::fetch_live_meeting;
use calling::relay::ic3_scope;
use calling::signaling::{CHATSVC_REGION, fetch_self};
use calling::{
    BackgroundCache, BackgroundChoice, CallCommand, CallEngine, CallSpec, CallState, CallUpdate, CaptionState, EngineConfig, MeetingTarget, Reaction, ShareKind,
    ShareSource,
};
use chatsvc::InstanceNames;
use serde_json::{Value, json};
use session::{DEFAULT_ENDPOINT, Method, Request, Scope, Session, SPACES};

const CREATE_URL: &str =
    "https://teams.cloud.microsoft/api/mt/emea/beta/me/calendarEvents/privateMeeting/schedulingService/create";
const SUBJECT: &str = "Native client test";
const CONNECT_LIMIT: Duration = Duration::from_secs(45);
const LIVE_STATE_LIMIT: Duration = Duration::from_secs(30);
const WAIT_FILE_LIMIT: Duration = Duration::from_secs(300);
const END_LIMIT: Duration = Duration::from_secs(20);
const GONE_LIMIT: Duration = Duration::from_secs(90);
const POLL: Duration = Duration::from_secs(2);
const MEDIA_DELAY: Duration = Duration::from_secs(3);
const THREAD_PROPERTY_RETRIES: usize = 8;
const CAPTIONS_HOLD: Duration = Duration::from_secs(10);
const FEATURE_LIMIT: Duration = Duration::from_secs(90);
const VIDEO_ENV: &str = "CALLING_VIDEO";
const RECORD_HOLD: Duration = Duration::from_secs(10);
const WHITEBOARD_WAIT: Duration = Duration::from_secs(45);
const BACKGROUND_CACHE: &str = "calling-meet-now-backgrounds";

struct Arguments {
    resolve_by_id: bool,
    camera: bool,
    share: bool,
    extras: bool,
    spotlight_self: bool,
    captions: bool,
    mute_all: bool,
    blur: bool,
    record: bool,
    whiteboard_state: bool,
    background: bool,
    stop_media_after: Option<Duration>,
    hold: Duration,
    thread_out: Option<PathBuf>,
    wait_file: Option<PathBuf>,
}

fn arguments() -> Arguments {
    let mut parsed = Arguments {
        resolve_by_id: false,
        camera: false,
        share: false,
        extras: false,
        spotlight_self: false,
        captions: false,
        mute_all: false,
        blur: false,
        record: false,
        whiteboard_state: false,
        background: false,
        stop_media_after: None,
        hold: Duration::from_secs(10),
        thread_out: None,
        wait_file: None,
    };
    let mut input = std::env::args().skip(1);
    while let Some(flag) = input.next() {
        match flag.as_str() {
            "--camera" => parsed.camera = true,
            "--share" => parsed.share = true,
            "--extras" => parsed.extras = true,
            "--spotlight-self" => parsed.spotlight_self = true,
            "--captions" => parsed.captions = true,
            "--mute-all" => parsed.mute_all = true,
            "--blur" => {
                parsed.blur = true;
                parsed.camera = true;
            }
            "--record" => parsed.record = true,
            "--whiteboard-state" => parsed.whiteboard_state = true,
            "--resolve-by-id" => parsed.resolve_by_id = true,
            "--background" => {
                parsed.background = true;
                parsed.camera = true;
            }
            "--stop-media-after" => parsed.stop_media_after = input.next().and_then(|value| value.parse().ok()).map(Duration::from_secs),
            "--hold" => parsed.hold = Duration::from_secs(input.next().and_then(|value| value.parse().ok()).unwrap_or(10)),
            "--thread-out" => parsed.thread_out = input.next().map(PathBuf::from),
            "--wait-file" => parsed.wait_file = input.next().map(PathBuf::from),
            _ => {}
        }
    }
    parsed
}

fn as_object(value: &Value) -> Option<Value> {
    match value {
        Value::String(text) => serde_json::from_str(text).ok(),
        Value::Object(_) => Some(value.clone()),
        _ => None,
    }
}

async fn tenant_and_organizer(session: &Session, thread_id: &str) -> (Option<String>, Option<String>) {
    let url = format!(
        "https://teams.cloud.microsoft/api/chatsvc/{CHATSVC_REGION}/v1/users/ME/conversations/{}?view=msnp24Equivalent",
        thread_id.replace(':', "%3A").replace('@', "%40")
    );
    let Ok(response) = session.send(Request::get(url), &ic3_scope()).await else {
        return (None, None);
    };
    let properties = &response.body["properties"];
    let meeting = as_object(&properties["meeting"]);
    let text = |value: &Value| value.as_str().filter(|text| !text.is_empty()).map(str::to_owned);
    let tenant = meeting
        .as_ref()
        .and_then(|meeting| text(&meeting["tenantId"]))
        .or_else(|| text(&properties["addedByTenantId"]));
    (tenant, meeting.as_ref().and_then(|meeting| text(&meeting["organizerId"])))
}

fn main() {
    let flags = arguments();
    if (flags.blur || flags.background) && std::env::var_os(VIDEO_ENV).is_none() {
        // SAFETY: nothing else runs yet; the runtime below starts after this.
        unsafe { std::env::set_var(VIDEO_ENV, "pattern") };
    }
    tokio::runtime::Runtime::new().expect("runtime").block_on(run());
}

fn http_status(reason: &str) -> String {
    reason
        .split("HTTP ")
        .nth(1)
        .map(|rest| rest.chars().take_while(char::is_ascii_digit).collect::<String>())
        .filter(|digits| !digits.is_empty())
        .map_or_else(|| "no status".to_owned(), |digits| format!("HTTP {digits}"))
}

#[derive(Default)]
struct Features {
    own_mri: Option<String>,
    spotlight_seen: bool,
    spotlight_cleared: bool,
    captions_requested: bool,
    captions_active: bool,
    captions_stop_due: Option<Instant>,
    captions_stopped: bool,
    caption_events: usize,
    blur_reports: usize,
    blur_total_ms: f64,
    recording_seen_on: bool,
    recording_seen_off: bool,
    recording_stop_due: Option<Instant>,
    recording_done: bool,
    whiteboard_url_seen: bool,
    whiteboard_share_seen: bool,
    whiteboard_wait_until: Option<Instant>,
    deadline: Option<Instant>,
}

impl Features {
    fn pending(&self, arguments: &Arguments) -> bool {
        if self.deadline.is_none_or(|deadline| Instant::now() >= deadline) {
            return false;
        }
        (arguments.spotlight_self && !self.spotlight_cleared)
            || (arguments.captions && !self.captions_stopped)
            || (arguments.record && !self.recording_done)
            || (arguments.whiteboard_state && self.whiteboard_wait_until.is_none_or(|until| Instant::now() < until) && !self.whiteboard_share_seen)
    }
}

async fn run() {
    let arguments = arguments();
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let poll_session = Session::connect(&endpoint).await.expect("browser");
    let me = fetch_self(&session).await.expect("own identity");

    let started = Instant::now();
    let reused = std::env::args().skip_while(|argument| argument != "--thread").nth(1);
    let (thread_id, created_meeting) = match reused {
        Some(thread_id) => (thread_id, None),
        None => {
            let body = json!({"meetingType": "MeetNow", "isStreamEnabled": false, "subject": SUBJECT, "unhideChatThread": true});
            let created = session
                .request(Method::Post, CREATE_URL, &Scope::new(SPACES, "user_impersonation"), Some(body))
                .await
                .expect("create meeting");
            println!("# create MeetNow: HTTP {} after {:?}", created.status, started.elapsed());
            let thread_id = created.body.pointer("/value/groupContext/threadId").and_then(Value::as_str).expect("meeting thread").to_owned();
            (thread_id, Some(created.body))
        }
    };
    if let Some(path) = &arguments.thread_out {
        std::fs::write(path, &thread_id).expect("thread file");
    }

    let (mut tenant_id, mut organizer_id) = tenant_and_organizer(&session, &thread_id).await;
    for _ in 0..THREAD_PROPERTY_RETRIES {
        if tenant_id.is_some() {
            break;
        }
        tokio::time::sleep(POLL).await;
        (tenant_id, organizer_id) = tenant_and_organizer(&session, &thread_id).await;
    }
    let organizer_id = organizer_id.unwrap_or_else(|| me.object_id.clone());
    println!(
        "# thread: tenant found {}, organizer is me {}",
        tenant_id.is_some(),
        organizer_id.eq_ignore_ascii_case(&me.object_id)
    );
    let target = MeetingTarget {
        thread_id: thread_id.clone(),
        tenant_id: tenant_id.expect("tenant id"),
        organizer_id,
        meeting_data: None,
    };

    let config = EngineConfig {
        ringable: false,
        trace: true,
        instance: InstanceNames {
            global: "__meetNowTrouter".into(),
            binding: "__meetNowRealtime".into(),
            endpoint_storage_key: "__meetNowEpid".into(),
        },
        ..EngineConfig::default()
    };
    let (engine, _events) = CallEngine::start(session.clone(), poll_session, config).await.expect("engine");
    println!("# engine ready after {:?}", started.elapsed());
    if arguments.resolve_by_id
        && let Some(created) = &created_meeting
    {
        check_resolve_by_id(&engine, created, &thread_id).await;
    }
    let join_started = Instant::now();
    let mut handle = engine.start_call(CallSpec::Meeting(target));

    let mut connected_after = None;
    let mut roster_count = None;
    let mut inbound = 0;
    let mut ended = None;
    let mut live_detected_after = None;
    let mut released_at = None;
    let mut end_sent_at = None;
    let mut media_due: Option<Instant> = None;
    let mut media_stop_due: Option<Instant> = None;
    let mut hand_lower_due: Option<Instant> = None;
    let mut hands_seen = 0usize;
    let mut reactions_seen = Vec::new();
    let mut sharing_local_level: f32 = 0.0;
    let mut sharing = false;
    let mut features = Features::default();
    let deadline = Instant::now() + CONNECT_LIMIT + LIVE_STATE_LIMIT + WAIT_FILE_LIMIT + arguments.hold + END_LIMIT;
    let mut tick = tokio::time::interval(POLL);
    while ended.is_none() && Instant::now() < deadline {
        tokio::select! {
            update = handle.updates.recv() => match update {
                Some(CallUpdate::State(CallState::Connected { .. })) => {
                    if connected_after.is_none() {
                        connected_after = Some(join_started.elapsed());
                        media_due = Some(Instant::now() + MEDIA_DELAY);
                    }
                }
                Some(CallUpdate::Camera(on)) => println!("# camera update: {on}"),
                Some(CallUpdate::LocalShare(label)) => {
                    sharing = label.is_some();
                    println!("# local share update: {}", label.is_some());
                }
                Some(CallUpdate::Notice(text)) => {
                    println!("# notice: {text}");
                    if arguments.record && text.to_ascii_lowercase().contains("recording") && !features.recording_seen_on {
                        features.recording_done = true;
                    }
                }
                Some(CallUpdate::Recording(on)) => {
                    println!("# recording state: {}", if on { "on" } else { "off" });
                    if on {
                        features.recording_seen_on = true;
                        features.recording_stop_due = Some(Instant::now() + RECORD_HOLD);
                    } else if features.recording_seen_on {
                        features.recording_seen_off = true;
                        features.recording_done = true;
                    }
                }
                Some(CallUpdate::Whiteboard(share)) => {
                    println!("# whiteboard share: {}", share.as_ref().map_or_else(|| "ended".to_owned(), |share| format!("presenter {:?}, whiteboard {}, url {}", share.presenter, share.whiteboard, share.url.is_some())));
                    features.whiteboard_share_seen |= share.is_some();
                }
                Some(CallUpdate::WhiteboardUrl(url)) => {
                    features.whiteboard_url_seen = true;
                    println!("# whiteboard board url fetched: host {}", url.split('/').nth(2).unwrap_or("?"));
                }
                Some(CallUpdate::State(CallState::Ended { reason })) => ended = Some(reason),
                Some(CallUpdate::OwnIdentity { mri }) => features.own_mri = Some(mri),
                Some(CallUpdate::Captions(state)) => {
                    println!("# captions state: {}", match &state {
                        CaptionState::Off => "off".to_owned(),
                        CaptionState::Starting => "starting".to_owned(),
                        CaptionState::On => "on".to_owned(),
                        CaptionState::Failed(reason) => format!("failed ({reason})"),
                    });
                    if state == CaptionState::On && !features.captions_active {
                        features.captions_active = true;
                        features.captions_stop_due = Some(Instant::now() + CAPTIONS_HOLD);
                    }
                    if matches!(state, CaptionState::Failed(_)) {
                        features.captions_stopped = true;
                    }
                }
                Some(CallUpdate::Caption(_)) => features.caption_events += 1,
                Some(CallUpdate::BlurTiming(average_ms)) => {
                    features.blur_reports += 1;
                    features.blur_total_ms += f64::from(average_ms);
                }
                Some(CallUpdate::Organizer { action, outcome }) => match outcome {
                    Ok(status) => println!("# organizer {action}: HTTP {status}"),
                    Err(reason) => println!("# organizer {action}: failed, {}", http_status(&reason)),
                },
                Some(CallUpdate::Roster(entries)) => {
                    let own_spotlit = entries.iter().any(|entry| Some(&entry.mri) == features.own_mri.as_ref() && entry.spotlight.is_some());
                    if own_spotlit && !features.spotlight_seen {
                        features.spotlight_seen = true;
                        println!("# own spotlight visible in the roster");
                        if let Some(mri) = features.own_mri.clone() {
                            let _ = handle.commands.send(CallCommand::StopSpotlight { mri });
                        }
                    }
                    if !own_spotlit && features.spotlight_seen && !features.spotlight_cleared {
                        features.spotlight_cleared = true;
                        println!("# own spotlight gone from the roster");
                    }
                    let hands = entries.iter().filter(|entry| entry.hand.is_some()).count();
                    if hands != hands_seen {
                        println!("# roster hands raised: {hands}");
                        hands_seen = hands;
                    }
                    roster_count = Some(entries.len());
                }
                Some(CallUpdate::Reaction { reaction, .. }) => {
                    println!("# reaction received: {reaction:?}");
                    reactions_seen.push(reaction);
                }
                Some(CallUpdate::MeetingChat(thread)) => {
                    let url = format!(
                        "https://teams.cloud.microsoft/api/chatsvc/{CHATSVC_REGION}/v1/users/ME/conversations/{}/messages?view=msnp24Equivalent&pageSize=5",
                        urlencoding_component(&thread)
                    );
                    let read = session.request(Method::Get, &url, &ic3_scope(), None).await;
                    match read {
                        Ok(answer) => println!(
                            "# meeting chat thread from join: same as meeting {}, read HTTP {}, messages {}",
                            thread == thread_id,
                            answer.status,
                            answer.body.get("messages").and_then(Value::as_array).map_or(0, Vec::len)
                        ),
                        Err(error) => println!("# meeting chat read failed: {error}"),
                    }
                }
                Some(CallUpdate::ShareSound(on)) => println!("# share sound: {on}"),
                Some(CallUpdate::Muted(muted)) => println!("# muted: {muted}"),
                Some(CallUpdate::Levels { local, .. }) => {
                    if sharing {
                        sharing_local_level = sharing_local_level.max(local);
                    }
                }
                Some(CallUpdate::Stats { inbound_packets, .. }) => inbound = inbound_packets,
                Some(_) => {}
                None => break,
            },
            _ = tick.tick() => {
                if media_stop_due.is_some_and(|due| Instant::now() >= due) {
                    media_stop_due = None;
                    let _ = handle.commands.send(CallCommand::SetCamera(false));
                    let _ = handle.commands.send(CallCommand::StopShare);
                }
                if features.captions_stop_due.is_some_and(|due| Instant::now() >= due) {
                    features.captions_stop_due = None;
                    features.captions_stopped = true;
                    let _ = handle.commands.send(CallCommand::SetCaptions(false));
                }
                if features.recording_stop_due.is_some_and(|due| Instant::now() >= due) {
                    features.recording_stop_due = None;
                    let _ = handle.commands.send(CallCommand::SetRecording { on: false, title: SUBJECT.to_owned() });
                }
                if hand_lower_due.is_some_and(|due| Instant::now() >= due) {
                    hand_lower_due = None;
                    let _ = handle.commands.send(CallCommand::SetHand(false));
                }
                if media_due.is_some_and(|due| Instant::now() >= due) {
                    media_due = None;
                    media_stop_due = arguments.stop_media_after.map(|after| Instant::now() + after);
                    if arguments.camera {
                        let _ = handle.commands.send(CallCommand::SetCamera(true));
                    }
                    if arguments.extras {
                        let _ = handle.commands.send(CallCommand::SetHand(true));
                        for reaction in Reaction::ALL {
                            let _ = handle.commands.send(CallCommand::SendReaction(reaction));
                        }
                        let _ = handle.commands.send(CallCommand::SetShareSound(true));
                        hand_lower_due = Some(Instant::now() + Duration::from_secs(6));
                    }
                    features.deadline = Some(Instant::now() + FEATURE_LIMIT);
                    if arguments.spotlight_self
                        && let Some(mri) = features.own_mri.clone()
                    {
                        let _ = handle.commands.send(CallCommand::Spotlight { mri });
                    }
                    if arguments.mute_all {
                        let _ = handle.commands.send(CallCommand::MuteAll);
                    }
                    if arguments.captions {
                        features.captions_requested = true;
                        let _ = handle.commands.send(CallCommand::SetCaptions(true));
                    }
                    if arguments.blur {
                        let _ = handle.commands.send(CallCommand::SetBackground(BackgroundChoice::Blur));
                    }
                    if arguments.record {
                        let _ = handle.commands.send(CallCommand::SetRecording { on: true, title: SUBJECT.to_owned() });
                    }
                    if arguments.whiteboard_state {
                        features.whiteboard_wait_until = Some(Instant::now() + WHITEBOARD_WAIT);
                        println!("# whiteboard: fetching the board url, then waiting {WHITEBOARD_WAIT:?} for a whiteboard share (start one from Teams web in this meeting)");
                        let _ = handle.commands.send(CallCommand::OpenWhiteboard { title: SUBJECT.to_owned() });
                    }
                    if arguments.background {
                        let image = background_image(&session).await;
                        if let Some(path) = image {
                            let _ = handle.commands.send(CallCommand::SetBackground(BackgroundChoice::Image(path)));
                        }
                    }
                    if arguments.share {
                        let source = ShareSource { id: 0, kind: ShareKind::Screen, title: String::new() };
                        let _ = handle.commands.send(CallCommand::StartShare(source));
                    }
                }
                if connected_after.is_none() {
                    if join_started.elapsed() > CONNECT_LIMIT {
                        println!("# not connected within {CONNECT_LIMIT:?}, hanging up");
                        let _ = handle.commands.send(CallCommand::Hangup);
                    }
                    continue;
                }
                if live_detected_after.is_none() && end_sent_at.is_none() {
                    if let Ok(Some(meeting)) = fetch_live_meeting(&session, &thread_id).await {
                        live_detected_after = Some(join_started.elapsed());
                        println!("# live state detected, join target complete: {}", meeting.target().is_some());
                        released_at = Some(Instant::now() + arguments.hold);
                    } else if join_started.elapsed() > CONNECT_LIMIT + LIVE_STATE_LIMIT {
                        println!("# live state not seen within {LIVE_STATE_LIMIT:?}");
                        released_at = Some(Instant::now());
                    }
                }
                let waiting_for_file = arguments.wait_file.as_ref().is_some_and(|path| !path.exists())
                    && join_started.elapsed() < CONNECT_LIMIT + WAIT_FILE_LIMIT;
                if end_sent_at.is_none() && released_at.is_some_and(|at| Instant::now() >= at) && !waiting_for_file && !features.pending(&arguments) {
                    println!("# roster before end: {roster_count:?} participant(s), inbound packets {inbound}");
                    let _ = handle.commands.send(CallCommand::EndMeeting);
                    end_sent_at = Some(Instant::now());
                }
            }
        }
    }
    println!(
        "# connected after {connected_after:?}, live state after {live_detected_after:?}, roster {roster_count:?}, inbound packets {inbound}"
    );
    println!("# ended: {ended:?}");
    if arguments.extras {
        println!(
            "# extras: reactions echoed {}/{}, max local level while sharing {sharing_local_level:.3}",
            reactions_seen.len(),
            Reaction::ALL.len()
        );
    }
    if arguments.spotlight_self {
        println!("# spotlight self: shown {}, cleared {}", features.spotlight_seen, features.spotlight_cleared);
    }
    if arguments.captions {
        println!("# captions: became active {}, caption events {}", features.captions_active, features.caption_events);
    }
    if arguments.blur || arguments.background {
        let average = if features.blur_reports == 0 { 0. } else { features.blur_total_ms / features.blur_reports as f64 };
        let label = if arguments.background { "background image" } else { "blur" };
        println!("# {label}: average processing {average:.1} ms over {} report(s)", features.blur_reports);
    }
    if arguments.record {
        println!("# recording: seen on {}, seen off {}", features.recording_seen_on, features.recording_seen_off);
    }
    if arguments.whiteboard_state {
        println!("# whiteboard: board url fetched {}, share update parsed {}", features.whiteboard_url_seen, features.whiteboard_share_seen);
    }
    let gone_started = Instant::now();
    let mut gone_after = None;
    while gone_started.elapsed() < GONE_LIMIT {
        if matches!(fetch_live_meeting(&session, &thread_id).await, Ok(None)) {
            gone_after = Some(gone_started.elapsed());
            break;
        }
        tokio::time::sleep(POLL).await;
    }
    println!("# live state gone after end: {gone_after:?}");
    engine.stop().await;
    println!("# engine stopped, total {:?}", started.elapsed());
}

async fn background_image(session: &Session) -> Option<PathBuf> {
    let cache = BackgroundCache::new(std::env::temp_dir().join(BACKGROUND_CACHE));
    let catalog = match cache.refresh_catalog(session).await {
        Ok(catalog) => catalog,
        Err(error) => {
            println!("# background image list failed: {error}");
            return None;
        }
    };
    println!("# background image list downloaded: {} entries", catalog.len());
    let first = catalog.first()?;
    match cache.ensure_thumbnail(session, first).await {
        Ok(path) => println!("# first thumbnail cached: {} bytes", std::fs::metadata(&path).map_or(0, |meta| meta.len())),
        Err(error) => println!("# first thumbnail failed: {error}"),
    }
    match cache.ensure_image(session, first).await {
        Ok(path) => {
            println!("# first image {} cached: {} bytes", first.id, std::fs::metadata(&path).map_or(0, |meta| meta.len()));
            Some(path)
        }
        Err(error) => {
            println!("# first image failed: {error}");
            None
        }
    }
}

fn urlencoding_component(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

fn find_text<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                if keys.iter().any(|wanted| key.eq_ignore_ascii_case(wanted))
                    && let Some(text) = inner.as_str().filter(|text| !text.is_empty())
                {
                    return Some(text);
                }
                if let Some(found) = find_text(inner, keys) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(|item| find_text(item, keys)),
        _ => None,
    }
}

async fn check_resolve_by_id(engine: &CallEngine, created: &Value, thread_id: &str) {
    let code = find_text(created, &["joinMeetingId", "meetingCode"]).map(|code| code.replace(' ', ""));
    let passcode = find_text(created, &["passcode"]);
    println!("# resolve by id: meeting id found {}, passcode found {}", code.is_some(), passcode.is_some());
    let Some(code) = code else {
        return;
    };
    let url = match passcode {
        Some(passcode) => format!("https://teams.microsoft.com/meet/{code}?p={passcode}"),
        None => format!("https://teams.microsoft.com/meet/{code}"),
    };
    let meeting_data = json!({"meetingCode": code, "passcode": passcode, "meetingUrl": url});
    match engine.resolve_meeting(&meeting_data).await {
        Ok(target) => println!(
            "# resolve by id: found, same thread {}, tenant and organizer present {}",
            target.thread_id == thread_id,
            !target.tenant_id.is_empty() && !target.organizer_id.is_empty()
        ),
        Err(error) => println!("# resolve by id: failed: {}", error.to_string().chars().take(160).collect::<String>()),
    }
}
