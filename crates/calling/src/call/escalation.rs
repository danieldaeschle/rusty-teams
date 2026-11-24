use super::*;

pub(super) struct NextPeer {
    pub(super) session: PeerSession,
    pub(super) offered: Option<OfferedMedia>,
    pub(super) answered: Option<AnsweredMedia>,
    pub(super) leg_links: BTreeMap<String, String>,
    pub(super) remote_audio: Option<RtcAudioTrack>,
    pub(super) reply_tags: Vec<String>,
    pub(super) deadline: Option<Instant>,
    pub(super) connected: bool,
    pub(super) retargeted: bool,
}

pub(super) async fn next_peer_event(next: &mut Option<NextPeer>) -> Option<PeerEvent> {
    match next {
        Some(next) => next.session.events.recv().await,
        None => std::future::pending().await,
    }
}

pub(super) fn video_possible(remote_set: bool, layout: Layout, path: MediaPath) -> bool {
    remote_set && !(layout == Layout::OneToOne && path == MediaPath::Mixer)
}

/// A direct 1:1 whose share line Teams rejected cannot share on it, and this client cannot move
/// the call to the mixer by itself: say so instead of failing on the stopped line.
pub(super) fn direct_share_refused(direct: bool, sharing: bool, share_line_rejected: bool) -> bool {
    direct && !sharing && share_line_rejected
}

/// Re-queuing while `next` is set spins the call task (defer -> pending -> defer...).
pub(super) fn camera_may_start(pending: bool, next_is_some: bool, possible: bool) -> bool {
    pending && !next_is_some && possible
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn start_escalation(
    factory: &PeerConnectionFactory,
    grant: &RelayGrant,
    options: &CallOptions,
    track: &RtcAudioTrack,
    remote: &RemoteCall,
    links: &CallbackLinks,
    media_leg_id: &str,
    acceptance_links: &BTreeMap<String, String>,
) -> Result<NextPeer> {
    let url = acceptance_links
        .get("mediaRenegotiation")
        .ok_or_else(|| Error::Signaling("acceptance without a mediaRenegotiation link".into()))?;
    let session = open_peer(factory, grant, &options.relay_host, false)?;
    let media = create_offer(&session.peer, factory, track, Layout::Conference, None).await?;
    let body = renegotiation_body(
        &remote.participant,
        links,
        &media.signaled.sdp,
        media_leg_id,
        &media.modalities,
        true,
        false,
    );
    let reply_tags = offer_reply_tags(&body);
    remote
        .signaling
        .post_json("POST mediaRenegotiation", url, body)
        .await?;
    Ok(NextPeer {
        session,
        offered: Some(media),
        answered: None,
        leg_links: acceptance_links.clone(),
        remote_audio: None,
        reply_tags,
        deadline: Some(Instant::now() + NEXT_PEER_LIMIT),
        connected: false,
        retargeted: false,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn escalate(
    factory: &PeerConnectionFactory,
    grant: &RelayGrant,
    options: &CallOptions,
    track: &RtcAudioTrack,
    remote: &RemoteCall,
    links: &CallbackLinks,
    media_leg_id: &str,
    acceptance_links: &BTreeMap<String, String>,
    negotiator: &mut MediaNegotiator,
    live: &mut Live,
    timeline: &Timeline,
) -> Option<NextPeer> {
    match start_escalation(
        factory,
        grant,
        options,
        track,
        remote,
        links,
        media_leg_id,
        acceptance_links,
    )
    .await
    {
        Ok(pending) => {
            timeline.record("escalation", "new peer connection, new offer sent");
            if let Some(media) = &pending.offered {
                timeline.record_sdp("local offer", &media.signaled.sdp);
            }
            live.peers.push(pending.session.peer.clone());
            Some(pending)
        }
        Err(error) => {
            negotiator.escalation_failed();
            timeline.record("escalation failed", error.to_string());
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn answer_on_new_peer(
    factory: &PeerConnectionFactory,
    grant: &RelayGrant,
    options: &CallOptions,
    track: &RtcAudioTrack,
    teams_offer: &str,
    camera_on: bool,
    camera: Option<&RtcVideoTrack>,
    share: Option<&RtcVideoTrack>,
) -> Result<(PeerSession, AnsweredMedia)> {
    let session = open_peer(factory, grant, &options.relay_host, false)?;
    // Keep every video mid from the mixer offer (camera, gallery, share).
    // Reject would stop them and leave set_track InvalidState after the new
    // peer replaces the old one. When the camera is already on, answer sendrecv
    // so the remote keeps receiving across the escalation.
    let video = match (camera_on, camera) {
        (true, Some(camera)) => AnswerVideo::Send(camera),
        _ => AnswerVideo::Receive,
    };
    let answered = answer_offer_with(
        &session.peer,
        Some(track),
        teams_offer,
        video,
        None,
        share,
        camera_on,
        None,
        true,
        true,
    )
    .await?;
    Ok((session, answered))
}

pub(super) fn video_lines_from_answer(
    peer: &PeerConnection,
    answered: &AnsweredMedia,
) -> Option<VideoLines> {
    let camera_mid = answered.camera_mid.as_deref()?;
    let camera = peer_transceiver(peer, camera_mid)?;
    let share = share_carrier_mid(&answered.remote).and_then(|mid| peer_transceiver(peer, &mid));
    Some(VideoLines { camera, share })
}

pub(super) async fn send_media_answer(
    remote: &RemoteCall,
    links: &CallbackLinks,
    reply_links: &BTreeMap<String, String>,
    answered: &AnsweredMedia,
    media_leg_id: &str,
) -> Result<(BTreeMap<String, String>, String)> {
    let url = reply_links
        .get("mediaAnswer")
        .ok_or_else(|| Error::Signaling("renegotiation without a mediaAnswer link".into()))?;
    let body = escalation_answer_body(
        &remote.participant,
        links,
        &answered.answer_sdp,
        media_leg_id,
    );
    let response = remote
        .signaling
        .post_json_body("POST mediaAnswer", url, body)
        .await?;
    Ok((nested_links(&response), object_keys(&response)))
}

pub(super) fn absorb_answered_links(
    into: &mut BTreeMap<String, String>,
    extra: &BTreeMap<String, String>,
    timeline: &Timeline,
    response_keys: &str,
) {
    if extra.is_empty() {
        if !response_keys.is_empty() {
            timeline.record(
                "media answer links",
                format!("none, body keys {response_keys}"),
            );
        }
        return;
    }
    let names = extra.keys().cloned().collect::<Vec<_>>().join(",");
    timeline.record("media answer links", names);
    into.extend(extra.clone());
}

#[allow(clippy::too_many_arguments)]
pub(super) fn negotiate_video(
    video: &mut VideoReceive,
    signaling: &Arc<Signaling>,
    control: &CallControl,
    timeline: &Timeline,
    answer_sdp: &str,
    media: &OfferedMedia,
    from_mixer: bool,
    links: &BTreeMap<String, String>,
    roster: &Roster,
) {
    negotiate_video_lines(
        video,
        signaling,
        control,
        timeline,
        answer_sdp,
        &media.signaled.lines,
        from_mixer,
        links,
        roster,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn negotiate_video_lines(
    video: &mut VideoReceive,
    signaling: &Arc<Signaling>,
    control: &CallControl,
    timeline: &Timeline,
    teams_sdp: &str,
    lines: &[crate::sdp::SignaledLine],
    from_mixer: bool,
    links: &BTreeMap<String, String>,
    roster: &Roster,
) {
    let streams = match stream_lines(teams_sdp, lines) {
        Ok(streams) => streams,
        Err(error) => {
            timeline.record("video slots unknown", error.to_string());
            return;
        }
    };
    let table = SlotTable::from_streams(&streams);
    let link = links
        .get("applyChannelParameters")
        .or_else(|| links.get("controlVideoStreaming"));
    timeline.record(
        "video slots",
        format!(
            "{} receive slot(s), screen {}, fromMixer={from_mixer}, request link {}",
            table.people.len(),
            table.screen.is_some(),
            link.is_some()
        ),
    );
    let requester = link.filter(|_| from_mixer).map(|url| {
        SourceRequester::start(
            signaling.clone(),
            url.clone(),
            video.router().clone(),
            control.video.clone(),
            timeline.clone(),
        )
    });
    video.negotiated(table, requester);
    send_receive_change(control, video.reselect(roster));
}

pub(super) fn tell_bound_camera(
    remote: &RemoteCall,
    links: &BTreeMap<String, String>,
    local_video: &LocalVideo,
    all_mids: &[String],
    request_id: &mut u32,
    timeline: &Timeline,
) {
    let mid = local_video
        .camera_mid()
        .unwrap_or_else(|| CAMERA_MID.to_owned());
    let suffix = if local_video.camera_on() {
        ";v_2"
    } else {
        ";v_1"
    };
    let tag = negotiation_tag(&remote.participant.participant_id, suffix);
    let descriptions = media_descriptions(
        all_mids,
        MediaDescription::camera_line(mid, local_video.camera_on()),
    );
    tell_media_descriptions(remote, links, descriptions, Some(tag), request_id, timeline);
}

pub(super) fn tell_bound_share(
    remote: &RemoteCall,
    links: &BTreeMap<String, String>,
    local_video: &LocalVideo,
    all_mids: &[String],
    request_id: &mut u32,
    timeline: &Timeline,
) {
    if !local_video.sharing() {
        return;
    }
    let mid = local_video
        .share_mid()
        .unwrap_or_else(|| SHARE_MID.to_owned());
    let tag = negotiation_tag(&remote.participant.participant_id, ";ss_1");
    let descriptions = media_descriptions(all_mids, MediaDescription::share_line(mid, true));
    tell_media_descriptions(remote, links, descriptions, Some(tag), request_id, timeline);
}

pub(super) async fn camera_devices(mode: VideoMode) -> Vec<CameraDevice> {
    match mode {
        VideoMode::Pattern => vec![CameraDevice {
            key: "pattern".into(),
            name: "Test pattern".into(),
        }],
        VideoMode::Off => Vec::new(),
        VideoMode::Platform => tokio::task::spawn_blocking(list_cameras)
            .await
            .unwrap_or_default(),
    }
}

pub(super) fn refresh_cameras(
    updates: mpsc::UnboundedSender<CallUpdate>,
    mode: VideoMode,
    timeline: Timeline,
) {
    tokio::spawn(async move {
        let started = Instant::now();
        let cameras = camera_devices(mode).await;
        timeline.record(
            "camera list",
            format!(
                "{} camera(s) in {} ms",
                cameras.len(),
                started.elapsed().as_millis()
            ),
        );
        let _ = updates.send(CallUpdate::Cameras(cameras));
    });
}

pub(super) const SHARE_THUMBNAIL_WINDOWS: usize = 12;

pub(super) const SHARE_THUMBNAIL_WIDTH: u32 = 320;

pub(super) const SHARE_THUMBNAIL_HEIGHT: u32 = 180;

pub(super) fn announce_share(control: &CallControl, source: &ShareSource) {
    control.send(CallUpdate::ShareKind(Some(source.kind)));
    control.send(CallUpdate::LocalShare(Some(source.label())));
}

pub(super) fn clear_share(control: &CallControl) {
    control.send(CallUpdate::ShareKind(None));
    control.send(CallUpdate::LocalShare(None));
}

pub(super) fn refresh_share_sources(
    updates: mpsc::UnboundedSender<CallUpdate>,
    mode: VideoMode,
    with_thumbnails: bool,
) {
    tokio::spawn(async move {
        let picker = current_share_picker();
        let sources = match mode {
            VideoMode::Pattern => vec![
                ShareSource {
                    id: 1,
                    kind: ShareKind::Screen,
                    title: "Test screen".into(),
                },
                ShareSource {
                    id: 2,
                    kind: ShareKind::Window,
                    title: "Test window".into(),
                },
            ],
            VideoMode::Off => Vec::new(),
            VideoMode::Platform if picker == SharePicker::Portal => Vec::new(),
            VideoMode::Platform => tokio::task::spawn_blocking(list_share_sources)
                .await
                .unwrap_or_default(),
        };
        let sources = share_sources_for_menu(picker, sources);
        let _ = updates.send(CallUpdate::ShareSources(sources.clone()));
        if picker == SharePicker::Portal || !with_thumbnails {
            return;
        }
        let wanted: Vec<ShareSource> = sources
            .iter()
            .filter(|source| source.kind == ShareKind::Screen)
            .chain(
                sources
                    .iter()
                    .filter(|source| source.kind == ShareKind::Window)
                    .take(SHARE_THUMBNAIL_WINDOWS),
            )
            .cloned()
            .collect();
        for source in wanted {
            let picture = match mode {
                VideoMode::Platform => {
                    let wanted = source.clone();
                    tokio::task::spawn_blocking(move || {
                        share_thumbnail(&wanted, SHARE_THUMBNAIL_WIDTH, SHARE_THUMBNAIL_HEIGHT)
                    })
                    .await
                    .ok()
                    .flatten()
                }
                VideoMode::Pattern => Some(VideoPicture::solid(
                    SHARE_THUMBNAIL_WIDTH,
                    SHARE_THUMBNAIL_HEIGHT,
                    120,
                    80,
                    40,
                )),
                VideoMode::Off => None,
            };
            if let Some(picture) = picture
                && updates
                    .send(CallUpdate::ShareThumbnail {
                        source,
                        picture: Arc::new(picture),
                    })
                    .is_err()
            {
                return;
            }
        }
    });
}

/// The web client sends every line it knows: the changed ones with their labels and directions, all
/// others `recvonly`. Sending only the changed line lets Teams reset the others.
pub(super) fn media_descriptions(
    all_mids: &[String],
    changed: MediaDescription,
) -> Vec<MediaDescription> {
    media_descriptions_many(all_mids, vec![changed])
}

pub(super) fn media_descriptions_many(
    all_mids: &[String],
    changed: Vec<MediaDescription>,
) -> Vec<MediaDescription> {
    let mut descriptions: Vec<MediaDescription> = all_mids
        .iter()
        .filter(|mid| !changed.iter().any(|line| line.mid == **mid))
        .map(|mid| MediaDescription {
            mid: mid.clone(),
            direction: "recvonly",
            label: None,
        })
        .collect();
    descriptions.extend(changed);
    descriptions
}

pub(super) fn negotiation_tag(participant_id: &str, suffix: &str) -> String {
    format!("{participant_id}{suffix}")
}

pub(super) fn tell_media_descriptions(
    remote: &RemoteCall,
    links: &BTreeMap<String, String>,
    descriptions: Vec<MediaDescription>,
    negotiation_tag: Option<String>,
    request_id: &mut u32,
    timeline: &Timeline,
) {
    let Some(url) = links.get("updateMediaDescriptions").cloned() else {
        timeline.record("updateMediaDescriptions skipped", "no link from the mixer");
        return;
    };
    *request_id += 1;
    let request_id = *request_id;
    let signaling = remote.signaling.clone();
    let timeline = timeline.clone();
    tokio::spawn(async move {
        let sent = format!("{} line(s) request {request_id}", descriptions.len());
        if let Err(error) = signaling
            .update_media_descriptions(&url, &descriptions, request_id, negotiation_tag.as_deref())
            .await
        {
            timeline.record("updateMediaDescriptions failed", format!("{sent}: {error}"));
        }
    });
}

pub(super) fn attach_share_line(
    peer: &PeerConnection,
    factory: &PeerConnectionFactory,
    local_video: &mut LocalVideo,
    camera_mid: Option<&str>,
) -> Result<()> {
    let camera = camera_mid.and_then(|mid| peer_transceiver(peer, mid));
    if !local_video.has_lines() && camera.is_none() {
        return Err(Error::Webrtc("no video line negotiated".into()));
    }
    let share = add_share_line(peer, factory)?;
    local_video
        .add_share_line(share.clone(), camera)
        .inspect_err(|_| {
            let _ = share.stop();
        })
}

pub(super) fn start_camera_line(
    local_video: &mut LocalVideo,
    peer: &PeerConnection,
    direct_mid: Option<&str>,
    choice: &DeviceChoice,
) -> Result<()> {
    if !local_video.has_lines() {
        let mid = direct_mid.ok_or_else(|| Error::Webrtc("no video line negotiated".into()))?;
        let camera = peer_transceiver(peer, mid)
            .ok_or_else(|| Error::Webrtc("no transceiver for the camera line".into()))?;
        if !matches!(
            camera.direction(),
            RtpTransceiverDirection::SendRecv | RtpTransceiverDirection::SendOnly
        ) {
            peer.add_track(
                MediaStreamTrack::Video(local_video.camera_track()),
                &[CAMERA_STREAM],
            )
            .map_err(|error| Error::Webrtc(error.to_string()))?;
        }
        local_video.set_lines(VideoLines {
            camera,
            share: None,
        })?;
    }
    local_video.start_camera(choice)
}

pub(super) fn defer_to_new_peer(
    change: Change,
    deferred: &mut Vec<Change>,
    local_video: &mut LocalVideo,
    control: &CallControl,
) {
    match change {
        Change::Camera(true) => {
            control.send(CallUpdate::Camera(true));
            deferred.push(change);
        }
        Change::Share(true) => deferred.push(change),
        change => undo_change(change, local_video, control),
    }
}

/// A server escalation that fails leaves the call direct, so only what waited for the new peer is undone.
pub(super) fn settle_failed_escalation(
    server: bool,
    layout: Layout,
    negotiator: &mut MediaNegotiator,
    deferred: &mut Vec<Change>,
    local_video: &mut LocalVideo,
    control: &CallControl,
) {
    if server {
        negotiator.server_escalation_failed();
        for change in deferred.drain(..) {
            undo_change(change, local_video, control);
        }
    } else if layout == Layout::OneToOne {
        drop_local_media(local_video, control);
    }
}

pub(super) fn drop_local_media(local_video: &mut LocalVideo, control: &CallControl) {
    if local_video.sharing() {
        local_video.stop_share();
        clear_share(control);
    }
    if local_video.camera_on() {
        local_video.stop_camera();
        control.send(CallUpdate::Camera(false));
    }
}

pub(super) fn undo_change(change: Change, local_video: &mut LocalVideo, control: &CallControl) {
    match change {
        Change::Camera(true) => {
            local_video.stop_camera();
            control.send(CallUpdate::Camera(false));
        }
        Change::Camera(false) => control.send(CallUpdate::Camera(false)),
        Change::Share(true) => {
            local_video.stop_share();
            clear_share(control);
            control.send(CallUpdate::Notice(notice_with_detail(
                SHARING_START_NOTICE,
                "",
            )));
        }
        Change::Share(false) => {}
        Change::Hold(_) => control.send(CallUpdate::Notice(HOLD_CHANGE_FAILED_NOTICE.into())),
    }
}

pub(super) async fn apply_background(
    choice: BackgroundChoice,
    local_video: &LocalVideo,
    control: &CallControl,
    timeline: &Timeline,
) {
    match choice {
        BackgroundChoice::None => local_video.set_background(false, None),
        BackgroundChoice::Blur => local_video.set_background(true, None),
        BackgroundChoice::Image(path) => {
            match tokio::task::spawn_blocking(move || BackgroundPicture::load(&path)).await {
                Ok(Ok(picture)) => local_video.set_background(true, Some(Arc::new(picture))),
                Ok(Err(error)) => {
                    timeline.record("background image failed", error.to_string());
                    control.send(CallUpdate::Notice(BACKGROUND_IMAGE_FAILED_NOTICE.into()));
                }
                Err(_) => {}
            }
        }
    }
}

pub(super) fn send_receive_change(control: &CallControl, change: ReceiveChange) {
    if let Some(sharer) = change.screen_sharer {
        control.send(CallUpdate::ScreenShare(sharer));
    }
}

#[cfg(test)]
mod media_description_tests {
    use super::*;

    #[test]
    fn a_flip_describes_every_line() {
        let mids = ["0".to_owned(), "1".to_owned(), "11".to_owned()];
        let all = media_descriptions(&mids, MediaDescription::share_line("11", true));
        assert_eq!(all.len(), mids.len());
        assert_eq!(all[0].mid, "0");
        assert_eq!(all[0].direction, "recvonly");
        assert_eq!(all[1].direction, "recvonly");
        assert_eq!(all[2].mid, "11");
        assert_eq!(all[2].direction, "sendrecv");
        assert_eq!(all[2].label, Some("applicationsharing-video"));
    }

    #[test]
    fn a_publish_off_flip_can_stop_camera_and_share_together() {
        let mids = ["0".to_owned(), "1".to_owned(), "11".to_owned()];
        let changed = vec![
            MediaDescription::camera_line("1", false),
            MediaDescription::share_line("11", false),
        ];
        let all = media_descriptions_many(&mids, changed);
        assert_eq!(all.len(), mids.len());
        assert_eq!(all[0].direction, "recvonly");
        assert_eq!(all[1].mid, "1");
        assert_eq!(all[1].direction, "recvonly");
        assert_eq!(all[2].mid, "11");
        assert_eq!(all[2].direction, "recvonly");
    }

    #[test]
    fn the_tag_is_the_participant_id_plus_the_change_suffix() {
        assert_eq!(negotiation_tag("p1", ";ss_1"), "p1;ss_1");
        assert_eq!(negotiation_tag("p1", ";v_2"), "p1;v_2");
    }
}
