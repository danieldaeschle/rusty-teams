use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use futures_util::StreamExt;
use libwebrtc::video_stream::native::NativeVideoStream;
use libwebrtc::video_track::RtcVideoTrack;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::roster::Roster;
use crate::signaling::Signaling;
use crate::timeline::Timeline;
use crate::video_frame::{FrameGate, VideoHub, VideoKey, picture_from_frame};
use crate::video_layout::{
    Assignment, SlotTable, SourceRequest, VideoRouter, plan_receive, prioritize, requests_for,
};

const HISTORY_LIMIT: usize = 16;
const REQUEST_LABEL: &str = "POST applyChannelParameters";

#[derive(Default)]
pub struct VideoCounters {
    pub received: AtomicU64,
    pub published: AtomicU64,
}

pub fn spawn_video_pump(
    track: RtcVideoTrack,
    mid: String,
    router: VideoRouter,
    hub: Arc<VideoHub>,
    counters: Arc<VideoCounters>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut stream = NativeVideoStream::new(track);
        let mut gates: HashMap<VideoKey, FrameGate> = HashMap::new();
        while let Some(frame) = stream.next().await {
            counters.received.fetch_add(1, Ordering::Relaxed);
            let Some(key) = router.key_for(&mid) else { continue };
            let gate = gates.entry(key.clone()).or_insert_with(|| FrameGate::new(key.max_fps()));
            if !gate.admit(Instant::now()) {
                continue;
            }
            let Some(picture) = picture_from_frame(frame, key.max_width()) else { continue };
            counters.published.fetch_add(1, Ordering::Relaxed);
            hub.publish(key, picture);
        }
    })
}

struct QueuedRequest {
    request: SourceRequest,
    route_after: Option<VideoKey>,
}

/// Sends source requests one at a time so their sequence numbers stay in order.
pub struct SourceRequester {
    queue: mpsc::UnboundedSender<QueuedRequest>,
    task: JoinHandle<()>,
}

impl SourceRequester {
    pub fn start(signaling: Arc<Signaling>, url: String, router: VideoRouter, hub: Arc<VideoHub>, timeline: Timeline) -> Self {
        let (queue, mut requests) = mpsc::unbounded_channel::<QueuedRequest>();
        let task = tokio::spawn(async move {
            let mut sequence_number = 0u64;
            while let Some(queued) = requests.recv().await {
                sequence_number += 1;
                let body = queued.request.body(sequence_number);
                match signaling.post_json(REQUEST_LABEL, &url, body).await {
                    Ok(()) => timeline.record(
                        "video source",
                        format!("mid {} source {:?} accepted", queued.request.mid, queued.request.source),
                    ),
                    Err(error) => timeline.record("video source failed", error.to_string()),
                }
                if let Some(key) = &queued.route_after {
                    hub.forget(key);
                }
                router.set(&queued.request.mid, queued.route_after);
            }
        });
        SourceRequester { queue, task }
    }

    fn send(&self, request: SourceRequest, route_after: Option<VideoKey>) {
        let _ = self.queue.send(QueuedRequest { request, route_after });
    }

}

impl Drop for SourceRequester {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReceiveChange {
    pub screen_sharer: Option<Option<String>>,
}

pub struct VideoReceive {
    own_mri: String,
    router: VideoRouter,
    table: SlotTable,
    held: Vec<Option<Assignment>>,
    screen_held: Option<Assignment>,
    history: Vec<String>,
    requester: Option<SourceRequester>,
}

impl VideoReceive {
    pub fn new(own_mri: String, router: VideoRouter) -> Self {
        VideoReceive {
            own_mri,
            router,
            table: SlotTable::default(),
            held: Vec::new(),
            screen_held: None,
            history: Vec::new(),
            requester: None,
        }
    }

    pub fn router(&self) -> &VideoRouter {
        &self.router
    }

    pub fn negotiated(&mut self, table: SlotTable, requester: Option<SourceRequester>) {
        self.requester = requester;
        self.held = vec![None; table.people.len()];
        self.screen_held = None;
        self.table = table;
    }

    pub fn is_ready(&self) -> bool {
        self.requester.is_some() && !self.table.is_empty()
    }

    pub fn note_speakers(&mut self, speakers: &[String]) -> bool {
        let Some(first) = speakers.iter().find(|mri| **mri != self.own_mri) else {
            return false;
        };
        if self.history.first() == Some(first) {
            return false;
        }
        self.history.retain(|mri| mri != first);
        self.history.insert(0, first.clone());
        self.history.truncate(HISTORY_LIMIT);
        true
    }

    pub fn reselect(&mut self, roster: &Roster) -> ReceiveChange {
        let mut change = ReceiveChange::default();
        let Some(requester) = &self.requester else { return change };
        let candidates = prioritize(roster.video_candidates(&self.own_mri), &self.history);
        let next = plan_receive(&self.held, &candidates);
        for request in requests_for(&self.held, &next, &self.table.people) {
            let slot = self.table.people.iter().position(|slot| slot.mid == request.mid);
            let route_after = slot
                .and_then(|index| next[index].as_ref())
                .map(|assignment| VideoKey::Person(assignment.mri.clone()));
            self.router.set(&request.mid, None);
            requester.send(request, route_after);
        }
        self.held = next;
        if let Some(slot) = &self.table.screen {
            let wanted = roster
                .screen_share(&self.own_mri)
                .map(|(mri, source)| Assignment { mri, source });
            if wanted != self.screen_held {
                let route_after = wanted.as_ref().map(|_| VideoKey::Screen);
                self.router.set(&slot.mid, None);
                requester.send(
                    SourceRequest {
                        mid: slot.mid.clone(),
                        stream_id: slot.stream_id,
                        source: wanted.as_ref().map(|assignment| assignment.source),
                        screen: true,
                    },
                    route_after,
                );
                change.screen_sharer = Some(wanted.as_ref().map(|assignment| assignment.mri.clone()));
                self.screen_held = wanted;
            }
        }
        change
    }
}
