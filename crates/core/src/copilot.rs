use chatsvc::compose_copilot::{ComposeAction, ComposeRewrite};
use chatsvc::copilot::CopilotAsk;
use chrono::Local;

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::remote::Remote;

const COPILOT_LOCALE: &str = "en-us";
const SECONDS_PER_HOUR: i32 = 3600;
const FALLBACK_TIME_ZONE: &str = "UTC";

impl<R: Remote> SyncEngine<R> {
    pub async fn ask_copilot(&self, chat_id: &str, prompt: &str) -> Result<String> {
        let ask = CopilotAsk {
            user_id: self.my_user_id().await?,
            tenant_id: self.my_tenant_id(chat_id).await?,
            chat_id: chat_id.to_owned(),
            prompt: prompt.to_owned(),
            locale: COPILOT_LOCALE.to_owned(),
            utc_offset_hours: Local::now().offset().local_minus_utc() / SECONDS_PER_HOUR,
        };
        self.remote.ask_copilot(&ask).await
    }

    /// Copilot's proposal for the draft as Teams HTML; the draft itself stays untouched.
    pub async fn rewrite_draft(&self, chat_id: &str, user_html: &str, action: ComposeAction) -> Result<String> {
        let rewrite = ComposeRewrite {
            tenant_id: self.my_tenant_id(chat_id).await?,
            thread_id: chat_id.to_owned(),
            user_html: user_html.to_owned(),
            action,
            ui_language: COPILOT_LOCALE.to_owned(),
            time_zone: iana_time_zone::get_timezone().unwrap_or_else(|_| FALLBACK_TIME_ZONE.to_owned()),
        };
        self.remote.rewrite_draft(&rewrite).await
    }
}
