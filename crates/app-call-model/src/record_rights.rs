use calling::{MeetingRole, RecorderOption, RecordingKind};

use super::model::{CallKind, CallModel};
use super::recording_menu::RecordAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordFeature {
    Recording,
    Transcription,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordBlock {
    Role,
    OrganizerOff,
    OrganizerOnly,
    Busy,
    External,
    CallPolicy,
    MeetingPolicy,
    Capability,
}

impl RecordBlock {
    pub fn locked(self) -> bool {
        self != RecordBlock::Busy
    }

    pub fn reason(self, feature: RecordFeature) -> &'static str {
        match (self, feature) {
            (RecordBlock::Role, RecordFeature::Recording) => {
                "Only meeting organisers and presenters can control recording"
            }
            (RecordBlock::Role, RecordFeature::Transcription) => {
                "Only meeting organisers and presenters can control transcription"
            }
            (RecordBlock::OrganizerOff, _) => {
                "Unavailable because the meeting organiser has disabled recording for this meeting"
            }
            (RecordBlock::OrganizerOnly, _) => {
                "Only meeting organiser and co-organiser can control recording for this meeting"
            }
            (RecordBlock::Busy, _) => "Starting...",
            (RecordBlock::External, _) => {
                "Unavailable to people in a different org from the organiser"
            }
            (RecordBlock::CallPolicy, RecordFeature::Recording) => {
                "Unavailable for this call due to organisation policy. Reach out to your IT admin to get access."
            }
            (RecordBlock::MeetingPolicy, RecordFeature::Recording) => {
                "Unavailable for this meeting due to organisation policy. Reach out to your IT admin to get access."
            }
            (
                RecordBlock::CallPolicy | RecordBlock::MeetingPolicy,
                RecordFeature::Transcription,
            ) => "Unavailable due to org policy. Reach out to your IT admin to get access.",
            (RecordBlock::Capability, _) => "Unavailable in this meeting",
        }
    }
}

impl CallModel {
    pub fn record_hidden(&self) -> bool {
        !self.is_live() || self.lobby || self.kind == CallKind::Test
    }

    pub fn record_block(&self) -> Option<RecordBlock> {
        self.block(RecordFeature::Recording)
    }

    pub fn transcribe_block(&self) -> Option<RecordBlock> {
        self.block(RecordFeature::Transcription)
    }

    pub fn can_record(&self) -> bool {
        !self.record_hidden() && self.record_block().is_none()
    }

    pub fn action_allowed(&self, action: RecordAction) -> bool {
        match action {
            RecordAction::Start(RecordingKind::Video | RecordingKind::AudioOnly) => {
                self.can_record()
            }
            _ => !self.record_hidden() && self.action_block(action).is_none(),
        }
    }

    pub fn stop_block(&self) -> Option<RecordBlock> {
        self.record_block()
            .filter(|block| matches!(block, RecordBlock::Role | RecordBlock::OrganizerOnly))
    }

    pub fn action_block(&self, action: RecordAction) -> Option<(RecordBlock, RecordFeature)> {
        match action {
            RecordAction::Start(RecordingKind::TranscriptOnly) => self
                .transcribe_block()
                .map(|block| (block, RecordFeature::Transcription)),
            RecordAction::Start(_) => self
                .record_block()
                .map(|block| (block, RecordFeature::Recording)),
            RecordAction::Stop(_) => self
                .stop_block()
                .map(|block| (block, RecordFeature::Recording)),
        }
    }

    pub fn record_menu_block(&self) -> Option<(RecordBlock, RecordFeature)> {
        if self.recording || self.transcribing {
            return None;
        }
        self.record_block()
            .zip(self.transcribe_block())
            .map(|(block, _)| (block, RecordFeature::Recording))
    }

    fn block(&self, feature: RecordFeature) -> Option<RecordBlock> {
        let capability = self.capability;
        let organizer = self.own_role == Some(MeetingRole::Organizer) || self.can_manage();
        if self.own_role == Some(MeetingRole::Attendee) {
            return Some(RecordBlock::Role);
        }
        match capability.recorder_option {
            RecorderOption::Nobody => return Some(RecordBlock::OrganizerOff),
            RecorderOption::OrganizersOnly if !organizer => {
                return Some(RecordBlock::OrganizerOnly);
            }
            _ => {}
        }
        if self.record_pending {
            return Some(RecordBlock::Busy);
        }
        let foreign = matches!(
            (&self.meeting_tenant, &self.own_tenant),
            (Some(meeting), Some(own)) if !meeting.eq_ignore_ascii_case(own)
        );
        if self.kind == CallKind::Meeting && foreign {
            return Some(RecordBlock::External);
        }
        let policy = self.recording_policy;
        let (call_allowed, meeting_allowed, capability_allowed) = match feature {
            RecordFeature::Recording => (
                policy.call_recording,
                policy.meeting_recording,
                capability.allow_cloud_recording,
            ),
            RecordFeature::Transcription => (
                policy.call_transcription,
                policy.meeting_transcription,
                capability.allow_transcription,
            ),
        };
        if self.kind == CallKind::Direct && !call_allowed {
            return Some(RecordBlock::CallPolicy);
        }
        if self.kind != CallKind::Direct && !meeting_allowed {
            return Some(RecordBlock::MeetingPolicy);
        }
        (capability_allowed == Some(false)).then_some(RecordBlock::Capability)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use calling::{CallState, CallUpdate, MeetingCapability};
    use teams_core::RecordingPolicy;

    use super::*;

    fn live(mut model: CallModel) -> CallModel {
        model.apply(CallUpdate::State(CallState::Connected {
            since: Instant::now(),
        }));
        model
    }

    fn meeting() -> CallModel {
        live(CallModel::meeting("Standup", false))
    }

    fn direct() -> CallModel {
        live(CallModel::people(
            "Bea",
            &[("8:orgid:bea".into(), "Bea".into())],
        ))
    }

    fn group() -> CallModel {
        live(CallModel::people(
            "Team",
            &[
                ("8:orgid:bea".into(), "Bea".into()),
                ("8:orgid:cy".into(), "Cy".into()),
            ],
        ))
    }

    fn capability(update: impl FnOnce(&mut MeetingCapability)) -> MeetingCapability {
        let mut capability = MeetingCapability::default();
        update(&mut capability);
        capability
    }

    #[test]
    fn everything_is_allowed_when_nothing_is_known() {
        for model in [meeting(), direct(), group()] {
            assert_eq!(model.record_block(), None);
            assert_eq!(model.transcribe_block(), None);
            assert!(model.can_record() && !model.record_hidden());
        }
    }

    #[test]
    fn hidden_until_live_and_while_in_the_lobby_or_the_echo_call() {
        assert!(CallModel::meeting("Standup", true).record_hidden());
        let mut lobby = meeting();
        lobby.lobby = true;
        assert!(lobby.record_hidden() && !lobby.can_record());
        assert!(live(CallModel::test()).record_hidden());
        assert!(!meeting().record_hidden());
    }

    #[test]
    fn an_attendee_is_blocked_and_a_presenter_is_not() {
        let mut model = meeting();
        model.own_role = Some(MeetingRole::Attendee);
        assert_eq!(model.record_block(), Some(RecordBlock::Role));
        assert_eq!(model.transcribe_block(), Some(RecordBlock::Role));
        model.own_role = Some(MeetingRole::Presenter);
        assert_eq!(model.record_block(), None);
    }

    #[test]
    fn the_recorder_option_off_blocks_everyone_even_the_organizer() {
        let mut model = meeting();
        model.own_role = Some(MeetingRole::Organizer);
        model.capability =
            capability(|capability| capability.recorder_option = RecorderOption::Nobody);
        assert_eq!(model.record_block(), Some(RecordBlock::OrganizerOff));
        assert_eq!(model.transcribe_block(), Some(RecordBlock::OrganizerOff));
    }

    #[test]
    fn the_organizer_only_option_blocks_presenters() {
        let mut model = meeting();
        model.capability =
            capability(|capability| capability.recorder_option = RecorderOption::OrganizersOnly);
        model.own_role = Some(MeetingRole::Presenter);
        assert_eq!(model.record_block(), Some(RecordBlock::OrganizerOnly));
        model.own_role = Some(MeetingRole::Organizer);
        assert_eq!(model.record_block(), None);
        model.own_role = None;
        model.organizer = true;
        assert_eq!(model.record_block(), None);
    }

    #[test]
    fn a_start_in_flight_blocks_with_a_reason_without_a_lock() {
        let mut model = meeting();
        model.apply(CallUpdate::RecordingPending(true));
        let block = model.record_block().unwrap();
        assert_eq!(block, RecordBlock::Busy);
        assert!(!block.locked());
        assert_eq!(model.transcribe_block(), Some(RecordBlock::Busy));
        model.apply(CallUpdate::RecordingPending(false));
        assert_eq!(model.record_block(), None);
    }

    #[test]
    fn a_meeting_of_another_tenant_is_blocked_but_a_call_is_not() {
        let mut model = meeting();
        model.apply(CallUpdate::MeetingTenant("tenant-a".into()));
        assert_eq!(model.record_block(), None);
        model.own_tenant = Some("tenant-b".into());
        assert_eq!(model.record_block(), Some(RecordBlock::External));
        assert_eq!(model.transcribe_block(), Some(RecordBlock::External));
        model.own_tenant = Some("TENANT-A".into());
        assert_eq!(model.record_block(), None);
        let mut call = direct();
        call.meeting_tenant = Some("tenant-a".into());
        call.own_tenant = Some("tenant-b".into());
        assert_eq!(call.record_block(), None);
    }

    #[test]
    fn direct_calls_use_the_calling_policy_and_the_rest_the_meeting_policy() {
        let no_calls = RecordingPolicy {
            call_recording: false,
            call_transcription: false,
            ..RecordingPolicy::default()
        };
        let no_meetings = RecordingPolicy {
            meeting_recording: false,
            meeting_transcription: false,
            ..RecordingPolicy::default()
        };
        for (mut model, policy, blocked) in [
            (direct(), no_calls, Some(RecordBlock::CallPolicy)),
            (direct(), no_meetings, None),
            (group(), no_calls, None),
            (group(), no_meetings, Some(RecordBlock::MeetingPolicy)),
            (meeting(), no_calls, None),
            (meeting(), no_meetings, Some(RecordBlock::MeetingPolicy)),
        ] {
            model.recording_policy = policy;
            assert_eq!(model.record_block(), blocked);
            assert_eq!(model.transcribe_block(), blocked);
        }
    }

    #[test]
    fn recording_and_transcription_follow_their_own_policy_flags() {
        let mut model = meeting();
        model.recording_policy = RecordingPolicy {
            meeting_transcription: false,
            ..RecordingPolicy::default()
        };
        assert_eq!(model.record_block(), None);
        assert_eq!(model.transcribe_block(), Some(RecordBlock::MeetingPolicy));
        assert!(model.can_record());
    }

    #[test]
    fn the_meeting_capability_blocks_only_when_it_says_false() {
        let mut model = meeting();
        model.capability = capability(|capability| capability.allow_cloud_recording = Some(true));
        assert_eq!(model.record_block(), None);
        model.capability = capability(|capability| capability.allow_cloud_recording = Some(false));
        assert_eq!(model.record_block(), Some(RecordBlock::Capability));
        assert_eq!(model.transcribe_block(), None);
        model.capability = capability(|capability| capability.allow_transcription = Some(false));
        assert_eq!(model.record_block(), None);
        assert_eq!(model.transcribe_block(), Some(RecordBlock::Capability));
    }

    #[test]
    fn the_checks_run_in_the_order_of_teams() {
        let mut model = meeting();
        model.own_role = Some(MeetingRole::Attendee);
        model.capability = capability(|capability| {
            capability.recorder_option = RecorderOption::Nobody;
            capability.allow_cloud_recording = Some(false);
        });
        model.recording_policy.meeting_recording = false;
        model.record_pending = true;
        model.meeting_tenant = Some("a".into());
        model.own_tenant = Some("b".into());
        let expected = [
            RecordBlock::Role,
            RecordBlock::OrganizerOff,
            RecordBlock::Busy,
            RecordBlock::External,
            RecordBlock::MeetingPolicy,
            RecordBlock::Capability,
        ];
        assert_eq!(model.record_block(), Some(expected[0]));
        model.own_role = None;
        assert_eq!(model.record_block(), Some(expected[1]));
        model.capability.recorder_option = RecorderOption::OrganizersAndPresenters;
        assert_eq!(model.record_block(), Some(expected[2]));
        model.record_pending = false;
        assert_eq!(model.record_block(), Some(expected[3]));
        model.own_tenant = None;
        assert_eq!(model.record_block(), Some(expected[4]));
        model.recording_policy.meeting_recording = true;
        assert_eq!(model.record_block(), Some(expected[5]));
    }

    #[test]
    fn the_reasons_use_the_teams_wording_per_feature() {
        assert_eq!(
            RecordBlock::Role.reason(RecordFeature::Transcription),
            "Only meeting organisers and presenters can control transcription"
        );
        assert_eq!(
            RecordBlock::CallPolicy.reason(RecordFeature::Recording),
            "Unavailable for this call due to organisation policy. Reach out to your IT admin to get access."
        );
        assert_eq!(
            RecordBlock::MeetingPolicy.reason(RecordFeature::Transcription),
            "Unavailable due to org policy. Reach out to your IT admin to get access."
        );
        assert_eq!(
            RecordBlock::Capability.reason(RecordFeature::Recording),
            "Unavailable in this meeting"
        );
    }

    #[test]
    fn the_menu_locks_only_when_both_features_are_blocked_and_nothing_runs() {
        let mut model = meeting();
        model.own_role = Some(MeetingRole::Attendee);
        assert_eq!(
            model.record_menu_block(),
            Some((RecordBlock::Role, RecordFeature::Recording))
        );
        model.recording = true;
        assert_eq!(model.record_menu_block(), None);
        model.recording = false;
        model.own_role = None;
        model.capability = capability(|capability| capability.allow_cloud_recording = Some(false));
        assert_eq!(model.record_menu_block(), None);
    }

    #[test]
    fn stopping_is_only_blocked_by_the_role_rules() {
        let mut model = meeting();
        model.recording_policy.meeting_recording = false;
        assert_eq!(model.stop_block(), None);
        model.own_role = Some(MeetingRole::Attendee);
        assert_eq!(model.stop_block(), Some(RecordBlock::Role));
    }

    #[test]
    fn each_action_asks_its_own_feature() {
        let mut model = meeting();
        model.recording_policy.meeting_transcription = false;
        let start = |kind| RecordAction::Start(kind);
        assert_eq!(model.action_block(start(RecordingKind::Video)), None);
        assert_eq!(
            model.action_block(start(RecordingKind::TranscriptOnly)),
            Some((RecordBlock::MeetingPolicy, RecordFeature::Transcription))
        );
        model.recording_policy = RecordingPolicy {
            meeting_recording: false,
            ..RecordingPolicy::default()
        };
        assert_eq!(
            model.action_block(start(RecordingKind::AudioOnly)),
            Some((RecordBlock::MeetingPolicy, RecordFeature::Recording))
        );
        assert_eq!(
            model.action_block(RecordAction::Stop(calling::StopScope::Both)),
            None
        );
    }
}
