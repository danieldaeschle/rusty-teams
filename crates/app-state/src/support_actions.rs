use gpui_kit::Context;

use crate::app_state::AppState;
use crate::notice::NoticeAction;
use crate::support::{self, ClientInfo, SupportBundlePaths};
use crate::support_bundle::{self, SupportBundle};
use calling::DeviceChoice;

impl AppState {
    pub fn client_info(&self) -> ClientInfo {
        let locale = app_base::ui_locale::effective_locale(app_base::ui_locale::load(&self.store));
        ClientInfo::capture(
            env!("CARGO_PKG_VERSION"),
            locale,
            self.directory.me.is_some(),
            self.call_report_enabled,
            device_label(&self.call_input),
            device_label(&self.call_output),
            device_label(&self.call_camera),
        )
    }

    pub fn create_support_bundle(&self) -> Result<SupportBundle, String> {
        let data_dir =
            support::local_data_dir().ok_or_else(|| "data directory unavailable".to_owned())?;
        let diagnostics = support::diagnostics_log_path()
            .unwrap_or_else(|| data_dir.join(app_base::diagnostics::DIAGNOSTICS_FILE));
        support_bundle::write_support_bundle(&data_dir, &self.client_info(), &diagnostics)
            .map_err(|error| error.to_string())
    }

    pub fn github_issue_url_for_bundle(&self, bundle: &SupportBundle) -> String {
        support::issue_url(
            &self.client_info(),
            &SupportBundlePaths {
                directory: bundle.directory.clone(),
                folder_name: bundle.folder_name.clone(),
            },
        )
    }

    pub fn reveal_support_bundle(bundle: &SupportBundle) {
        crate::downloads::reveal_in_file_manager(&bundle.directory);
    }

    pub fn reveal_logs_folder() {
        if let Some(directory) = support::local_data_dir().or_else(app_base::diagnostics::directory)
        {
            crate::downloads::reveal_in_file_manager(&directory);
        }
    }

    pub fn open_github_issue_with_bundle(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let bundle = self.create_support_bundle()?;
        let url = self.github_issue_url_for_bundle(&bundle);
        let folder = bundle.directory.clone();
        crate::external_browser::open_url(cx, &self.store, &url);
        let action = NoticeAction::new("Open folder", move |_, _| {
            crate::downloads::reveal_in_file_manager(&folder);
        });
        self.raise_notice(
            format!(
                "Support package ready: {}. Attach those files to the GitHub issue.",
                bundle.folder_name
            ),
            Some(action),
            cx,
        );
        Ok(())
    }

    pub fn diagnostics_enabled(&self) -> bool {
        app_base::diagnostics::is_enabled()
    }

    pub fn set_diagnostics_enabled(&mut self, on: bool, cx: &mut Context<Self>) {
        app_base::diagnostics::set_enabled(on);
        let _ = self.store.set_meta(
            app_base::diagnostics::DIAGNOSTICS_SETTING_KEY,
            if on { "1" } else { "0" },
        );
        if on {
            app_base::support::record_startup_client_line(
                env!("CARGO_PKG_VERSION"),
                app_base::ui_locale::effective_locale(app_base::ui_locale::load(&self.store)),
                self.directory.me.is_some(),
                self.call_report_enabled,
            );
        }
        cx.notify();
    }

    pub fn clear_diagnostic_logs(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = app_base::diagnostics::clear_logs() {
            app_base::diagnostics::record(format!("clear logs: {error}"));
        }
        cx.notify();
    }
}

fn device_label(choice: &DeviceChoice) -> String {
    match choice {
        DeviceChoice::SystemDefault => "default".to_owned(),
        DeviceChoice::Device(key) if key.is_empty() => "none".to_owned(),
        DeviceChoice::Device(_) => "selected".to_owned(),
    }
}
