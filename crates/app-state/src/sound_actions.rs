use chatsvc::sound_settings::SoundSettings;
use gpui_kit::Context;

use crate::app_state::{AppEvent, AppState};
use crate::runtime;

const ORGID_PREFIX: &str = "8:orgid:";

pub fn user_mri(user_id: &str) -> String {
    if user_id.starts_with("8:") {
        user_id.to_owned()
    } else {
        format!("{ORGID_PREFIX}{user_id}")
    }
}

impl AppState {
    pub fn refresh_sound_settings(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let read = runtime::spawn(async move { engine.sound_settings().await });
        cx.spawn(async move |this, cx| {
            let settings = match read.await {
                Ok(Ok(settings)) => settings,
                Ok(Err(error)) => {
                    log::warn!("sound settings not read: {error}");
                    return;
                }
                Err(_) => return,
            };
            this.update(cx, |state, cx| state.apply_account_sounds(settings, cx))
                .ok();
        })
        .detach();
    }

    pub fn save_sound_settings(
        &mut self,
        previous: SoundSettings,
        updated: SoundSettings,
        cx: &mut Context<Self>,
    ) {
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            self.apply_account_sounds(previous, cx);
            return;
        };
        let Some(engine) = engine_or_demo else {
            self.account_sounds = Some(updated);
            return;
        };
        let Some(user_mri) = self.directory.me.as_ref().map(|me| user_mri(&me.user_id)) else {
            self.apply_account_sounds(previous, cx);
            return;
        };
        self.account_sounds = Some(updated.clone());
        let restore = previous.clone();
        self.run_chat_action(
            "Sound setting",
            async move {
                engine
                    .save_sound_settings(&user_mri, &previous, &updated)
                    .await
            },
            move |state, cx| state.apply_account_sounds(restore, cx),
            |_, _| {},
            cx,
        );
    }

    fn apply_account_sounds(&mut self, settings: SoundSettings, cx: &mut Context<Self>) {
        self.account_sounds = Some(settings);
        cx.emit(AppEvent::SoundSettings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ringtone_endpoint_gets_a_skype_mri() {
        assert_eq!(user_mri("f74e540a"), "8:orgid:f74e540a");
        assert_eq!(user_mri("8:orgid:f74e540a"), "8:orgid:f74e540a");
    }
}
