use std::rc::Rc;

use chrono::{DateTime, Local, Utc};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use mcp::MeetingProposal;

use super::proposal_card::{CardAction, age_label, card_button};
use crate::agent_api::{self, meetings};
use crate::theme;

pub const MEETING_PROPOSAL_CARD: &str = "calendar/meeting-proposal";
const CARD_MARGIN: f32 = 12.;
const CARD_PADDING: f32 = 12.;
const CARD_GAP: f32 = 6.;
const CARD_RADIUS: f32 = 10.;
const DESCRIPTION_LINES: usize = 3;

pub fn when_label(start: DateTime<Utc>, end: DateTime<Utc>) -> String {
    let (start, end) = (start.with_timezone(&Local), end.with_timezone(&Local));
    let clock = crate::format::clock_format();
    let end_text = if end.date_naive() == start.date_naive() {
        end.format(clock).to_string()
    } else {
        format!("{} {}", end.format("%a %-d %b,"), end.format(clock))
    };
    format!("{} {} - {end_text}", start.format("%a %-d %b,"), start.format(clock))
}

fn description_excerpt(description: &str) -> Option<String> {
    let lines: Vec<&str> = description
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    let mut excerpt = lines[..lines.len().min(DESCRIPTION_LINES)].join("\n");
    if lines.len() > DESCRIPTION_LINES {
        excerpt.push_str(" ...");
    }
    Some(excerpt)
}

fn card(proposal: &MeetingProposal, now: DateTime<Utc>) -> Div {
    let agent = agent_api::agent_label(proposal.agent.as_deref());
    let draft = &proposal.draft;
    let (create_id, discard_id) = (proposal.id.clone(), proposal.id.clone());
    let create: CardAction = Rc::new(move |_, cx| meetings::create_meeting(&create_id, cx));
    let discard: CardAction = Rc::new(move |_, cx| meetings::discard_meeting(&discard_id, cx));
    v_flex()
        .flex_none()
        .mx(px(CARD_MARGIN))
        .mt(px(CARD_MARGIN))
        .p(px(CARD_PADDING))
        .gap(px(CARD_GAP))
        .rounded(px(CARD_RADIUS))
        .bg(theme::row_hover())
        .debug_selector(|| MEETING_PROPOSAL_CARD.into())
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme::accent_text())
                .child(format!("{agent} proposes a meeting - {}", age_label(proposal.created_at, now))),
        )
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(draft.subject.clone()),
        )
        .child(
            div()
                .text_size(px(13.))
                .child(when_label(draft.start, draft.end)),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .child(format!("Invites {}", draft.attendees.join(", "))),
        )
        .children(description_excerpt(&draft.description).map(|excerpt| {
            div()
                .text_size(px(12.))
                .text_color(theme::text_muted())
                .whitespace_normal()
                .child(excerpt)
        }))
        .child(
            h_flex()
                .pt(px(2.))
                .gap(px(8.))
                .child(card_button("meeting-proposal-create", "Create", theme::accent(), theme::on_accent(), create))
                .child(card_button("meeting-proposal-discard", "Discard", theme::surface_raised(), theme::text(), discard)),
        )
}

pub fn meeting_proposal_cards(cx: &App) -> Vec<AnyElement> {
    let now = Utc::now();
    meetings::pending_meetings(cx)
        .iter()
        .map(|proposal| card(proposal, now).id(SharedString::from(proposal.id.clone())).into_any_element())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::description_excerpt;

    #[test]
    fn long_descriptions_show_three_lines() {
        assert_eq!(description_excerpt("  \n"), None);
        assert_eq!(description_excerpt("a\n\nb\nc\nd").as_deref(), Some("a\nb\nc ..."));
    }
}
