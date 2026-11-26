use app_base::orgs::{OPEN_CONVERSATION_FLAG, Org, Orgs, REFUSED_ORG_FLAG};
use gpui_kit::Context;

use crate::app_state::AppState;

impl AppState {
    pub(crate) fn take_org_list(&mut self, list: Vec<Org>) {
        if self.orgs.list == list {
            return;
        }
        self.orgs.replace_list(list);
        self.save_orgs(&self.orgs);
    }

    /// A guest org has no calendar for the account, like in Teams.
    pub fn calendar_available(&self) -> bool {
        self.orgs.active_guest().is_none()
    }

    /// Orgs to offer in the account menu; a single org needs no switcher, like Teams.
    pub fn switchable_orgs(&self) -> &[Org] {
        if self.orgs.list.len() > 1 {
            &self.orgs.list
        } else {
            &[]
        }
    }

    /// Chats with new messages in `org` since this window opened; zero for the open org.
    pub fn org_unread_count(&self, org: &Org) -> usize {
        self.org_unread.get(&org.id).map_or(0, |chats| chats.len())
    }

    /// Restarts the app on `org`: its own cache opens at once and the backend signs in to its tenant.
    /// `open_conversation` opens that chat once the org has loaded.
    pub fn switch_org(
        &mut self,
        org: &Org,
        open_conversation: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        if self.orgs.is_active(org) {
            return;
        }
        // A switch the user asks for reports its own failure.
        self.org_restart_failure_shown = false;
        let mut next = self.orgs.clone();
        next.activate(org);
        let added =
            open_conversation.map(|conversation_id| (OPEN_CONVERSATION_FLAG, conversation_id));
        self.restart_on(next, added, cx);
    }

    /// The banner's way out of a guest org that cannot connect, e.g. a sign-in the user cannot finish.
    pub fn leave_guest_org(&mut self, cx: &mut Context<Self>) {
        if self.orgs.active_guest().is_none() {
            return;
        }
        self.org_restart_failure_shown = false;
        let mut next = self.orgs.clone();
        next.active = None;
        self.restart_on(next, None, cx);
    }

    /// Microsoft refused the open guest org's tokens (e.g. MFA there) and its sign-in did not bring new ones,
    /// or cannot be shown, so the window goes back to the home org and says so.
    /// A running call is not cut: the backend repeats the event, and the restart waits for the call to end.
    pub(crate) fn leave_refused_guest_org(&mut self, tenant_id: &str, cx: &mut Context<Self>) {
        if self.call.is_some() {
            return;
        }
        let mut next = self.orgs.clone();
        next.active = None;
        self.restart_on(next, Some((REFUSED_ORG_FLAG, tenant_id)), cx);
    }

    /// Shown after the restart from [`AppState::leave_refused_guest_org`].
    /// `false` while the org list does not know the org yet.
    pub(crate) fn announce_refused_org(&mut self, tenant_id: &str, cx: &mut Context<Self>) -> bool {
        let Some(org) = self.orgs.list.iter().find(|org| org.id == tenant_id) else {
            return false;
        };
        // Only Linux and macOS open the guest org's own sign-in before leaving it.
        let key = if cfg!(any(target_os = "linux", target_os = "macos")) {
            "notice.refused_org"
        } else {
            "notice.refused_org_no_sign_in"
        };
        let text = app_base::ui_locale::tr(key).replace("%{name}", &org.name);
        self.raise_notice(text, None, cx);
        true
    }

    fn restart_on(&mut self, next: Orgs, added: Option<(&str, &str)>, cx: &mut Context<Self>) {
        if !self.save_orgs(&next) {
            if !self.mode.demo {
                self.raise_restart_failure(&format!("{} not saved", app_base::orgs::ORGS_FILE), cx);
            }
            return;
        }
        match std::env::current_exe().and_then(|exe| {
            std::process::Command::new(exe)
                .args(restart_arguments(std::env::args_os().skip(1), added))
                .env(
                    app_base::updater::RELAUNCH_ENV,
                    std::process::id().to_string(),
                )
                .spawn()
        }) {
            Ok(_) => {
                self.orgs = next;
                cx.quit()
            }
            // The window still shows the old org, so the in-memory state keeps it too.
            Err(error) => {
                crate::diagnostics::record(format!("orgs: restart for the switch failed: {error}"));
                self.raise_restart_failure(&crate::notice::short_error(&error), cx);
            }
        }
    }

    fn raise_restart_failure(&mut self, detail: &str, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.org_restart_failure_shown, true) {
            return;
        }
        let text = app_base::ui_locale::tr("notice.generic_detail").replace("%{detail}", detail);
        self.raise_notice(text, None, cx);
    }

    fn save_orgs(&self, orgs: &Orgs) -> bool {
        if self.mode.demo {
            return false;
        }
        let saved = Orgs::path().map(|path| orgs.save(&path));
        if let Some(Err(error)) = &saved {
            crate::diagnostics::record(format!(
                "orgs: {} not saved: {error}",
                app_base::orgs::ORGS_FILE
            ));
        }
        matches!(saved, Some(Ok(())))
    }
}

/// The process's own flags without the earlier start-up flags of a switch, plus the one for this start.
fn restart_arguments(
    current: impl Iterator<Item = std::ffi::OsString>,
    added: Option<(&str, &str)>,
) -> Vec<std::ffi::OsString> {
    let mut arguments = Vec::new();
    let mut current = current;
    while let Some(argument) = current.next() {
        if argument == OPEN_CONVERSATION_FLAG || argument == REFUSED_ORG_FLAG {
            current.next();
            continue;
        }
        arguments.push(argument);
    }
    if let Some((flag, value)) = added {
        arguments.push(flag.into());
        arguments.push(value.into());
    }
    arguments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restart_keeps_the_flags_and_replaces_the_open_target() {
        let current = ["--endpoint", "x", OPEN_CONVERSATION_FLAG, "19:old"].map(Into::into);
        assert_eq!(
            restart_arguments(
                current.into_iter(),
                Some((OPEN_CONVERSATION_FLAG, "19:new"))
            ),
            ["--endpoint", "x", OPEN_CONVERSATION_FLAG, "19:new"].map(std::ffi::OsString::from)
        );
    }

    #[test]
    fn a_later_switch_drops_the_refused_org_of_the_earlier_one() {
        let current = ["--endpoint", "x", REFUSED_ORG_FLAG, "bbbb-02"].map(Into::into);
        assert_eq!(
            restart_arguments(current.into_iter(), None),
            ["--endpoint", "x"].map(std::ffi::OsString::from)
        );
    }
}
