use chrono::{DateTime, Local, Utc};
use gpui_kit::*;
use store::MessageRecord;

use crate::app_state::AppState;
use crate::notice::short_error;
use crate::rows::message_text;
use crate::runtime;

pub const SHARE_TO_OUTLOOK_LABEL: &str = "Share to Outlook";

pub fn share_subject(record: &MessageRecord, conversation_title: &str) -> String {
    record
        .subject
        .as_deref()
        .map(str::trim)
        .filter(|subject| !subject.is_empty())
        .unwrap_or(conversation_title)
        .to_owned()
}

pub fn share_body(record: &MessageRecord, link: Option<&str>) -> String {
    let sender = record.sender_name.as_deref().unwrap_or_default();
    let header = format!("{sender} {}", sent_label(record.created_at))
        .trim()
        .to_owned();
    let mut parts = vec![header, message_text(record)];
    parts.extend(link.map(str::to_owned));
    parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn sent_label(sent_at: DateTime<Utc>) -> String {
    sent_at
        .with_timezone(&Local)
        .format("%d/%m/%Y %H:%M")
        .to_string()
}

impl AppState {
    pub fn share_to_outlook(
        &mut self,
        conversation_id: &str,
        message_id: &str,
        conversation_title: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self.message_record(conversation_id, message_id) else {
            return;
        };
        let subject = share_subject(&record, conversation_title);
        let Some(engine) = self.engine.clone() else {
            if self.mode.demo {
                let link = teams_core::chat_message_link(conversation_id, message_id, None);
                let body = share_body(&record, Some(&link));
                cx.open_url(&teams_core::outlook_compose_link(&subject, &body));
            }
            return;
        };
        let (conversation_id, message_id) = (conversation_id.to_owned(), message_id.to_owned());
        let receiver =
            runtime::spawn(async move { engine.message_link(&conversation_id, &message_id).await });
        cx.spawn(async move |this, cx| {
            let link = match receiver.await {
                Ok(Ok(link)) => Ok(link),
                Ok(Err(error)) => Err(short_error(&error)),
                Err(_) => Err("cancelled".to_owned()),
            };
            this.update(cx, |state, cx| {
                if let Err(reason) = &link {
                    state.raise_notice(format!("No link to the message: {reason}"), None, cx);
                }
                let body = share_body(&record, link.as_deref().ok());
                cx.open_url(&teams_core::outlook_compose_link(&subject, &body));
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use store::MessageRecord;

    use super::{share_body, share_subject};

    fn record(subject: Option<&str>, html: &str) -> MessageRecord {
        MessageRecord {
            conversation_id: "19:c@thread.v2".into(),
            message_id: "1".into(),
            reply_to_id: None,
            sender_id: Some("u".into()),
            sender_name: Some("Ada Example".into()),
            sender_application_id: None,
            links_json: "[]".to_owned(),
            created_at: DateTime::from_timestamp(1_790_000_000, 0).unwrap(),
            edited_at: None,
            deleted: false,
            body_html: html.into(),
            attachments_json: "[]".into(),
            reactions_json: "[]".into(),
            mentions_json: "[]".into(),
            subject: subject.map(str::to_owned),
            call_event_json: None,
            importance: Default::default(),
        }
    }

    #[test]
    fn the_subject_is_the_post_subject_or_the_conversation_title() {
        assert_eq!(
            share_subject(&record(Some("Launch"), "<p>x</p>"), "General"),
            "Launch"
        );
        assert_eq!(
            share_subject(&record(Some("  "), "<p>x</p>"), "General"),
            "General"
        );
        assert_eq!(
            share_subject(&record(None, "<p>x</p>"), "Release planning"),
            "Release planning"
        );
    }

    #[test]
    fn the_body_has_sender_and_time_then_the_text_then_the_link() {
        let body = share_body(
            &record(None, "<p>Ship it <b>today</b></p>"),
            Some("https://teams.microsoft.com/l/message/1"),
        );
        let parts: Vec<&str> = body.split("\n\n").collect();
        assert_eq!(parts.len(), 3);
        assert!(parts[0].starts_with("Ada Example "));
        assert!(parts[1].contains("Ship it"));
        assert_eq!(parts[2], "https://teams.microsoft.com/l/message/1");
        assert_eq!(
            share_body(&record(None, "<p>hi</p>"), None)
                .split("\n\n")
                .count(),
            2
        );
    }
}
