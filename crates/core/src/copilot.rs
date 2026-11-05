use chatsvc::copilot::CopilotAsk;
use chrono::Local;

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::remote::Remote;

const COPILOT_LOCALE: &str = "en-us";
const SECONDS_PER_HOUR: i32 = 3600;

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
}
