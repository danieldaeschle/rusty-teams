use std::rc::Rc;

use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::agent_api::{ProposalEntry, ProposalStatus, agent_label};
use crate::theme;
use crate::views::widgets::icon;

const CARD_MARGIN_X: f32 = 20.;
const CARD_MARGIN_BOTTOM: f32 = 8.;
const CARD_PADDING: f32 = 12.;
const CARD_RADIUS: f32 = 10.;
const CARD_GAP: f32 = 8.;
const BUTTON_HEIGHT: f32 = 28.;
const BUTTON_RADIUS: f32 = 7.;
const BODY_LINE_HEIGHT: f32 = 20.;
const BODY_MAX_LINES: f32 = 4.;
const QUOTE_BAR_WIDTH: f32 = 2.;

pub type CardAction = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
pub struct CardActions {
    pub send: CardAction,
    pub edit: CardAction,
    pub discard: CardAction,
}

fn open_session_available() -> bool {
    false
}

fn open_session(_session_id: &str, _window: &mut Window, _cx: &mut App) {}

pub fn age_label(created_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let minutes = (now - created_at).num_minutes().max(0);
    match minutes {
        0 => "just now".to_owned(),
        1..=59 => format!("{minutes} min ago"),
        60..=1439 => format!("{} h ago", minutes / 60),
        _ => format!("{} d ago", minutes / 1440),
    }
}

fn card_button(
    id: &'static str,
    label: &'static str,
    background: Hsla,
    color: Hsla,
    action: CardAction,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(BUTTON_HEIGHT))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(BUTTON_RADIUS))
        .cursor_pointer()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .bg(background)
        .text_color(color)
        .hover(|button| button.opacity(0.85))
        .child(label)
        .on_click(move |_, window, cx| action(window, cx))
}

fn header_text(entry: &ProposalEntry, now: DateTime<Utc>) -> (String, Hsla, Option<String>) {
    let agent = agent_label(entry.proposal.agent.as_deref());
    match &entry.status {
        ProposalStatus::Pending => (
            format!("{agent} proposes a reply"),
            theme::accent_text(),
            Some(format!(" - {}", age_label(entry.proposal.created_at, now))),
        ),
        ProposalStatus::Stale { answered_at } => (
            format!(
                "{agent}'s reply is outdated - you answered at {}",
                answered_at.format("%H:%M")
            ),
            theme::text_muted(),
            None,
        ),
        ProposalStatus::Failed { reason } => (
            format!("Could not send - {reason}"),
            theme::red_soft(),
            None,
        ),
    }
}

pub fn render(
    entry: &ProposalEntry,
    quote: Option<String>,
    actions: CardActions,
    now: DateTime<Utc>,
) -> Div {
    let stale = matches!(entry.status, ProposalStatus::Stale { .. });
    let failed = matches!(entry.status, ProposalStatus::Failed { .. });
    let (title, title_color, age) = header_text(entry, now);
    let close = actions.discard.clone();
    let header = h_flex()
        .w_full()
        .items_center()
        .gap(px(4.))
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .items_baseline()
                .overflow_hidden()
                .text_size(px(12.))
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(title_color)
                        .child(title),
                )
                .children(
                    age.map(|age| div().flex_none().text_color(theme::text_faint()).child(age)),
                ),
        )
        .child(
            div()
                .id("proposal-close")
                .size(px(BUTTON_HEIGHT))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(BUTTON_RADIUS))
                .cursor_pointer()
                .hover(|button| button.bg(theme::surface_raised()))
                .child(icon(IconName::Close, 14., theme::text_muted()))
                .on_click(move |_, window, cx| close(window, cx)),
        );
    let quote = quote.map(|quote| {
        div()
            .w_full()
            .truncate()
            .pl(px(8.))
            .border_l(px(QUOTE_BAR_WIDTH))
            .border_color(theme::border_strong())
            .text_size(px(12.))
            .text_color(theme::text_muted())
            .child(quote)
    });
    let body = div()
        .w_full()
        .max_h(px(BODY_LINE_HEIGHT * BODY_MAX_LINES))
        .overflow_hidden()
        .text_size(px(14.))
        .line_height(px(BODY_LINE_HEIGHT))
        .text_color(if stale {
            theme::text_faint()
        } else {
            theme::text()
        })
        .child(entry.proposal.text.clone());
    let session = entry
        .proposal
        .session_id
        .clone()
        .filter(|_| open_session_available());
    let buttons = h_flex()
        .w_full()
        .items_center()
        .gap(px(8.))
        .when(!stale, |row| {
            row.child(card_button(
                "proposal-send",
                if failed { "Retry" } else { "Send" },
                theme::accent(),
                theme::on_accent(),
                actions.send.clone(),
            ))
        })
        .child(card_button(
            "proposal-edit",
            "Edit",
            theme::surface_raised(),
            theme::text(),
            actions.edit.clone(),
        ))
        .child(card_button(
            "proposal-discard",
            "Discard",
            theme::surface_raised(),
            theme::text(),
            actions.discard.clone(),
        ))
        .children(session.map(|session_id| {
            div()
                .id("proposal-open-session")
                .ml_auto()
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(theme::accent_text())
                .child("Open session")
                .on_click(move |_, window, cx| open_session(&session_id, window, cx))
        }));
    v_flex()
        .flex_none()
        .mx(px(CARD_MARGIN_X))
        .mb(px(CARD_MARGIN_BOTTOM))
        .p(px(CARD_PADDING))
        .gap(px(CARD_GAP))
        .rounded(px(CARD_RADIUS))
        .bg(theme::row_hover())
        .child(header)
        .children(quote)
        .child(body)
        .child(buttons)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::age_label;

    #[test]
    fn age_labels_follow_the_elapsed_time() {
        let now = Utc::now();
        let label = |minutes: i64| age_label(now - Duration::minutes(minutes), now);
        assert_eq!(label(0), "just now");
        assert_eq!(label(1), "1 min ago");
        assert_eq!(label(59), "59 min ago");
        assert_eq!(label(120), "2 h ago");
        assert_eq!(label(3000), "2 d ago");
        assert_eq!(age_label(now + Duration::minutes(5), now), "just now");
    }
}
