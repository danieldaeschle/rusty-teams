use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, WindowExt as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::new_meeting_dialog::labeled;
use crate::i18n::tr;
use crate::app_state::AppState;
use crate::team_admin::{can_create_channel, channel_name_problem};
use crate::theme;

const DIALOG_WIDTH: f32 = 480.;
const FIELD_GAP: f32 = 12.;
const ERROR_SIZE: f32 = 12.;

struct EditChannelForm {
    state: Entity<AppState>,
    channel_id: String,
    name: Entity<InputState>,
    description: Entity<InputState>,
    description_loaded: bool,
    _subscriptions: Vec<Subscription>,
}

impl EditChannelForm {
    fn new(
        state: Entity<AppState>,
        channel_id: String,
        current_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Letters, numbers, and spaces are allowed")
                .default_value(current_name.to_owned())
        });
        let description = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Help others find the right channel by providing a description")
        });
        let subscriptions = vec![
            cx.subscribe_in(&name, window, Self::on_input),
            cx.subscribe_in(&description, window, Self::on_input),
        ];
        let loading = state.update(cx, |app, cx| app.load_channel_description(&channel_id, cx));
        cx.spawn_in(window, async move |form, cx| {
            let loaded = loading.await;
            form.update_in(cx, |form, window, cx| {
                form.fill_description(loaded, window, cx)
            })
            .ok();
        })
        .detach();
        EditChannelForm {
            state,
            channel_id,
            name,
            description,
            description_loaded: false,
            _subscriptions: subscriptions,
        }
    }

    fn fill_description(
        &mut self,
        loaded: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = loaded else {
            return;
        };
        self.description_loaded = true;
        if self.description.read(cx).value().is_empty() {
            self.description
                .update(cx, |input, cx| input.set_value(text, window, cx));
        }
        cx.notify();
    }

    fn on_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::PressEnter { .. }) && self.submit(cx) {
            window.close_dialog(cx);
        }
        cx.notify();
    }

    fn can_save(&self, cx: &App) -> bool {
        can_create_channel(&self.name.read(cx).value())
    }

    fn submit(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.can_save(cx) {
            return false;
        }
        let name = self.name.read(cx).value().to_string();
        let typed = self.description.read(cx).value().to_string();
        let description = (self.description_loaded || !typed.is_empty()).then_some(typed);
        let channel_id = self.channel_id.clone();
        self.state.update(cx, |state, cx| {
            state.edit_channel(&channel_id, &name, description.as_deref(), cx)
        });
        true
    }
}

impl Render for EditChannelForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let problem = channel_name_problem(&self.name.read(cx).value());
        v_flex()
            .debug_selector(|| "edit-channel".into())
            .gap(px(FIELD_GAP))
            .child(labeled("Channel name", Input::new(&self.name)))
            .when_some(problem, |form, problem| {
                form.child(
                    div()
                        .text_size(px(ERROR_SIZE))
                        .text_color(theme::red_soft())
                        .child(problem),
                )
            })
            .child(labeled("Description", Input::new(&self.description)))
    }
}

pub fn open_edit_channel_dialog(
    state: Entity<AppState>,
    channel_id: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let (channel_name, team_name) = {
        let app = state.read(cx);
        let channel_name = app.channel_name(channel_id).unwrap_or_default();
        let team_name = app
            .channel_team_id(channel_id)
            .and_then(|team_id| app.team_name(&team_id))
            .unwrap_or_default();
        (channel_name, team_name)
    };
    let title = format!("Edit \"{channel_name}\" channel in \"{team_name}\" team");
    let channel_id = channel_id.to_owned();
    let form = cx.new(|cx| EditChannelForm::new(state, channel_id, &channel_name, window, cx));
    let field = form.read(cx).name.clone();
    window.open_dialog(cx, move |dialog: Dialog, _, cx| {
        let submit_form = form.clone();
        let can_save = form.read(cx).can_save(cx);
        let footer = DialogFooter::new()
            .child(
                Button::new("edit-channel-cancel")
                    .label(tr("common.cancel"))
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("edit-channel-save")
                    .primary()
                    .label(tr("common.save"))
                    .disabled(!can_save)
                    .on_click(move |_, window, cx| {
                        if submit_form.update(cx, |form, cx| form.submit(cx)) {
                            window.close_dialog(cx);
                        }
                    }),
            );
        dialog
            .title(title.clone())
            .w(px(DIALOG_WIDTH))
            .child(form.clone())
            .footer(footer)
    });
    window.defer(cx, move |window, cx| {
        field.update(cx, |input, cx| input.focus(window, cx));
    });
}

pub fn confirm_delete_channel(
    state: Entity<AppState>,
    channel_id: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let (channel_name, private) = {
        let app = state.read(cx);
        (
            app.channel_name(channel_id).unwrap_or_default(),
            app.channel_is_private(channel_id),
        )
    };
    let description = if private {
        format!(
            "Are you sure you want to delete the {channel_name} channel? All files and conversations will be deleted."
        )
    } else {
        format!(
            "Are you sure you want to delete the {channel_name} channel? All conversations will be deleted."
        )
    };
    let channel_id = channel_id.to_owned();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (state, channel_id) = (state.clone(), channel_id.clone());
        let cancel = Button::new("delete-channel-cancel")
            .label(tr("common.cancel"))
            .on_click(|_, window, cx| window.close_dialog(cx));
        let delete = Button::new("delete-channel-confirm")
            .label(tr("common.delete"))
            .danger()
            .on_click(move |_, window, cx| {
                state.update(cx, |state, cx| state.delete_channel(&channel_id, cx));
                window.close_dialog(cx);
            });
        alert
            .title("Delete channel")
            .description(description.clone())
            .footer(DialogFooter::new().child(cancel).child(delete))
            .on_ok(|_, _, _| false)
    });
}
