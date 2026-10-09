use chrono::{DateTime, Local, Utc};
use teams_core::{Draft, ScheduledDraft, html_to_spans};

use crate::pending_rows::pending_row;
use crate::rows::{Delivery, MessageRow, ScheduledState};
use crate::schedule_time::describe;
use crate::views::composer::Outgoing;

pub const SCHEDULED_KEY_PREFIX: &str = "scheduled-";

pub fn scheduled_key(draft_id: &str) -> String {
    format!("{SCHEDULED_KEY_PREFIX}{draft_id}")
}

pub fn draft_key(draft: &ScheduledDraft) -> &str {
    if draft.id.is_empty() {
        &draft.client_message_id
    } else {
        &draft.id
    }
}

pub fn draft_id_of(key: &str) -> Option<&str> {
    key.strip_prefix(SCHEDULED_KEY_PREFIX)
}

pub fn scheduled_outgoing(html: &str) -> Outgoing {
    Outgoing {
        draft: Draft::from_spans(&html_to_spans(html)).trimmed(),
        mentions: Vec::new(),
        reply: None,
        edit: None,
        images: Vec::new(),
        files: Vec::new(),
        link_preview: None,
    }
}

pub fn is_visible(draft: &ScheduledDraft, now: DateTime<Utc>) -> bool {
    draft.delivery_state.is_some() || draft.send_at > now
}

pub fn scheduled_label(send_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    describe(send_at.with_timezone(&Local), now.with_timezone(&Local))
}

pub fn scheduled_rows(
    drafts: &[ScheduledDraft],
    inner_thread_id: &str,
    conversation_id: &str,
    my_user_id: Option<&str>,
    now: DateTime<Utc>,
    change_failed: Option<&str>,
) -> Vec<MessageRow> {
    let mut shown: Vec<&ScheduledDraft> = drafts
        .iter()
        .filter(|draft| draft.inner_thread_id == inner_thread_id && is_visible(draft, now))
        .collect();
    shown.sort_by_key(|draft| draft.send_at);
    shown
        .into_iter()
        .map(|draft| {
            let state = if change_failed == Some(draft_key(draft)) {
                ScheduledState::ChangeFailed
            } else if draft.delivery_state.is_some() {
                ScheduledState::DeliveryFailed
            } else {
                ScheduledState::Waiting
            };
            let mut row = pending_row(
                scheduled_key(draft_key(draft)),
                conversation_id.to_owned(),
                my_user_id.map(str::to_owned),
                draft.send_at,
                &scheduled_outgoing(&draft.html),
                Delivery::Scheduled(state),
            );
            row.time = format!("Scheduled {}", scheduled_label(draft.send_at, now));
            row
        })
        .collect()
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
            client_message_id: format!("c{id}"),
            inner_thread_id: thread.to_owned(),
            send_at: now() + Duration::minutes(minutes_ahead),
            html: format!("<p>text {id}</p>"),
            delivery_state: failed.then(|| "failed".to_owned()),
        }
    }

    fn rows(drafts: &[ScheduledDraft], thread: &str, failed: Option<&str>) -> Vec<MessageRow> {
        scheduled_rows(drafts, thread, "chat", Some("me"), now(), failed)
    }

    #[test]
    fn only_drafts_of_the_open_thread_show_sorted_by_time() {
        let drafts = [
            draft("late", "chat", 120, false),
            draft("other", "other-chat", 30, false),
            draft("soon", "chat", 30, false),
            draft("reply", "chan;messageid=m1", 30, false),
        ];
        let keys: Vec<_> = rows(&drafts, "chat", None)
            .into_iter()
            .map(|row| row.key)
            .collect();
        assert_eq!(keys, ["scheduled-soon", "scheduled-late"]);
        let replies = rows(&drafts, "chan;messageid=m1", None);
        assert_eq!(replies.len(), 1);
    }

    #[test]
    fn a_passed_time_hides_the_draft_unless_delivery_failed() {
        let drafts = [
            draft("gone", "chat", -1, false),
            draft("stuck", "chat", -1, true),
        ];
        let shown = rows(&drafts, "chat", None);
        assert_eq!(shown.len(), 1);
        assert_eq!(
            shown[0].delivery,
            Delivery::Scheduled(ScheduledState::DeliveryFailed)
        );
    }

    #[test]
    fn a_failed_change_marks_only_its_draft() {
        let drafts = [draft("a", "chat", 30, false), draft("b", "chat", 60, false)];
        let shown = rows(&drafts, "chat", Some("b"));
        assert_eq!(
            shown[0].delivery,
            Delivery::Scheduled(ScheduledState::Waiting)
        );
        assert_eq!(
            shown[1].delivery,
            Delivery::Scheduled(ScheduledState::ChangeFailed)
        );
    }

    #[test]
    fn the_row_is_an_own_bubble_labelled_with_the_send_time() {
        let shown = rows(&[draft("a", "chat", 30, false)], "chat", None);
        assert!(shown[0].own);
        assert!(shown[0].time.starts_with("Scheduled "), "{}", shown[0].time);
        assert!(!shown[0].blocks.is_empty());
        assert_eq!(draft_id_of(&shown[0].key), Some("a"));
    }

    #[test]
    fn the_html_of_a_draft_comes_back_as_an_editable_draft() {
        let outgoing = scheduled_outgoing("<p>hello <b>there</b></p>");
        assert_eq!(outgoing.text(), "hello there");
        assert!(outgoing.draft.to_html().contains("<b>there</b>"));
        assert!(outgoing.edit.is_none() && outgoing.mentions.is_empty());
    }
}
