use super::*;

pub(super) enum PeerEvent {
    Ice(IceConnectionState),
    Connection(PeerConnectionState),
    Track(RtcAudioTrack),
    VideoTrack { mid: String, track: RtcVideoTrack },
    Data(Vec<u8>),
}

pub(super) struct PeerSession {
    pub(super) peer: PeerConnection,
    pub(super) events: mpsc::UnboundedReceiver<PeerEvent>,
    pub(super) _data_channel: Option<DataChannel>,
    pub(super) gathered: Gathered,
}

#[derive(Clone, Default)]
pub(super) struct Gathered {
    pub(super) candidates: Arc<std::sync::Mutex<Vec<String>>>,
    pub(super) complete: Arc<AtomicBool>,
    pub(super) relay_seen_at: Arc<std::sync::Mutex<Option<Instant>>>,
    pub(super) waited_ms: Arc<AtomicU64>,
}

/// libwebrtc gathers continually here, and TCP/TLS relays trickle in late, so a UDP relay plus a short settle is enough.
pub(super) fn gathering_settled(complete: bool, since_udp_relay: Option<Duration>) -> bool {
    complete || since_udp_relay.is_some_and(|since| since >= RELAY_SETTLE)
}

impl Gathered {
    fn settled(&self) -> bool {
        let since_udp_relay = self
            .relay_seen_at
            .lock()
            .expect("relay time lock")
            .map(|at| at.elapsed());
        gathering_settled(self.complete.load(Ordering::SeqCst), since_udp_relay)
    }

    pub(super) async fn candidates(&self) -> Vec<String> {
        let waited = Instant::now();
        while !self.settled() && waited.elapsed() < GATHERING_LIMIT {
            tokio::time::sleep(GATHERING_POLL).await;
        }
        self.complete.store(true, Ordering::SeqCst);
        self.waited_ms
            .store(waited.elapsed().as_millis() as u64, Ordering::SeqCst);
        self.candidates.lock().expect("candidates lock").clone()
    }

    pub(super) fn summary(&self) -> String {
        let count = self.candidates.lock().expect("candidates lock").len();
        format!(
            "{count} candidate(s) after {} ms",
            self.waited_ms.load(Ordering::SeqCst)
        )
    }
}

pub(super) async fn direct_sdp(
    teams_sdp: &str,
    gathered: Option<&Gathered>,
    kind: SdpKind,
) -> Result<String> {
    match gathered {
        Some(gathered) => with_candidates(teams_sdp, &gathered.candidates().await, kind),
        None => Ok(teams_sdp.to_owned()),
    }
}

pub(super) struct OfferedMedia {
    pub(super) browser_offer: String,
    pub(super) signaled: SignaledOffer,
    pub(super) video: VideoLines,
    pub(super) plan: OfferPlan,
    pub(super) modalities: Vec<String>,
}

#[derive(Clone, Copy)]
pub(super) enum AnswerVideo<'a> {
    Reject,
    Receive,
    Send(&'a RtcVideoTrack),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Change {
    Hold(bool),
    Camera(bool),
    Share(bool),
}

pub(super) struct SamePeerOffer {
    pub(super) browser_offer: String,
    pub(super) signaled: SignaledOffer,
    pub(super) change: Change,
    pub(super) reply_tags: Vec<String>,
}

pub(super) struct GlareResend {
    pub(super) change: Change,
    pub(super) reply_tags: Vec<String>,
    pub(super) at: Instant,
}

pub(super) fn answers_offer(callback_path: &str, reply_tags: &[String]) -> bool {
    match callback_tag(callback_path) {
        Some(tag) if !reply_tags.is_empty() => reply_tags.contains(&tag),
        _ => true,
    }
}

pub(super) fn open_peer(
    factory: &PeerConnectionFactory,
    grant: &RelayGrant,
    relay_host: &str,
    with_data_channel: bool,
) -> Result<PeerSession> {
    let mut configuration = RtcConfiguration::default();
    configuration.ice_servers = vec![grant.ice_server(relay_host)];
    let peer = factory
        .create_peer_connection(configuration)
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let (sender, events) = mpsc::unbounded_channel();
    let data_channel = with_data_channel
        .then(|| open_data_channel(&peer, sender.clone()))
        .transpose()?;
    let gathered = Gathered::default();
    let candidate_sink = gathered.candidates.clone();
    let relay_seen_at = gathered.relay_seen_at.clone();
    peer.on_ice_candidate(Some(Box::new(move |candidate| {
        if candidate.sdp_mline_index() == 0 {
            let text = candidate.candidate();
            if is_udp_relay(&text) {
                relay_seen_at
                    .lock()
                    .expect("relay time lock")
                    .get_or_insert_with(Instant::now);
            }
            candidate_sink.lock().expect("candidates lock").push(text);
        }
    })));
    let gathering_done = gathered.complete.clone();
    peer.on_ice_gathering_state_change(Some(Box::new(move |state| match state {
        IceGatheringState::Complete => gathering_done.store(true, Ordering::SeqCst),
        IceGatheringState::Gathering => gathering_done.store(false, Ordering::SeqCst),
        IceGatheringState::New => {}
    })));
    let ice_sender = sender.clone();
    peer.on_ice_connection_state_change(Some(Box::new(move |state| {
        let _ = ice_sender.send(PeerEvent::Ice(state));
    })));
    let connection_sender = sender.clone();
    peer.on_connection_state_change(Some(Box::new(move |state| {
        let _ = connection_sender.send(PeerEvent::Connection(state));
    })));
    peer.on_track(Some(Box::new(move |event| match event.track {
        MediaStreamTrack::Audio(track) => {
            let _ = sender.send(PeerEvent::Track(track));
        }
        MediaStreamTrack::Video(track) => {
            let mid = event.transceiver.mid().unwrap_or_default();
            let _ = sender.send(PeerEvent::VideoTrack { mid, track });
        }
    })));
    Ok(PeerSession {
        peer,
        events,
        _data_channel: data_channel,
        gathered,
    })
}

fn open_data_channel(
    peer: &PeerConnection,
    sender: mpsc::UnboundedSender<PeerEvent>,
) -> Result<DataChannel> {
    let channel = peer
        .create_data_channel(DATA_CHANNEL_LABEL, DataChannelInit::default())
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    channel.on_message(Some(Box::new(move |buffer| {
        let _ = sender.send(PeerEvent::Data(buffer.data.to_vec()));
    })));
    Ok(channel)
}

fn one_to_one_directions<'a>(
    camera: bool,
    share: bool,
    camera_mid: &'a str,
    share_mid: Option<&'a str>,
    remote_shares: bool,
) -> impl Fn(MediaLine<'_>) -> Option<&'static str> + 'a {
    move |line| match line.kind {
        "video" if line.mid == camera_mid => Some(if camera { "sendrecv" } else { INACTIVE }),
        "video" if Some(line.mid) == share_mid => Some(match (share, remote_shares) {
            (true, true) => "sendrecv",
            (true, false) => "sendonly",
            (false, true) => "recvonly",
            (false, false) => INACTIVE,
        }),
        _ => None,
    }
}

fn share_mid_in(offer_text: &str) -> Option<String> {
    let mut mid = None;
    for line in offer_text.lines().map(|line| line.trim_end_matches('\r')) {
        if line.starts_with("m=") {
            mid = None;
        } else if let Some(value) = line.strip_prefix("a=mid:") {
            mid = Some(value.to_owned());
        } else if line.starts_with(&format!("a=msid:{SCREEN_STREAM} ")) && mid.is_some() {
            return mid;
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub(super) fn directions_for(
    layout: Layout,
    hold: bool,
    camera: bool,
    share: bool,
    camera_mid: &str,
    share_mid: Option<&str>,
    remote_shares: bool,
    text: &str,
) -> String {
    match (hold, layout) {
        (true, _) => rewrite_directions(text, |_| Some(INACTIVE)),
        (false, Layout::OneToOne) => rewrite_directions(
            text,
            one_to_one_directions(camera, share, camera_mid, share_mid, remote_shares),
        ),
        (false, Layout::Conference) => text.to_owned(),
    }
}

pub(super) async fn create_offer(
    peer: &PeerConnection,
    factory: &PeerConnectionFactory,
    track: &RtcAudioTrack,
    layout: Layout,
    gathered: Option<&Gathered>,
) -> Result<OfferedMedia> {
    let transceiver_init = RtpTransceiverInit {
        direction: RtpTransceiverDirection::SendRecv,
        stream_ids: vec![MIC_STREAM.into()],
        send_encodings: Vec::new(),
    };
    peer.add_transceiver(MediaStreamTrack::Audio(track.clone()), transceiver_init)
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let (video, plan) = match layout {
        Layout::Conference => (add_video_lines(peer, factory)?, offer_plan()),
        Layout::OneToOne => (add_one_to_one_lines(peer, factory)?, one_to_one_plan()),
    };
    let offer = peer
        .create_offer(OfferOptions {
            offer_to_receive_audio: true,
            offer_to_receive_video: true,
            ..OfferOptions::default()
        })
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let browser_offer = directions_for(
        layout,
        false,
        false,
        false,
        CAMERA_MID,
        Some(ONE_TO_ONE_SHARE_MID),
        false,
        &offer.to_string(),
    );
    let description = parse_sdp(&browser_offer, SdpType::Offer)?;
    peer.set_local_description(description)
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let mut numbers = random_u32;
    let mut signaled = to_teams_offer(&browser_offer, &plan, &mut numbers)?;
    signaled.sdp = direct_sdp(&signaled.sdp, gathered, SdpKind::Offer).await?;
    Ok(OfferedMedia {
        browser_offer,
        signaled,
        video,
        plan,
        modalities: accepted_modalities(false),
    })
}

pub(super) struct AnsweredMedia {
    pub(super) remote: RemoteOffer,
    pub(super) answer_sdp: String,
    pub(super) camera_mid: Option<String>,
    pub(super) receiving_share_mid: Option<String>,
}

pub(super) fn main_video_mid(remote: &RemoteOffer) -> Option<String> {
    remote
        .lines
        .iter()
        .find(|line| line.role == LineRole::MainVideo)
        .and_then(|line| line.browser_mids.first().cloned())
}

pub(super) fn share_mids(remote: &RemoteOffer) -> Vec<String> {
    remote
        .lines
        .iter()
        .filter(|line| line.role == LineRole::ScreenShare)
        .filter_map(|line| line.browser_mids.first().cloned())
        .collect()
}

pub(super) async fn answer_offer(
    peer: &PeerConnection,
    track: Option<&RtcAudioTrack>,
    teams_offer: &str,
    video: AnswerVideo<'_>,
    sending_share_mid: Option<String>,
    camera_on: bool,
    gathered: Option<&Gathered>,
) -> Result<AnsweredMedia> {
    let remote = from_teams_offer(teams_offer)?;
    let description = parse_sdp(&remote.browser_sdp, SdpType::Offer)?;
    peer.set_remote_description(description)
        .await
        .map_err(|error| {
            let lines: Vec<String> = remote
                .lines
                .iter()
                .map(|line| format!("{:?}{:?}", line.role, line.browser_mids))
                .collect();
            Error::Webrtc(format!("{error} (offer lines: {})", lines.join(" ")))
        })?;
    let camera_mid = main_video_mid(&remote).filter(|_| !matches!(video, AnswerVideo::Reject));
    let kept_share_mids = if matches!(video, AnswerVideo::Reject) {
        Vec::new()
    } else {
        share_mids(&remote)
    };
    for transceiver in peer.transceivers() {
        let is_video = matches!(
            transceiver.receiver().track(),
            Some(MediaStreamTrack::Video(_))
        );
        let unused = transceiver.mid().is_some_and(|mid| {
            Some(&mid) != camera_mid.as_ref()
                && !kept_share_mids.contains(&mid)
                && Some(&mid) != sending_share_mid.as_ref()
        });
        if is_video && unused {
            let _ = transceiver.stop();
        }
    }
    if let Some(track) = track {
        peer.add_track(MediaStreamTrack::Audio(track.clone()), &[MIC_STREAM])
            .map_err(|error| Error::Webrtc(error.to_string()))?;
    }
    if let (AnswerVideo::Send(camera), Some(_)) = (video, &camera_mid) {
        peer.add_track(MediaStreamTrack::Video(camera.clone()), &[CAMERA_STREAM])
            .map_err(|error| Error::Webrtc(error.to_string()))?;
    }
    let answer = peer
        .create_answer(AnswerOptions::default())
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let mut answer_text = answer.to_string();
    for mid in kept_share_mids
        .iter()
        .filter(|mid| sending_share_mid.as_deref() != Some(mid.as_str()))
    {
        answer_text = stop_sending_on(&answer_text, mid);
    }
    let sends_camera = camera_on || matches!(video, AnswerVideo::Send(_));
    if let Some(mid) = camera_mid.as_deref().filter(|_| !sends_camera) {
        answer_text = stop_sending_on(&answer_text, mid);
    }
    peer.set_local_description(parse_sdp(&answer_text, SdpType::Answer)?)
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let receiving_share_mid = kept_share_mids
        .into_iter()
        .filter(|mid| sending_share_mid.as_deref() != Some(mid.as_str()))
        .find(|mid| {
            matches!(
                direction_of(&answer_text, mid).as_deref(),
                Some("recvonly" | "sendrecv")
            )
        });
    let mut numbers = random_u32;
    let signaled = to_teams_answer(&answer_text, &remote, &mut numbers)?;
    Ok(AnsweredMedia {
        remote,
        answer_sdp: direct_sdp(&signaled.sdp, gathered, SdpKind::Answer).await?,
        camera_mid,
        receiving_share_mid,
    })
}

pub(super) fn new_media_leg_id() -> String {
    uuid::Uuid::new_v4().simple().to_string().to_uppercase()
}

pub(super) fn peer_transceiver(peer: &PeerConnection, mid: &str) -> Option<RtpTransceiver> {
    peer.transceivers()
        .into_iter()
        .find(|transceiver| transceiver.mid().as_deref() == Some(mid))
}

pub(super) async fn offer_same_peer(
    peer: &PeerConnection,
    gathered: &Gathered,
    own_share_mid: Option<String>,
    plan: &OfferPlan,
    layout: Layout,
    camera_mid: &str,
    hold: bool,
    camera: bool,
    share: bool,
    remote: &RemoteCall,
    links: &CallbackLinks,
    media_leg_id: &str,
    media_links: &BTreeMap<String, String>,
    negotiator: &mut MediaNegotiator,
) -> Result<(String, SignaledOffer, Vec<String>)> {
    let direct = layout == Layout::OneToOne && negotiator.path() == MediaPath::Direct;
    let url = media_links
        .get("mediaRenegotiation")
        .cloned()
        .ok_or_else(|| Error::Signaling("this call has no mediaRenegotiation link".into()))?;
    if !negotiator.begin_same_peer_offer() {
        return Err(Error::Signaling(
            "another media negotiation is running".into(),
        ));
    }
    let offered = build_same_peer_offer(
        peer,
        plan,
        layout,
        hold,
        camera,
        share,
        camera_mid,
        own_share_mid,
        direct.then_some(gathered),
    )
    .await;
    let (text, signaled) = match offered {
        Ok(offered) => offered,
        Err(error) => {
            negotiator.same_peer_offer_failed();
            return Err(error);
        }
    };
    let modalities = offer_modalities(
        layout == Layout::OneToOne && camera && !hold,
        share && !hold,
    );
    let body = renegotiation_body(
        &remote.participant,
        links,
        &signaled.sdp,
        media_leg_id,
        &modalities,
        false,
        direct,
    );
    let reply_tags = offer_reply_tags(&body);
    if let Err(error) = remote
        .signaling
        .post_json("POST mediaRenegotiation (same peer)", &url, body)
        .await
    {
        negotiator.same_peer_offer_failed();
        rollback_offer(peer).await;
        return Err(error);
    }
    Ok((text, signaled, reply_tags))
}

#[allow(clippy::too_many_arguments)]
async fn build_same_peer_offer(
    peer: &PeerConnection,
    plan: &OfferPlan,
    layout: Layout,
    hold: bool,
    camera: bool,
    share: bool,
    camera_mid: &str,
    own_share_mid: Option<String>,
    gathered: Option<&Gathered>,
) -> Result<(String, SignaledOffer)> {
    let offer = peer
        .create_offer(OfferOptions {
            offer_to_receive_audio: true,
            offer_to_receive_video: true,
            ..OfferOptions::default()
        })
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let offer_text = offer.to_string();
    let share_mid = own_share_mid.or_else(|| share_mid_in(&offer_text));
    let remote_shares = share_mid.as_deref().is_some_and(|mid| {
        peer.current_remote_description()
            .and_then(|remote| direction_of(&remote.to_string(), mid))
            .is_some_and(|direction| matches!(direction.as_str(), "sendonly" | "sendrecv"))
    });
    let text = directions_for(
        layout,
        false,
        camera,
        share,
        camera_mid,
        share_mid.as_deref(),
        remote_shares,
        &offer_text,
    );
    let description = parse_sdp(&text, SdpType::Offer)?;
    peer.set_local_description(description)
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))?;
    let mut numbers = random_u32;
    let mut plan = plan.clone();
    if let Some(mid) = share_mid
        .as_ref()
        .filter(|mid| !plan.screen_share_mids.contains(mid))
    {
        plan.screen_share_mids.push(mid.clone());
    }
    let mut signaled = to_teams_offer(
        &directions_for(
            layout,
            hold,
            camera,
            share,
            camera_mid,
            share_mid.as_deref(),
            remote_shares,
            &text,
        ),
        &plan,
        &mut numbers,
    )?;
    signaled.sdp = direct_sdp(&signaled.sdp, gathered, SdpKind::Offer).await?;
    if gathered.is_some()
        && let Some(role) = negotiated_setup(
            peer.current_local_description()
                .map(|local| local.to_string())
                .as_deref(),
            peer.current_remote_description()
                .map(|remote| remote.to_string())
                .as_deref(),
        )
    {
        signaled.sdp = signaled
            .sdp
            .replace("a=setup:actpass", &format!("a=setup:{role}"));
    }
    Ok((text, signaled))
}

/// libwebrtc panics while decoding some parse errors, which used to kill the call task and leave the call hanging.
pub(super) fn parse_sdp(text: &str, kind: SdpType) -> Result<SessionDescription> {
    match std::panic::catch_unwind(|| SessionDescription::parse(text, kind)) {
        Ok(Ok(description)) => Ok(description),
        Ok(Err(error)) => Err(Error::Sdp(format!(
            "{kind:?} rejected: {}",
            error.description
        ))),
        Err(_) => Err(Error::Sdp(format!(
            "{kind:?} rejected, libwebrtc gave no readable reason"
        ))),
    }
}

pub(super) async fn rollback_offer(peer: &PeerConnection) {
    if let Ok(rollback) = parse_sdp("", SdpType::Rollback) {
        let _ = peer.set_local_description(rollback).await;
    }
}

pub(super) async fn rollback_remote_offer(peer: &PeerConnection) {
    if let Ok(rollback) = parse_sdp("", SdpType::Rollback) {
        let _ = peer.set_remote_description(rollback).await;
    }
}

pub(super) fn is_glare_rejection(body: &serde_json::Value) -> bool {
    body["mediaNegotiationFailure"]["code"].as_i64() == Some(GLARE_CODE)
}

pub(super) fn offer_reply_tags(body: &serde_json::Value) -> Vec<String> {
    let links = &body["mediaNegotiation"]["links"];
    ["mediaAnswer", "rejection"]
        .iter()
        .filter_map(|name| links[*name].as_str().and_then(callback_tag))
        .collect()
}

#[cfg(test)]
mod reply_tag_tests {
    use super::*;

    #[test]
    fn answers_and_rejections_match_the_offer_by_tag() {
        let tags = vec!["aaaa1111".to_owned(), "bbbb2222".to_owned()];
        assert!(answers_offer(
            "callAgent/c/aaaa1111/call/mediaAnswer/",
            &tags
        ));
        assert!(answers_offer("callAgent/c/bbbb2222/call/rejection/", &tags));
        assert!(!answers_offer(
            "callAgent/c/cccc3333/call/mediaAnswer/",
            &tags
        ));
        assert!(answers_offer("callAgent/c/cccc3333/call/mediaAnswer/", &[]));
    }

    #[test]
    fn an_offer_names_both_reply_tags() {
        let body = serde_json::json!({"mediaNegotiation": {"links": {
            "mediaAnswer": "https://t.example/callAgent/c/aaaa1111/call/mediaAnswer/",
            "rejection": "https://t.example/callAgent/c/bbbb2222/call/rejection/"}}});
        assert_eq!(
            offer_reply_tags(&body),
            vec!["aaaa1111".to_owned(), "bbbb2222".to_owned()]
        );
    }
}

pub(super) async fn apply_answer(
    peer: &PeerConnection,
    teams_answer: &str,
    signaled: &SignaledOffer,
    browser_offer: &str,
) -> Result<()> {
    let answer = to_browser_answer(teams_answer, signaled, browser_offer)?;
    set_browser_answer(peer, &answer).await
}

/// libwebrtc does not restart a send stream that an inactive answer stopped, so a hold keeps the streams and only mutes.
pub(super) async fn apply_hold_answer(
    peer: &PeerConnection,
    teams_answer: &str,
    signaled: &SignaledOffer,
    browser_offer: &str,
) -> Result<()> {
    let answer = to_browser_answer(teams_answer, signaled, browser_offer)?;
    set_browser_answer(peer, &keep_media_running(&answer, browser_offer)).await
}

async fn set_browser_answer(peer: &PeerConnection, answer: &str) -> Result<()> {
    let description = parse_sdp(answer, SdpType::Answer)?;
    peer.set_remote_description(description)
        .await
        .map_err(|error| Error::Webrtc(error.to_string()))
}

/// Writes an SDP with ICE credentials, fingerprints and addresses masked, for debugging the dialect.
pub(super) fn dump_sdp(options: &CallOptions, name: &str, sdp: &str) {
    let Some(directory) = &options.sdp_dump_dir else {
        return;
    };
    let masked: Vec<String> = sdp
        .lines()
        .map(|line| {
            for prefix in ["a=ice-ufrag:", "a=ice-pwd:", "a=fingerprint:", "a=crypto:"] {
                if line.starts_with(prefix) {
                    return format!("{prefix}<masked>");
                }
            }
            line.split(' ')
                .map(mask_address)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let _ = std::fs::create_dir_all(directory);
    let _ = std::fs::write(directory.join(name), masked.join("\n"));
}

pub(super) fn dump_timeline(options: &CallOptions, timeline: &Timeline, failure: Option<&Error>) {
    let Some(directory) = &options.sdp_dump_dir else {
        return;
    };
    let mut lines: Vec<String> = timeline
        .entries()
        .iter()
        .map(|entry| {
            format!(
                "{:>6} ms  {}  {}",
                entry.elapsed_ms, entry.label, entry.detail
            )
        })
        .collect();
    lines.push(match failure {
        Some(error) => format!("failed: {error}"),
        None => "ended without error".to_owned(),
    });
    let _ = std::fs::create_dir_all(directory);
    let _ = std::fs::write(directory.join("timeline.txt"), lines.join("\n"));
}

fn mask_address(token: &str) -> String {
    let dotted_quad =
        token.split('.').count() == 4 && token.split('.').all(|part| part.parse::<u8>().is_ok());
    let colon_address =
        token.matches(':').count() >= 2 && !token.starts_with("a=") && !token.starts_with("urn:");
    if (dotted_quad && token != "10.10.10.10" && token != "0.0.0.0" && token != "127.0.0.1")
        || colon_address
    {
        "<addr>".to_owned()
    } else {
        token.to_owned()
    }
}
