//! Call notice payloads: i18n keys (and optional detail after [`NOTICE_DETAIL_SEP`]).
pub const NOTICE_DETAIL_SEP: char = '\u{1f}';

pub const CONSENT_FIRST_NOTICE: &str = "calls.notice.consent_first";
pub const ON_HOLD_NOTICE: &str = "calls.notice.on_hold";
pub const CHANGE_RUNNING_NOTICE: &str = "calls.notice.change_running";
pub const VIDEO_FAILED_NOTICE: &str = "calls.notice.video_failed";
pub const SHARE_QUEUED_NOTICE: &str = "calls.notice.share_queued";
pub const SHARE_REFUSED_NOTICE: &str = "calls.notice.share_refused";
pub const NOT_CONNECTED_FOR_VIDEO: &str = "calls.notice.video_starts_once_connected";
pub const PORTAL_UNAVAILABLE_NOTICE: &str = "calls.notice.portal_unavailable";
pub const CALL_TRANSFERRED_NOTICE: &str = "calls.notice.call_transferred";
pub const TRANSFER_FAILED_NOTICE: &str = "calls.notice.transfer_failed";
pub const RECORDING_JOIN_FAILED_NOTICE: &str = "calls.notice.recording_join_failed";
pub const CAPTIONS_JOIN_FAILED_NOTICE: &str = "calls.notice.captions_join_failed";
pub const CAPTION_LANGUAGE_FAILED_NOTICE: &str = "calls.notice.caption_language_failed";
pub const CAMERA_START_NOTICE: &str = "calls.notice.camera_start";
pub const CAMERA_RESTART_NOTICE: &str = "calls.notice.camera_restart";
pub const SHARING_START_NOTICE: &str = "calls.notice.sharing_start";
pub const RAISE_HAND_FAILED_NOTICE: &str = "calls.notice.raise_hand_failed";
pub const LOWER_HAND_FAILED_NOTICE: &str = "calls.notice.lower_hand_failed";
pub const LOWER_THAT_HAND_FAILED_NOTICE: &str = "calls.notice.lower_that_hand_failed";
pub const LOWER_ALL_HANDS_FAILED_NOTICE: &str = "calls.notice.lower_all_hands_failed";
pub const CONFIRM_RECORDING_FAILED_NOTICE: &str = "calls.notice.confirm_recording_failed";
pub const HOLD_FAILED_NOTICE: &str = "calls.notice.hold_failed";
pub const RESUME_FAILED_NOTICE: &str = "calls.notice.resume_failed";
pub const TRANSFER_NOT_ALLOWED_NOTICE: &str = "calls.notice.transfer_not_allowed";
pub const TRANSFER_CALL_FAILED_NOTICE: &str = "calls.notice.transfer_call_failed";
pub const TRANSFERRING_NOTICE: &str = "calls.notice.transferring";
pub const WHITEBOARDS_MEETINGS_ONLY_NOTICE: &str = "calls.notice.whiteboards_meetings_only";
pub const WHITEBOARD_OPEN_FAILED_NOTICE: &str = "calls.notice.whiteboard_open_failed";
pub const REACTION_FAILED_NOTICE: &str = "calls.notice.reaction_failed";
pub const RECORDING_NOT_STARTED_NOTICE: &str = "calls.notice.recording_not_started";
pub const RECORDING_START_FAILED_NOTICE: &str = "calls.notice.recording_start_failed";
pub const CAPTIONS_START_FAILED_NOTICE: &str = "calls.notice.captions_start_failed";
pub const HOLD_CHANGE_FAILED_NOTICE: &str = "calls.notice.hold_change_failed";
pub const RECORDING_STOP_FAILED_NOTICE: &str = "calls.notice.recording_stop_failed";
pub const BACKGROUND_IMAGE_FAILED_NOTICE: &str = "calls.notice.background_image_failed";
pub const CAMERA_STOPPED_NOTICE: &str = "calls.notice.camera_stopped";
pub const SCREEN_SHARE_STOPPED_NOTICE: &str = "calls.notice.screen_share_stopped";
pub const ADMIT_FAILED_NOTICE: &str = "calls.notice.admit_failed";
pub const ADMIT_ALL_FAILED_NOTICE: &str = "calls.notice.admit_all_failed";
pub const DENY_FAILED_NOTICE: &str = "calls.notice.deny_failed";
pub const REMOVE_PARTICIPANT_FAILED_NOTICE: &str = "calls.notice.remove_participant_failed";
pub const MUTE_PARTICIPANT_FAILED_NOTICE: &str = "calls.notice.mute_participant_failed";
pub const MUTE_ALL_FAILED_NOTICE: &str = "calls.notice.mute_all_failed";
pub const MEETING_LOCK_FAILED_NOTICE: &str = "calls.notice.meeting_lock_failed";
pub const CHANGE_ROLE_FAILED_NOTICE: &str = "calls.notice.change_role_failed";
pub const MIC_SETTING_FAILED_NOTICE: &str = "calls.notice.mic_setting_failed";
pub const CAMERA_SETTING_FAILED_NOTICE: &str = "calls.notice.camera_setting_failed";
pub const ATTENDEE_SETTINGS_FAILED_NOTICE: &str = "calls.notice.attendee_settings_failed";
pub const INVITE_PERSON_FAILED_NOTICE: &str = "calls.notice.invite_person_failed";
pub const SPOTLIGHT_FAILED_NOTICE: &str = "calls.notice.spotlight_failed";
pub const STOP_SPOTLIGHT_FAILED_NOTICE: &str = "calls.notice.stop_spotlight_failed";

pub fn notice_with_detail(key: &'static str, detail: impl std::fmt::Display) -> String {
    format!("{key}{NOTICE_DETAIL_SEP}{detail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_id_fixed_keys_match_catalog_paths() {
        assert_eq!(CONSENT_FIRST_NOTICE, "calls.notice.consent_first");
        assert_eq!(ON_HOLD_NOTICE, "calls.notice.on_hold");
        assert_eq!(CHANGE_RUNNING_NOTICE, "calls.notice.change_running");
        assert_eq!(VIDEO_FAILED_NOTICE, "calls.notice.video_failed");
        assert_eq!(SHARE_QUEUED_NOTICE, "calls.notice.share_queued");
        assert_eq!(SHARE_REFUSED_NOTICE, "calls.notice.share_refused");
        assert_eq!(
            NOT_CONNECTED_FOR_VIDEO,
            "calls.notice.video_starts_once_connected"
        );
        assert_eq!(PORTAL_UNAVAILABLE_NOTICE, "calls.notice.portal_unavailable");
        assert_eq!(CALL_TRANSFERRED_NOTICE, "calls.notice.call_transferred");
        assert_eq!(TRANSFER_FAILED_NOTICE, "calls.notice.transfer_failed");
        assert_eq!(
            RECORDING_JOIN_FAILED_NOTICE,
            "calls.notice.recording_join_failed"
        );
        assert_eq!(
            CAPTIONS_JOIN_FAILED_NOTICE,
            "calls.notice.captions_join_failed"
        );
        assert_eq!(
            CAPTION_LANGUAGE_FAILED_NOTICE,
            "calls.notice.caption_language_failed"
        );
        assert_eq!(CAMERA_START_NOTICE, "calls.notice.camera_start");
        assert_eq!(CAMERA_RESTART_NOTICE, "calls.notice.camera_restart");
        assert_eq!(SHARING_START_NOTICE, "calls.notice.sharing_start");
        assert_eq!(RAISE_HAND_FAILED_NOTICE, "calls.notice.raise_hand_failed");
        assert_eq!(TRANSFERRING_NOTICE, "calls.notice.transferring");
        assert_eq!(ADMIT_FAILED_NOTICE, "calls.notice.admit_failed");
        assert_eq!(
            BACKGROUND_IMAGE_FAILED_NOTICE,
            "calls.notice.background_image_failed"
        );
    }

    #[test]
    fn notice_id_camera_payload_uses_key_and_detail_sep() {
        let payload = notice_with_detail(CAMERA_START_NOTICE, "device busy");
        assert!(payload.starts_with("calls.notice.camera_start"));
        assert!(payload.contains(NOTICE_DETAIL_SEP));
        assert!(payload.ends_with("device busy"));
    }
}
