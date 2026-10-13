use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use calling::meeting::fetch_live_meeting;
use calling::relay::ic3_scope;
use calling::signaling::{CHATSVC_REGION, fetch_self};
use calling::{
    CallCommand, CallEngine, CallSpec, CallState, CallUpdate, CaptionState, EngineConfig, MeetingOptions, MeetingRole, MeetingTarget, Modality,
    MuteCommand, Reaction, RecordingKind, RosterEntry, ShareKind, ShareSource, StopScope, TileDetail, VideoKey, VideoWant,
};
use chatsvc::InstanceNames;
use chatsvc::recap::Recap;
use serde_json::{Value, json};
use session::{DEFAULT_ENDPOINT, Method, Request, Scope, Session, SPACES};

const CREATE_URL: &str =
    "https://teams.cloud.microsoft/api/mt/emea/beta/me/calendarEvents/privateMeeting/schedulingService/create";
const SUBJECT: &str = "Native client test";
const REPORT_EVERY: Duration = Duration::from_secs(2);
const THREAD_PROPERTY_RETRIES: usize = 8;
const GONE_LIMIT: Duration = Duration::from_secs(90);
const GRAPH_CHATS: &str = "https://graph.microsoft.com/v1.0/chats";

struct Arguments {
    link_out: Option<PathBuf>,
    thread_out: Option<PathBuf>,
    recap_thread: Option<String>,
}

fn arguments() -> Arguments {
    let mut parsed = Arguments { link_out: None, thread_out: None, recap_thread: None };
    let mut input = std::env::args().skip(1);
    while let Some(flag) = input.next() {
        match flag.as_str() {
            "--link-out" => parsed.link_out = input.next().map(PathBuf::from),
            "--thread-out" => parsed.thread_out = input.next().map(PathBuf::from),
            "--recap" => parsed.recap_thread = input.next(),
            _ => {}
        }
    }
    parsed
}

fn emit(value: Value) {
    println!("@{value}");
}

fn main() {
    for (name, value) in [("CALLING_AUDIO", "tone"), ("CALLING_VIDEO", "pattern")] {
        if std::env::var_os(name).is_none() {
            // SAFETY: nothing else runs yet; the runtime below starts after this.
            unsafe { std::env::set_var(name, value) };
        }
    }
    tokio::runtime::Runtime::new().expect("runtime").block_on(run());
}

fn as_object(value: &Value) -> Option<Value> {
    match value {
        Value::String(text) => serde_json::from_str(text).ok(),
        Value::Object(_) => Some(value.clone()),
        _ => None,
    }
}

fn encoded(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

async fn tenant_and_organizer(session: &Session, thread_id: &str) -> (Option<String>, Option<String>) {
    let url = format!(
        "https://teams.cloud.microsoft/api/chatsvc/{CHATSVC_REGION}/v1/users/ME/conversations/{}?view=msnp24Equivalent",
        encoded(thread_id)
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

fn find_text<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    match value {
        Value::Object(map) => map.iter().find_map(|(key, inner)| {
            let direct = keys.iter().any(|wanted| key.eq_ignore_ascii_case(wanted)).then(|| inner.as_str().filter(|text| !text.is_empty())).flatten();
            direct.or_else(|| find_text(inner, keys))
        }),
        Value::Array(items) => items.iter().find_map(|item| find_text(item, keys)),
        _ => None,
    }
}

fn roster_json(entries: &[RosterEntry], own_mri: Option<&str>) -> Value {
    Value::Array(
        entries
            .iter()
            .map(|entry| {
                json!({
                    "name": if Some(entry.mri.as_str()) == own_mri { "(me)" } else { entry.display_name.as_str() },
                    "muted": entry.muted,
                    "lobby": entry.in_lobby,
                    "video": entry.has_video,
                    "sharing": entry.sharing,
                    "hand": entry.hand.is_some(),
                    "spotlight": entry.spotlight.is_some(),
                    "organizer": entry.organizer,
                    "role": entry.role.map(|role| format!("{role:?}")),
                    "mic_blocked": entry.limits.mic_blocked.is_some(),
                    "camera_blocked": entry.limits.camera_blocked.is_some(),
                    "mic_released": entry.limits.mic_released.is_some(),
                    "camera_released": entry.limits.camera_released.is_some(),
                })
            })
            .collect(),
    )
}

fn options_json(options: &MeetingOptions) -> Value {
    json!({
        "bypass_lobby": options.bypass_lobby,
        "presenters": options.presenters,
        "attendee_mic": options.attendee_mic,
        "attendee_camera": options.attendee_camera,
        "meeting_chat": options.meeting_chat,
        "reactions": options.reactions,
        "record_automatically": options.record_automatically,
    })
}

fn reaction_named(name: &str) -> Option<Reaction> {
    Reaction::ALL.into_iter().find(|reaction| format!("{reaction:?}").eq_ignore_ascii_case(name))
}

#[derive(Default)]
struct Window {
    local_level: f32,
    remote_level: f32,
    frames: HashMap<VideoKey, (usize, u32, u32)>,
}

struct Matrix {
    session: Session,
    thread_id: String,
    own_mri: Option<String>,
    roster: Vec<RosterEntry>,
    options: Option<MeetingOptions>,
    original_options: Option<MeetingOptions>,
}

impl Matrix {
    fn mri_of(&self, name: &str) -> Option<String> {
        self.roster
            .iter()
            .find(|entry| entry.display_name.eq_ignore_ascii_case(name))
            .or_else(|| self.roster.iter().find(|entry| entry.display_name.to_lowercase().contains(&name.to_lowercase())))
            .map(|entry| entry.mri.clone())
    }

    fn name_of(&self, key: &VideoKey) -> String {
        match key {
            VideoKey::Person(mri) => self
                .roster
                .iter()
                .find(|entry| &entry.mri == mri)
                .map_or_else(|| "unknown".to_owned(), |entry| entry.display_name.clone()),
            other => format!("{other:?}"),
        }
    }

    async fn command(&mut self, line: &str) -> Vec<CallCommand> {
        let line = line.trim();
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        let person = |matrix: &Matrix, name: &str| {
            let found = matrix.mri_of(name);
            if found.is_none() {
                emit(json!({"event": "error", "command": verb, "reason": "no such person in the roster"}));
            }
            found
        };
        let on = |text: &str| matches!(text, "on" | "true" | "yes");
        let split_last = |text: &str| text.rsplit_once(' ').map(|(head, tail)| (head.trim().to_owned(), tail.trim().to_owned()));
        match verb {
            "admit" => person(self, rest).map(|mri| vec![CallCommand::Admit { mri }]).unwrap_or_default(),
            "deny" => person(self, rest).map(|mri| vec![CallCommand::Deny { mri }]).unwrap_or_default(),
            "admit-all" => vec![CallCommand::AdmitAll],
            "mute" => person(self, rest).map(|mri| vec![CallCommand::MuteParticipant { mri }]).unwrap_or_default(),
            "mute-all" => vec![CallCommand::MuteAll],
            "spotlight" => person(self, rest).map(|mri| vec![CallCommand::Spotlight { mri }]).unwrap_or_default(),
            "unspotlight" => person(self, rest).map(|mri| vec![CallCommand::StopSpotlight { mri }]).unwrap_or_default(),
            "remove" => person(self, rest).map(|mri| vec![CallCommand::RemoveParticipant { mri }]).unwrap_or_default(),
            "lower" => person(self, rest).map(|mri| vec![CallCommand::LowerHand { mri }]).unwrap_or_default(),
            "lower-all" => vec![CallCommand::LowerAllHands],
            "role" => {
                let Some((name, role)) = split_last(rest) else { return Vec::new() };
                let role = if role == "presenter" { MeetingRole::Presenter } else { MeetingRole::Attendee };
                person(self, &name).map(|mri| vec![CallCommand::SetRole { mri, role }]).unwrap_or_default()
            }
            "allow-mic" | "allow-camera" | "block-mic" | "block-camera" => {
                let modality = if verb.ends_with("mic") { Modality::Mic } else { Modality::Camera };
                let allowed = verb.starts_with("allow");
                person(self, rest).map(|mri| vec![CallCommand::SetAttendeeModality { mri, modality, allowed }]).unwrap_or_default()
            }
            "restrict" => {
                let mic_off = rest.contains("mic");
                let camera_off = rest.contains("camera");
                vec![CallCommand::SetAttendeeRestrictions { mic_off, camera_off }]
            }
            "lock" => vec![CallCommand::SetLock(on(rest))],
            "options" => vec![CallCommand::LoadMeetingOptions],
            "option" => {
                let (Some(current), Some((name, value))) = (self.options.clone(), split_last(rest)) else {
                    emit(json!({"event": "error", "command": verb, "reason": "load options first"}));
                    return Vec::new();
                };
                let mut edited = current.clone();
                match name.as_str() {
                    "reactions" => edited.reactions = on(&value),
                    "chat" => edited.meeting_chat = value,
                    "lobby" => edited.bypass_lobby = value,
                    "presenters" => edited.presenters = value,
                    "attendee_mic" => edited.attendee_mic = on(&value),
                    "attendee_camera" => edited.attendee_camera = on(&value),
                    _ => {}
                }
                vec![CallCommand::SaveMeetingOptions(current.changes(&edited))]
            }
            "options-restore" => match (&self.options, &self.original_options) {
                (Some(current), Some(original)) => vec![CallCommand::SaveMeetingOptions(current.changes(original))],
                _ => Vec::new(),
            },
            "captions" => vec![CallCommand::SetCaptions(on(rest))],
            "language" => vec![CallCommand::SetCaptionLanguage(rest.to_owned())],
            "record" => {
                let kind = match rest {
                    "audio" => RecordingKind::AudioOnly,
                    "transcript" => RecordingKind::TranscriptOnly,
                    _ => RecordingKind::Video,
                };
                vec![CallCommand::StartRecording { kind, title: SUBJECT.to_owned() }]
            }
            "stop" => {
                let scope = match rest {
                    "recording" => StopScope::RecordingOnly,
                    "transcription" => StopScope::TranscriptionOnly,
                    _ => StopScope::Both,
                };
                vec![CallCommand::StopRecording { scope }]
            }
            "camera" => vec![CallCommand::SetCamera(on(rest))],
            "share" if on(rest) => vec![CallCommand::StartShare(ShareSource { id: 0, kind: ShareKind::Screen, title: String::new() })],
            "share" => vec![CallCommand::StopShare],
            "self-mute" => vec![CallCommand::Mute(if on(rest) { MuteCommand::Mute } else { MuteCommand::Unmute })],
            "hand" => vec![CallCommand::SetHand(on(rest))],
            "react" => reaction_named(rest).map(|reaction| vec![CallCommand::SendReaction(reaction)]).unwrap_or_default(),
            "incoming" => vec![CallCommand::SetIncomingVideo(on(rest))],
            "wants" => {
                let wants = rest
                    .split(',')
                    .filter_map(|item| {
                        let (detail, name) = item.trim().split_once(':')?;
                        let detail = if detail == "stage" { TileDetail::Stage } else { TileDetail::Small };
                        Some(VideoWant { mri: self.mri_of(name)?, detail })
                    })
                    .collect();
                vec![CallCommand::SetVideoWants(wants)]
            }
            "chat" => {
                self.send_chat(rest).await;
                Vec::new()
            }
            "chat-read" => {
                self.read_chat().await;
                Vec::new()
            }
            "end" => vec![CallCommand::EndMeeting],
            "hangup" => vec![CallCommand::Hangup],
            _ => {
                emit(json!({"event": "error", "command": verb, "reason": "unknown command"}));
                Vec::new()
            }
        }
    }

    async fn send_chat(&self, text: &str) {
        let url = format!("{GRAPH_CHATS}/{}/messages", encoded(&self.thread_id));
        let body = json!({"body": {"contentType": "text", "content": text}});
        let outcome = self.session.request(Method::Post, &url, &Scope::graph("Chat.ReadWrite"), Some(body)).await;
        emit(json!({"event": "chat_sent", "status": outcome.as_ref().map(|answer| answer.status).ok(), "error": outcome.err().map(|error| error.to_string().chars().take(160).collect::<String>())}));
    }

    async fn read_chat(&self) {
        let url = format!("{GRAPH_CHATS}/{}/messages?$top=20", encoded(&self.thread_id));
        match self.session.request(Method::Get, &url, &Scope::graph("Chat.ReadWrite"), None).await {
            Ok(answer) => {
                let messages: Vec<Value> = answer.body["value"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|message| message["messageType"].as_str() == Some("message"))
                    .map(|message| {
                        let sender = message.pointer("/from/user/displayName").and_then(Value::as_str).unwrap_or("?");
                        let own = message.pointer("/from/user/id").and_then(Value::as_str).is_some_and(|id| self.own_mri.as_deref().is_some_and(|mri| mri.ends_with(id)));
                        json!({"from": if own { "(me)" } else { sender }, "text": message.pointer("/body/content").and_then(Value::as_str).unwrap_or_default().chars().take(120).collect::<String>()})
                    })
                    .collect();
                emit(json!({"event": "chat_read", "status": answer.status, "messages": messages}));
            }
            Err(error) => emit(json!({"event": "chat_read", "error": error.to_string().chars().take(160).collect::<String>()})),
        }
    }
}

async fn recap(session: &Session, thread_id: &str) {
    let recap = Recap::new(session);
    let occurrences = match recap.occurrences(thread_id).await {
        Ok(occurrences) => occurrences,
        Err(error) => {
            emit(json!({"event": "recap", "error": format!("{error:?}")}));
            return;
        }
    };
    emit(json!({"event": "recap_occurrences", "count": occurrences.len()}));
    for occurrence in &occurrences {
        match recap.details(occurrence).await {
            Ok(details) => {
                emit(json!({
                    "event": "recap_recording",
                    "audio_only": details.audio_only,
                    "duration_seconds": details.duration_seconds,
                    "expires_set": details.expires_at.is_some(),
                    "transcript": details.transcript_id.is_some(),
                }));
                let deleted = recap.delete_recording(&details).await;
                emit(json!({"event": "recap_deleted", "ok": deleted.is_ok(), "error": deleted.err().map(|error| format!("{error:?}"))}));
            }
            Err(error) => emit(json!({"event": "recap_recording", "error": format!("{error:?}")})),
        }
    }
}

async fn run() {
    let arguments = arguments();
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    if let Some(thread_id) = &arguments.recap_thread {
        recap(&session, thread_id).await;
        return;
    }
    let poll_session = Session::connect(&endpoint).await.expect("browser");
    let me = fetch_self(&session).await.expect("own identity");
    let started = Instant::now();

    let body = json!({"meetingType": "MeetNow", "isStreamEnabled": false, "subject": SUBJECT, "unhideChatThread": true});
    let created = session
        .request(Method::Post, CREATE_URL, &Scope::new(SPACES, "user_impersonation"), Some(body))
        .await
        .expect("create meeting");
    let thread_id = created.body.pointer("/value/groupContext/threadId").and_then(Value::as_str).expect("meeting thread").to_owned();
    emit(json!({"event": "created", "status": created.status}));
    if let Some(path) = &arguments.thread_out {
        std::fs::write(path, &thread_id).expect("thread file");
    }

    let (mut tenant_id, mut organizer_id) = tenant_and_organizer(&session, &thread_id).await;
    for _ in 0..THREAD_PROPERTY_RETRIES {
        if tenant_id.is_some() {
            break;
        }
        tokio::time::sleep(REPORT_EVERY).await;
        (tenant_id, organizer_id) = tenant_and_organizer(&session, &thread_id).await;
    }
    let target = MeetingTarget {
        thread_id: thread_id.clone(),
        tenant_id: tenant_id.expect("tenant id"),
        organizer_id: organizer_id.unwrap_or_else(|| me.object_id.clone()),
        meeting_data: None,
    };
    let join_link = find_text(&created.body, &["joinUrl", "meetingJoinUrl", "joinWebUrl"]).map_or_else(
        || {
            let context = json!({"Tid": target.tenant_id, "Oid": target.organizer_id}).to_string();
            format!("https://teams.microsoft.com/l/meetup-join/{}/0?context={}", encoded(&thread_id), encoded(&context))
        },
        str::to_owned,
    );
    if let Some(path) = &arguments.link_out {
        std::fs::write(path, &join_link).expect("link file");
    }
    let config = EngineConfig {
        ringable: false,
        trace: std::env::var_os("LIVE_TRACE").is_some(),
        instance: InstanceNames {
            global: "__liveMatrixTrouter".into(),
            binding: "__liveMatrixRealtime".into(),
            endpoint_storage_key: "__liveMatrixEpid".into(),
        },
        ..EngineConfig::default()
    };
    let (engine, _events) = CallEngine::start(session.clone(), poll_session, config).await.expect("engine");
    let mut handle = engine.start_call(CallSpec::Meeting(target));
    emit(json!({"event": "joining", "after_ms": started.elapsed().as_millis()}));

    let (line_sender, mut lines) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            if line_sender.send(line).is_err() {
                break;
            }
        }
    });

    let mut matrix = Matrix { session: session.clone(), thread_id: thread_id.clone(), own_mri: None, roster: Vec::new(), options: None, original_options: None };
    let mut window = Window::default();
    let mut last_roster = Value::Null;
    let mut last_health = Value::Null;
    let mut packets = (0u64, 0u64);
    let mut ended = None;
    let mut tick = tokio::time::interval(REPORT_EVERY);
    while ended.is_none() {
        tokio::select! {
            update = handle.updates.recv() => match update {
                Some(CallUpdate::State(CallState::Ended { reason })) => ended = Some(format!("{reason:?}")),
                Some(CallUpdate::State(state)) => emit(json!({"event": "state", "state": format!("{state:?}").split([' ', '{', '(']).next().unwrap_or_default()})),
                Some(CallUpdate::OwnIdentity { mri }) => matrix.own_mri = Some(mri),
                Some(CallUpdate::Roster(entries)) => {
                    let summary = roster_json(&entries, matrix.own_mri.as_deref());
                    if summary != last_roster {
                        emit(json!({"event": "roster", "entries": summary}));
                        last_roster = summary;
                    }
                    matrix.roster = entries;
                }
                Some(CallUpdate::Lobby(waiting)) => emit(json!({"event": "lobby_self", "waiting": waiting})),
                Some(CallUpdate::Levels { local, remote }) => {
                    window.local_level = window.local_level.max(local);
                    window.remote_level = window.remote_level.max(remote);
                }
                Some(CallUpdate::Stats { inbound_packets, outbound_packets }) => packets = (inbound_packets, outbound_packets),
                Some(CallUpdate::VideoReady) => {
                    for (key, picture) in handle.video.drain() {
                        let slot = window.frames.entry(key).or_insert((0, 0, 0));
                        slot.0 += 1;
                        slot.1 = picture.width;
                        slot.2 = picture.height;
                    }
                }
                Some(CallUpdate::Speakers(speakers)) => {
                    let names: Vec<String> = speakers.iter().map(|mri| matrix.name_of(&VideoKey::Person(mri.clone()))).collect();
                    emit(json!({"event": "speakers", "names": names}));
                }
                Some(CallUpdate::ScreenShare(sharer)) => emit(json!({"event": "screen_share", "sharer": sharer.map(|mri| matrix.name_of(&VideoKey::Person(mri)))})),
                Some(CallUpdate::LocalShare(label)) => emit(json!({"event": "local_share", "on": label.is_some()})),
                Some(CallUpdate::Camera(on)) => emit(json!({"event": "camera", "on": on})),
                Some(CallUpdate::Muted(muted)) => emit(json!({"event": "muted", "muted": muted})),
                Some(CallUpdate::OwnHand(hand)) => emit(json!({"event": "own_hand", "raised": hand.is_some()})),
                Some(CallUpdate::Reaction { mri, reaction }) => emit(json!({"event": "reaction", "from": matrix.name_of(&VideoKey::Person(mri)), "reaction": format!("{reaction:?}")})),
                Some(CallUpdate::Captions(state)) => emit(json!({"event": "captions", "state": match state {
                    CaptionState::Off => "off".to_owned(),
                    CaptionState::Starting => "starting".to_owned(),
                    CaptionState::On => "on".to_owned(),
                    CaptionState::Failed(reason) => format!("failed: {reason}"),
                }})),
                Some(CallUpdate::Caption(entry)) => {
                    if entry.is_final {
                        emit(json!({"event": "caption", "speaker": entry.display_name, "text": entry.text}));
                    }
                }
                Some(CallUpdate::Recording(on)) => emit(json!({"event": "recording", "on": on})),
                Some(CallUpdate::RecordingAudioOnly(on)) => emit(json!({"event": "recording_audio_only", "on": on})),
                Some(CallUpdate::Transcribing(on)) => emit(json!({"event": "transcribing", "on": on})),
                Some(CallUpdate::ConsentRequired(on)) => emit(json!({"event": "consent_required", "on": on})),
                Some(CallUpdate::Capability(capability)) => emit(json!({"event": "capability", "locked": capability.locked, "attendee_mic_off": capability.attendee_mic_off, "attendee_camera_off": capability.attendee_camera_off})),
                Some(CallUpdate::MeetingOptions(result) | CallUpdate::MeetingOptionsSaved(result)) => match result {
                    Ok(options) => {
                        emit(json!({"event": "options", "options": options_json(&options)}));
                        if matrix.original_options.is_none() {
                            matrix.original_options = Some(options.clone());
                        }
                        matrix.options = Some(options);
                    }
                    Err(reason) => emit(json!({"event": "options", "error": reason.chars().take(200).collect::<String>()})),
                },
                Some(CallUpdate::Organizer { action, outcome }) => emit(json!({"event": "organizer", "action": action, "status": outcome.as_ref().ok(), "error": outcome.err().map(|reason| reason.chars().take(200).collect::<String>())})),
                Some(CallUpdate::Whiteboard(share)) => emit(json!({"event": "whiteboard", "active": share.is_some()})),
                Some(CallUpdate::BreakoutMove(_)) => emit(json!({"event": "breakout_move"})),
                Some(CallUpdate::BreakoutRoom { .. }) => emit(json!({"event": "breakout_room"})),
                Some(CallUpdate::MeetingChat(thread)) => emit(json!({"event": "meeting_chat", "same_thread": thread == thread_id})),
                Some(CallUpdate::Health(health)) => {
                    let summary = json!({"event": "health", "level": format!("{:?}", health.level), "path": format!("{:?}", health.path)});
                    if summary != last_health {
                        emit(summary.clone());
                        last_health = summary;
                    }
                }
                Some(CallUpdate::Notice(text)) => emit(json!({"event": "notice", "text": text})),
                Some(_) => {}
                None => break,
            },
            line = lines.recv() => {
                let Some(line) = line else { continue };
                emit(json!({"event": "command", "line": line}));
                for command in matrix.command(&line).await {
                    let _ = handle.commands.send(command);
                }
            }
            _ = tick.tick() => {
                let frames: serde_json::Map<String, Value> = window
                    .frames
                    .iter()
                    .map(|(key, (count, width, height))| (matrix.name_of(key), json!({"frames": count, "size": format!("{width}x{height}")})))
                    .collect();
                emit(json!({
                    "event": "stats",
                    "t": started.elapsed().as_secs(),
                    "local_level": (window.local_level * 1000.0).round() / 1000.0,
                    "remote_level": (window.remote_level * 1000.0).round() / 1000.0,
                    "inbound": packets.0,
                    "outbound": packets.1,
                    "video": frames,
                }));
                window = Window::default();
            }
        }
    }
    emit(json!({"event": "ended", "reason": ended}));
    let gone_started = Instant::now();
    let mut gone = false;
    while gone_started.elapsed() < GONE_LIMIT {
        if matches!(fetch_live_meeting(&session, &thread_id).await, Ok(None)) {
            gone = true;
            break;
        }
        tokio::time::sleep(REPORT_EVERY).await;
    }
    emit(json!({"event": "live_state_gone", "gone": gone, "after_s": gone_started.elapsed().as_secs()}));
    engine.stop().await;
}
