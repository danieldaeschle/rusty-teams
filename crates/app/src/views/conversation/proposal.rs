use std::collections::HashSet;
use std::rc::Rc;

use chrono::Utc;
use gpui_kit::*;
use mcp::{Outcome, ReactionOutcome};

use super::{ConversationView, ReactionAttempt};
use crate::agent_api::{self, ProposalEntry};
use crate::rows::reaction_glyph;
use crate::views::composer::ComposerEvent;
use crate::views::proposal_card::{
    self, CardAction, CardActions, needs_upload, outgoing_for, picture_files,
};

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
        if needs_upload(&entry) {
            self.show_notice(
                "Large images upload as files - check the draft and press Send",
                cx,
            );
            self.edit_proposal(window, cx);
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
        let outgoing = outgoing_for(&entry, quote);
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
        let mentions = outgoing_for(&entry, None).mentions;
        let pictures = picture_files(&entry);
        composer.update(cx, |composer, cx| {
            composer.load_proposal(&entry.proposal.text, &mentions, pictures, window, cx);
            composer.focus(window, cx);
        });
        agent_api::resolve_proposal(&conversation_id, Outcome::Edited, cx);
    }

    fn discard_proposal(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(conversation_id) = self.conversation_id() {
            agent_api::resolve_proposal(&conversation_id, Outcome::Discarded, cx);
        }
    }

    pub(super) fn accept_reaction(
        &mut self,
        message_id: &str,
        proposal_id: &str,
        emoji: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(conversation_id) = self.conversation_id() else {
            return;
        };
        let in_flight = self
            .pending_reactions
            .get(message_id)
            .is_some_and(|entries| {
                entries
                    .iter()
                    .any(|(glyph, added)| *added && *glyph == reaction_glyph(emoji))
            });
        if in_flight
            || !self.reaction_proposal_is_current(&conversation_id, message_id, proposal_id, cx)
        {
            return;
        }
        let outcome =
            match self.change_reaction(message_id, emoji, true, Some(proposal_id.to_owned()), cx) {
                ReactionAttempt::Started => return,
                ReactionAttempt::Acted => ReactionOutcome::Accepted,
                ReactionAttempt::AlreadyThere | ReactionAttempt::MissingMessage => {
                    ReactionOutcome::Stale
                }
                ReactionAttempt::Blocked => return,
            };
        agent_api::resolve_reaction_proposal(
            &conversation_id,
            message_id,
            proposal_id,
            outcome,
            cx,
        );
    }

    fn reaction_proposal_is_current(
        &self,
        conversation_id: &str,
        message_id: &str,
        proposal_id: &str,
        cx: &App,
    ) -> bool {
        agent_api::reaction_proposals(conversation_id, cx)
            .get(message_id)
            .is_some_and(|pending| pending.id == proposal_id)
    }

    pub(super) fn discard_reaction(
        &mut self,
        message_id: &str,
        proposal_id: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(conversation_id) = self.conversation_id() {
            agent_api::resolve_reaction_proposal(
                &conversation_id,
                message_id,
                proposal_id,
                ReactionOutcome::Discarded,
                cx,
            );
        }
    }

    pub(super) fn refresh_reaction_pills(&mut self, cx: &App) -> HashSet<String> {
        let Some(conversation_id) = self.conversation_id() else {
            return HashSet::new();
        };
        let latest = agent_api::reaction_proposals(&conversation_id, cx);
        let mut shown = self.reaction_proposals.borrow_mut();
        let changed = latest
            .iter()
            .filter(|(message_id, proposal)| shown.get(*message_id) != Some(*proposal))
            .map(|(message_id, _)| message_id.clone())
            .chain(
                shown
                    .keys()
                    .filter(|message_id| !latest.contains_key(*message_id))
                    .cloned(),
            )
            .collect();
        *shown = latest;
        changed
    }

    pub(super) fn sync_reaction_pills(&mut self, cx: &mut Context<Self>) {
        let changed = self.refresh_reaction_pills(cx);
        if changed.is_empty() {
            return;
        }
        self.remeasure_rows_of(&changed, cx);
        cx.notify();
    }
}
