use gpui_kit::*;
use teams_core::{AdaptiveCard, CardActionOutcome, ExecuteAction, ExecuteTrigger};
use tokio::sync::oneshot;

use crate::app_state::AppState;
use crate::card_actions::CardAnswer;
use crate::card_state::{CardScope, short_reason};
use crate::runtime;

impl AppState {
    pub fn request_card_refreshes(
        &mut self,
        conversation_id: &str,
        message_id: &str,
        cards: &[AdaptiveCard],
        cx: &mut Context<Self>,
    ) {
        if self.mode.read_only {
            return;
        }
        let Some((card_index, refresh)) = cards
            .iter()
            .enumerate()
            .find_map(|(index, card)| card.refresh.as_ref().map(|refresh| (index, refresh)))
        else {
            return;
        };
        let member_count = self
            .sidebar
            .chats
            .iter()
            .find(|chat| chat.id == conversation_id)
            .map(|chat| chat.members.len());
        let my_user_id = self.directory.me.as_ref().map(|me| me.user_id.as_str());
        if !refresh.runs_automatically(member_count, my_user_id) {
            return;
        }
        let scope = CardScope::message(conversation_id, message_id, card_index);
        self.start_card_refresh(scope, refresh.action.clone(), ExecuteTrigger::Automatic, cx);
    }

    pub fn refresh_card_manually(
        &mut self,
        scope: CardScope,
        action: ExecuteAction,
        cx: &mut Context<Self>,
    ) {
        self.start_card_refresh(scope, action, ExecuteTrigger::Manual, cx);
    }

    fn start_card_refresh(
        &mut self,
        scope: CardScope,
        action: ExecuteAction,
        trigger: ExecuteTrigger,
        cx: &mut Context<Self>,
    ) {
        let manual = trigger == ExecuteTrigger::Manual;
        if !self
            .cards
            .begin_refresh(&scope.conversation_id, &scope.message_id, manual)
        {
            return;
        }
        let Some(receiver) = self.refresh_answer(&scope, action, trigger) else {
            self.cards
                .cancel_refresh(&scope.conversation_id, &scope.message_id);
            return;
        };
        cx.spawn(async move |this, cx| {
            let answer = receiver
                .await
                .unwrap_or_else(|_| Err("cancelled".to_owned()));
            this.update(cx, |state, cx| {
                state.finish_card_refresh(&scope, answer, cx)
            })
            .ok();
        })
        .detach();
    }

    fn refresh_answer(
        &self,
        scope: &CardScope,
        action: ExecuteAction,
        trigger: ExecuteTrigger,
    ) -> Option<oneshot::Receiver<CardAnswer>> {
        if self.mode.demo {
            return Some(runtime::spawn(crate::demo::refresh_answer()));
        }
        let engine = self.engine.clone()?;
        let (conversation_id, message_id) =
            (scope.conversation_id.clone(), scope.message_id.clone());
        Some(runtime::spawn(async move {
            let outcome = engine
                .card_refresh(&conversation_id, &message_id, &action, trigger)
                .await
                .map_err(|error| short_reason(&error.to_string()))?;
            Ok((outcome, None))
        }))
    }

    fn finish_card_refresh(
        &mut self,
        scope: &CardScope,
        answer: CardAnswer,
        cx: &mut Context<Self>,
    ) {
        let was_full = self.cards.finish_refresh();
        let replaced = match answer {
            Ok((CardActionOutcome::ReplaceCard(json), _)) => self.replace_card(scope, &json, cx),
            Ok((CardActionOutcome::Failed(reason), _)) | Err(reason) => {
                eprintln!("card refresh failed: {reason}");
                false
            }
            Ok(_) => false,
        };
        if was_full && !replaced {
            self.announce_cards(scope, cx);
        }
    }
}
