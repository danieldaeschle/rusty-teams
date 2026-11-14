use super::*;

#[derive(Clone)]
pub(super) struct RemoteCall {
    pub(super) signaling: Arc<Signaling>,
    pub(super) conversation: Conversation,
    pub(super) participant: Participant,
    pub(super) threadless: bool,
}

#[derive(Default)]
pub(super) struct Live {
    pub(super) peers: Vec<PeerConnection>,
    pub(super) audio: Option<AudioSetup>,
    pub(super) remote: Option<RemoteCall>,
    pub(super) broker: Option<JoinHandle<()>>,
    pub(super) broker_running: Arc<AtomicBool>,
    pub(super) callbacks: Option<mpsc::UnboundedReceiver<TrouterCallback>>,
    pub(super) route_id: Option<String>,
    pub(super) skip_leave: bool,
    pub(super) cancel_leave: bool,
    pub(super) video_tasks: Vec<JoinHandle<()>>,
    pub(super) video_counters: Arc<VideoCounters>,
}

pub(super) async fn tear_down(
    mut live: Live,
    inner: &Inner,
    timeline: &Timeline,
    report: &mut CallReport,
    cancelled: bool,
) {
    for peer in &live.peers {
        peer.close();
    }
    if let Some(audio) = live.audio.take() {
        audio.stop().await;
    }
    live.broker_running.store(false, Ordering::SeqCst);
    if !live.skip_leave
        && let Some(remote) = &live.remote
    {
        let reason = if cancelled {
            LeaveReason::Cancel
        } else {
            LeaveReason::Hangup
        };
        match remote
            .signaling
            .leave(&remote.conversation, &remote.participant, reason)
            .await
        {
            Ok(()) => report.left_cleanly = true,
            Err(error) => timeline.record("leave failed", error.to_string()),
        }
    }
    if report.end.is_none()
        && let Some(callbacks) = live.callbacks.as_mut()
        && live.remote.is_some()
    {
        let waited = timeout(END_CALLBACK_WAIT, async {
            while let Some(callback) = callbacks.recv().await {
                let event = handle_callback(&callback, timeline);
                if let Ok(CallEvent::End(end)) = event {
                    return Some(end);
                }
            }
            None
        })
        .await;
        report.end = waited.ok().flatten();
    }
    if let Some(broker) = live.broker.take() {
        broker.abort();
    }
    for task in live.video_tasks.drain(..) {
        task.abort();
    }
    if let Some(route_id) = &live.route_id {
        inner.unregister_route(route_id);
    }
    timeline.record("closed", "peer closed, call route removed");
}

pub(super) fn setup_end_kind(end: &CallEnd) -> EndKind {
    match end.kind() {
        EndKind::Normal | EndKind::Other => EndKind::Unavailable,
        kind => kind,
    }
}

pub(super) fn handle_callback(
    callback: &TrouterCallback,
    timeline: &Timeline,
) -> Result<CallEvent> {
    let body = decode_body(callback)?;
    let (_, event) = classify(&callback.path, body)?;
    let detail = match &event {
        CallEvent::Acceptance(acceptance) => format!(
            "answer sdp {} bytes, fromMixer={}",
            acceptance.sdp.len(),
            acceptance.from_mixer
        ),
        CallEvent::End(end) => format!(
            "code={} subCode={} phrase={}",
            end.code, end.sub_code, end.phrase
        ),
        CallEvent::Progress(status) => format!("{status:?}"),
        CallEvent::ConversationEnd(body)
        | CallEvent::Rejection(body)
        | CallEvent::AddParticipantFailure(body)
        | CallEvent::RetargetCompletion(body) => outcome_summary(body),
        CallEvent::ControlVideoStreaming(body) => parse_request(body)
            .map_or_else(|| "body not understood".to_owned(), |request| request.shape),
        _ => String::new(),
    };
    timeline.record(format!("Trouter {}", event.name()), detail);
    match &event {
        CallEvent::Acceptance(acceptance) => timeline.record_sdp("remote answer", &acceptance.sdp),
        CallEvent::MediaAnswer(answer) => timeline.record_sdp("remote answer", &answer.sdp),
        CallEvent::MediaRenegotiation(renegotiation) => {
            timeline.record_sdp("remote offer", &renegotiation.sdp)
        }
        _ => {}
    }
    Ok(event)
}

pub(super) async fn poll_broker(
    signaling: Arc<Signaling>,
    conversation: Conversation,
    running: Arc<AtomicBool>,
) {
    let Ok(first) = conversation.link("subscribe") else {
        return;
    };
    let mut url = first.to_owned();
    while running.load(Ordering::SeqCst) {
        match signaling.poll_broker(&url).await {
            Ok(Some(next)) => url = next,
            Ok(None) => tokio::time::sleep(BROKER_IDLE_WAIT).await,
            Err(_) => tokio::time::sleep(BROKER_RETRY_WAIT).await,
        }
    }
}
