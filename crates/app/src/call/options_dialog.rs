use calling::{MeetingOptions, OptionWrite};

#[derive(Debug, Clone, PartialEq)]
pub struct OptionsDialog {
    pub original: Option<MeetingOptions>,
    pub draft: Option<MeetingOptions>,
    pub saving: bool,
    pub error: Option<String>,
}

impl OptionsDialog {
    pub fn loading() -> Self {
        OptionsDialog { original: None, draft: None, saving: false, error: None }
    }

    pub fn loaded(&mut self, result: Result<MeetingOptions, String>) {
        match result {
            Ok(options) => {
                self.original = Some(options.clone());
                self.draft = Some(options);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn is_loading(&self) -> bool {
        self.draft.is_none() && self.error.is_none()
    }

    pub fn changes(&self) -> Vec<OptionWrite> {
        match (&self.original, &self.draft) {
            (Some(original), Some(draft)) => original.changes(draft),
            _ => Vec::new(),
        }
    }

    pub fn can_save(&self) -> bool {
        !self.saving && !self.changes().is_empty()
    }

    pub fn begin_save(&mut self) -> Option<Vec<OptionWrite>> {
        if !self.can_save() {
            return None;
        }
        self.saving = true;
        self.error = None;
        Some(self.changes())
    }

    pub fn saved(&mut self, result: Result<MeetingOptions, String>) -> bool {
        self.saving = false;
        match result {
            Ok(_) => true,
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn options() -> MeetingOptions {
        let option = |name: &str, value: serde_json::Value| json!({"name": name, "currentValue": value});
        MeetingOptions::from_response(&json!({"options": [
            option("AutoAdmittedUsers", json!("EveryoneInCompany")),
            option("PresenterOption", json!("Everyone")),
            option("AttendeeRestrictions", json!("UnRestricted")),
            option("AllowMeetingChat", json!("Enabled")),
            option("AllowTeamsMeetingReactions", json!(true)),
            option("AutoRecordAndTranscribeMode", json!("Off")),
        ]}))
        .unwrap()
    }

    #[test]
    fn a_dialog_loads_then_saves_only_what_changed() {
        let mut dialog = OptionsDialog::loading();
        assert!(dialog.is_loading());
        assert!(dialog.begin_save().is_none());
        dialog.loaded(Ok(options()));
        assert!(!dialog.is_loading());
        assert!(!dialog.can_save());
        dialog.draft.as_mut().unwrap().reactions = false;
        let writes = dialog.begin_save().expect("a change to save");
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].name, "AllowTeamsMeetingReactions");
        assert!(dialog.saving);
        assert!(dialog.begin_save().is_none());
    }

    #[test]
    fn errors_stay_inline_and_a_failed_save_can_be_retried() {
        let mut dialog = OptionsDialog::loading();
        dialog.loaded(Err("The options could not be read".into()));
        assert!(!dialog.is_loading());
        assert_eq!(dialog.error.as_deref(), Some("The options could not be read"));
        dialog.loaded(Ok(options()));
        assert_eq!(dialog.error, None);
        dialog.draft.as_mut().unwrap().meeting_chat = "Disabled".into();
        dialog.begin_save();
        assert!(!dialog.saved(Err("the service refused AllowMeetingChat".into())));
        assert!(!dialog.saving);
        assert_eq!(dialog.error.as_deref(), Some("the service refused AllowMeetingChat"));
        assert!(dialog.can_save());
        dialog.begin_save();
        assert!(dialog.saved(Ok(options())));
    }
}
