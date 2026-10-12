use std::rc::Rc;

use chrono::Utc;
use gpui_kit::*;
use mcp::Outcome;
use teams_core::Draft;

use super::ConversationView;
use crate::agent_api::{self, ProposalEntry};
use crate::views::composer::{ComposerEvent, Outgoing};
use crate::views::proposal_card::{self, CardAction, CardActions};

const QUOTE_CHARS: usize = 80;

impl ConversationView {
    pub(super) fn render_proposal_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.draft_active {
            return None;
        }
        let conversation_id = self.conversation_id()?;
        let entry = agent_api::proposal(&conversation_id, cx)?;
        let quote = entry
            .proposal
            .reply_to_message_id
            .as_deref()
            .and_then(|message_id| self.reply_target(message_id, cx))
            .map(|(_, preview)| {
                let excerpt: String = preview.excerpt.chars().take(QUOTE_CHARS).collect();
                format!("{}: {excerpt}", preview.author)
            });
        let view = cx.entity();
        let action = |handler: fn(&mut Self, &mut Window, &mut Context<Self>)| -> CardAction {
            let view = view.clone();
            Rc::new(move |window, cx| view.update(cx, |this, cx| handler(this, window, cx)))
        };
        let actions = CardActions {
            send: action(Self::send_proposal),
            edit: action(Self::edit_proposal),
            discard: action(Self::discard_proposal),
        };
        Some(proposal_card::render(&entry, quote, actions, Utc::now(), cx).into_any_element())
    }

    pub(super) fn report_composer_activity(&self, event: &ComposerEvent, cx: &App) {
        let Some(conversation_id) = self.conversation_id().filter(|_| !self.draft_active) else {
            return;
        };
        match event {
            ComposerEvent::Typing(true) => agent_api::user_typing(&conversation_id, cx),
            ComposerEvent::Submit(_) => agent_api::user_sent(&conversation_id, cx),
            _ => {}
        }
    }

    fn pending_proposal(&self, cx: &App) -> Option<(String, ProposalEntry)> {
        let conversation_id = self.conversation_id()?;
        let entry = agent_api::proposal(&conversation_id, cx)?;
        Some((conversation_id, entry))
    }

    fn send_proposal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((conversation_id, entry)) = self.pending_proposal(cx) else {
            return;
        };
        let state = self.app.read(cx);
        let refusal = if state.mode.read_only || state.mode.demo {
            Some("read-only mode")
        } else if state.engine.is_none() {
            Some("not connected yet")
        } else {
            None
        };
        if let Some(reason) = refusal {
            agent_api::fail_proposal(&conversation_id, reason.to_owned(), cx);
            return;
        }
        let target = entry
            .proposal
            .reply_to_message_id
            .as_deref()
            .and_then(|message_id| self.reply_target(message_id, cx));
        let thread_root = target
            .as_ref()
            .filter(|_| self.in_feed())
            .map(|(record, _)| {
                record
                    .reply_to_id
                    .clone()
                    .unwrap_or_else(|| record.message_id.clone())
            });
        let quote = target
            .filter(|(record, _)| !self.in_feed() || record.reply_to_id.is_some())
            .map(|(_, preview)| preview);
        let outgoing = Outgoing {
            draft: Draft::from_markdown(entry.proposal.text.trim()),
            subject: None,
            mentions: Vec::new(),
            reply: quote,
            edit: None,
            images: Vec::new(),
            files: Vec::new(),
            link_preview: None,
        };
        match thread_root {
            Some(root_id) => self.send_in_thread(root_id, outgoing, window, cx),
            None => self.send(outgoing, window, cx),
        }
        agent_api::resolve_proposal(&conversation_id, Outcome::Sent, cx);
    }

    fn edit_proposal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((conversation_id, entry)) = self.pending_proposal(cx) else {
            return;
        };
        let reply_to = entry.proposal.reply_to_message_id.as_deref();
        if let Some(message_id) = reply_to {
            self.begin_reply(message_id, window, cx);
        } else if self.in_feed() {
            self.open_new_post(window, cx);
        }
        let composer = match self.reply_composer.clone() {
            Some(reply_composer) if self.in_feed() && reply_to.is_some() => reply_composer,
            _ => self.composer.clone(),
        };
        composer.update(cx, |composer, cx| {
            composer.set_text(&entry.proposal.text, window, cx);
            composer.focus(window, cx);
        });
        agent_api::resolve_proposal(&conversation_id, Outcome::Edited, cx);
    }

    fn discard_proposal(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(conversation_id) = self.conversation_id() {
            agent_api::resolve_proposal(&conversation_id, Outcome::Discarded, cx);
        }
    }
}
