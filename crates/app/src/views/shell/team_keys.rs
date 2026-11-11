use calling::AcceptMode;
use gpui_kit::*;
use teams_core::{ForcedAvailability, ForcedKind};

use super::AppShell;
use crate::app_state::AppState;
use crate::sidebar_model::Step;
use crate::views::shortcuts::{
    AcceptAudioCall, AcceptCall, GoToComposeBox, NextUnreadConversation,
    PreviousUnreadConversation, ReplyToLastMessage, SetPresenceAvailable, SetPresenceAway,
    SetPresenceBeRightBack, SetPresenceBusy, SetPresenceDoNotDisturb, ToggleBackgroundBlur,
    ToggleLiveCaptions,
};

fn force_presence(state: &Entity<AppState>, kind: ForcedKind, cx: &mut App) {
    let forced = ForcedAvailability {
        kind,
        expires_at: None,
    };
    state.update(cx, |state, cx| state.set_own_availability(Some(forced), cx));
}

fn presence_listener<A: Action>(
    state: &Entity<AppState>,
    kind: ForcedKind,
) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    let state = state.clone();
    move |_, _, cx| force_presence(&state, kind, cx)
}

impl AppShell {
    pub(super) fn team_keys(&self, shell: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        shell
            .on_action(cx.listener(|this, _: &GoToComposeBox, window, cx| {
                this.close_image_viewer(window, cx);
                this.conversation.update(cx, |conversation, cx| {
                    conversation.focus_composer(window, cx)
                });
            }))
            .on_action(cx.listener(|this, _: &ReplyToLastMessage, window, cx| {
                this.close_image_viewer(window, cx);
                this.conversation.update(cx, |conversation, cx| {
                    conversation.reply_to_latest_from_others(window, cx)
                });
            }))
            .on_action(
                cx.listener(|this, _: &PreviousUnreadConversation, window, cx| {
                    this.close_image_viewer(window, cx);
                    this.select_unread(Step::Previous, cx);
                }),
            )
            .on_action(cx.listener(|this, _: &NextUnreadConversation, window, cx| {
                this.close_image_viewer(window, cx);
                this.select_unread(Step::Next, cx);
            }))
            .on_action(presence_listener::<SetPresenceAvailable>(
                &self.state,
                ForcedKind::Available,
            ))
            .on_action(presence_listener::<SetPresenceBusy>(
                &self.state,
                ForcedKind::Busy,
            ))
            .on_action(presence_listener::<SetPresenceDoNotDisturb>(
                &self.state,
                ForcedKind::DoNotDisturb,
            ))
            .on_action(presence_listener::<SetPresenceBeRightBack>(
                &self.state,
                ForcedKind::BeRightBack,
            ))
            .on_action(presence_listener::<SetPresenceAway>(
                &self.state,
                ForcedKind::Away,
            ))
            .on_action(cx.listener(|this, _: &AcceptCall, _, cx| {
                this.state.update(cx, |state, cx| {
                    state.accept_ringing_call(AcceptMode::Video, cx)
                });
            }))
            .on_action(cx.listener(|this, _: &AcceptAudioCall, _, cx| {
                this.state.update(cx, |state, cx| {
                    state.accept_ringing_call(AcceptMode::Audio, cx)
                });
            }))
            .on_action(cx.listener(|this, _: &ToggleBackgroundBlur, _, cx| {
                this.state
                    .update(cx, |state, cx| state.toggle_call_background_blur(cx));
            }))
            .on_action(cx.listener(|this, _: &ToggleLiveCaptions, _, cx| {
                this.state
                    .update(cx, |state, cx| state.toggle_call_captions(cx));
            }))
    }

    fn select_unread(&mut self, step: Step, cx: &mut Context<Self>) {
        let target = self.state.read(cx).adjacent_unread_chat_id(step);
        if let Some(chat_id) = target {
            self.sidebar
                .update(cx, |sidebar, cx| sidebar.show_chats(cx));
            self.state.update(cx, |state, cx| {
                state.select(crate::app_state::Selection::Chat(chat_id), cx)
            });
        }
    }
}
