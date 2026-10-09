use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use gpui_kit::Context;
use store::{MessageRecord, OutboxRecord, OutboxState, OutboxTarget, plain_text};
use uuid::Uuid;

use crate::app_state::{AppEvent, AppState};
use crate::backend::Engine;
use crate::notice::short_error;
use crate::runtime;
use crate::stored_outgoing::{decode, encode};
use crate::views::composer::Outgoing;

const RETRY_BACKOFF: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(8),
    Duration::from_secs(30),
];
const DUPLICATE_WINDOW_SECONDS: i64 = 5;
const RECENT_MESSAGES: usize = 30;

pub fn new_outbox_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn outbox_record(
    id: &str,
    conversation_id: &str,
    target: OutboxTarget,
    thread_root_id: Option<&str>,
    outgoing: &Outgoing,
    created_at: DateTime<Utc>,
) -> Option<OutboxRecord> {
    let stored = encode(outgoing)?;
    Some(OutboxRecord {
        id: id.to_owned(),
        conversation_id: conversation_id.to_owned(),
        target,
        thread_root_id: thread_root_id.map(str::to_owned),
        payload: stored.payload,
        images: stored.images,
        state: OutboxState::Sending,
        last_error: None,
        created_at,
    })
}

pub fn outgoing_of(record: &OutboxRecord) -> Option<Outgoing> {
    decode(&record.payload, &record.images)
}

pub async fn deliver(
    engine: &Engine,
    conversation_id: &str,
    target: OutboxTarget,
    thread_root_id: Option<&str>,
    outgoing: &Outgoing,
) -> teams_core::Result<()> {
    let extras = outgoing.extras();
    let html = outgoing.html();
    let mentions = &outgoing.mentions;
    let record = match (target, &outgoing.reply) {
        (_, Some(reply)) => {
            engine
                .reply_to_with_extras(conversation_id, &reply.message_id, &html, mentions, &extras)
                .await
        }
        (OutboxTarget::Flat, None) => {
            engine
                .send_message_with_extras(conversation_id, &html, None, mentions, &extras)
                .await
        }
        (OutboxTarget::Post, None) => {
            engine
                .post_to_channel_with_extras(
                    conversation_id,
                    &html,
                    outgoing.subject.as_deref(),
                    mentions,
                    &extras,
                )
                .await
        }
        (OutboxTarget::Thread, None) => {
            engine
                .send_message_with_extras(conversation_id, &html, thread_root_id, mentions, &extras)
                .await
        }
    }?;
    if let Some(preview) = &outgoing.link_preview {
        let _ = engine
            .attach_link_preview(conversation_id, &record.message_id, preview)
            .await;
    }
    Ok(())
}

fn is_permanent(error: &teams_core::Error) -> bool {
    match error {
        teams_core::Error::Graph(graph::Error::Session(session::Error::Api { status, .. })) => {
            (400..500).contains(status) && !matches!(status, 408 | 429)
        }
        teams_core::Error::Graph(graph::Error::NotAMember)
        | teams_core::Error::Unsupported(_)
        | teams_core::Error::UnknownConversation(_) => true,
        _ => false,
    }
}

fn comparable(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn is_duplicate(
    recent: &[MessageRecord],
    my_user_id: &str,
    created_at: DateTime<Utc>,
    text: &str,
) -> bool {
    let wanted = comparable(text);
    let since = created_at - chrono::Duration::seconds(DUPLICATE_WINDOW_SECONDS);
    !wanted.is_empty()
        && recent.iter().any(|message| {
            !message.deleted
                && message.sender_id.as_deref() == Some(my_user_id)
                && message.created_at >= since
                && comparable(&plain_text(&message.body_html)) == wanted
        })
}

async fn already_sent(
    engine: &Engine,
    record: &OutboxRecord,
    outgoing: &Outgoing,
) -> teams_core::Result<bool> {
    engine.fetch_newer(&record.conversation_id).await?;
    let Some(me) = engine.me() else {
        return Ok(false);
    };
    let recent = engine
        .store()
        .messages(&record.conversation_id, None, RECENT_MESSAGES)?;
    Ok(is_duplicate(
        &recent,
        &me.user_id,
        record.created_at,
        &outgoing.text(),
    ))
}

async fn attempt(
    engine: &Engine,
    record: &OutboxRecord,
    outgoing: &Outgoing,
) -> teams_core::Result<()> {
    // Graph has no idempotency key: a send cut off by the quit may have gone through.
    if already_sent(engine, record, outgoing).await? {
        return Ok(());
    }
    deliver(
        engine,
        &record.conversation_id,
        record.target,
        record.thread_root_id.as_deref(),
        outgoing,
    )
    .await
}

async fn resend(engine: &Engine, record: &OutboxRecord, outgoing: &Outgoing) -> Result<(), String> {
    let mut failure = String::new();
    for attempt_number in 0..=RETRY_BACKOFF.len() {
        if let Some(delay) = attempt_number
            .checked_sub(1)
            .map(|index| RETRY_BACKOFF[index])
        {
            tokio::time::sleep(delay).await;
        }
        match attempt(engine, record, outgoing).await {
            Ok(()) => return Ok(()),
            Err(error) if is_permanent(&error) => return Err(short_error(&error)),
            Err(error) => failure = short_error(&error),
        }
    }
    Err(failure)
}

async fn resend_record(engine: &Engine, record: OutboxRecord) {
    let store = engine.store();
    let outcome = match outgoing_of(&record) {
        Some(outgoing) => resend(engine, &record, &outgoing).await,
        None => Err("The saved message could not be read".to_owned()),
    };
    match outcome {
        Ok(()) => store.delete_outbox(&record.id).ok(),
        Err(error) => store.mark_outbox_failed(&record.id, &error).ok(),
    };
}

impl AppState {
    pub fn resend_outbox(&mut self, cx: &mut Context<Self>) {
        if self.mode.demo || self.mode.read_only {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let Ok(records) = self.store.sending_outbox() else {
            return;
        };
        let mut by_conversation: Vec<(String, Vec<OutboxRecord>)> = Vec::new();
        let mut positions: HashMap<String, usize> = HashMap::new();
        for record in records {
            let position = *positions
                .entry(record.conversation_id.clone())
                .or_insert_with(|| {
                    by_conversation.push((record.conversation_id.clone(), Vec::new()));
                    by_conversation.len() - 1
                });
            by_conversation[position].1.push(record);
        }
        let (finished, mut finished_conversations) = tokio::sync::mpsc::unbounded_channel();
        for (conversation_id, records) in by_conversation {
            let (engine, finished) = (engine.clone(), finished.clone());
            runtime::handle().spawn(async move {
                for record in records {
                    resend_record(&engine, record).await;
                    let _ = finished.send(conversation_id.clone());
                }
            });
        }
        drop(finished);
        cx.spawn(async move |this, cx| {
            while let Some(conversation_id) = finished_conversations.recv().await {
                this.update(cx, |state, cx| state.outbox_changed(conversation_id, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub fn outbox_changed(&mut self, conversation_id: String, cx: &mut Context<Self>) {
        self.refresh_local_previews(cx);
        cx.emit(AppEvent::Outbox(conversation_id));
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn message(sender: &str, second: u32, html: &str) -> MessageRecord {
        MessageRecord {
            sender_id: Some(sender.to_owned()),
            created_at: Utc.with_ymd_and_hms(2026, 10, 6, 9, 0, second).unwrap(),
            body_html: html.to_owned(),
            ..MessageRecord::default()
        }
    }

    fn sent_at(second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 6, 9, 0, second).unwrap()
    }

    #[test]
    fn an_own_message_with_the_same_text_after_the_send_is_a_duplicate() {
        let recent = [message("me", 31, "<p>Hello <at>Ada</at>!</p>")];
        assert!(is_duplicate(&recent, "me", sent_at(30), "Hello @Ada!"));
    }

    #[test]
    fn a_message_up_to_five_seconds_before_the_send_still_counts() {
        let recent = [message("me", 25, "<p>Hi</p>")];
        assert!(is_duplicate(&recent, "me", sent_at(30), "Hi"));
        let recent = [message("me", 24, "<p>Hi</p>")];
        assert!(!is_duplicate(&recent, "me", sent_at(30), "Hi"));
    }

    #[test]
    fn other_senders_other_text_and_empty_text_are_no_duplicates() {
        let recent = [
            message("other", 31, "<p>Hi</p>"),
            message("me", 32, "<p>Bye</p>"),
        ];
        assert!(!is_duplicate(&recent, "me", sent_at(30), "Hi"));
        let image_only = [message("me", 31, "<img src=\"x\">")];
        assert!(!is_duplicate(&image_only, "me", sent_at(30), ""));
    }

    #[test]
    fn deleted_messages_are_no_duplicates() {
        let mut deleted = message("me", 31, "<p>Hi</p>");
        deleted.deleted = true;
        assert!(!is_duplicate(&[deleted], "me", sent_at(30), "Hi"));
    }

    #[test]
    fn client_errors_fail_at_once_and_server_errors_retry() {
        let api = |status| {
            teams_core::Error::Graph(graph::Error::Session(session::Error::api(
                status,
                "https://graph.test",
                serde_json::json!({}),
            )))
        };
        assert!(is_permanent(&api(403)));
        assert!(is_permanent(&api(400)));
        assert!(!is_permanent(&api(429)));
        assert!(!is_permanent(&api(503)));
        assert!(!is_permanent(&teams_core::Error::Graph(
            graph::Error::Download("x".into())
        )));
    }
}
