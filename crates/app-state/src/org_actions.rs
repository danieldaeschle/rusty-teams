use app_base::orgs::{Org, Orgs};
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

    /// Restarts the app on `org`: its own cache opens at once and the backend signs in to its tenant.
    pub fn switch_org(&mut self, org: &Org, cx: &mut Context<Self>) {
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
                .args(std::env::args_os().skip(1))
                .env(app_base::updater::RELAUNCH_ENV, std::process::id().to_string())
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
            crate::diagnostics::record(format!("orgs: {} not saved: {error}", app_base::orgs::ORGS_FILE));
        }
        matches!(saved, Some(Ok(())))
    }
}
