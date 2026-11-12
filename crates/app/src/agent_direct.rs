use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use chrono::Utc;
use gpui_kit::*;
use mcp::{Outcome, ReactionOutcome, ReactionProposal};
use store::{MessageRecord, OutboxTarget};

use crate::agent_api::{self, ProposalEntry};
use crate::app_state::AppState;
use crate::notice::short_error;
use crate::outbox::{deliver, new_outbox_id, outbox_record};
use crate::rows::{api_reaction, reaction_glyph, reply_excerpt};
use crate::runtime;
use crate::views::composer::ReplyPreview;
use crate::views::proposal_card::{needs_upload, outgoing_for};

pub fn install(state: Entity<AppState>, cx: &mut App) {
    let Some(api) = agent_api::handle(cx) else {
        return;
    };
    let reacting: Rc<RefCell<HashSet<String>>> = Rc::default();
    cx.observe(&api, move |_, cx| {
        for entry in agent_api::direct_messages(cx) {
            send_message(&state, entry, cx);
        }
        for reaction in agent_api::direct_reactions(cx) {
            if reacting.borrow_mut().insert(reaction.id.clone()) {
                add_reaction(&state, reaction, reacting.clone(), cx);
            }
        }
    })
    .detach();
}

fn live_engine(state: &Entity<AppState>, cx: &App) -> Option<std::sync::Arc<crate::backend::Engine>> {
    let state = state.read(cx);
    state
        .engine
        .clone()
        .filter(|_| !state.mode.read_only && !state.mode.demo)
}

fn message(state: &Entity<AppState>, conversation_id: &str, message_id: &str, cx: &App) -> Option<MessageRecord> {
    state
        .read(cx)
        .store
        .messages_by_id(conversation_id, &[message_id.to_owned()])
        .ok()
        .and_then(|mut found| found.remove(message_id))
        .filter(|record| !record.deleted)
}

#[derive(Debug, PartialEq)]
struct Route {
    target: OutboxTarget,
    thread_root_id: Option<String>,
    quoted: Option<MessageRecord>,
}

fn route(in_channel: bool, target_message: Option<MessageRecord>) -> Route {
    match (in_channel, target_message) {
        (true, Some(record)) => Route {
            target: OutboxTarget::Thread,
            thread_root_id: Some(record.reply_to_id.clone().unwrap_or_else(|| record.message_id.clone())),
            quoted: record.reply_to_id.is_some().then_some(record),
        },
        (true, None) => Route { target: OutboxTarget::Post, thread_root_id: None, quoted: None },
        (false, quoted) => Route { target: OutboxTarget::Flat, thread_root_id: None, quoted },
    }
}

fn send_message(state: &Entity<AppState>, entry: ProposalEntry, cx: &mut App) {
    let conversation_id = entry.proposal.conversation_id.clone();
    let Some(engine) = live_engine(state, cx).filter(|_| !needs_upload(&entry)) else {
        agent_api::hold_for_review(&conversation_id, cx);
        return;
    };
    let in_channel = state.read(cx).store.channel(&conversation_id).ok().flatten().is_some();
    let target_message = entry
        .proposal
        .reply_to_message_id
        .as_deref()
        .and_then(|message_id| message(state, &conversation_id, message_id, cx));
    let Route { target, thread_root_id, quoted } = route(in_channel, target_message);
    let quote = quoted.map(|record| ReplyPreview {
        message_id: record.message_id.clone(),
        author: record.sender_name.clone().unwrap_or_else(|| "Unknown".to_owned()),
        excerpt: reply_excerpt(&record),
    });
    let outgoing = outgoing_for(&entry, quote);
    let outbox_id = new_outbox_id();
    let store = state.read(cx).store.clone();
    if let Some(record) = outbox_record(&outbox_id, &conversation_id, target, thread_root_id.as_deref(), &outgoing, Utc::now()) {
        store.put_outbox(&record).ok();
    }
    agent_api::resolve_proposal(&conversation_id, Outcome::Sent, cx);
    state.update(cx, |state, cx| state.outbox_changed(conversation_id.clone(), cx));
    let receiver = runtime::spawn({
        let conversation_id = conversation_id.clone();
        async move { deliver(&engine, &conversation_id, target, thread_root_id.as_deref(), &outgoing).await }
    });
    let state = state.downgrade();
    cx.spawn(async move |cx| {
        let failure = match receiver.await {
            Ok(Ok(_)) => None,
            Ok(Err(error)) => Some(short_error(&error)),
            Err(error) => Some(short_error(&error)),
        };
        match failure {
            None => store.delete_outbox(&outbox_id).ok(),
            Some(error) => store.mark_outbox_failed(&outbox_id, &error).ok(),
        };
        state.update(cx, |state, cx| state.outbox_changed(conversation_id, cx)).ok();
    })
    .detach();
}

fn add_reaction(
    state: &Entity<AppState>,
    reaction: ReactionProposal,
    reacting: Rc<RefCell<HashSet<String>>>,
    cx: &mut App,
) {
    let (conversation_id, message_id) = (reaction.conversation_id.clone(), reaction.message_id.clone());
    let Some(engine) = live_engine(state, cx) else {
        agent_api::hold_reaction_for_review(&conversation_id, &message_id, cx);
        reacting.borrow_mut().remove(&reaction.id);
        return;
    };
    if message(state, &conversation_id, &message_id, cx).is_none() {
        agent_api::resolve_reaction_proposal(&conversation_id, &message_id, &reaction.id, ReactionOutcome::Stale, cx);
        reacting.borrow_mut().remove(&reaction.id);
        return;
    }
    let reaction_type = api_reaction(&reaction_glyph(&reaction.emoji));
    let receiver = runtime::spawn({
        let (conversation_id, message_id) = (conversation_id.clone(), message_id.clone());
        async move { engine.set_reaction(&conversation_id, &message_id, &reaction_type).await }
    });
    cx.spawn(async move |cx| {
        let added = matches!(receiver.await, Ok(Ok(())));
        cx.update(|cx| {
            if added {
                agent_api::resolve_reaction_proposal(&conversation_id, &message_id, &reaction.id, ReactionOutcome::Accepted, cx);
            } else {
                agent_api::hold_reaction_for_review(&conversation_id, &message_id, cx);
            }
            reacting.borrow_mut().remove(&reaction.id);
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use store::{MessageRecord, OutboxTarget};

    use super::{Route, route};

    fn record(message_id: &str, reply_to_id: Option<&str>) -> MessageRecord {
        MessageRecord {
            message_id: message_id.to_owned(),
            reply_to_id: reply_to_id.map(str::to_owned),
            ..MessageRecord::default()
        }
    }

    #[test]
    fn chats_quote_and_channels_answer_in_the_thread() {
        let chat = route(false, Some(record("m1", None)));
        assert_eq!((chat.target, chat.quoted.map(|quoted| quoted.message_id)), (OutboxTarget::Flat, Some("m1".to_owned())));
        assert_eq!(route(true, None), Route { target: OutboxTarget::Post, thread_root_id: None, quoted: None });
        let to_post = route(true, Some(record("root", None)));
        assert_eq!((to_post.target, to_post.thread_root_id.as_deref(), to_post.quoted), (OutboxTarget::Thread, Some("root"), None));
        let to_reply = route(true, Some(record("r2", Some("root"))));
        assert_eq!(to_reply.thread_root_id.as_deref(), Some("root"));
        assert_eq!(to_reply.quoted.map(|quoted| quoted.message_id).as_deref(), Some("r2"));
    }
}
