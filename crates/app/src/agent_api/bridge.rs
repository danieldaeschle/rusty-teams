use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Local, Utc};
use gpui_kit::*;
use mcp::{AgentHub, UiEvent};
use teams_core::CoreEvent;
use tokio::sync::mpsc::UnboundedReceiver;

use super::source::AppSource;
use super::{AgentApi, ProposalEntry, ProposalStatus};
use crate::backend::Engine;
use crate::runtime;

const PREVIEW_CHARACTERS: usize = 200;
const NEW_MESSAGE_BATCH: usize = 50;

pub fn drain_ui_events(
    mut receiver: UnboundedReceiver<UiEvent>,
    api: WeakEntity<AgentApi>,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        while let Some(event) = receiver.recv().await {
            if api
                .update(cx, |api, cx| api.apply_ui_event(event, cx))
                .is_err()
            {
                return;
            }
        }
    })
    .detach();
}

pub fn apply(proposals: &mut HashMap<String, ProposalEntry>, event: UiEvent) {
    match event {
        UiEvent::ProposalChanged(proposal) => {
            proposals.insert(
                proposal.conversation_id.clone(),
                ProposalEntry {
                    proposal,
                    status: ProposalStatus::Pending,
                },
            );
        }
        UiEvent::ProposalStale(proposal) => {
            proposals.insert(
                proposal.conversation_id.clone(),
                ProposalEntry {
                    proposal,
                    status: ProposalStatus::Stale {
                        answered_at: Local::now(),
                    },
                },
            );
        }
        UiEvent::ProposalRemoved { conversation_id } => {
            proposals.remove(&conversation_id);
        }
    }
}

pub fn spawn_message_bridge(engine: Arc<Engine>, hub: Arc<AgentHub>, source: Arc<AppSource>) {
    let started_at = Utc::now();
    drop(runtime::spawn(async move {
        let mut receiver = engine.subscribe();
        let mut watermarks: HashMap<String, DateTime<Utc>> = HashMap::new();
        loop {
            let conversation_id = match receiver.recv().await {
                Ok(CoreEvent::MessagesChanged { conversation_id }) => conversation_id,
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            };
            let Ok(store) = mcp::TeamsSource::store(source.as_ref()) else {
                continue;
            };
            let watermark = watermarks
                .entry(conversation_id.clone())
                .or_insert(started_at);
            let Ok(messages) =
                store.messages_after(&conversation_id, *watermark, NEW_MESSAGE_BATCH)
            else {
                continue;
            };
            let my_id = mcp::TeamsSource::me_id(source.as_ref());
            for message in messages {
                *watermark = (*watermark).max(message.created_at);
                if message.deleted {
                    continue;
                }
                let sender_is_me = my_id.is_some() && message.sender_id == my_id;
                let preview: String = store::plain_text(&message.body_html)
                    .chars()
                    .take(PREVIEW_CHARACTERS)
                    .collect();
                hub.message_arrived(
                    &conversation_id,
                    &message.message_id,
                    sender_is_me,
                    message.sender_name,
                    &preview,
                );
            }
        }
    }));
}
