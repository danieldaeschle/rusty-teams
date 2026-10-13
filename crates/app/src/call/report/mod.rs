mod document;
mod markdown;
mod scrub;
mod series;
mod storage;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use calling::{CallCommand, CallDiagnostics, CallState, CallUpdate, CaptionState, EndReason, HoldState, MediaPath};
use chrono::{DateTime, Utc};

pub use document::Document;
pub use storage::{report_directory, save_report};

use super::model::CallKind;
use series::StatsSeries;

pub const SETTING_META_KEY: &str = "call_test_report";

pub type SharedRecorder = Rc<RefCell<CallRecorder>>;

#[derive(Debug, Clone, PartialEq)]
enum Outcome {
    Requested,
    Confirmed(u128),
    Failed(String),
}

#[derive(Debug, Clone)]
struct FeatureRecord {
    at_ms: u128,
    feature: &'static str,
    detail: String,
    outcome: Outcome,
}

#[derive(Debug, Clone)]
pub struct OwnIdentity {
    pub mri: String,
    pub display_name: String,
    pub email: String,
}

#[derive(Debug, Clone)]
pub struct CallRecorder {
    started_at: DateTime<Utc>,
    started: Instant,
    kind: CallKind,
    incoming: bool,
    demo: bool,
    me: OwnIdentity,
    people: Vec<(String, String)>,
    peak_participants: usize,
    states: Vec<(u128, String)>,
    connected_ms: Option<u128>,
    ended: Option<(u128, EndReason)>,
    features: Vec<FeatureRecord>,
    notices: Vec<(u128, String)>,
    captions: u32,
    series: StatsSeries,
    paths: Vec<(u128, MediaPath)>,
    diagnostics: Option<CallDiagnostics>,
}

pub fn load_enabled(store: &store::Store) -> bool {
    store.meta(SETTING_META_KEY).ok().flatten().as_deref() != Some("0")
}

pub fn shared(recorder: CallRecorder) -> SharedRecorder {
    Rc::new(RefCell::new(recorder))
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

impl CallRecorder {
    pub fn begin(kind: CallKind, incoming: bool, demo: bool, me: OwnIdentity, people: Vec<(String, String)>) -> Self {
        CallRecorder {
            started_at: Utc::now(),
            started: Instant::now(),
            kind,
            incoming,
            demo,
            me,
            people,
            peak_participants: 0,
            states: Vec::new(),
            connected_ms: None,
            ended: None,
            features: Vec::new(),
            notices: Vec::new(),
            captions: 0,
            series: StatsSeries::default(),
            paths: Vec::new(),
            diagnostics: None,
        }
    }

    fn elapsed_ms(&self) -> u128 {
        self.started.elapsed().as_millis()
    }

    pub fn feature(&mut self, feature: &'static str, detail: impl Into<String>) {
        let at_ms = self.elapsed_ms();
        self.features.push(FeatureRecord { at_ms, feature, detail: detail.into(), outcome: Outcome::Requested });
    }

    pub fn local_feature(&mut self, feature: &'static str, detail: impl Into<String>) {
        self.feature(feature, detail);
        self.confirm(feature);
    }

    pub fn command(&mut self, command: &CallCommand) {
        if let Some((feature, detail)) = describe(command) {
            self.feature(feature, detail);
        }
    }

    fn settle(&mut self, feature: &str, outcome: Outcome) {
        if let Some(record) = self
            .features
            .iter_mut()
            .rev()
            .find(|record| record.feature == feature && record.outcome == Outcome::Requested)
        {
            record.outcome = outcome;
        }
    }

    fn confirm(&mut self, feature: &str) {
        let at_ms = self.elapsed_ms();
        self.settle(feature, Outcome::Confirmed(at_ms));
    }

    fn remember_person(&mut self, mri: &str, display_name: &str) {
        if !self.people.iter().any(|(known, _)| known == mri) {
            self.people.push((mri.to_owned(), display_name.to_owned()));
        }
    }

    pub fn observe(&mut self, update: &CallUpdate) {
        let at_ms = self.elapsed_ms();
        match update {
            CallUpdate::State(state) => self.note_state(state, at_ms),
            CallUpdate::OwnIdentity { mri } => self.me.mri.clone_from(mri),
            CallUpdate::Muted(_) => self.confirm("mute"),
            CallUpdate::Camera(_) => self.confirm("camera"),
            CallUpdate::LocalShare(_) | CallUpdate::ShareSound(_) => self.confirm("share"),
            CallUpdate::Hold(state) => {
                self.states.push((at_ms, format!("hold {}", hold_name(*state))));
                self.confirm("hold");
            }
            CallUpdate::Recording(_) => self.confirm("recording"),
            CallUpdate::OwnHand(_) => self.confirm("hand"),
            CallUpdate::Reaction { mri, .. } if *mri == self.me.mri => self.confirm("reaction"),
            CallUpdate::Captions(CaptionState::Failed(message)) => {
                self.settle("captions", Outcome::Failed(message.clone()));
            }
            CallUpdate::Captions(CaptionState::On) => self.confirm("captions"),
            CallUpdate::Caption(_) => self.captions += 1,
            CallUpdate::Organizer { outcome: Ok(_), .. } => self.confirm("people action"),
            CallUpdate::Organizer { outcome: Err(message), .. } => {
                self.settle("people action", Outcome::Failed(message.clone()));
            }
            CallUpdate::Roster(entries) => {
                for entry in entries {
                    self.remember_person(&entry.mri, &entry.display_name);
                }
                let present = entries.iter().filter(|entry| !entry.in_lobby).count();
                self.peak_participants = self.peak_participants.max(present);
            }
            CallUpdate::Notice(text) => self.notices.push((at_ms, text.clone())),
            CallUpdate::Health(health) => {
                self.series.observe_health(health, at_ms);
                if self.paths.last().map(|(_, path)| *path) != Some(health.path) {
                    self.paths.push((at_ms, health.path));
                }
            }
            CallUpdate::Sample(sample) => self.series.observe_sample(sample),
            CallUpdate::Diagnostics(diagnostics) => self.diagnostics = Some((**diagnostics).clone()),
            _ => {}
        }
    }

    fn note_state(&mut self, state: &CallState, at_ms: u128) {
        let name = match state {
            CallState::Idle => "idle",
            CallState::Connecting => "connecting",
            CallState::Connected { .. } => "connected",
            CallState::Reconnecting { .. } => "reconnecting",
            CallState::Ended { .. } => "ended",
        };
        if self.states.last().is_none_or(|(_, last)| last != name) {
            self.states.push((at_ms, name.to_owned()));
        }
        match state {
            CallState::Connected { .. } if self.connected_ms.is_none() => self.connected_ms = Some(at_ms),
            CallState::Ended { reason } if self.ended.is_none() => {
                self.ended = Some((at_ms, reason.clone()));
                self.confirm("hangup");
            }
            _ => {}
        }
    }
}

fn hold_name(state: HoldState) -> &'static str {
    match state {
        HoldState::Active => "active",
        HoldState::Local => "local",
        HoldState::Remote => "remote",
    }
}

fn describe(command: &CallCommand) -> Option<(&'static str, String)> {
    use CallCommand::*;
    Some(match command {
        Mute(mute) => ("mute", format!("{mute:?}").to_lowercase()),
        SetCamera(on) => ("camera", on_off(*on).to_owned()),
        StartShare(source) => ("share", format!("start {}", format!("{:?}", source.kind).to_lowercase())),
        StopShare => ("share", "stop".to_owned()),
        SetShareSound(on) => ("share", format!("sound {}", on_off(*on))),
        SetShareMotion(on) => ("share", format!("motion {}", on_off(*on))),
        SetShareStandout(on) => ("share", format!("standout {}", on_off(*on))),
        Hold(on) => ("hold", (if *on { "hold" } else { "resume" }).to_owned()),
        Transfer { target, replaces } => {
            let way = if replaces.is_some() { "consulted" } else { "blind" };
            ("transfer", format!("{way} to {}", target.mri))
        }
        Invite { callees, group_thread } => {
            let group = if group_thread.is_some() { ", into a group chat" } else { "" };
            ("add people", format!("{} invited{group}", callees.len()))
        }
        StartRecording { kind, .. } => ("recording", format!("start {kind:?}")),
        StopRecording { scope } => ("recording", format!("stop {scope:?}")),
        ConsentToRecording => ("recording", "consent".to_owned()),
        SetCaptions(on) => ("captions", on_off(*on).to_owned()),
        SetCaptionLanguage(tag) => ("captions", format!("language {tag}")),
        SetHand(on) => ("hand", on_off(*on).to_owned()),
        LowerHand { mri } => ("hand", format!("lower {mri}")),
        LowerAllHands => ("hand", "lower all".to_owned()),
        SendReaction(reaction) => ("reaction", format!("{reaction:?}")),
        Admit { mri } => ("people action", format!("admit {mri}")),
        AdmitAll => ("people action", "admit all".to_owned()),
        Deny { mri } => ("people action", format!("deny {mri}")),
        MuteParticipant { mri } => ("people action", format!("mute {mri}")),
        MuteAll => ("people action", "mute all".to_owned()),
        Spotlight { mri } => ("people action", format!("spotlight {mri}")),
        StopSpotlight { mri } => ("people action", format!("stop spotlight {mri}")),
        RemoveParticipant { mri } => ("people action", format!("remove {mri}")),
        SetLock(on) => ("people action", format!("lock {}", on_off(*on))),
        SetRole { mri, role } => ("people action", format!("role {role:?} for {mri}")),
        SetAttendeeModality { mri, modality, allowed } => {
            ("people action", format!("{modality:?} {} for {mri}", if *allowed { "allowed" } else { "blocked" }))
        }
        SetAttendeeRestrictions { mic_off, camera_off } => {
            ("people action", format!("attendee restrictions mic_off={mic_off} camera_off={camera_off}"))
        }
        EndMeeting => ("people action", "end meeting for all".to_owned()),
        OpenWhiteboard { .. } => ("whiteboard", "open".to_owned()),
        Hangup => ("hangup", "local".to_owned()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use calling::call::VideoReport;
    use calling::timeline::TimelineEntry;
    use calling::{
        Callee, HealthSample, MuteCommand, NetworkLevel, RosterEntry, SdpLine, SdpSummary, StatsSample, TransportDetails,
    };

    use super::*;

    const PEER_MRI: &str = "8:orgid:aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

    fn recorder() -> CallRecorder {
        let me = OwnIdentity {
            mri: "8:orgid:11111111-2222-3333-4444-555555555555".to_owned(),
            display_name: "Daniel Daeschle".to_owned(),
            email: "daniel@example.com".to_owned(),
        };
        CallRecorder::begin(CallKind::Direct, false, true, me, vec![(PEER_MRI.to_owned(), "Mara Lindqvist".to_owned())])
    }

    fn sample(packets: u64) -> CallUpdate {
        CallUpdate::Sample(StatsSample {
            audio_in_packets: packets,
            audio_out_packets: packets,
            video: VideoReport { inbound_width: 1280, inbound_height: 720, ..VideoReport::default() },
            ..StatsSample::default()
        })
    }

    fn diagnostics() -> CallUpdate {
        let entry = |elapsed_ms, label: &str, detail: &str| TimelineEntry {
            elapsed_ms,
            label: label.to_owned(),
            detail: detail.to_owned(),
        };
        CallUpdate::Diagnostics(Box::new(CallDiagnostics {
            timeline: vec![
                entry(10, "roster", "Mara Lindqvist 8:orgid:aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee from 203.0.113.9"),
                entry(20, "raise hand failed", "HTTP 500 for https://emea.msg.teams.microsoft.com/v1/users/ME/conversations/19%3Aabc%40thread.v2?token=secret"),
                entry(30, "escalation", "new peer connection, new offer sent"),
            ],
            sdp: vec![SdpSummary {
                at_ms: 5,
                role: "local offer".to_owned(),
                lines: vec![SdpLine {
                    kind: "audio".to_owned(),
                    mid: "0".to_owned(),
                    direction: "sendrecv".to_owned(),
                    codecs: vec!["opus".to_owned()],
                    payload_count: 1,
                    ssrc_count: 1,
                    rejected: false,
                }],
            }],
            ..CallDiagnostics::default()
        }))
    }

    fn finished_call() -> CallRecorder {
        let mut recorder = recorder();
        recorder.observe(&CallUpdate::State(CallState::Connecting));
        recorder.observe(&CallUpdate::State(CallState::Connected { since: Instant::now() }));
        recorder.observe(&CallUpdate::Roster(vec![RosterEntry {
            mri: PEER_MRI.to_owned(),
            display_name: "Mara Lindqvist".to_owned(),
            ..RosterEntry::default()
        }]));
        for packets in [100, 200, 200, 300] {
            recorder.observe(&sample(packets));
        }
        recorder.observe(&CallUpdate::Health(calling::CallHealth {
            level: NetworkLevel::Poor,
            cpu_limited: false,
            sample: HealthSample { loss_percent: 6., ..HealthSample::default() },
            transport: TransportDetails::default(),
            path: MediaPath::Direct,
        }));
        recorder.observe(&CallUpdate::Notice("Mara Lindqvist declined the transfer".to_owned()));
        recorder.command(&CallCommand::Mute(MuteCommand::Toggle));
        recorder.observe(&CallUpdate::Muted(true));
        recorder.command(&CallCommand::SetHand(true));
        recorder.command(&CallCommand::Transfer {
            target: Callee { mri: PEER_MRI.to_owned(), display_name: "Mara Lindqvist".to_owned() },
            replaces: None,
        });
        recorder.command(&CallCommand::Hangup);
        recorder.observe(&CallUpdate::State(CallState::Ended { reason: EndReason::LocalHangup }));
        recorder.observe(&diagnostics());
        recorder
    }

    #[test]
    fn the_report_names_the_call_its_features_and_findings() {
        let document = finished_call().document();
        let json: serde_json::Value = serde_json::from_str(&document.json).expect("valid json");
        assert_eq!(json["call"]["kind"], "1:1");
        assert_eq!(json["call"]["direction"], "outgoing");
        assert_eq!(json["call"]["end_reason"], "local hangup");
        assert_eq!(json["media"]["path"], "direct");
        assert_eq!(json["media"]["escalations"], 1);
        assert_eq!(json["stats"]["samples"], 4);
        assert_eq!(json["stats"]["audio_in_stalled_samples"], 1);
        let results: Vec<(&str, &str)> = json["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|feature| (feature["feature"].as_str().unwrap(), feature["result"].as_str().unwrap()))
            .collect();
        assert_eq!(results, [("mute", "ok"), ("hand", "failed"), ("transfer", "no confirmation"), ("hangup", "ok")]);
        assert_eq!(json["timeline"].as_array().unwrap().len(), 3);
        assert_eq!(json["sdp"][0]["lines"][0]["codecs"][0], "opus");
        assert!(document.markdown.starts_with("# Call test report"));
        assert!(document.markdown.contains("## Findings"));
        assert!(document.markdown.contains("Inbound audio stalled in 1 of 4 samples."));
        assert!(document.stem.ends_with("-1to1"));
    }

    #[test]
    fn neither_file_holds_anything_personal() {
        let document = finished_call().document();
        for text in [&document.json, &document.markdown] {
            for leaked in [
                "Mara", "Lindqvist", "Daniel", "Daeschle", "aaaaaaaa", "11111111", "example.com", "203.0.113.9", "token=secret", "abc%40thread",
            ] {
                assert!(!text.contains(leaked), "{leaked} leaked into: {text}");
            }
            assert!(text.contains("person-1"));
        }
    }

    #[test]
    fn a_call_that_never_ended_still_gives_a_report() {
        let mut recorder = recorder();
        recorder.observe(&CallUpdate::State(CallState::Connecting));
        let document = recorder.document();
        let json: serde_json::Value = serde_json::from_str(&document.json).unwrap();
        assert_eq!(json["call"]["end_reason"], "still running when saved");
        assert!(document.markdown.contains("Never connected"));
    }
}
