use gpui_kit::*;
use serde_json::Value;
use teams_core::{
    AdaptiveCard, CardAction, CardActionKind, CardActionOutcome, ChatApp, TaskDialog,
    TaskDialogKind, adaptive_cards,
};
use tokio::sync::oneshot;

use crate::app_state::{AppEvent, AppState};
use crate::card_state::{
    ActionPhase, BotIdentity, CardOverride, CardScope, DONE_HOLD, Surface, TaskDialogState,
    short_reason,
};
use crate::data::{is_one_on_one, others};
use crate::runtime;
use crate::task_dialog::{UrlDialog, UrlDialogNext, open_url_dialog};

const NOT_CONNECTED: &str = "Not connected";
const READ_ONLY: &str = "Read-only mode";
const CANCELLED: &str = "Cancelled";
const DIALOG_MESSAGE_HOLD: std::time::Duration = std::time::Duration::from_millis(2200);

pub enum CardTask {
    Action(CardAction),
    Submit(Value),
}

pub type CardAnswer = Result<(CardActionOutcome, Option<(ChatApp, String)>), String>;

impl AppState {
    pub fn run_card_action(
        &mut self,
        scope: CardScope,
        action_key: String,
        action: CardAction,
        cx: &mut Context<Self>,
    ) {
        let task = match (&scope.surface, &action.kind) {
            (Surface::Dialog, CardActionKind::Submit(submit)) => {
                CardTask::Submit(submit.data.clone())
            }
            _ => CardTask::Action(action),
        };
        if self.cards.is_busy(&action_key) {
            return;
        }
        self.cards.set_phase(&action_key, ActionPhase::Busy);
        self.cards.clear_note(&scope.card_key());
        self.announce_cards(&scope, cx);
        let answer = self.card_answer(&scope, task);
        cx.spawn(async move |this, cx| {
            let answer = match answer {
                Some(receiver) => receiver.await.unwrap_or_else(|_| Err(CANCELLED.to_owned())),
                None => Err(NOT_CONNECTED.to_owned()),
            };
            this.update(cx, |state, cx| {
                state.finish_card_task(scope, action_key, answer, cx)
            })
            .ok();
        })
        .detach();
    }

    fn card_answer(
        &self,
        scope: &CardScope,
        task: CardTask,
    ) -> Option<oneshot::Receiver<CardAnswer>> {
        if self.mode.demo {
            return Some(runtime::spawn(crate::demo::card_answer(task)));
        }
        if self.mode.read_only {
            return Some(runtime::spawn(async { Err(READ_ONLY.to_owned()) }));
        }
        let engine = self.engine.clone()?;
        let (conversation_id, message_id) =
            (scope.conversation_id.clone(), scope.message_id.clone());
        Some(runtime::spawn(async move {
            let outcome = match task {
                CardTask::Action(action) => {
                    engine
                        .card_action(&conversation_id, &message_id, &action)
                        .await
                }
                CardTask::Submit(data) => {
                    engine
                        .task_submit(&conversation_id, &message_id, data)
                        .await
                }
            }
            .map_err(|error| short_reason(&error.to_string()))?;
            let app = engine.card_app(&conversation_id, &message_id).await.ok();
            Ok((outcome, app))
        }))
    }

    fn finish_card_task(
        &mut self,
        scope: CardScope,
        action_key: String,
        answer: CardAnswer,
        cx: &mut Context<Self>,
    ) {
        let (outcome, app) = match answer {
            Ok(answer) => answer,
            Err(reason) => {
                self.fail_card_action(&scope, &action_key, reason, cx);
                return;
            }
        };
        let in_dialog = scope.surface == Surface::Dialog;
        match outcome {
            CardActionOutcome::Sent => {
                self.mark_done(&scope, &action_key, cx);
                if in_dialog {
                    self.close_task_dialog(cx);
                }
            }
            CardActionOutcome::ReplaceCard(json) => {
                if self.replace_card(&scope, &json, cx) {
                    self.mark_done(&scope, &action_key, cx);
                } else {
                    self.fail_card_action(&scope, &action_key, "Unreadable card".into(), cx);
                }
            }
            CardActionOutcome::Message(text) => {
                self.mark_done(&scope, &action_key, cx);
                self.note_card(&scope, text, cx);
                if in_dialog {
                    self.close_task_dialog_later(cx);
                }
            }
            CardActionOutcome::Dialog(dialog) => {
                self.mark_done(&scope, &action_key, cx);
                self.open_task_dialog(&scope, dialog, app, cx);
            }
            CardActionOutcome::Failed(reason) => {
                self.fail_card_action(&scope, &action_key, reason, cx)
            }
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn submit_url_dialog(
        &mut self,
        scope: CardScope,
        result: Value,
        cx: &mut Context<Self>,
    ) -> Task<UrlDialogNext> {
        let answer = self.card_answer(&scope, CardTask::Submit(result));
        cx.spawn(async move |this, cx| {
            let answer = match answer {
                Some(receiver) => receiver.await.unwrap_or_else(|_| Err(CANCELLED.to_owned())),
                None => Err(NOT_CONNECTED.to_owned()),
            };
            this.update(cx, |state, cx| {
                state.finish_url_dialog_submit(&scope, answer, cx)
            })
            .unwrap_or(UrlDialogNext::Close)
        })
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn finish_url_dialog_submit(
        &mut self,
        scope: &CardScope,
        answer: CardAnswer,
        cx: &mut Context<Self>,
    ) -> UrlDialogNext {
        let (outcome, app) = match answer {
            Ok(answer) => answer,
            Err(reason) => (CardActionOutcome::Failed(reason), None),
        };
        match outcome {
            CardActionOutcome::Sent => {}
            CardActionOutcome::ReplaceCard(json) => {
                self.replace_card(scope, &json, cx);
            }
            CardActionOutcome::Message(text) => self.note_card(scope, text, cx),
            CardActionOutcome::Failed(reason) => self.note_card(scope, short_reason(&reason), cx),
            CardActionOutcome::Dialog(dialog) => {
                if let TaskDialogKind::Url { url, .. } = &dialog.kind {
                    return UrlDialogNext::Navigate {
                        url: url.clone(),
                        width: dialog.width,
                        height: dialog.height,
                    };
                }
                self.open_task_dialog(scope, dialog, app, cx);
            }
        }
        UrlDialogNext::Close
    }

    fn note_card(&mut self, scope: &CardScope, text: String, cx: &mut Context<Self>) {
        self.cards.set_note(&scope.card_key(), text);
        self.announce_cards(scope, cx);
    }

    fn fail_card_action(
        &mut self,
        scope: &CardScope,
        action_key: &str,
        reason: String,
        cx: &mut Context<Self>,
    ) {
        self.cards
            .set_phase(action_key, ActionPhase::Failed(short_reason(&reason)));
        self.announce_cards(scope, cx);
    }

    fn mark_done(&mut self, scope: &CardScope, action_key: &str, cx: &mut Context<Self>) {
        self.cards.set_phase(action_key, ActionPhase::Done);
        self.announce_cards(scope, cx);
        let (scope, action_key) = (scope.clone(), action_key.to_owned());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DONE_HOLD).await;
            this.update(cx, |state, cx| {
                state.cards.clear_done(&action_key);
                state.announce_cards(&scope, cx);
            })
            .ok();
        })
        .detach();
    }

    fn announce_cards(&mut self, scope: &CardScope, cx: &mut Context<Self>) {
        cx.emit(AppEvent::Cards(scope.conversation_id.clone()));
        cx.notify();
    }

    fn replace_card(&mut self, scope: &CardScope, json: &str, cx: &mut Context<Self>) -> bool {
        let Some(card) = AdaptiveCard::parse(json) else {
            return false;
        };
        if scope.surface == Surface::Dialog {
            if let Some(dialog) = self.task_dialog.as_mut() {
                dialog.card = card;
            }
            self.announce_cards(scope, cx);
            return true;
        }
        let Some(record) = self
            .store
            .messages_by_id(
                &scope.conversation_id,
                std::slice::from_ref(&scope.message_id),
            )
            .ok()
            .and_then(|mut records| records.remove(&scope.message_id))
        else {
            return false;
        };
        let mut cards = adaptive_cards(&record);
        match cards.get_mut(scope.card_index) {
            Some(slot) => *slot = card,
            None => cards.push(card),
        }
        self.cards.set_override(
            &scope.conversation_id,
            &scope.message_id,
            CardOverride {
                basis: record.attachments_json,
                cards,
            },
        );
        self.announce_cards(scope, cx);
        true
    }

    fn open_task_dialog(
        &mut self,
        scope: &CardScope,
        dialog: TaskDialog,
        app: Option<(ChatApp, String)>,
        cx: &mut Context<Self>,
    ) {
        let app_name = app.as_ref().map(|(app, _)| app.name.clone());
        let title = dialog.title.clone().or(app_name).unwrap_or_default();
        let in_dialog = scope.surface == Surface::Dialog;
        match dialog.kind {
            TaskDialogKind::Card(json) => {
                let Some(card) = AdaptiveCard::parse(&json) else {
                    return;
                };
                self.cards.clear_card(&scope.card_key());
                self.task_dialog = Some(TaskDialogState {
                    title,
                    card,
                    scope: CardScope {
                        card_index: 0,
                        surface: Surface::Dialog,
                        ..scope.clone()
                    },
                    width: dialog.width,
                    height: dialog.height,
                });
                cx.emit(AppEvent::TaskDialog);
                cx.notify();
            }
            TaskDialogKind::Url { url, fallback_url } => {
                let (app_id, bot_id, web_application_resource) = match app {
                    Some((app, bot_id)) => (app.app_id, bot_id, app.web_application_resource),
                    None => (String::new(), String::new(), None),
                };
                open_url_dialog(
                    UrlDialog {
                        title,
                        url,
                        fallback_url,
                        width: dialog.width,
                        height: dialog.height,
                        app_id,
                        bot_id,
                        scope: scope.clone(),
                        web_application_resource,
                    },
                    cx,
                );
                if in_dialog {
                    self.close_task_dialog(cx);
                }
            }
        }
    }

    pub fn close_task_dialog(&mut self, cx: &mut Context<Self>) {
        if self.task_dialog.take().is_some() {
            self.cards.clear_card(&CardScope::dialog_key());
            cx.emit(AppEvent::TaskDialog);
            cx.notify();
        }
    }

    fn close_task_dialog_later(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DIALOG_MESSAGE_HOLD).await;
            this.update(cx, |state, cx| state.close_task_dialog(cx))
                .ok();
        })
        .detach();
    }

    pub fn toggle_card_elements(
        &mut self,
        scope: &CardScope,
        root_key: &str,
        elements: Vec<(String, bool, Option<bool>)>,
        cx: &mut Context<Self>,
    ) {
        for (element_id, initial, forced) in elements {
            self.cards
                .toggle_visibility(&format!("{root_key}/{element_id}"), initial, forced);
        }
        self.announce_cards(scope, cx);
    }

    pub fn toggle_show_card(
        &mut self,
        scope: &CardScope,
        actions_key: &str,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        self.cards.toggle_open_card(actions_key, index);
        self.announce_cards(scope, cx);
    }

    pub fn bot_identity(&self, conversation_id: &str, application_id: &str) -> Option<BotIdentity> {
        let apps = self.bot_apps.get(conversation_id)?;
        let matched = apps
            .iter()
            .find(|app| app.has_bot(application_id) || app.app_id == application_id);
        let mut with_bots = apps.iter().filter(|app| !app.bot_ids.is_empty());
        let app = match (matched, with_bots.next(), with_bots.next()) {
            (Some(app), _, _) | (None, Some(app), None) => app,
            _ => return None,
        };
        Some(BotIdentity {
            name: app.name.clone(),
            icon_url: app.small_image_url.clone(),
        })
    }

    pub fn request_chat_apps(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        if self.bot_apps.contains_key(conversation_id)
            || self.bot_apps_pending.contains(conversation_id)
        {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        self.bot_apps_pending.insert(conversation_id.to_owned());
        let conversation_id = conversation_id.to_owned();
        let receiver = {
            let conversation_id = conversation_id.clone();
            runtime::spawn(async move { engine.chat_apps(&conversation_id).await })
        };
        cx.spawn(async move |this, cx| {
            let apps = receiver.await.ok().and_then(Result::ok).unwrap_or_default();
            this.update(cx, |state, cx| {
                state.bot_apps_pending.remove(&conversation_id);
                state.bot_apps.insert(conversation_id, apps);
                state.apply_bot_titles();
                cx.emit(AppEvent::Sidebar);
                cx.emit(AppEvent::Directory);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn apply_bot_titles(&mut self) {
        let me = self.directory.me.clone();
        for chat in self.sidebar.chats.iter_mut() {
            let Some(apps) = self.bot_apps.get(&chat.id) else {
                continue;
            };
            if !is_one_on_one(chat) || !others(chat, me.as_ref()).is_empty() {
                continue;
            }
            let mut with_bots = apps.iter().filter(|app| !app.bot_ids.is_empty());
            if let (Some(app), None) = (with_bots.next(), with_bots.next()) {
                chat.title = app.name.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::{AppContext as _, TestAppContext};
    use store::Store;
    use teams_core::{CardActionOutcome, TaskDialog, TaskDialogKind};

    use super::CardAnswer;
    use crate::app_state::{AppState, Mode};
    use crate::card_state::CardScope;
    use crate::task_dialog::UrlDialogNext;

    fn finish(cx: &mut TestAppContext, answer: CardAnswer) -> (UrlDialogNext, Option<String>) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        let scope = CardScope::message("19:chat@thread.v2", "m1", 0);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                let next = state.finish_url_dialog_submit(&scope, answer, cx);
                (next, state.cards.note(&scope.card_key()).map(str::to_owned))
            })
        })
    }

    fn dialog(kind: TaskDialogKind) -> CardActionOutcome {
        CardActionOutcome::Dialog(TaskDialog {
            title: None,
            width: 640,
            height: 300,
            kind,
        })
    }

    #[gpui_kit::test]
    fn a_sent_submit_closes_the_url_dialog(cx: &mut TestAppContext) {
        let (next, note) = finish(cx, Ok((CardActionOutcome::Sent, None)));
        assert_eq!(next, UrlDialogNext::Close);
        assert_eq!(note, None);
    }

    #[gpui_kit::test]
    fn a_message_answer_is_shown_on_the_card_and_closes_the_dialog(cx: &mut TestAppContext) {
        let (next, note) = finish(cx, Ok((CardActionOutcome::Message("Saved".into()), None)));
        assert_eq!(next, UrlDialogNext::Close);
        assert_eq!(note.as_deref(), Some("Saved"));
    }

    #[gpui_kit::test]
    fn a_failed_submit_is_shown_on_the_card(cx: &mut TestAppContext) {
        let (next, note) = finish(cx, Err("Not connected".into()));
        assert_eq!(next, UrlDialogNext::Close);
        assert_eq!(note.as_deref(), Some("Not connected"));
    }

    #[gpui_kit::test]
    fn a_continued_url_task_navigates_the_open_dialog(cx: &mut TestAppContext) {
        let kind = TaskDialogKind::Url {
            url: "https://bot.example/next".into(),
            fallback_url: "https://bot.example/next".into(),
        };
        let (next, _) = finish(cx, Ok((dialog(kind), None)));
        assert_eq!(
            next,
            UrlDialogNext::Navigate {
                url: "https://bot.example/next".into(),
                width: 640,
                height: 300
            }
        );
    }
}
