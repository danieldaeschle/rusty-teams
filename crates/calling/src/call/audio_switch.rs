use super::*;

#[derive(Default)]
pub(super) struct Recording {
    pub(super) samples: Vec<i16>,
    pub(super) current_second: Vec<i16>,
    pub(super) per_second: Vec<(f32, f32)>,
}

pub(super) fn withhold_media(
    muted: &mut bool,
    audio: &AudioSetup,
    software_audio: Option<&SoftwareAudio>,
    remote: &RemoteCall,
    local_video: &mut LocalVideo,
    control: &CallControl,
) {
    *muted = apply_mute(
        MuteCommand::Mute,
        *muted,
        audio,
        software_audio,
        remote,
        control,
    );
    if local_video.camera_on() {
        local_video.stop_camera();
        control.send(CallUpdate::Camera(false));
    }
}

pub(super) fn record_audio_devices(
    timeline: &Timeline,
    lists: &devices::DeviceLists,
    input: Option<&AppliedDevice>,
    output: Option<&AppliedDevice>,
) {
    let name = |devices: &[AudioDevice], applied: Option<&AppliedDevice>| {
        applied
            .and_then(|applied| devices.iter().find(|device| device.index == applied.index))
            .map_or("none", |device| device.name.as_str())
            .to_owned()
    };
    timeline.record(
        "audio devices",
        format!(
            "input={} output={}",
            name(&lists.inputs, input),
            name(&lists.outputs, output)
        ),
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn follow_device_change(
    flow: Flow,
    (previous, current): (&[AudioDevice], &[AudioDevice]),
    choice: &DeviceChoice,
    applied: &mut Option<AppliedDevice>,
    touched_ids: &[String],
    select: impl Fn(u16) -> bool,
    timeline: &Timeline,
    control: &CallControl,
) -> bool {
    let system_default = devices::system_default_id(flow);
    let (index, fell_back, recovery) =
        match devices::reselect(applied.as_ref(), current, choice, system_default.as_deref()) {
            Reselect::Keep => {
                // On Windows, a Bluetooth headset profile switch or brief disconnect+reconnect
                // within the debounce window can leave the same endpoint index while the legacy
                // WASAPI render/capture thread has already died on timeout/invalidation.
                let Some(active) = applied.as_ref() else {
                    return false;
                };
                let active_touched = touched_ids
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(&active.key));
                if !active_touched {
                    return false;
                }
                let Some(still_present) = current
                    .iter()
                    .find(|device| device.key().eq_ignore_ascii_case(&active.key))
                else {
                    return false;
                };
                (still_present.index, false, true)
            }
            Reselect::Select(index) => (index, false, false),
            Reselect::FallBack(index) => (index, true, false),
        };
    let selected = select(index);
    timeline.record(
        if recovery {
            "device recovery"
        } else {
            "device change"
        },
        format!("{flow:?} {choice:?} -> index {index} fallback={fell_back} applied={selected}"),
    );
    if selected {
        *applied = current
            .iter()
            .find(|device| device.index == index)
            .map(|device| AppliedDevice {
                index,
                key: device.key(),
            });
    }
    if fell_back {
        let fallback = current.iter().find(|device| device.index == index);
        control.send(CallUpdate::Notice(devices::disconnect_notice(
            previous, choice, fallback,
        )));
    }
    selected
}

pub(super) fn apply_mute(
    command: MuteCommand,
    currently_muted: bool,
    audio: &AudioSetup,
    software_audio: Option<&SoftwareAudio>,
    remote: &RemoteCall,
    control: &CallControl,
) -> bool {
    let effect = MuteEffect::for_muted(command.target(currently_muted));
    audio.set_device_tracks_enabled(effect.track_enabled);
    if let Some(software_audio) = software_audio {
        software_audio.set_microphone_muted(effect.muted);
    }
    control.send(CallUpdate::Muted(effect.muted));
    let remote = remote.clone();
    tokio::spawn(async move {
        let _ = remote
            .signaling
            .update_endpoint_state(
                &remote.conversation,
                &remote.participant,
                effect.endpoint_is_muted,
            )
            .await;
    });
    effect.muted
}

pub(super) fn silence_for_server_mute(
    audio: &AudioSetup,
    software_audio: Option<&SoftwareAudio>,
    control: &CallControl,
) {
    let effect = MuteEffect::for_muted(true);
    audio.set_device_tracks_enabled(effect.track_enabled);
    if let Some(software_audio) = software_audio {
        software_audio.set_microphone_muted(true);
    }
    control.send(CallUpdate::Muted(true));
}

pub(super) fn silence_for_hold(
    on_hold: bool,
    muted: bool,
    audio: Option<&AudioSetup>,
    software_audio: Option<&SoftwareAudio>,
) {
    let silent = on_hold || muted;
    if let Some(audio) = audio {
        audio.set_device_tracks_enabled(MuteEffect::for_muted(silent).track_enabled);
    }
    if let Some(software_audio) = software_audio {
        software_audio.set_microphone_muted(silent);
    }
}

pub(super) fn swap_audio_track(peer: &PeerConnection, track: &RtcAudioTrack) -> Result<()> {
    let sender = peer
        .senders()
        .into_iter()
        .find(|sender| matches!(sender.track(), Some(MediaStreamTrack::Audio(_))))
        .ok_or_else(|| Error::Webrtc("no audio sender".into()))?;
    sender
        .set_track(Some(MediaStreamTrack::Audio(track.clone())))
        .map_err(|error| Error::Webrtc(error.to_string()))
}

pub(super) fn outgoing_track<'a>(
    software_audio: Option<&'a SoftwareAudio>,
    device_track: &'a RtcAudioTrack,
) -> &'a RtcAudioTrack {
    pick_track(software_audio.map(|audio| &audio.track), device_track)
}

pub(super) fn pick_track<'a, T>(software: Option<&'a T>, device: &'a T) -> &'a T {
    software.unwrap_or(device)
}

pub(super) fn software_wanted(
    mic_settings: MicSettings,
    share_sound_wanted: bool,
    local_video: &LocalVideo,
    audio: Option<&AudioSetup>,
) -> Option<SoftwareAudioPlan> {
    let sharing_sound = share_sound_wanted && local_video.sharing();
    SoftwareAudioPlan::wanted(
        mic_settings,
        sharing_sound,
        audio.is_some_and(AudioSetup::uses_devices),
    )
}

pub(super) fn restore_device_track(
    factory: &PeerConnectionFactory,
    input: &DeviceChoice,
    peer: &PeerConnection,
    device_track: &RtcAudioTrack,
    timeline: &Timeline,
) {
    use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
    if !factory.adm_recording_enabled() {
        factory.set_adm_recording_enabled(true);
        devices::select_input(factory, input);
    }
    if let Err(error) = swap_audio_track(peer, device_track) {
        timeline.record("microphone track not restored", error.to_string());
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_software_audio(
    factory: &PeerConnectionFactory,
    options: &CallOptions,
    input: &DeviceChoice,
    muted: bool,
    remote_audio: Option<RtcAudioTrack>,
    peer: &PeerConnection,
    device_track: &RtcAudioTrack,
    wanted: Option<SoftwareAudioPlan>,
    restart: bool,
    software_audio: &mut Option<SoftwareAudio>,
    mic_settings: MicSettings,
    control: &CallControl,
    timeline: &Timeline,
) {
    let running = software_audio.as_ref().map(|audio| audio.plan);
    if running == wanted && !restart {
        crate::noise_stage::apply_for_outgoing(mic_settings, software_audio.is_none());
        return;
    }
    if running.is_some() {
        timeline.record("software audio", "stopping");
    }
    *software_audio = None;
    let was_sharing_sound = running.is_some_and(|plan| plan.computer_sound);
    let Some(plan) = wanted else {
        restore_device_track(factory, input, peer, device_track, timeline);
        control.send(CallUpdate::ShareSound(false));
        crate::noise_stage::apply_for_outgoing(mic_settings, true);
        return;
    };
    let started = SoftwareAudio::start(
        factory,
        options.audio,
        options.tone_hz,
        input,
        plan,
        muted,
        remote_audio,
    )
    .and_then(|audio| swap_audio_track(peer, &audio.track).map(|()| audio));
    match started {
        Ok(audio) => {
            timeline.record(
                "software audio",
                format!("{:?}, computer sound {:?}", plan.processing, audio.loopback),
            );
            if let Some(source) = &audio.microphone_source {
                timeline.record("mic capture", source.clone());
            }
            if audio.microphone_source.is_some() {
                use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
                factory.set_adm_recording_enabled(false);
            }
            if let Some(warning) = &audio.warning {
                timeline.record("computer sound warning", warning.clone());
            }
            control.send(CallUpdate::ShareSound(plan.computer_sound));
            if let Some(notice) = audio.microphone_notice {
                control.send(CallUpdate::Notice(notice.into()));
            }
            *software_audio = Some(audio);
        }
        Err(error) => {
            timeline.record("software audio failed", error.to_string());
            restore_device_track(factory, input, peer, device_track, timeline);
            if was_sharing_sound || plan.computer_sound {
                control.send(CallUpdate::ShareSound(false));
            }
            let notice = if plan.computer_sound {
                "Computer sound could not start, sharing without it"
            } else {
                "Microphone processing could not start, using the standard microphone"
            };
            control.send(CallUpdate::Notice(notice.into()));
        }
    }
    crate::noise_stage::apply_for_outgoing(mic_settings, software_audio.is_none());
}

pub(super) async fn record_track(
    track: RtcAudioTrack,
    recording: Arc<Mutex<Recording>>,
    tone_hz: f32,
) {
    let mut stream = NativeAudioStream::new(track, SAMPLE_RATE as i32, 1);
    while let Some(frame) = stream.next().await {
        let mut recording = recording.lock().await;
        recording.samples.extend_from_slice(&frame.data);
        recording.current_second.extend_from_slice(&frame.data);
        if recording.current_second.len() >= SAMPLE_RATE as usize {
            let second: Vec<i16> = std::mem::take(&mut recording.current_second);
            let summary = (rms(&second), tone_ratio(&second, tone_hz));
            recording.per_second.push(summary);
        }
    }
}
