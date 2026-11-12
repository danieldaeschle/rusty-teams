use std::collections::HashSet;

use gpui_kit::*;
use mcp::{MeetingOutcome, MeetingProposal, TeamsSource};

use super::handle;
use crate::app_state::AppHandle;
use crate::notice::NoticeAction;

pub fn unannounced<'a>(announced: &HashSet<String>, pending: &'a [MeetingProposal]) -> Vec<&'a MeetingProposal> {
    pending
        .iter()
        .filter(|proposal| !announced.contains(&proposal.id))
        .collect()
}

pub fn announce(proposal: &MeetingProposal, cx: &mut App) {
    let Some(handle) = cx.try_global::<AppHandle>() else {
        return;
    };
    let text = format!(
        "{} proposes a meeting: {}",
        super::agent_label(proposal.agent.as_deref()),
        proposal.draft.subject
    );
    let action = NoticeAction::new("Review", |state, cx| state.open_calendar(cx));
    handle
        .0
        .clone()
        .update(cx, |state, cx| state.raise_notice(text, Some(action), cx));
}

pub fn pending_meetings(cx: &App) -> Vec<MeetingProposal> {
    handle(cx)
        .map(|api| api.read(cx).hub.pending_meetings())
        .unwrap_or_default()
}

pub fn discard_meeting(proposal_id: &str, cx: &mut App) {
    let Some(api) = handle(cx) else {
        return;
    };
    api.update(cx, |api, cx| {
        api.hub.resolve_meeting(proposal_id, MeetingOutcome::Discarded, None);
        cx.notify();
    });
}

pub fn create_meeting(proposal_id: &str, cx: &mut App) {
    let Some(api) = handle(cx) else {
        return;
    };
    let (hub, source, demo) = {
        let api = api.read(cx);
        (api.hub.clone(), api.source.clone(), api.demo)
    };
    let Some(proposal) = hub.take_meeting(proposal_id) else {
        return;
    };
    api.update(cx, |_, cx| cx.notify());
    if !hub.policy().can_organize_meetings() {
        hub.resolve_meeting(&proposal.id, MeetingOutcome::Discarded, None);
        raise(format!("Organizing meetings is off - {} was not created", proposal.draft.subject), cx);
        return;
    }
    if demo {
        hub.resolve_meeting(&proposal.id, MeetingOutcome::Created, None);
        return;
    }
    let draft = proposal.draft.clone();
    let receiver = crate::runtime::spawn(async move { source.create_meeting(draft).await });
    cx.spawn(async move |cx| {
        match receiver.await {
            Ok(Ok(created)) => hub.resolve_meeting(&proposal.id, MeetingOutcome::Created, created.join_url),
            outcome => {
                let reason = match outcome {
                    Ok(Err(error)) => error.to_string(),
                    _ => "cancelled".to_owned(),
                };
                hub.resolve_meeting(&proposal.id, MeetingOutcome::Failed, None);
                let text = format!("Could not create {}: {reason}", proposal.draft.subject);
                cx.update(|cx| raise(text, cx));
            }
        }
    })
    .detach();
}

fn raise(text: String, cx: &mut App) {
    if let Some(handle) = cx.try_global::<AppHandle>() {
        handle.0.clone().update(cx, |state, cx| state.raise_notice(text, None, cx));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::Utc;
    use mcp::{MeetingDraft, MeetingProposal};

    use super::unannounced;

    fn proposal(id: &str) -> MeetingProposal {
        MeetingProposal {
            id: id.to_owned(),
            draft: MeetingDraft {
                subject: "Sync".to_owned(),
                start: Utc::now(),
                end: Utc::now(),
                attendees: Vec::new(),
                description: String::new(),
            },
            agent: None,
            session_id: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn only_new_meeting_proposals_are_announced() {
        let pending = [proposal("a"), proposal("b")];
        let announced = HashSet::from(["a".to_owned()]);
        let fresh: Vec<&str> = unannounced(&announced, &pending).iter().map(|proposal| proposal.id.as_str()).collect();
        assert_eq!(fresh, ["b"]);
    }
}
