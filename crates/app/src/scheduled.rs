use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use gpui_kit::Context;
use teams_core::ScheduledDraft;

use crate::app_state::{AppEvent, AppState};
use crate::runtime;

const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const DELIVERY_SETTLE: Duration = Duration::from_secs(3);
const THREAD_SUFFIX: &str = ";messageid=";

pub fn conversation_of(inner_thread_id: &str) -> &str {
    inner_thread_id
        .split_once(THREAD_SUFFIX)
        .map_or(inner_thread_id, |(conversation_id, _)| conversation_id)
}

pub fn thread_root_of(inner_thread_id: &str) -> Option<&str> {
    inner_thread_id
        .split_once(THREAD_SUFFIX)
        .map(|(_, root_id)| root_id)
}

pub fn keep_unconfirmed(
    mut incoming: Vec<ScheduledDraft>,
    known: &[ScheduledDraft],
) -> Vec<ScheduledDraft> {
    let unconfirmed: Vec<ScheduledDraft> = known
        .iter()
        .filter(|draft| {
            draft.id.is_empty()
                && !incoming
                    .iter()
                    .any(|found| found.client_message_id == draft.client_message_id)
        })
        .cloned()
        .collect();
    incoming.extend(unconfirmed);
    incoming
}

fn is_due(draft: &ScheduledDraft, now: DateTime<Utc>) -> bool {
    draft.delivery_state.is_none() && draft.send_at <= now
}

pub fn pending_drafts(drafts: Vec<ScheduledDraft>, now: DateTime<Utc>) -> Vec<ScheduledDraft> {
    drafts
        .into_iter()
        .filter(|draft| !is_due(draft, now))
        .collect()
}

pub fn due_conversations(drafts: &[ScheduledDraft], now: DateTime<Utc>) -> Vec<String> {
    let mut seen = HashSet::new();
    drafts
        .iter()
        .filter(|draft| is_due(draft, now))
        .map(|draft| conversation_of(&draft.inner_thread_id))
        .filter(|conversation_id| seen.insert(*conversation_id))
        .map(str::to_owned)
        .collect()
}

impl AppState {
    pub fn refresh_scheduled(&mut self, cx: &mut Context<Self>) {
        if self.mode.demo {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let receiver = runtime::spawn(async move { engine.scheduled_messages().await });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(drafts)) = receiver.await {
                this.update(cx, |state, cx| state.apply_scheduled(drafts, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub fn apply_scheduled(&mut self, drafts: Vec<ScheduledDraft>, cx: &mut Context<Self>) {
        let now = Utc::now();
        let delivered = due_conversations(&self.scheduled, now);
        self.scheduled = pending_drafts(keep_unconfirmed(drafts, &self.scheduled), now);
        self.fetch_delivered(delivered);
        self.poll_scheduled(cx);
        cx.emit(AppEvent::Scheduled);
        cx.notify();
    }

    pub fn scheduled_upserted(&mut self, draft: ScheduledDraft, cx: &mut Context<Self>) {
        let mut drafts = std::mem::take(&mut self.scheduled);
        let unconfirmed = draft.id.is_empty();
        let same = |known: &ScheduledDraft| {
            if draft.id.is_empty() || known.id.is_empty() {
                known.client_message_id == draft.client_message_id
            } else {
                known.id == draft.id
            }
        };
        match drafts.iter_mut().find(|known| same(known)) {
            Some(known) => *known = draft,
            None => drafts.push(draft),
        }
        self.apply_scheduled(drafts, cx);
        if unconfirmed {
            self.refresh_scheduled(cx);
        }
    }

    pub fn scheduled_removed(&mut self, draft_id: &str, cx: &mut Context<Self>) {
        let mut drafts = std::mem::take(&mut self.scheduled);
        drafts.retain(|draft| draft.id != draft_id);
        self.apply_scheduled(drafts, cx);
    }

    fn fetch_delivered(&self, conversation_ids: Vec<String>) {
        let Some(engine) = self.engine.clone().filter(|_| !self.mode.demo) else {
            return;
        };
        for conversation_id in conversation_ids {
            let engine = engine.clone();
            runtime::handle().spawn(async move {
                tokio::time::sleep(DELIVERY_SETTLE).await;
                let _ = engine.fetch_newer(&conversation_id).await;
            });
        }
    }

    fn poll_scheduled(&mut self, cx: &mut Context<Self>) {
        if self.scheduled.is_empty() || self.scheduled_polling {
            return;
        }
        self.scheduled_polling = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH_INTERVAL).await;
                let keep_going = this.update(cx, |state, cx| state.tick_scheduled(cx));
                if !keep_going.unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
    }

    fn tick_scheduled(&mut self, cx: &mut Context<Self>) -> bool {
        let keep_going = !self.scheduled.is_empty();
        self.scheduled_polling = keep_going;
        let current = self.scheduled.clone();
        self.apply_scheduled(current, cx);
        self.refresh_scheduled(cx);
        keep_going
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 9, 10, 0, 0).unwrap()
    }

    fn draft(id: &str, thread: &str, minutes_ahead: i64, failed: bool) -> ScheduledDraft {
        ScheduledDraft {
            id: id.to_owned(),
            client_message_id: id.to_owned(),
            inner_thread_id: thread.to_owned(),
            send_at: now() + Duration::minutes(minutes_ahead),
            html: String::new(),
            delivery_state: failed.then(|| "failed".to_owned()),
        }
    }

    #[test]
    fn a_thread_draft_belongs_to_its_channel() {
        assert_eq!(conversation_of("19:chat@thread.v2"), "19:chat@thread.v2");
        assert_eq!(
            conversation_of("19:channel@thread.tacv2;messageid=1791"),
            "19:channel@thread.tacv2"
        );
    }

    #[test]
    fn a_draft_without_an_id_stays_until_the_server_list_shows_it() {
        let mut stub = draft("", "chat", 5, false);
        stub.client_message_id = "client-1".to_owned();
        let known = [stub.clone()];
        let other = draft("other", "chat", 9, false);
        assert_eq!(
            keep_unconfirmed(vec![other.clone()], &known),
            [other.clone(), stub]
        );
        let mut confirmed = draft("d1", "chat", 5, false);
        confirmed.client_message_id = "client-1".to_owned();
        assert_eq!(
            keep_unconfirmed(vec![confirmed.clone()], &known),
            [confirmed]
        );
    }

    #[test]
    fn a_thread_draft_names_its_root() {
        assert_eq!(
            thread_root_of("19:c@thread.tacv2;messageid=1791"),
            Some("1791")
        );
        assert_eq!(thread_root_of("19:chat@thread.v2"), None);
    }

    #[test]
    fn due_drafts_leave_the_list_and_failed_ones_stay() {
        let drafts = vec![
            draft("future", "a", 5, false),
            draft("due", "a", -1, false),
            draft("failed", "a", -1, true),
        ];
        let kept = pending_drafts(drafts, now());
        let ids: Vec<_> = kept.iter().map(|draft| draft.id.as_str()).collect();
        assert_eq!(ids, ["future", "failed"]);
    }

    #[test]
    fn each_conversation_with_a_due_draft_is_fetched_once() {
        let drafts = [
            draft("a", "chat", -1, false),
            draft("b", "chat", -2, false),
            draft("c", "chan;messageid=m1", 0, false),
            draft("d", "other", 9, false),
            draft("e", "stuck", -1, true),
        ];
        assert_eq!(due_conversations(&drafts, now()), ["chat", "chan"]);
    }
}
