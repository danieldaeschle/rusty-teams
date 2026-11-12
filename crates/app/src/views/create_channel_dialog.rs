use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::Radio;
use gpui_kit::component::{Disableable as _, WindowExt as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::ChannelPrivacy;

use super::new_meeting_dialog::labeled;
use crate::i18n::tr;
use crate::app_state::AppState;
use crate::team_admin::{can_create_channel, channel_name_problem};
use crate::theme;

const DIALOG_WIDTH: f32 = 480.;
const FIELD_GAP: f32 = 12.;
const OPTION_GAP: f32 = 8.;
const ERROR_SIZE: f32 = 12.;

struct CreateChannelForm {
    state: Entity<AppState>,
    team_id: String,
    name: Entity<InputState>,
    description: Entity<InputState>,
    privacy: ChannelPrivacy,
    private_allowed: bool,
    _subscriptions: Vec<Subscription>,
}

impl CreateChannelForm {
    fn new(
        state: Entity<AppState>,
        team_id: String,
        private_allowed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Letters, numbers, and spaces are allowed")
        });
        let description = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Help others find the right channel by providing a description")
        });
        let subscriptions = vec![
            cx.subscribe_in(&name, window, Self::on_input),
            cx.subscribe_in(&description, window, Self::on_input),
        ];
        CreateChannelForm {
            state,
            team_id,
            name,
            description,
            privacy: ChannelPrivacy::Standard,
            private_allowed,
            _subscriptions: subscriptions,
        }
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

    fn can_create(&self, cx: &App) -> bool {
        can_create_channel(&self.name.read(cx).value())
    }

    fn submit(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.can_create(cx) {
            return false;
        }
        let name = self.name.read(cx).value().to_string();
        let description = self.description.read(cx).value().to_string();
        let (team_id, privacy) = (self.team_id.clone(), self.privacy);
        self.state.update(cx, |state, cx| {
            state.create_channel(&team_id, &name, &description, privacy, cx)
        });
        true
    }

    fn choose(&mut self, privacy: ChannelPrivacy, cx: &mut Context<Self>) {
        self.privacy = privacy;
        cx.notify();
    }

    fn privacy_options(&self, cx: &mut Context<Self>) -> Div {
        let form = cx.entity();
        let option = |id: &'static str, label: &'static str, privacy: ChannelPrivacy| {
            let form = form.clone();
            Radio::new(id)
                .label(label)
                .checked(self.privacy == privacy)
                .on_click(move |_, _, cx| form.update(cx, |form, cx| form.choose(privacy, cx)))
        };
        v_flex()
            .debug_selector(|| "create-channel/type".into())
            .gap(px(OPTION_GAP))
            .child(option(
                "create-channel-standard",
                "Standard: Everyone on the team has access",
                ChannelPrivacy::Standard,
            ))
            .when(self.private_allowed, |options| {
                options.child(option(
                    "create-channel-private",
                    "Private: Specific teammates have access",
                    ChannelPrivacy::Private,
                ))
            })
    }
}

impl Render for CreateChannelForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let problem = channel_name_problem(&self.name.read(cx).value());
        v_flex()
            .debug_selector(|| "create-channel".into())
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
            .child(labeled(
                "Description (optional)",
                Input::new(&self.description),
            ))
            .child(labeled("Choose a channel type", self.privacy_options(cx)))
    }
}

pub fn open_create_channel_dialog(
    state: Entity<AppState>,
    team_id: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let (team_name, rights) = {
        let app = state.read(cx);
        (
            app.team_name(team_id).unwrap_or_default(),
            app.team_rights(team_id),
        )
    };
    let title = format!("Create a channel for \"{team_name}\" team");
    let team_id = team_id.to_owned();
    let form =
        cx.new(|cx| CreateChannelForm::new(state, team_id, rights.add_private_channel, window, cx));
    let field = form.read(cx).name.clone();
    window.open_dialog(cx, move |dialog: Dialog, _, cx| {
        let submit_form = form.clone();
        let can_create = form.read(cx).can_create(cx);
        let footer = DialogFooter::new()
            .child(
                Button::new("create-channel-cancel")
                    .label(tr("common.cancel"))
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("create-channel-confirm")
                    .primary()
                    .label(tr("common.create"))
                    .disabled(!can_create)
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
