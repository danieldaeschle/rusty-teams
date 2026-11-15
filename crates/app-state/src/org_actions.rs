use app_base::orgs::{OPEN_CONVERSATION_FLAG, Org, Orgs};
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
        let mut next = self.orgs.clone();
        next.activate(org);
        if !self.save_orgs(&next) {
            return;
        }
        self.orgs = next;
        match std::env::current_exe().and_then(|exe| {
            std::process::Command::new(exe)
                .args(restart_arguments(
                    std::env::args_os().skip(1),
                    open_conversation,
                ))
                .env(
                    app_base::updater::RELAUNCH_ENV,
                    std::process::id().to_string(),
                )
                .spawn()
        }) {
            Ok(_) => cx.quit(),
            Err(error) => {
                crate::diagnostics::record(format!("orgs: restart for the switch failed: {error}"))
            }
        }
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

/// The process's own flags without an earlier open target, plus the chat to open now.
fn restart_arguments(
    current: impl Iterator<Item = std::ffi::OsString>,
    open_conversation: Option<&str>,
) -> Vec<std::ffi::OsString> {
    let mut arguments = Vec::new();
    let mut current = current;
    while let Some(argument) = current.next() {
        if argument == OPEN_CONVERSATION_FLAG {
            current.next();
            continue;
        }
        arguments.push(argument);
    }
    if let Some(conversation_id) = open_conversation {
        arguments.push(OPEN_CONVERSATION_FLAG.into());
        arguments.push(conversation_id.into());
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
            restart_arguments(current.into_iter(), Some("19:new")),
            ["--endpoint", "x", OPEN_CONVERSATION_FLAG, "19:new"].map(std::ffi::OsString::from)
        );
    }
}
