use gpui_kit::component::input::{Input, InputContentType, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::rows::{ROW_INSET, control_row, stacked_field, text_button};
use crate::app_state::AppState;
use crate::i18n::tr;
use crate::theme;

pub struct GoogleClientForm {
    app: Entity<AppState>,
    client_id: Entity<InputState>,
    secret: Entity<InputState>,
    saved_id: String,
    secret_present: bool,
}

impl GoogleClientForm {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let saved_id = crate::google_oauth_client::load_client_id(&app.read(cx).store);
        let client_id =
            cx.new(|cx| InputState::new(window, cx).placeholder("….apps.googleusercontent.com"));
        if !saved_id.is_empty() {
            let text = saved_id.clone();
            client_id.update(cx, |input, cx| input.set_value(text, window, cx));
        }
        let secret = cx.new(|cx| InputState::new(window, cx));
        secret.update(cx, |input, cx| input.toggle_masked(window, cx));
        if !saved_id.is_empty() {
            cx.spawn_in(window, async move |form, cx| {
                let present =
                    crate::runtime::spawn(crate::google_oauth_client::secret_is_stored()).await;
                if !matches!(present, Ok(true)) {
                    return;
                }
                form.update_in(cx, |form, window, cx| {
                    form.secret_present = true;
                    form.secret.update(cx, |input, cx| {
                        input.set_placeholder(
                            tr("settings.calendar.google_client_secret_kept"),
                            window,
                            cx,
                        );
                    });
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        GoogleClientForm {
            app,
            client_id,
            secret,
            saved_id,
            secret_present: false,
        }
    }

    fn status_detail(&self) -> String {
        if !self.saved_id.is_empty() && self.secret_present {
            tr("settings.calendar.google_client_status_own")
        } else if chatsvc::client_id_from_env().is_some() {
            tr("settings.calendar.google_client_status_builtin")
        } else {
            tr("settings.calendar.google_client_status_missing")
        }
    }

    fn submit(&mut self, clear: bool, window: &mut Window, cx: &mut Context<Self>) {
        let client_id = if clear {
            String::new()
        } else {
            self.client_id.read(cx).value().to_string()
        };
        let secret = if clear {
            String::new()
        } else {
            self.secret.read(cx).value().to_string()
        };
        self.secret
            .update(cx, |input, cx| input.set_value("", window, cx));
        let store = self.app.read(cx).store.clone();
        let connected = self.app.read(cx).calendar_accounts().has_google();
        let app = self.app.clone();
        let id_field = self.client_id.clone();
        let secret_field = self.secret.clone();
        let saved_label = client_id.trim().to_owned();
        cx.spawn_in(window, async move |form, cx| {
            let result = crate::runtime::spawn(async move {
                crate::google_oauth_client::save(&store, &client_id, &secret).await
            })
            .await;
            form.update_in(cx, |form, window, cx| {
                let notice = match result {
                    Ok(Ok(crate::google_oauth_client::GoogleClientSave::Saved)) => {
                        form.saved_id = saved_label;
                        form.secret_present = true;
                        if connected {
                            tr("settings.calendar.google_client_saved_reconnect")
                        } else {
                            tr("settings.calendar.google_client_saved")
                        }
                    }
                    Ok(Ok(crate::google_oauth_client::GoogleClientSave::Cleared)) => {
                        form.saved_id.clear();
                        form.secret_present = false;
                        id_field.update(cx, |input, cx| input.set_value("", window, cx));
                        secret_field.update(cx, |input, cx| input.set_placeholder("", window, cx));
                        tr("settings.calendar.google_client_cleared")
                    }
                    Ok(Err(crate::google_oauth_client::GoogleClientSaveError::SecretRequired)) => {
                        tr("settings.calendar.google_client_secret_required")
                    }
                    _ => tr("settings.calendar.google_client_save_failed"),
                };
                if form.secret_present {
                    secret_field.update(cx, |input, cx| {
                        input.set_placeholder(
                            tr("settings.calendar.google_client_secret_kept"),
                            window,
                            cx,
                        );
                    });
                }
                app.update(cx, |state, cx| state.raise_notice(notice, None, cx));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for GoogleClientForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let save = cx.entity().clone();
        let reset = cx.entity().clone();
        v_flex()
            .w_full()
            .gap(px(8.))
            .child(control_row(
                tr("settings.calendar.google_client_status_title"),
                self.status_detail(),
                div(),
            ))
            .child(stacked_field(
                tr("settings.calendar.google_client_id"),
                "",
                Input::new(&self.client_id),
            ))
            .child(stacked_field(
                tr("settings.calendar.google_client_secret"),
                "",
                Input::new(&self.secret)
                    .content_type(InputContentType::Password)
                    .mask_toggle(),
            ))
            .child(guide())
            .child(control_row(
                tr("settings.calendar.google_client_save"),
                "",
                h_flex()
                    .gap_2()
                    .child(
                        text_button(
                            "settings-google-client-save",
                            tr("settings.calendar.google_client_save"),
                        )
                        .on_click(move |_, window, cx| {
                            save.update(cx, |form, cx| form.submit(false, window, cx))
                        }),
                    )
                    .child(
                        text_button(
                            "settings-google-client-reset",
                            tr("settings.calendar.google_client_reset"),
                        )
                        .on_click(move |_, window, cx| {
                            reset.update(cx, |form, cx| form.submit(true, window, cx))
                        }),
                    ),
            ))
    }
}

fn guide() -> Div {
    v_flex()
        .w_full()
        .px(px(ROW_INSET))
        .gap(px(2.))
        .child(guide_line(tr("settings.calendar.google_client_guide_1")))
        .child(guide_line(tr("settings.calendar.google_client_guide_2")))
        .child(guide_line(tr("settings.calendar.google_client_guide_3")))
        .child(guide_line(tr("settings.calendar.google_client_guide_4")))
        .child(guide_line(tr("settings.calendar.google_client_guide_5")))
}

fn guide_line(text: String) -> Div {
    div()
        .w_full()
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(theme::text_muted())
        .child(text)
}
