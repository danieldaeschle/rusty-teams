use gpui_kit::*;

use crate::app_state::AppState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsenceNotice {
    pub name: String,
}

impl AbsenceNotice {
    pub fn text(&self) -> String {
        format!("{} is out of office and may not respond", self.name)
    }
}

fn notice_for(
    dismissed: bool,
    out_of_office: Option<&str>,
    profile_name: Option<&str>,
    partner_name: &str,
) -> Option<AbsenceNotice> {
    if dismissed {
        return None;
    }
    out_of_office?;
    let name = profile_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(partner_name);
    Some(AbsenceNotice { name: name.to_owned() })
}

impl AppState {
    pub fn refresh_partner_absence(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        if self.mode.demo {
            return;
        }
        if let Some(partner) = self.one_on_one_partner(conversation_id) {
            self.request_profile(&partner.user_id, false, cx);
        }
    }

    pub fn absence_notice(&self, conversation_id: &str) -> Option<AbsenceNotice> {
        let partner = self.one_on_one_partner(conversation_id)?;
        let demo_profile;
        let profile = if self.mode.demo {
            demo_profile = crate::demo_profiles::profile(&partner.user_id)?;
            &demo_profile
        } else {
            self.profiles.get(&partner.user_id)?.profile.as_deref()?
        };
        notice_for(
            self.dismissed_absence_notices.contains(conversation_id),
            profile.out_of_office.as_deref(),
            profile.display_name.as_deref(),
            &partner.name,
        )
    }

    pub fn dismiss_absence_notice(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        self.dismissed_absence_notices.insert(conversation_id.to_owned());
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::notice_for;

    #[test]
    fn out_of_office_shows_the_notice_until_it_is_closed() {
        let notice = notice_for(false, Some(""), Some("Lea Schneider"), "Lea").unwrap();
        assert_eq!(notice.text(), "Lea Schneider is out of office and may not respond");
        assert_eq!(notice_for(true, Some(""), Some("Lea Schneider"), "Lea"), None);
        assert_eq!(notice_for(false, None, Some("Lea Schneider"), "Lea"), None);
        assert_eq!(notice_for(false, Some("Back Monday"), Some(" "), "Lea").unwrap().name, "Lea");
    }
}
