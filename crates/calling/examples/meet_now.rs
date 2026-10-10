use std::path::PathBuf;
use std::time::{Duration, Instant};

use calling::meeting::fetch_live_meeting;
use calling::relay::ic3_scope;
use calling::signaling::{CHATSVC_REGION, fetch_self};
use calling::{CallCommand, CallEngine, CallSpec, CallState, CallUpdate, EngineConfig, MeetingTarget, Reaction, ShareKind, ShareSource};
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

struct Arguments {
    camera: bool,
    share: bool,
    extras: bool,
    stop_media_after: Option<Duration>,
    hold: Duration,
    thread_out: Option<PathBuf>,
    wait_file: Option<PathBuf>,
}

fn arguments() -> Arguments {
    let mut parsed = Arguments {
        camera: false,
        share: false,
        extras: false,
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

#[tokio::main]
async fn main() {
    let arguments = arguments();
    let endpoint = std::env::var("CDP_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let session = Session::connect(&endpoint).await.expect("browser");
    let poll_session = Session::connect(&endpoint).await.expect("browser");
    let me = fetch_self(&session).await.expect("own identity");

    let started = Instant::now();
    let reused = std::env::args().skip_while(|argument| argument != "--thread").nth(1);
    let thread_id = match reused {
        Some(thread_id) => thread_id,
        None => {
            let body = json!({"meetingType": "MeetNow", "isStreamEnabled": false, "subject": SUBJECT, "unhideChatThread": true});
            let created = session
                .request(Method::Post, CREATE_URL, &Scope::new(SPACES, "user_impersonation"), Some(body))
                .await
                .expect("create meeting");
            println!("# create MeetNow: HTTP {} after {:?}", created.status, started.elapsed());
            created.body.pointer("/value/groupContext/threadId").and_then(Value::as_str).expect("meeting thread").to_owned()
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
                Some(CallUpdate::Notice(text)) => println!("# notice: {text}"),
                Some(CallUpdate::State(CallState::Ended { reason })) => ended = Some(reason),
                Some(CallUpdate::Roster(entries)) => {
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
                if end_sent_at.is_none() && released_at.is_some_and(|at| Instant::now() >= at) && !waiting_for_file {
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

fn urlencoding_component(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}
