use calling::{RecordingKind, StopScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordAction {
    Start(RecordingKind),
    Stop(StopScope),
}

impl RecordAction {
    pub fn label(self) -> &'static str {
        match self {
            RecordAction::Start(RecordingKind::Video) => "Start recording",
            RecordAction::Start(RecordingKind::AudioOnly) => "Record audio only",
            RecordAction::Start(RecordingKind::TranscriptOnly) => "Start transcription only",
            RecordAction::Stop(StopScope::Both | StopScope::RecordingOnly) => "Stop recording",
            RecordAction::Stop(StopScope::TranscriptionOnly) => "Stop transcription",
        }
    }

    pub fn confirm_title(self) -> Option<&'static str> {
        match self {
            RecordAction::Start(_) => None,
            RecordAction::Stop(StopScope::Both) => Some("Stop recording and transcription?"),
            RecordAction::Stop(StopScope::RecordingOnly) => Some("Stop recording?"),
            RecordAction::Stop(StopScope::TranscriptionOnly) => Some("Stop transcription?"),
        }
    }
}

pub fn record_actions(recording: bool, transcribing: bool) -> Vec<(RecordAction, &'static str)> {
    let labelled = |action: RecordAction| (action, action.label());
    match (recording, transcribing) {
        (false, false) => vec![
            labelled(RecordAction::Start(RecordingKind::Video)),
            labelled(RecordAction::Start(RecordingKind::AudioOnly)),
            labelled(RecordAction::Start(RecordingKind::TranscriptOnly)),
        ],
        (true, true) => vec![
            labelled(RecordAction::Stop(StopScope::RecordingOnly)),
            labelled(RecordAction::Stop(StopScope::TranscriptionOnly)),
            (RecordAction::Stop(StopScope::Both), "Stop both"),
        ],
        (true, false) => vec![labelled(RecordAction::Stop(StopScope::Both))],
        (false, true) => vec![labelled(RecordAction::Stop(StopScope::TranscriptionOnly))],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingChip {
    Recording,
    RecordingAudio,
    Transcribing,
}

impl RecordingChip {
    pub fn label(self) -> &'static str {
        match self {
            RecordingChip::Recording => "Recording",
            RecordingChip::RecordingAudio => "Recording audio",
            RecordingChip::Transcribing => "Transcribing",
        }
    }
}

pub fn recording_chip(recording: bool, audio_only: bool, transcribing: bool) -> Option<RecordingChip> {
    match (recording, audio_only, transcribing) {
        (true, true, _) => Some(RecordingChip::RecordingAudio),
        (true, false, _) => Some(RecordingChip::Recording),
        (false, _, true) => Some(RecordingChip::Transcribing),
        (false, _, false) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(recording: bool, transcribing: bool) -> Vec<&'static str> {
        record_actions(recording, transcribing).into_iter().map(|(_, label)| label).collect()
    }

    #[test]
    fn nothing_running_offers_the_three_starts() {
        assert_eq!(labels(false, false), ["Start recording", "Record audio only", "Start transcription only"]);
    }

    #[test]
    fn recording_with_transcript_offers_the_three_stop_variants() {
        let actions = record_actions(true, true);
        assert_eq!(actions[0].0, RecordAction::Stop(StopScope::RecordingOnly));
        assert_eq!(actions[1].0, RecordAction::Stop(StopScope::TranscriptionOnly));
        assert_eq!(actions[2].0, RecordAction::Stop(StopScope::Both));
        assert_eq!(labels(true, true), ["Stop recording", "Stop transcription", "Stop both"]);
    }

    #[test]
    fn a_single_running_part_offers_the_one_matching_stop() {
        assert_eq!(record_actions(true, false), [(RecordAction::Stop(StopScope::Both), "Stop recording")]);
        assert_eq!(record_actions(false, true), [(RecordAction::Stop(StopScope::TranscriptionOnly), "Stop transcription")]);
    }

    #[test]
    fn only_stops_ask_for_confirmation() {
        assert_eq!(RecordAction::Start(RecordingKind::Video).confirm_title(), None);
        assert_eq!(RecordAction::Stop(StopScope::Both).confirm_title(), Some("Stop recording and transcription?"));
        assert_eq!(RecordAction::Stop(StopScope::RecordingOnly).confirm_title(), Some("Stop recording?"));
        assert_eq!(RecordAction::Stop(StopScope::TranscriptionOnly).confirm_title(), Some("Stop transcription?"));
    }

    #[test]
    fn the_chip_names_what_is_running() {
        assert_eq!(recording_chip(true, false, true), Some(RecordingChip::Recording));
        assert_eq!(recording_chip(true, true, true), Some(RecordingChip::RecordingAudio));
        assert_eq!(recording_chip(false, false, true), Some(RecordingChip::Transcribing));
        assert_eq!(recording_chip(false, false, false), None);
        assert_eq!(RecordingChip::RecordingAudio.label(), "Recording audio");
    }
}
